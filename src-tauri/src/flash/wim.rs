use std::path::Path;

#[cfg(target_os = "linux")]
use wimlib_sys as wim_sys;

#[cfg(target_os = "linux")]
pub fn split(input_path: &Path, output_path: &Path, part_bytes: u64) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;

    let mut input = input_path.as_os_str().as_bytes().to_vec();
    input.push(0);
    let mut output = output_path.as_os_str().as_bytes().to_vec();
    output.push(0);
    perform_split(input.as_ptr().cast(), output.as_ptr().cast(), part_bytes)
}

#[cfg(target_os = "linux")]
fn perform_split(
    input: *const wim_sys::wimlib_tchar,
    output: *const wim_sys::wimlib_tchar,
    part_bytes: u64,
) -> Result<(), String> {
    // SAFETY: esta operação é serializada pelo FlashManager. Os caminhos são
    // strings terminadas em NUL e permanecem vivos até a função retornar.
    let init_status = unsafe { wim_sys::wimlib_global_init(0) };
    if init_status != 0 {
        return Err(format!(
            "não foi possível inicializar wimlib: {}",
            error_message(init_status)
        ));
    }
    let _global = WimGlobalGuard;

    let mut image = std::ptr::null_mut();
    // SAFETY: `input` é válido e `image` aponta para memória gravável.
    let open_status = unsafe { wim_sys::wimlib_open_wim(input, 0, &mut image) };
    if open_status != 0 {
        return Err(format!(
            "não foi possível abrir install.wim: {}",
            error_message(open_status)
        ));
    }
    let image = WimImageGuard(image);
    // SAFETY: o WIM foi aberto com sucesso e `output` é uma string válida.
    let split_status = unsafe { wim_sys::wimlib_split(image.0, output, part_bytes, 0) };
    if split_status != 0 {
        let system_detail = relevant_write_error()
            .map(|error| format!("; sistema: {error}"))
            .unwrap_or_default();
        return Err(format!(
            "não foi possível dividir install.wim (wimlib {split_status}): {}{system_detail}",
            error_message(split_status),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct WimGlobalGuard;

#[cfg(target_os = "linux")]
impl Drop for WimGlobalGuard {
    fn drop(&mut self) {
        // SAFETY: corresponde à inicialização bem-sucedida desta thread.
        unsafe { wim_sys::wimlib_global_cleanup() };
    }
}

#[cfg(target_os = "linux")]
struct WimImageGuard(*mut wim_sys::WIMStruct);

#[cfg(target_os = "linux")]
impl Drop for WimImageGuard {
    fn drop(&mut self) {
        // SAFETY: o ponteiro foi retornado por wimlib_open_wim e é liberado uma vez.
        unsafe { wim_sys::wimlib_free(self.0) };
    }
}

#[cfg(target_os = "linux")]
fn error_message(code: i32) -> String {
    // SAFETY: wimlib retorna uma string estática terminada em NUL.
    let pointer = unsafe { wim_sys::wimlib_get_error_string(code as _) };
    if pointer.is_null() {
        return format!("erro {code}");
    }
    // SAFETY: validade garantida pelo contrato de wimlib_get_error_string.
    unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(target_os = "linux")]
fn relevant_write_error() -> Option<String> {
    let error = std::io::Error::last_os_error();
    let code = error.raw_os_error()?;
    [
        libc::ENOSPC,
        libc::EDQUOT,
        libc::EFBIG,
        libc::EACCES,
        libc::EROFS,
        libc::EIO,
    ]
    .contains(&code)
    .then(|| format!("{error} (errno {code})"))
}

#[cfg(target_os = "windows")]
pub fn split(input_path: &Path, output_path: &Path, part_bytes: u64) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{FreeLibrary, GENERIC_READ, HANDLE, HMODULE};
    use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };

    type WimCreateFile =
        unsafe extern "system" fn(*const u16, u32, u32, u32, u32, *mut u32) -> HANDLE;
    type WimSplitFile = unsafe extern "system" fn(HANDLE, *const u16, *mut i64, u32) -> i32;
    type WimCloseHandle = unsafe extern "system" fn(HANDLE) -> i32;

    struct WimApi {
        module: HMODULE,
        create_file: WimCreateFile,
        split_file: WimSplitFile,
        close_handle: WimCloseHandle,
    }

    impl WimApi {
        fn load() -> Result<Self, String> {
            let library = "wimgapi.dll"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            // SAFETY: o nome é UTF-16 terminado em NUL e restringimos a busca
            // ao System32 para impedir carregamento de DLL a partir do projeto.
            let module = unsafe {
                LoadLibraryExW(
                    library.as_ptr(),
                    std::ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            };
            if module.is_null() {
                return Err(format!(
                    "não foi possível carregar a API de imagens do Windows: {}",
                    std::io::Error::last_os_error()
                ));
            }

            macro_rules! load_function {
                ($name:literal, $kind:ty) => {{
                    // SAFETY: `module` permanece carregado durante toda a vida
                    // de WimApi e cada assinatura corresponde a wimgapi.h.
                    let function = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) };
                    let Some(function) = function else {
                        // SAFETY: nenhum ponteiro de função foi usado ainda.
                        unsafe { FreeLibrary(module) };
                        return Err(format!(
                            "a função {} não está disponível em wimgapi.dll",
                            $name
                        ));
                    };
                    // SAFETY: a assinatura concreta acima corresponde à
                    // função exportada com convenção de chamada `system`.
                    unsafe {
                        std::mem::transmute::<unsafe extern "system" fn() -> isize, $kind>(function)
                    }
                }};
            }

            Ok(Self {
                module,
                create_file: load_function!("WIMCreateFile", WimCreateFile),
                split_file: load_function!("WIMSplitFile", WimSplitFile),
                close_handle: load_function!("WIMCloseHandle", WimCloseHandle),
            })
        }
    }

    impl Drop for WimApi {
        fn drop(&mut self) {
            // SAFETY: o módulo foi carregado uma vez por WimApi::load.
            unsafe { FreeLibrary(self.module) };
        }
    }

    struct WimHandle {
        raw: HANDLE,
        close: WimCloseHandle,
    }

    impl Drop for WimHandle {
        fn drop(&mut self) {
            // SAFETY: o handle foi retornado por WIMCreateFile e é fechado uma vez.
            unsafe { (self.close)(self.raw) };
        }
    }

    let api = WimApi::load()?;
    let input = input_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let output = output_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: os caminhos são UTF-16 terminados em NUL; os flags abrem um WIM
    // existente somente para leitura e o resultado opcional é nulo.
    let handle = unsafe {
        (api.create_file)(
            input.as_ptr(),
            GENERIC_READ,
            OPEN_EXISTING,
            0,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() {
        return Err(format!(
            "não foi possível abrir install.wim com a API do Windows: {}",
            std::io::Error::last_os_error()
        ));
    }
    let handle = WimHandle {
        raw: handle,
        close: api.close_handle,
    };
    let mut part_size = i64::try_from(part_bytes)
        .map_err(|_| "o tamanho solicitado para o SWM é inválido".to_owned())?;
    // SAFETY: o handle e os ponteiros permanecem válidos durante a chamada;
    // WIMSplitFile recebe LARGE_INTEGER por ponteiro e flags reservados zero.
    let success = unsafe { (api.split_file)(handle.raw, output.as_ptr(), &mut part_size, 0) };
    if success == 0 {
        return Err(format!(
            "não foi possível dividir install.wim com a API do Windows: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;

    fn c_path(path: &Path) -> Vec<u8> {
        let mut value = path.as_os_str().as_bytes().to_vec();
        value.push(0);
        value
    }

    fn assert_wim_status(status: i32) {
        assert_eq!(status, 0, "wimlib: {}", error_message(status));
    }

    #[test]
    fn bundled_wimlib_is_linked() {
        // SAFETY: esta função apenas retorna a versão numérica da biblioteca.
        let version = unsafe { wim_sys::wimlib_get_version() };
        assert_ne!(version, 0);
    }

    #[test]
    fn creates_and_splits_a_real_wim() {
        let temporary = tempfile::tempdir().unwrap();
        let source_directory = temporary.path().join("payload");
        std::fs::create_dir(&source_directory).unwrap();
        for index in 0..3u8 {
            let path = source_directory.join(format!("part-{index}.bin"));
            let mut file = std::fs::File::create(path).unwrap();
            let data = (0..(512 * 1024))
                .map(|offset| index.wrapping_add((offset % 251) as u8))
                .collect::<Vec<_>>();
            file.write_all(&data).unwrap();
        }

        let input_wim = temporary.path().join("install.wim");
        let output_swm = temporary.path().join("install.swm");
        let source = c_path(&source_directory);
        let output = c_path(&input_wim);
        let image_name = b"Meraki test\0";

        // SAFETY: todos os caminhos e o nome são terminados em NUL; os guards
        // mantêm a ordem correta entre WIMStruct e o estado global da biblioteca.
        unsafe {
            assert_wim_status(wim_sys::wimlib_global_init(0));
            let global = WimGlobalGuard;
            let mut image = std::ptr::null_mut();
            assert_wim_status(wim_sys::wimlib_create_new_wim(
                wim_sys::wimlib_compression_type_WIMLIB_COMPRESSION_TYPE_NONE,
                &mut image,
            ));
            let image = WimImageGuard(image);
            assert_wim_status(wim_sys::wimlib_add_image(
                image.0,
                source.as_ptr().cast(),
                image_name.as_ptr().cast(),
                std::ptr::null(),
                0,
            ));
            assert_wim_status(wim_sys::wimlib_write(
                image.0,
                output.as_ptr().cast(),
                wim_sys::WIMLIB_ALL_IMAGES,
                0,
                1,
            ));
            drop(image);
            drop(global);
        }

        split(&input_wim, &output_swm, 700_000).unwrap();

        assert!(output_swm.is_file());
        assert!(temporary.path().join("install2.swm").is_file());
        let parts = std::fs::read_dir(temporary.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("swm"))
            })
            .count();
        assert!(parts >= 2);
    }
}
