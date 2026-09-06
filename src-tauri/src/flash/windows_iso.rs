use super::Reporter;
use super::sector_io::SectorIo;
use super::platform;
use super::protocol::{FlashPhase, FlashProgress};
use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, ReadWriteSeek};
use fscommon::{BufStream, StreamSlice};
use isomage::{TreeNode, cat_node};
use mbrman::{BOOT_ACTIVE, CHS, MBR, MBRPartitionEntry};
use meraki_flash_core::UsbDevice;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::{Builder, TempDir};
use uuid::Uuid;

const ALIGNMENT_BYTES: u64 = 1024 * 1024;
const CLEAR_BYTES: u64 = 1024 * 1024;
const COPY_BUFFER_SIZE: usize = 4 * 1024 * 1024;
const FAT32_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024 - 1;
const SWM_PART_BYTES: u64 = 3_800_000_000;
const MIN_FREE_MARGIN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_UNATTEND_BYTES: usize = 16 * 1024;
const WIM_WORKSPACE_RESERVE_BYTES: u64 = 512 * 1024 * 1024;

pub struct WindowsImage {
    source: File,
    root: TreeNode,
    split: Option<SplitArtifacts>,
    unattend_xml: Option<String>,
    tracker: ProgressTracker,
}

struct SplitArtifacts {
    _temporary_directory: TempDir,
    parts: Vec<PathBuf>,
}

pub fn prepare(
    mut source: File,
    iso_path: &Path,
    device: &UsbDevice,
    unattend_xml: Option<String>,
    operation_id: &str,
    reporter: &mut Reporter,
) -> Result<WindowsImage, String> {
    send_status(
        reporter,
        operation_id,
        FlashPhase::Analyzing,
        0.0,
        "Analisando a estrutura ISO/UDF do Windows…",
    );

    let iso_size = source
        .metadata()
        .map_err(|error| format!("não foi possível consultar a ISO: {error}"))?
        .len();
    let root = parse_windows_filesystem(&mut source)
        .map_err(|error| format!("a imagem não contém uma mídia Windows legível: {error}"))?;
    let summary = validate_windows_tree(&root, iso_size)?;

    if !has_uefi_loader(&root) {
        return Err("a ISO não contém um carregador UEFI em EFI/BOOT (boot*.efi)".to_owned());
    }

    let install_wim = find_path_case_insensitive(&root, &["sources", "install.wim"]);
    let install_esd = find_path_case_insensitive(&root, &["sources", "install.esd"]);
    let existing_swm = find_path_case_insensitive(&root, &["sources", "install.swm"]);
    if install_wim.is_none() && install_esd.is_none() && existing_swm.is_none() {
        return Err("a ISO não contém sources/install.wim, install.esd ou install.swm".to_owned());
    }
    if install_esd.is_some_and(|node| node.size > FAT32_MAX_FILE_BYTES) {
        return Err(
            "sources/install.esd excede 4 GiB e usa compactação sólida; converta-o para WIM antes de gravar"
                .to_owned(),
        );
    }

    let wim_to_split = install_wim.filter(|node| node.size > FAT32_MAX_FILE_BYTES);
    let unattend_bytes = unattend_xml.as_ref().map_or(0, |xml| xml.len() as u64);
    let total_work = summary
        .total_file_bytes
        .saturating_add(
            wim_to_split
                .map(|node| node.size.saturating_mul(2))
                .unwrap_or(0),
        )
        .saturating_add(unattend_bytes);
    let mut tracker = ProgressTracker::new(total_work.max(1));

    let split = if let Some(wim) = wim_to_split {
        Some(prepare_split_wim(
            &mut source,
            wim,
            iso_path,
            operation_id,
            reporter,
            &mut tracker,
        )?)
    } else {
        None
    };

    let copied_payload_bytes = if let (Some(wim), Some(split)) = (wim_to_split, split.as_ref()) {
        let split_bytes = split.parts.iter().try_fold(0u64, |total, path| {
            let size = path
                .metadata()
                .map_err(|error| format!("não foi possível consultar {}: {error}", path.display()))?
                .len();
            total
                .checked_add(size)
                .ok_or_else(|| "o tamanho da mídia excede o limite suportado".to_owned())
        })?;
        summary
            .total_file_bytes
            .saturating_sub(wim.size)
            .saturating_add(split_bytes)
    } else {
        summary.total_file_bytes
    }
    .saturating_add(unattend_bytes);

    let usable_bytes = device.total_bytes.saturating_sub(2 * ALIGNMENT_BYTES);
    let safety_margin = (copied_payload_bytes / 100).max(MIN_FREE_MARGIN_BYTES);
    if copied_payload_bytes.saturating_add(safety_margin) > usable_bytes {
        return Err(format!(
            "o conteúdo da ISO precisa de aproximadamente {} bytes, mas o pendrive oferece {} bytes úteis",
            copied_payload_bytes.saturating_add(safety_margin),
            usable_bytes
        ));
    }

    send_status(
        reporter,
        operation_id,
        FlashPhase::Analyzing,
        tracker.percentage(),
        &format!("ISO Windows validada: {}", iso_path.display()),
    );

    Ok(WindowsImage {
        source,
        root,
        split,
        unattend_xml,
        tracker,
    })
}

