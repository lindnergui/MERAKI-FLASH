mod platform;
mod sector_io;
pub mod protocol;
mod wim;
mod windows_iso;

use crate::elevation;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use meraki_flash_core::{UsbDevice, find_removable_device, validate_iso_for_device};
use protocol::{
    ElevatedFlashRequest, FlashPhase, FlashProgress, HelperEnvelope, ImageKind, StartFlashRequest,
    StartFlashResponse,
};
use std::fs::File;
use std::io::{BufReader as StdBufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream as StdTcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

pub const HELPER_FLAG: &str = "--meraki-flash-elevated-helper";
const PROGRESS_EVENT: &str = "flash-progress";
const BUFFER_SIZE: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct FlashManager {
    running: AtomicBool,
}

impl FlashManager {
    pub fn begin(&self) -> Result<(), String> {
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "já existe uma gravação em andamento".to_owned())
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::Release);
    }
}

pub async fn start_flash(
    app: AppHandle,
    manager: &FlashManager,
    request: StartFlashRequest,
) -> Result<StartFlashResponse, String> {
    let device_id = request.device_id.clone();
    let device = tauri::async_runtime::spawn_blocking(move || find_removable_device(&device_id))
        .await
        .map_err(|error| format!("falha ao consultar o dispositivo: {error}"))?
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "o dispositivo selecionado não está mais conectado ou não é removível".to_owned()
        })?;

    if device.read_only {
        return Err("o dispositivo selecionado está protegido contra gravação".to_owned());
    }
    let (iso_path, iso_size) = validate_iso_for_device(&request.iso_path, &device)?;
    let image_kind = request.image_kind;
    let unattend_xml_content =
        validate_unattend_for_image(image_kind, request.unattend_xml_content.as_deref())?;
    manager.begin()?;

    let operation_id = Uuid::new_v4().to_string();
    let response = StartFlashResponse {
        operation_id: operation_id.clone(),
    };
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_flash_operation(
            &app_for_task,
            device,
            iso_path.to_string_lossy().into_owned(),
            iso_size,
            image_kind,
            unattend_xml_content,
            operation_id.clone(),
        )
        .await;

        app_for_task.state::<FlashManager>().finish();
        let terminal = result.unwrap_or_else(|message| FlashProgress::new(
            operation_id, FlashPhase::Error, 0.0, 0.0, None, Some(message),
        ));
        emit_progress(&app_for_task, terminal);
    });

    Ok(response)
}

async fn run_flash_operation(
    app: &AppHandle,
    device: UsbDevice,
    iso_path: String,
    iso_size: u64,
    image_kind: ImageKind,
    unattend_xml_content: Option<String>,
    operation_id: String,
) -> Result<FlashProgress, String> {
    emit_progress(
        app,
        FlashProgress::new(
            operation_id.clone(),
            FlashPhase::Preparing,
            0.0,
            0.0,
            None,
            Some("Aguardando autorização administrativa…".to_owned()),
        ),
    );

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("não foi possível abrir o canal de progresso: {error}"))?;
    let callback_port = listener
        .local_addr()
        .map_err(|error| format!("não foi possível configurar o canal de progresso: {error}"))?
        .port();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let helper_request = ElevatedFlashRequest {
        operation_id: operation_id.clone(),
        token: token.clone(),
        callback_port,
        iso_path,
        iso_size,
        device_id: device.id,
        device_path: device.device_path,
        device_size: device.total_bytes,
        device_serial: device.serial,
        image_kind,
        unattend_xml_content,
    };
    let encoded = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&helper_request)
            .map_err(|error| format!("não foi possível preparar o helper: {error}"))?,
    );

    let mut elevation = Box::pin(elevation::launch_elevated(encoded));
    let mut connection = Box::pin(tokio::time::timeout(
        Duration::from_secs(120),
        accept_authenticated(&listener, &token, &operation_id),
    ));
    let (reader, first_progress) = tokio::select! {
        biased;
        accepted = &mut connection => accepted
            .map_err(|_| "a autenticação administrativa expirou".to_owned())??,
        elevated = &mut elevation => {
            elevated?;
            return Err("o helper terminou antes de abrir o canal de progresso".to_owned());
        }
    };

    let forwarded = forward_progress(app, reader, first_progress, &token, &operation_id).await;
    let helper_result = elevation.await;
    let terminal = forwarded?;
    match (helper_result, terminal.phase) {
        (Ok(0), FlashPhase::Done) | (_, FlashPhase::Error) => Ok(terminal),
        (Err(error), _) => Err(error),
        (Ok(code), _) => Err(format!("o helper terminou com o código {code}")),
    }
}

