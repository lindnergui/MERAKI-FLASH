use crate::flash::HELPER_FLAG;

#[cfg(target_os = "linux")]
pub async fn launch_elevated(encoded_request: String) -> Result<i32, String> {
    use tokio::process::Command;

    let executable = std::env::current_exe()
        .map_err(|error| format!("não foi possível localizar o executável: {error}"))?;
    let output = Command::new("pkexec")
        .arg(executable)
        .arg(HELPER_FLAG)
        .arg(encoded_request)
        .kill_on_drop(false)
        .output()
        .await
        .map_err(|error| {
            format!(
                "não foi possível iniciar pkexec: {error}. Verifique se o polkit está instalado e se há um agente de autenticação ativo"
            )
        })?;

    if output.status.success() {
        return Ok(0);
    }

    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let message = match code {
        126 => "a autenticação foi cancelada pelo usuário".to_owned(),
        127 => "a autorização administrativa foi recusada".to_owned(),
        _ if stderr.is_empty() => format!("o helper elevado terminou com o código {code}"),
        _ => stderr,
    };
    Err(message)
}

#[cfg(target_os = "windows")]
pub async fn launch_elevated(encoded_request: String) -> Result<i32, String> {
    tokio::task::spawn_blocking(move || launch_elevated_blocking(&encoded_request))
        .await
        .map_err(|error| format!("falha ao aguardar o helper elevado: {error}"))?
}

#[cfg(target_os = "windows")]
fn launch_elevated_blocking(encoded_request: &str) -> Result<i32, String> {
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_FAILED};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let executable = std::env::current_exe()
        .map_err(|error| format!("não foi possível localizar o executável: {error}"))?;
    let executable = wide(executable.as_os_str());
    let verb = wide(OsStr::new("runas"));
    let parameters = wide(OsStr::new(&format!("{HELPER_FLAG} {encoded_request}")));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: ptr::null_mut(),
        lpVerb: verb.as_ptr(),
        lpFile: executable.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: ptr::null(),
        nShow: SW_HIDE,
        hInstApp: ptr::null_mut(),
        lpIDList: ptr::null_mut(),
        lpClass: ptr::null(),
        hkeyClass: ptr::null_mut(),
        dwHotKey: 0,
        Anonymous: Default::default(),
        hProcess: ptr::null_mut(),
    };

    // SAFETY: todas as strings são UTF-16 terminadas em NUL e permanecem vivas
    // durante a chamada. A estrutura tem o tamanho correto para esta arquitetura.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(1223) {
            "a solicitação do UAC foi cancelada".to_owned()
        } else {
            format!("não foi possível abrir o helper com UAC: {error}")
        });
    }
    if info.hProcess.is_null() {
        return Err("o Windows não retornou o processo elevado".to_owned());
    }

    // SAFETY: hProcess é um handle de processo válido retornado por ShellExecuteExW.
    let wait_result = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    if wait_result == WAIT_FAILED {
        let error = std::io::Error::last_os_error();
        // SAFETY: o handle ainda pertence a esta função.
        unsafe { CloseHandle(info.hProcess) };
        return Err(format!("falha ao aguardar o helper elevado: {error}"));
    }

    let mut exit_code = 1u32;
    // SAFETY: hProcess permanece válido até CloseHandle abaixo.
    let got_exit_code = unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) };
    // SAFETY: esta é a única liberação do handle.
    unsafe { CloseHandle(info.hProcess) };
    if got_exit_code == 0 {
        return Err(format!(
            "não foi possível obter o resultado do helper: {}",
            std::io::Error::last_os_error()
        ));
    }
    if exit_code == 0 {
        Ok(0)
    } else {
        Err(format!(
            "o helper elevado terminou com o código {exit_code}"
        ))
    }
}

#[cfg(target_os = "windows")]
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub async fn launch_elevated(_encoded_request: String) -> Result<i32, String> {
    Err("elevação disponível apenas no Linux e Windows".to_owned())
}