pub fn write(
    mut image: WindowsImage,
    device: &UsbDevice,
    operation_id: &str,
    reporter: &mut Reporter,
) -> Result<(), String> {
    send_status(
        reporter,
        operation_id,
        FlashPhase::Preparing,
        image.tracker.percentage(),
        "Desmontando e bloqueando o dispositivo…",
    );
    let mut prepared = platform::prepare_device(device)?;
    let sector_size = platform::logical_sector_size(&prepared.file)?;

    let mut target = SectorIo::new(&mut prepared.file, sector_size, device.total_bytes)
        .map_err(|error| format!("geometria do dispositivo inválida: {error}"))?;
    send_status(
        reporter,
        operation_id,
        FlashPhase::Formatting,
        image.tracker.percentage(),
        "Criando uma partição MBR/FAT32 compatível com UEFI…",
    );
    let (partition_start, partition_end) =
        create_partition_table(&mut target, device.total_bytes, sector_size)?;
    format_fat32(
        &mut target,
        partition_start,
        partition_end,
        sector_size,
    )?;

    let partition = StreamSlice::new(&mut target, partition_start, partition_end)
        .map_err(|error| format!("não foi possível abrir a partição FAT32: {error}"))?;
    let filesystem = FileSystem::new(BufStream::new(partition), FsOptions::new())
        .map_err(|error| format!("não foi possível abrir o FAT32 recém-criado: {error}"))?;
    {
        let root_dir = filesystem.root_dir();
        copy_directory(
            &mut image.source,
            &image.root,
            &root_dir,
            "",
            image.split.as_ref(),
            operation_id,
            reporter,
            &mut image.tracker,
        )?;
        if let Some(xml) = image.unattend_xml.as_deref() {
            send_status(
                reporter,
                operation_id,
                FlashPhase::Extracting,
                image.tracker.percentage(),
                "Adicionando autounattend.xml à raiz do pendrive…",
            );
            write_unattend_file(&root_dir, xml)?;
            image.tracker.add(
                xml.len() as u64,
                FlashPhase::Extracting,
                reporter,
                operation_id,
                Some("autounattend.xml criado com sucesso".to_owned()),
            );
        }
    }
    filesystem
        .unmount()
        .map_err(|error| format!("não foi possível finalizar o FAT32: {error}"))?;

    send_status(
        reporter,
        operation_id,
        FlashPhase::Syncing,
        100.0,
        "Sincronizando a tabela de partições e os arquivos…",
    );
    let _ = target.finish().map_err(|error| format!("falha ao finalizar os setores: {error}"))?;
    prepared
        .file
        .sync_all()
        .map_err(|error| format!("falha ao sincronizar o dispositivo: {error}"))?;
    drop(prepared);
    Ok(())
}

pub fn validate_unattend_xml(content: Option<&str>) -> Result<Option<String>, String> {
    let Some(content) = content else {
        return Ok(None);
    };
    let content = content.trim();
    if content.is_empty() {
        return Err("o conteúdo de autounattend.xml está vazio".to_owned());
    }
    if content.len() > MAX_UNATTEND_BYTES {
        return Err(format!(
            "autounattend.xml excede o limite de {} KiB",
            MAX_UNATTEND_BYTES / 1024
        ));
    }
    if content.contains('\0') {
        return Err("autounattend.xml contém um caractere NUL inválido".to_owned());
    }

    let mut reader = Reader::from_str(content);
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut root_closed = false;
    let mut settings_seen = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                if depth == 0 {
                    if root_seen || element.name().as_ref() != b"unattend" {
                        return Err(
                            "autounattend.xml deve ter um único elemento raiz <unattend>"
                                .to_owned(),
                        );
                    }
                    root_seen = true;
                } else if root_closed {
                    return Err("autounattend.xml contém conteúdo após a raiz".to_owned());
                } else if depth == 1 && element.name().as_ref() == b"settings" {
                    settings_seen = true;
                }
                depth += 1;
            }
            Ok(Event::Empty(element)) => {
                if depth == 0 {
                    return Err(format!(
                        "autounattend.xml não pode usar uma raiz vazia <{} />",
                        String::from_utf8_lossy(element.name().as_ref())
                    ));
                }
                if depth == 1 && element.name().as_ref() == b"settings" {
                    settings_seen = true;
                }
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Err("autounattend.xml contém uma tag de fechamento inválida".to_owned());
                }
                depth -= 1;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::DocType(_)) => {
                return Err("autounattend.xml não pode conter DTD/DOCTYPE".to_owned());
            }
            Ok(Event::Text(text)) if depth == 0 => {
                let bytes: &[u8] = text.as_ref();
                if bytes.iter().any(|byte| !byte.is_ascii_whitespace()) {
                    return Err("autounattend.xml contém texto fora da raiz".to_owned());
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("autounattend.xml é inválido: {error}")),
        }
    }

    if !root_seen || !root_closed || depth != 0 || !settings_seen {
        return Err(
            "autounattend.xml precisa conter <unattend> e ao menos uma seção <settings>".to_owned(),
        );
    }
    Ok(Some(content.to_owned()))
}

fn parse_windows_filesystem(source: &mut File) -> Result<TreeNode, String> {
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    match isomage::udf::parse_udf(source) {
        Ok(root) => Ok(root),
        Err(udf_error) => {
            source
                .seek(SeekFrom::Start(0))
                .map_err(|error| error.to_string())?;
            isomage::iso9660::parse_iso9660(source)
                .map_err(|iso_error| format!("UDF: {udf_error}; ISO9660: {iso_error}"))
        }
    }
}