async fn accept_authenticated(
    listener: &TcpListener,
    token: &str,
    operation_id: &str,
) -> Result<(BufReader<TcpStream>, FlashProgress), String> {
    loop {
        let (stream, address) = listener
            .accept()
            .await
            .map_err(|error| format!("falha no canal de progresso: {error}"))?;
        if !address.ip().is_loopback() {
            continue;
        }

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let bytes = match tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut line)).await {
            Ok(Ok(bytes)) => bytes,
            _ => continue,
        };
        if bytes == 0 {
            continue;
        }
        if let Ok(envelope) = serde_json::from_str::<HelperEnvelope>(&line)
            && envelope.token == token
            && envelope.progress.operation_id == operation_id
        {
            return Ok((reader, envelope.progress));
        }
    }
}

async fn forward_progress(
    app: &AppHandle,
    mut reader: BufReader<TcpStream>,
    first_progress: FlashProgress,
    token: &str,
    operation_id: &str,
) -> Result<FlashProgress, String> {
    let mut terminal = first_progress;
    if !matches!(terminal.phase, FlashPhase::Done | FlashPhase::Error) {
        emit_progress(app, terminal.clone());
    }

    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|error| format!("o canal de progresso foi interrompido: {error}"))?;
        if bytes == 0 {
            break;
        }
        let envelope: HelperEnvelope = serde_json::from_str(&line)
            .map_err(|error| format!("mensagem inválida do helper: {error}"))?;
        if envelope.token != token || envelope.progress.operation_id != operation_id {
            return Err("o helper enviou uma mensagem não autenticada".to_owned());
        }
        terminal = envelope.progress;
        if !matches!(terminal.phase, FlashPhase::Done | FlashPhase::Error) {
            emit_progress(app, terminal.clone());
        }
    }
    Ok(terminal)
}

fn emit_progress(app: &AppHandle, progress: FlashProgress) {
    let _ = app.emit(PROGRESS_EVENT, progress);
}

pub fn run_elevated_helper(encoded_request: &str) -> i32 {
    let request = match URL_SAFE_NO_PAD
        .decode(encoded_request)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<ElevatedFlashRequest>(&bytes).map_err(|e| e.to_string())
        }) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("solicitação inválida para o helper: {error}");
            return 2;
        }
    };

    let mut reporter = match Reporter::connect(&request) {
        Ok(reporter) => reporter,
        Err(error) => {
            eprintln!("não foi possível conectar ao aplicativo: {error}");
            return 3;
        }
    };
    if let Err(error) = execute_elevated_flash(&request, &mut reporter) {
        let percentage = reporter.last_percentage();
        let _ = reporter.send(FlashProgress::new(
            request.operation_id.clone(),
            FlashPhase::Error,
            percentage,
            0.0,
            None,
            Some(error),
        ));
        return 1;
    }
    0
}