#[derive(Debug)]
struct TreeSummary {
    total_file_bytes: u64,
}

fn validate_windows_tree(root: &TreeNode, iso_size: u64) -> Result<TreeSummary, String> {
    fn visit(
        node: &TreeNode,
        components: &mut Vec<String>,
        iso_size: u64,
        total: &mut u64,
    ) -> Result<(), String> {
        if node.name != "/" {
            validate_fat_name(&node.name)?;
            components.push(node.name.clone());
        }

        if node.is_directory {
            let mut names = HashSet::new();
            for child in &node.children {
                if !names.insert(child.name.to_lowercase()) {
                    return Err(format!(
                        "a ISO contém nomes incompatíveis com FAT32 no mesmo diretório: {}",
                        child.name
                    ));
                }
                visit(child, components, iso_size, total)?;
            }
        } else {
            let relative = components.join("/");
            if node.size > FAT32_MAX_FILE_BYTES
                && !relative.eq_ignore_ascii_case("sources/install.wim")
            {
                return Err(format!(
                    "o arquivo {relative} excede o limite de 4 GiB do FAT32"
                ));
            }
            if node.size > 0 {
                let location = node.file_location.ok_or_else(|| {
                    format!("a ISO não informa onde os dados de {relative} estão armazenados")
                })?;
                let length = node
                    .file_length
                    .ok_or_else(|| format!("a ISO não informa o tamanho físico de {relative}"))?;
                if length != node.size
                    || location
                        .checked_add(length)
                        .is_none_or(|end| end > iso_size)
                {
                    return Err(format!("a extensão de dados de {relative} é inválida"));
                }
            }
            *total = total
                .checked_add(node.size)
                .ok_or_else(|| "o tamanho da ISO excede o limite suportado".to_owned())?;
        }

        if node.name != "/" {
            components.pop();
        }
        Ok(())
    }

    let mut total = 0;
    visit(root, &mut Vec::new(), iso_size, &mut total)?;
    Ok(TreeSummary {
        total_file_bytes: total,
    })
}

fn validate_fat_name(name: &str) -> Result<(), String> {
    let utf16_len = name.encode_utf16().count();
    let invalid = name.is_empty()
        || matches!(name, "." | "..")
        || utf16_len > 255
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|character| character < '\u{20}' || "<>:\"/\\|?*\0".contains(character));
    if invalid {
        return Err(format!(
            "nome incompatível com FAT32 encontrado na ISO: {name:?}"
        ));
    }

    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        return Err(format!(
            "nome reservado do Windows encontrado na ISO: {name}"
        ));
    }
    Ok(())
}

fn has_uefi_loader(root: &TreeNode) -> bool {
    find_path_case_insensitive(root, &["efi", "boot"]).is_some_and(|directory| {
        directory.is_directory
            && directory.children.iter().any(|node| {
                let name = node.name.to_ascii_lowercase();
                !node.is_directory && name.starts_with("boot") && name.ends_with(".efi")
            })
    })
}

fn find_path_case_insensitive<'a>(root: &'a TreeNode, components: &[&str]) -> Option<&'a TreeNode> {
    components.iter().try_fold(root, |current, component| {
        current
            .children
            .iter()
            .find(|child| child.name.eq_ignore_ascii_case(component))
    })
}

fn prepare_split_wim(
    source: &mut File,
    wim: &TreeNode,
    iso_path: &Path,
    operation_id: &str,
    reporter: &mut Reporter,
    tracker: &mut ProgressTracker,
) -> Result<SplitArtifacts, String> {
    let required_bytes = required_wim_workspace_bytes(wim.size);
    let temporary_directory = create_wim_workspace(iso_path, required_bytes)?;
    let input_path = temporary_directory.path().join("source-install.wim");
    let output_path = temporary_directory.path().join("install.swm");

    send_status(
        reporter,
        operation_id,
        FlashPhase::Splitting,
        tracker.percentage(),
        &format!(
            "Preparando install.wim em {} ({} reservados)…",
            temporary_directory.path().display(),
            format_bytes(required_bytes)
        ),
    );
    let mut temporary_wim = File::create(&input_path)
        .map_err(|error| format!("não foi possível criar o WIM temporário: {error}"))?;
    {
        let mut output = TrackedWriter::new(
            &mut temporary_wim,
            tracker,
            reporter,
            operation_id,
            FlashPhase::Splitting,
        );
        cat_node(source, wim, &mut output)
            .map_err(|error| format!("não foi possível extrair install.wim da ISO: {error}"))?;
    }
    temporary_wim
        .sync_all()
        .map_err(|error| format!("não foi possível sincronizar o WIM temporário: {error}"))?;
    drop(temporary_wim);

    split_wim_with_progress(
        &input_path,
        &output_path,
        wim.size,
        operation_id,
        reporter,
        tracker,
    )?;

    let parts = collect_and_validate_split_parts(temporary_directory.path())?;
    // O WIM original não é mais necessário. Liberá-lo aqui reduz o uso do
    // workspace pela metade durante a cópia dos SWMs para o pendrive.
    std::fs::remove_file(&input_path)
        .map_err(|error| format!("não foi possível liberar o WIM temporário: {error}"))?;

    Ok(SplitArtifacts {
        _temporary_directory: temporary_directory,
        parts,
    })
}