fn execute_elevated_flash(
    request: &ElevatedFlashRequest,
    reporter: &mut Reporter,
) -> Result<(), String> {
    if !platform::is_elevated() {
        return Err("o helper não recebeu privilégios administrativos".to_owned());
    }
    reporter.send(FlashProgress::new(
        request.operation_id.clone(),
        FlashPhase::Preparing,
        0.0,
        0.0,
        None,
        Some("Validando novamente o dispositivo…".to_owned()),
    ))?;

    let device = revalidate_device(request)?;

    let (iso_path, iso_size) = validate_iso_for_device(&request.iso_path, &device)?;
    if iso_size != request.iso_size {
        return Err("o tamanho da ISO mudou depois da confirmação".to_owned());
    }
    let mut source =
        File::open(&iso_path).map_err(|error| format!("não foi possível abrir a ISO: {error}"))?;
    let unattend_xml_content =
        validate_unattend_for_image(request.image_kind, request.unattend_xml_content.as_deref())?;

    match request.image_kind {
        ImageKind::Linux => {
            validate_hybrid_image(&mut source)?;
            reporter.send(FlashProgress::new(
                request.operation_id.clone(),
                FlashPhase::Preparing,
                0.0,
                0.0,
                None,
                Some("Desmontando e bloqueando o dispositivo…".to_owned()),
            ))?;
            let mut prepared = platform::prepare_device(&device)?;
            let sector_size = platform::logical_sector_size(&prepared.file)?;
            write_image(
                source,
                &mut prepared.file,
                iso_size,
                (sector_size, device.total_bytes),
                &request.operation_id,
                reporter,
            )?;
            drop(prepared);
        }
        ImageKind::Windows => {
            let image = windows_iso::prepare(
                source,
                &iso_path,
                &device,
                unattend_xml_content,
                &request.operation_id,
                reporter,
            )?;
            // Dividir WIMs grandes pode levar minutos. A identidade física é
            // conferida novamente imediatamente antes da primeira escrita.
            let device = revalidate_device(request)?;
            windows_iso::write(image, &device, &request.operation_id, reporter)?;
        }
    }

    let _ = reporter.send(FlashProgress::new(
        request.operation_id.clone(),
        FlashPhase::Done,
        100.0,
        0.0,
        Some(0),
        Some("Pendrive gravado com sucesso. Já é seguro removê-lo.".to_owned()),
    ));
    Ok(())
}

fn validate_unattend_for_image(
    image_kind: ImageKind,
    content: Option<&str>,
) -> Result<Option<String>, String> {
    match (image_kind, content) {
        (ImageKind::Linux, Some(_)) => {
            Err("autounattend.xml só pode ser usado com imagens do Windows".to_owned())
        }
        (ImageKind::Linux, None) => Ok(None),
        (ImageKind::Windows, content) => windows_iso::validate_unattend_xml(content),
    }
}

fn revalidate_device(request: &ElevatedFlashRequest) -> Result<UsbDevice, String> {
    let device = find_removable_device(&request.device_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "o dispositivo removível não está mais conectado".to_owned())?;
    if device.device_path != request.device_path
        || device.total_bytes != request.device_size
        || device.serial != request.device_serial
    {
        return Err(
            "a identidade física do dispositivo mudou; a operação foi cancelada".to_owned(),
        );
    }
    if device.read_only {
        return Err("o dispositivo está protegido contra gravação".to_owned());
    }
    Ok(device)
}