fn required_wim_workspace_bytes(wim_bytes: u64) -> u64 {
    let reserve = (wim_bytes / 10).max(WIM_WORKSPACE_RESERVE_BYTES);
    wim_bytes.saturating_mul(2).saturating_add(reserve)
}

fn create_wim_workspace(iso_path: &Path, required_bytes: u64) -> Result<TempDir, String> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "linux")]
    candidates.push(PathBuf::from("/var/tmp"));
    candidates.push(std::env::temp_dir());
    if let Some(parent) = iso_path.parent() {
        candidates.push(parent.to_path_buf());
    }

    create_wim_workspace_from_candidates(
        candidates,
        required_bytes,
        filesystem_available_space,
        |base| Builder::new().prefix("meraki-flash-wim-").tempdir_in(base),
    )
}

fn create_wim_workspace_from_candidates<A, C>(
    candidates: Vec<PathBuf>,
    required_bytes: u64,
    mut available_space: A,
    mut create_directory: C,
) -> Result<TempDir, String>
where
    A: FnMut(&Path) -> Result<u64, String>,
    C: FnMut(&Path) -> std::io::Result<TempDir>,
{
    let mut seen = HashSet::new();
    let mut attempts = Vec::new();
    for base in candidates {
        if !seen.insert(base.clone()) {
            continue;
        }
        let workspace = match create_directory(&base) {
            Ok(workspace) => workspace,
            Err(error) => {
                attempts.push(format!("{}: {error}", base.display()));
                continue;
            }
        };
        match available_space(workspace.path()) {
            Ok(available) if available >= required_bytes => return Ok(workspace),
            Ok(available) => attempts.push(format!(
                "{}: somente {} livres",
                base.display(),
                format_bytes(available)
            )),
            Err(error) => attempts.push(format!("{}: {error}", base.display())),
        }
    }

    Err(format!(
        "espaço temporário insuficiente para dividir install.wim: são necessários {}. Locais verificados: {}. Libere espaço no disco do sistema ou mova a ISO para um volume com mais espaço livre",
        format_bytes(required_bytes),
        attempts.join("; ")
    ))
}

#[cfg(target_os = "linux")]
fn filesystem_available_space(path: &Path) -> Result<u64, String> {
    let statistics = nix::sys::statvfs::statvfs(path)
        .map_err(|error| format!("não foi possível consultar o espaço livre: {error}"))?;
    let fragment_size = (statistics.fragment_size() as u64).max(1);
    (statistics.blocks_available() as u64)
        .checked_mul(fragment_size)
        .ok_or_else(|| "o espaço livre retornado pelo sistema é inválido".to_owned())
}

#[cfg(target_os = "windows")]
fn filesystem_available_space(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut available = 0u64;
    // SAFETY: o caminho é UTF-16 terminado em NUL e o ponteiro de saída é válido.
    let success = unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(format!(
            "não foi possível consultar o espaço livre: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(available)
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = (1024 * 1024 * 1024) as f64;
    const MIB: f64 = (1024 * 1024) as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else {
        format!("{:.0} MiB", bytes as f64 / MIB)
    }
}

fn collect_and_validate_split_parts(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut parts = std::fs::read_dir(directory)
        .map_err(|error| format!("não foi possível listar os SWMs temporários: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("swm"))
        })
        .collect::<Vec<_>>();
    parts.sort_by_key(|path| swm_part_number(path));
    if parts.is_empty() {
        return Err("wimlib não produziu nenhum arquivo SWM".to_owned());
    }

    for (index, part) in parts.iter().enumerate() {
        let expected_name = if index == 0 {
            "install.swm".to_owned()
        } else {
            format!("install{}.swm", index + 1)
        };
        let actual_name = part
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "wimlib produziu um nome SWM inválido".to_owned())?;
        if !actual_name.eq_ignore_ascii_case(&expected_name) {
            return Err(format!(
                "sequência SWM inválida: esperado {expected_name}, encontrado {actual_name}"
            ));
        }

        let size = part
            .metadata()
            .map_err(|error| format!("não foi possível consultar {}: {error}", part.display()))?
            .len();
        if size == 0 {
            return Err(format!("{actual_name} foi criado vazio"));
        }
        if size > FAT32_MAX_FILE_BYTES {
            return Err(format!(
                "{actual_name} ainda excede 4 GiB; o WIM contém um recurso indivisível muito grande"
            ));
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(part)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("não foi possível sincronizar {actual_name}: {error}"))?;
    }
    if parts.len() < 2 {
        return Err("wimlib não produziu a segunda parte install2.swm".to_owned());
    }
    Ok(parts)
}

fn swm_part_number(path: &Path) -> u32 {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    stem.strip_prefix("install")
        .filter(|suffix| !suffix.is_empty())
        .and_then(|suffix| suffix.parse().ok())
        .unwrap_or(1)
}

fn split_wim_with_progress(
    input_path: &Path,
    output_path: &Path,
    work_bytes: u64,
    operation_id: &str,
    reporter: &mut Reporter,
    tracker: &mut ProgressTracker,
) -> Result<(), String> {
    let output_directory = output_path
        .parent()
        .ok_or_else(|| "diretório temporário SWM inválido".to_owned())?
        .to_path_buf();
    let input_path = input_path.to_path_buf();
    let output_path = output_path.to_path_buf();
    let worker =
        thread::spawn(move || super::wim::split(&input_path, &output_path, SWM_PART_BYTES));
    let mut mapped_progress = 0u64;

    while !worker.is_finished() {
        let produced = split_output_bytes(&output_directory).unwrap_or(mapped_progress);
        let mapped = produced.min(work_bytes);
        if mapped > mapped_progress {
            tracker.add(
                mapped - mapped_progress,
                FlashPhase::Splitting,
                reporter,
                operation_id,
                Some("Gerando os arquivos install.swm…".to_owned()),
            );
            mapped_progress = mapped;
        }
        thread::sleep(Duration::from_millis(250));
    }

    let result = worker
        .join()
        .map_err(|_| "o processo interno de divisão do WIM falhou".to_owned())?;
    if let Err(error) = result {
        return Err(describe_split_failure(
            &output_directory,
            work_bytes,
            &error,
        ));
    }
    if mapped_progress < work_bytes {
        tracker.add(
            work_bytes - mapped_progress,
            FlashPhase::Splitting,
            reporter,
            operation_id,
            Some("Divisão do WIM concluída".to_owned()),
        );
    }
    Ok(())
}

fn describe_split_failure(directory: &Path, input_bytes: u64, error: &str) -> String {
    let produced = split_output_bytes(directory).unwrap_or(0);
    match filesystem_available_space(directory) {
        Ok(available)
            if available
                < input_bytes
                    .saturating_sub(produced)
                    .saturating_add(MIN_FREE_MARGIN_BYTES) =>
        {
            format!(
                "espaço temporário esgotado durante a divisão do WIM: {} livres em {}, {} já gerados. Detalhe: {error}",
                format_bytes(available),
                directory.display(),
                format_bytes(produced)
            )
        }
        Ok(available) => format!(
            "{error}. Área temporária: {} ({} livres; {} gerados)",
            directory.display(),
            format_bytes(available),
            format_bytes(produced)
        ),
        Err(space_error) => format!(
            "{error}. Área temporária: {} ({space_error})",
            directory.display()
        ),
    }
}

fn split_output_bytes(directory: &Path) -> Result<u64, String> {
    std::fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("swm"))
        })
        .try_fold(0u64, |total, path| {
            let size = path.metadata().map_err(|error| error.to_string())?.len();
            total
                .checked_add(size)
                .ok_or_else(|| "tamanho temporário SWM inválido".to_owned())
        })
}

fn create_partition_table(
    target: &mut (impl Read + Write + Seek),
    device_size: u64,
    sector_size: u32,
) -> Result<(u64, u64), String> {
    let sector_size_u64 = u64::from(sector_size);
    let start = align_up(ALIGNMENT_BYTES, sector_size_u64);
    let end = align_down(device_size.saturating_sub(ALIGNMENT_BYTES), sector_size_u64);
    if end <= start {
        return Err("o dispositivo é pequeno demais para a mídia Windows".to_owned());
    }
    let disk_sectors = device_size / sector_size_u64;
    let partition_sectors = (end - start) / sector_size_u64;
    if disk_sectors > u64::from(u32::MAX) || partition_sectors > u64::from(u32::MAX) {
        return Err("dispositivos maiores que 2 TiB ainda não são suportados".to_owned());
    }

    clear_range(target, 0, CLEAR_BYTES.min(device_size))?;
    if device_size > CLEAR_BYTES {
        clear_range(target, device_size - CLEAR_BYTES, CLEAR_BYTES)?;
    }

    let signature = Uuid::new_v4();
    let signature: [u8; 4] = signature.as_bytes()[..4]
        .try_into()
        .map_err(|_| "não foi possível gerar a assinatura do disco".to_owned())?;
    let mut bounded = StreamSlice::new(&mut *target, 0, device_size)
        .map_err(|error| format!("não foi possível delimitar o dispositivo: {error}"))?;
    let mut mbr = MBR::new_from(&mut bounded, sector_size, signature)
        .map_err(|error| format!("não foi possível criar a tabela MBR: {error}"))?;
    mbr[1] = MBRPartitionEntry {
        boot: BOOT_ACTIVE,
        first_chs: CHS::empty(),
        sys: 0x0c,
        last_chs: CHS::empty(),
        starting_lba: u32::try_from(start / sector_size_u64)
            .map_err(|_| "início da partição fora do limite MBR".to_owned())?,
        sectors: u32::try_from(partition_sectors)
            .map_err(|_| "partição fora do limite MBR".to_owned())?,
    };
    mbr.write_into(&mut bounded)
        .map_err(|error| format!("não foi possível gravar a tabela MBR: {error}"))?;
    bounded
        .flush()
        .map_err(|error| format!("não foi possível finalizar a tabela MBR: {error}"))?;
    Ok((start, end))
}

fn format_fat32(target: &mut (impl Read + Write + Seek), start: u64, end: u64, sector_size: u32) -> Result<(), String> {
    let partition_sectors = (end - start) / u64::from(sector_size);
    let options = FormatVolumeOptions::new()
        .bytes_per_sector(
            u16::try_from(sector_size)
                .map_err(|_| format!("tamanho de setor inválido: {sector_size}"))?,
        )
        .bytes_per_cluster(32 * 1024)
        .fat_type(FatType::Fat32)
        .total_sectors(
            u32::try_from(partition_sectors)
                .map_err(|_| "a partição FAT32 excede o limite suportado".to_owned())?,
        )
        .volume_id(Uuid::new_v4().as_u128() as u32)
        .volume_label(*b"MERAKI     ");
    let partition = StreamSlice::new(&mut *target, start, end)
        .map_err(|error| format!("não foi possível delimitar a partição FAT32: {error}"))?;
    fatfs::format_volume(BufStream::new(partition), options)
        .map_err(|error| format!("não foi possível formatar a partição FAT32: {error}"))
}