fn write_image(
    source: File,
    target_file: &mut File,
    total_bytes: u64,
    geometry: (u32, u64),
    operation_id: &str,
    reporter: &mut Reporter,
) -> Result<(), String> {
    let mut source = StdBufReader::with_capacity(BUFFER_SIZE, source);
    let mut target = sector_io::SectorIo::new(&mut *target_file, geometry.0, geometry.1)
        .map_err(|error| error.to_string())?;
    target
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("não foi possível posicionar o dispositivo: {error}"))?;

    let started = Instant::now();
    let mut last_report = Instant::now();
    let mut written = 0u64;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let _ = reporter.send(FlashProgress::new(
        operation_id,
        FlashPhase::Writing,
        0.0,
        0.0,
        None,
        Some("Gravando a imagem bit a bit…".to_owned()),
    ));

    while written < total_bytes {
        let wanted = usize::try_from((total_bytes - written).min(BUFFER_SIZE as u64))
            .map_err(|_| "tamanho de bloco inválido".to_owned())?;
        source
            .read_exact(&mut buffer[..wanted])
            .map_err(|error| format!("falha ao ler a ISO: {error}"))?;
        target
            .write_all(&buffer[..wanted])
            .map_err(|error| format!("falha ao escrever no dispositivo: {error}"))?;
        written += wanted as u64;

        if last_report.elapsed() >= Duration::from_millis(250) || written == total_bytes {
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let speed = written as f64 / elapsed;
            let eta = (speed > 0.0).then(|| ((total_bytes - written) as f64 / speed).ceil() as u64);
            let percentage = written as f64 * 100.0 / total_bytes as f64;
            let _ = reporter.send(FlashProgress::new(
                operation_id,
                FlashPhase::Writing,
                percentage,
                speed,
                eta,
                None,
            ));
            last_report = Instant::now();
        }
    }

    let _ = reporter.send(FlashProgress::new(
        operation_id,
        FlashPhase::Syncing,
        100.0,
        0.0,
        None,
        Some("Sincronizando os dados com o dispositivo…".to_owned()),
    ));
    let _ = target.finish().map_err(|error| error.to_string())?;
    target_file.sync_all()
        .map_err(|error| format!("falha ao sincronizar o dispositivo: {error}"))?;
    let mut target = sector_io::SectorIo::new(target_file, geometry.0, geometry.1)
        .map_err(|error| error.to_string())?;
    source.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
    let _ = reporter.send(FlashProgress::new(operation_id, FlashPhase::Verifying,
        0.0, 0.0, None, Some("Conferindo os dados gravados…".to_owned())));
    let mut verified = 0u64;
    let mut actual = vec![0; BUFFER_SIZE];
    while verified < total_bytes {
        let count = (total_bytes - verified).min(BUFFER_SIZE as u64) as usize;
        source.read_exact(&mut buffer[..count]).map_err(|error| format!("falha ao reler a ISO: {error}"))?;
        target.read_exact(&mut actual[..count]).map_err(|error| format!("falha ao verificar o dispositivo: {error}"))?;
        if buffer[..count] != actual[..count] {
            return Err(format!("a verificação encontrou dados diferentes no dispositivo a partir do byte {verified}; grave novamente antes de usá-lo"));
        }
        verified += count as u64;
        if last_report.elapsed() >= Duration::from_millis(250) || verified == total_bytes {
            let _ = reporter.send(FlashProgress::new(operation_id, FlashPhase::Verifying,
                verified as f64 * 100.0 / total_bytes as f64, 0.0, None,
                Some("Conferindo os dados gravados…".to_owned())));
            last_report = Instant::now();
        }
    }
    Ok(())
}

struct Reporter {
    stream: StdTcpStream,
    token: String,
    last_percentage: f64,
}

impl Reporter {
    fn connect(request: &ElevatedFlashRequest) -> Result<Self, String> {
        let stream = StdTcpStream::connect_timeout(
            &format!("127.0.0.1:{}", request.callback_port)
                .parse()
                .map_err(|error| format!("endereço de retorno inválido: {error}"))?,
            Duration::from_secs(10),
        )
        .map_err(|error| format!("falha ao abrir o canal de progresso: {error}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| format!("falha ao configurar o canal de progresso: {error}"))?;
        Ok(Self {
            stream,
            token: request.token.clone(),
            last_percentage: 0.0,
        })
    }

    fn send(&mut self, progress: FlashProgress) -> Result<(), String> {
        self.last_percentage = progress.percentage;
        serde_json::to_writer(
            &mut self.stream,
            &HelperEnvelope {
                token: self.token.clone(),
                progress,
            },
        )
        .map_err(|error| format!("falha ao serializar o progresso: {error}"))?;
        self.stream
            .write_all(b"\n")
            .and_then(|_| self.stream.flush())
            .map_err(|error| format!("falha ao enviar o progresso: {error}"))
    }

    fn last_percentage(&self) -> f64 {
        self.last_percentage
    }
}

fn validate_hybrid_image(source: &mut File) -> Result<(), String> {
    let mut header = [0u8; 512];
    source.seek(SeekFrom::Start(0)).and_then(|_| source.read_exact(&mut header))
        .map_err(|error| format!("não foi possível validar a imagem híbrida: {error}"))?;
    source.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
    if header[510..] != [0x55, 0xaa] || !header[446..510].as_chunks::<16>().0.iter()
        .any(|entry| entry[4] != 0 && entry[12..16] != [0, 0, 0, 0]) {
        return Err("a imagem Linux não contém uma tabela de partições híbrida inicializável; confira a ISO e o sistema selecionado".to_owned());
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::{
        ElevatedFlashRequest, FlashPhase, HelperEnvelope, ImageKind, Reporter, write_image,
    };
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::net::TcpListener;
    use std::thread;
    use tempfile::{Builder, NamedTempFile};

    #[test]
    fn rejects_non_hybrid_images_before_destructive_work() {
        let mut iso = NamedTempFile::new().unwrap();
        iso.write_all(&[0; 512]).unwrap();
        assert!(super::validate_hybrid_image(iso.as_file_mut()).is_err());
        let mut header = [0u8; 512];
        header[510..].copy_from_slice(&[0x55, 0xaa]);
        header[450] = 0x83;
        header[458] = 1;
        iso.seek(SeekFrom::Start(0)).unwrap();
        iso.write_all(&header).unwrap();
        super::validate_hybrid_image(iso.as_file_mut()).unwrap();
        assert_eq!(iso.stream_position().unwrap(), 0);
    }

    #[test]
    fn copies_an_image_exactly_and_reports_completion() {
        let payload = (0..(1024 * 1024 + 137))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let mut source = Builder::new().suffix(".iso").tempfile().unwrap();
        source.write_all(&payload).unwrap();
        source.flush().unwrap();
        let mut target = NamedTempFile::new().unwrap();
        target.as_file_mut().set_len((payload.len() as u64).div_ceil(512) * 512).unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let callback_port = listener.local_addr().unwrap().port();
        let reader = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut messages = String::new();
            stream.read_to_string(&mut messages).unwrap();
            messages
        });
        let request = ElevatedFlashRequest {
            operation_id: "test-operation".to_owned(),
            token: "test-token".to_owned(),
            callback_port,
            iso_path: source.path().to_string_lossy().into_owned(),
            iso_size: payload.len() as u64,
            device_id: "test-device".to_owned(),
            device_path: "test-target".to_owned(),
            device_size: payload.len() as u64,
            device_serial: None,
            image_kind: ImageKind::Linux,
            unattend_xml_content: None,
        };
        let mut reporter = Reporter::connect(&request).unwrap();
        let source_file = source.reopen().unwrap();
        let mut target_file = target.reopen().unwrap();

        write_image(
            source_file,
            &mut target_file,
            payload.len() as u64,
            (512, (payload.len() as u64).div_ceil(512) * 512),
            &request.operation_id,
            &mut reporter,
        )
        .unwrap();
        drop(reporter);

        target_file.seek(SeekFrom::Start(0)).unwrap();
        let mut copied = Vec::new();
        target_file.read_to_end(&mut copied).unwrap();
        assert_eq!(&copied[..payload.len()], payload.as_slice());
        assert!(copied[payload.len()..].iter().all(|byte| *byte == 0));

        let messages = reader.join().unwrap();
        let envelopes = messages
            .lines()
            .map(|line| serde_json::from_str::<HelperEnvelope>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(envelopes.iter().any(|event| event.progress.phase == FlashPhase::Verifying && event.progress.percentage == 100.0));
        assert!(envelopes.iter().any(|event| {
            event.progress.phase == FlashPhase::Writing && event.progress.percentage == 100.0
        }));
        assert!(
            envelopes
                .iter()
                .any(|event| event.progress.phase == FlashPhase::Syncing)
        );
    }
}