#[allow(clippy::too_many_arguments)]
fn copy_directory<T: ReadWriteSeek>(
    source: &mut File,
    source_directory: &TreeNode,
    target_directory: &fatfs::Dir<'_, T>,
    parent_path: &str,
    split: Option<&SplitArtifacts>,
    operation_id: &str,
    reporter: &mut Reporter,
    tracker: &mut ProgressTracker,
) -> Result<(), String> {
    for node in &source_directory.children {
        let relative_path = if parent_path.is_empty() {
            node.name.clone()
        } else {
            format!("{parent_path}/{}", node.name)
        };
        if node.is_directory {
            let directory = target_directory.create_dir(&node.name).map_err(|error| {
                format!("não foi possível criar {relative_path} no FAT32: {error}")
            })?;
            copy_directory(
                source,
                node,
                &directory,
                &relative_path,
                split,
                operation_id,
                reporter,
                tracker,
            )?;
            continue;
        }

        if relative_path.eq_ignore_ascii_case("sources/install.wim")
            && let Some(split) = split
        {
            for part in &split.parts {
                let name = part
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| "nome temporário SWM inválido".to_owned())?;
                let mut input = BufReader::with_capacity(
                    COPY_BUFFER_SIZE,
                    File::open(part).map_err(|error| {
                        format!("não foi possível abrir {}: {error}", part.display())
                    })?,
                );
                let mut output = target_directory
                    .create_file(name)
                    .map_err(|error| format!("não foi possível criar sources/{name}: {error}"))?;
                output.truncate().map_err(|error| {
                    format!("não foi possível preparar sources/{name}: {error}")
                })?;
                let expected_bytes = part
                    .metadata()
                    .map_err(|error| format!("não foi possível consultar {name}: {error}"))?
                    .len();
                let copied_bytes = {
                    let mut tracked = TrackedWriter::new(
                        &mut output,
                        tracker,
                        reporter,
                        operation_id,
                        FlashPhase::Extracting,
                    );
                    copy_reader(&mut input, &mut tracked).map_err(|error| {
                        format!("não foi possível copiar sources/{name}: {error}")
                    })?
                };
                if copied_bytes != expected_bytes {
                    return Err(format!(
                        "sources/{name} ficou incompleto: esperado {expected_bytes} bytes, gravado {copied_bytes} bytes"
                    ));
                }
                output.flush().map_err(|error| {
                    format!("não foi possível finalizar sources/{name}: {error}")
                })?;
            }
            continue;
        }

        let mut output = target_directory
            .create_file(&node.name)
            .map_err(|error| format!("não foi possível criar {relative_path} no FAT32: {error}"))?;
        output
            .truncate()
            .map_err(|error| format!("não foi possível preparar {relative_path}: {error}"))?;
        if node.size > 0 {
            let mut tracked = TrackedWriter::new(
                &mut output,
                tracker,
                reporter,
                operation_id,
                FlashPhase::Extracting,
            );
            cat_node(source, node, &mut tracked)
                .map_err(|error| format!("não foi possível extrair {relative_path}: {error}"))?;
        }
    }
    Ok(())
}

fn copy_reader(reader: &mut impl Read, writer: &mut impl Write) -> std::io::Result<u64> {
    let mut buffer = vec![0u8; COPY_BUFFER_SIZE];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            writer.flush()?;
            return Ok(total);
        }
        writer.write_all(&buffer[..read])?;
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| std::io::Error::other("tamanho copiado inválido"))?;
    }
}

fn write_unattend_file<T: ReadWriteSeek>(
    root_directory: &fatfs::Dir<'_, T>,
    content: &str,
) -> Result<(), String> {
    let mut file = root_directory
        .create_file("autounattend.xml")
        .map_err(|error| format!("não foi possível criar autounattend.xml: {error}"))?;
    file.truncate()
        .map_err(|error| format!("não foi possível substituir autounattend.xml: {error}"))?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.flush())
        .map_err(|error| format!("não foi possível gravar autounattend.xml: {error}"))
}

fn clear_range(target: &mut (impl Read + Write + Seek), start: u64, length: u64) -> Result<(), String> {
    target
        .seek(SeekFrom::Start(start))
        .map_err(|error| format!("não foi possível posicionar o dispositivo: {error}"))?;
    let zeros = vec![0u8; COPY_BUFFER_SIZE.min(length as usize)];
    let mut remaining = length;
    while remaining > 0 {
        let count = remaining.min(zeros.len() as u64) as usize;
        target
            .write_all(&zeros[..count])
            .map_err(|error| format!("não foi possível limpar assinaturas antigas: {error}"))?;
        remaining -= count as u64;
    }
    Ok(())
}

const fn align_up(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

const fn align_down(value: u64, alignment: u64) -> u64 {
    value / alignment * alignment
}

struct ProgressTracker {
    total_bytes: u64,
    completed_bytes: u64,
    started: Instant,
    last_report: Instant,
}

impl ProgressTracker {
    fn new(total_bytes: u64) -> Self {
        Self {
            total_bytes,
            completed_bytes: 0,
            started: Instant::now(),
            last_report: Instant::now(),
        }
    }

    fn percentage(&self) -> f64 {
        self.completed_bytes as f64 * 100.0 / self.total_bytes as f64
    }

    fn add(
        &mut self,
        bytes: u64,
        phase: FlashPhase,
        reporter: &mut Reporter,
        operation_id: &str,
        message: Option<String>,
    ) {
        self.completed_bytes = self
            .completed_bytes
            .saturating_add(bytes)
            .min(self.total_bytes);
        if self.last_report.elapsed() < Duration::from_millis(250)
            && self.completed_bytes < self.total_bytes
            && message.is_none()
        {
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let speed = self.completed_bytes as f64 / elapsed;
        let eta = (speed > 0.0)
            .then(|| ((self.total_bytes - self.completed_bytes) as f64 / speed).ceil() as u64);
        let _ = reporter.send(FlashProgress::new(
            operation_id,
            phase,
            self.percentage(),
            speed,
            eta,
            message,
        ));
        self.last_report = Instant::now();
    }
}

struct TrackedWriter<'a, W> {
    inner: W,
    tracker: &'a mut ProgressTracker,
    reporter: &'a mut Reporter,
    operation_id: &'a str,
    phase: FlashPhase,
}

impl<'a, W> TrackedWriter<'a, W> {
    fn new(
        inner: W,
        tracker: &'a mut ProgressTracker,
        reporter: &'a mut Reporter,
        operation_id: &'a str,
        phase: FlashPhase,
    ) -> Self {
        Self {
            inner,
            tracker,
            reporter,
            operation_id,
            phase,
        }
    }
}

impl<W: Write> Write for TrackedWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.tracker.add(
            written as u64,
            self.phase,
            self.reporter,
            self.operation_id,
            None,
        );
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn send_status(
    reporter: &mut Reporter,
    operation_id: &str,
    phase: FlashPhase,
    percentage: f64,
    message: &str,
) {
    let _ = reporter.send(FlashProgress::new(
        operation_id,
        phase,
        percentage,
        0.0,
        None,
        Some(message.to_owned()),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Seek, SeekFrom, Write};
    use tempfile::NamedTempFile;

    const TEST_DISK_BYTES: u64 = 3 * 1024 * 1024 * 1024;
    const TEST_SECTOR_BYTES: u32 = 512;
    const TEST_UNATTEND: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="windowsPE"></settings>
</unattend>
"#;

    fn windows_tree_with_install_wim(size: u64) -> TreeNode {
        let mut root = TreeNode::new_directory("/".to_owned());
        let mut sources = TreeNode::new_directory("sources".to_owned());
        sources.add_child(TreeNode::new_file_with_location(
            "install.wim".to_owned(),
            size,
            0,
            size,
        ));
        root.add_child(sources);
        root.calculate_directory_size();
        root
    }

    #[test]
    fn permits_an_oversized_install_wim_for_later_splitting() {
        let size = FAT32_MAX_FILE_BYTES + 1;
        let tree = windows_tree_with_install_wim(size);

        let summary = validate_windows_tree(&tree, size).unwrap();

        assert_eq!(summary.total_file_bytes, size);
    }

    #[test]
    fn rejects_other_files_that_exceed_the_fat32_limit() {
        let size = FAT32_MAX_FILE_BYTES + 1;
        let mut root = TreeNode::new_directory("/".to_owned());
        root.add_child(TreeNode::new_file_with_location(
            "payload.bin".to_owned(),
            size,
            0,
            size,
        ));

        let error = validate_windows_tree(&root, size).unwrap_err();

        assert!(error.contains("payload.bin"));
        assert!(error.contains("4 GiB"));
    }

    #[test]
    fn workspace_budget_covers_the_input_and_split_outputs() {
        let five_gib = 5 * 1024 * 1024 * 1024;

        let required = required_wim_workspace_bytes(five_gib);

        assert!(required >= five_gib * 2 + WIM_WORKSPACE_RESERVE_BYTES);
    }

    #[test]
    fn workspace_skips_a_volume_without_enough_space() {
        let root = tempfile::tempdir().unwrap();
        let small = root.path().join("small");
        let large = root.path().join("large");
        std::fs::create_dir(&small).unwrap();
        std::fs::create_dir(&large).unwrap();
        let small_for_check = small.clone();
        let large_for_assertion = large.clone();

        let workspace = create_wim_workspace_from_candidates(
            vec![small, large],
            1_000,
            move |path| {
                if path.starts_with(&small_for_check) {
                    Ok(999)
                } else {
                    Ok(10_000)
                }
            },
            |base| Builder::new().prefix("workspace-").tempdir_in(base),
        )
        .unwrap();

        assert!(workspace.path().starts_with(large_for_assertion));
    }

    #[test]
    fn validates_a_complete_sequential_swm_set() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("install.swm"), b"part-one").unwrap();
        std::fs::write(directory.path().join("install2.swm"), b"part-two").unwrap();

        let parts = collect_and_validate_split_parts(directory.path()).unwrap();

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].file_name().unwrap(), "install.swm");
        assert_eq!(parts[1].file_name().unwrap(), "install2.swm");
    }

    #[test]
    fn copy_reader_reports_the_exact_number_of_bytes() {
        let payload = (0..(COPY_BUFFER_SIZE + 137))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let mut source = Cursor::new(payload.clone());
        let mut target = Vec::new();

        let copied = copy_reader(&mut source, &mut target).unwrap();

        assert_eq!(copied, payload.len() as u64);
        assert_eq!(target, payload);
    }

    #[test]
    fn validates_a_safe_unattend_document() {
        assert_eq!(
            validate_unattend_xml(Some(TEST_UNATTEND)).unwrap(),
            Some(TEST_UNATTEND.trim().to_owned())
        );
        assert_eq!(validate_unattend_xml(None).unwrap(), None);
    }

    #[test]
    fn rejects_malformed_or_unsafe_unattend_documents() {
        let malformed = "<unattend><settings></unattend>";
        let wrong_root = "<answer><settings /></answer>";
        let with_doctype = "<!DOCTYPE unattend><unattend><settings /></unattend>";
        let oversized = format!(
            "<unattend><settings>{}</settings></unattend>",
            " ".repeat(MAX_UNATTEND_BYTES)
        );

        assert!(validate_unattend_xml(Some(malformed)).is_err());
        assert!(validate_unattend_xml(Some(wrong_root)).is_err());
        assert!(validate_unattend_xml(Some(with_doctype)).is_err());
        assert!(validate_unattend_xml(Some(&oversized)).is_err());
    }

    #[test]
    fn creates_a_bootable_mbr_and_a_writable_fat32_filesystem() {
        let mut disk = NamedTempFile::new().unwrap();
        disk.as_file_mut().set_len(TEST_DISK_BYTES).unwrap();
        let split_directory = tempfile::tempdir().unwrap();
        let first_part = split_directory.path().join("install.swm");
        let second_part = split_directory.path().join("install2.swm");
        std::fs::write(&first_part, b"first-swm-part").unwrap();
        std::fs::write(&second_part, b"second-swm-part").unwrap();
        let split = SplitArtifacts {
            _temporary_directory: split_directory,
            parts: vec![first_part, second_part],
        };
        let windows_tree = windows_tree_with_install_wim(32);
        let mut empty_iso = NamedTempFile::new().unwrap().reopen().unwrap();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let report_reader = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut messages = Vec::new();
            stream.read_to_end(&mut messages).unwrap();
            messages
        });
        let mut reporter = Reporter {
            stream: std::net::TcpStream::connect(address).unwrap(),
            token: "test-token".to_owned(),
            last_percentage: 0.0,
        };
        let mut tracker = ProgressTracker::new(64);

        let (start, end) =
            create_partition_table(disk.as_file_mut(), TEST_DISK_BYTES, TEST_SECTOR_BYTES).unwrap();
        disk.as_file_mut().seek(SeekFrom::Start(0)).unwrap();
        let mbr = MBR::read_from(disk.as_file_mut(), TEST_SECTOR_BYTES).unwrap();
        assert_eq!(mbr[1].boot, BOOT_ACTIVE);
        assert_eq!(mbr[1].sys, 0x0c);
        assert_eq!(
            u64::from(mbr[1].starting_lba) * u64::from(TEST_SECTOR_BYTES),
            start
        );

        format_fat32(disk.as_file_mut(), start, end, TEST_SECTOR_BYTES).unwrap();
        {
            let partition = StreamSlice::new(disk.as_file_mut(), start, end).unwrap();
            let filesystem = FileSystem::new(BufStream::new(partition), FsOptions::new()).unwrap();
            let root = filesystem.root_dir();
            let efi = root.create_dir("EFI").unwrap();
            let mut marker = efi.create_file("MERAKI.TXT").unwrap();
            marker.write_all(b"fat32-ok").unwrap();
            write_unattend_file(&root, TEST_UNATTEND).unwrap();
            copy_directory(
                &mut empty_iso,
                &windows_tree,
                &root,
                "",
                Some(&split),
                "test-operation",
                &mut reporter,
                &mut tracker,
            )
            .unwrap();
            drop(marker);
            drop(efi);
            drop(root);
            filesystem.unmount().unwrap();
        }

        let partition = StreamSlice::new(disk.as_file_mut(), start, end).unwrap();
        let filesystem = FileSystem::new(BufStream::new(partition), FsOptions::new()).unwrap();
        let root = filesystem.root_dir();
        let efi = root.open_dir("EFI").unwrap();
        let mut marker = efi.open_file("MERAKI.TXT").unwrap();
        let mut contents = String::new();
        marker.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "fat32-ok");
        let mut unattend = root.open_file("autounattend.xml").unwrap();
        let mut xml = String::new();
        unattend.read_to_string(&mut xml).unwrap();
        assert_eq!(xml, TEST_UNATTEND);
        let sources = root.open_dir("sources").unwrap();
        let mut first = String::new();
        sources
            .open_file("install.swm")
            .unwrap()
            .read_to_string(&mut first)
            .unwrap();
        let mut second = String::new();
        sources
            .open_file("install2.swm")
            .unwrap()
            .read_to_string(&mut second)
            .unwrap();
        assert_eq!(first, "first-swm-part");
        assert_eq!(second, "second-swm-part");
        assert!(sources.open_file("install.wim").is_err());
        drop(reporter);
        let _messages = report_reader.join().unwrap();
    }
}
