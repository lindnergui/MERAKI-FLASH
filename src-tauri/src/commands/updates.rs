const RELEASES_URL: &str = "https://github.com/lindnergui/MERAKI-FLASH/releases/latest";

#[tauri::command]
pub async fn open_releases_page() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(open_page)
        .await
        .map_err(|error| error.to_string())?
}

fn open_page() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::ptr;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = RELEASES_URL.encode_utf16().chain(Some(0)).collect();
        // SAFETY: strings UTF-16 válidas e terminadas em NUL; URL fixa do projeto.
        let result = unsafe {
            ShellExecuteW(ptr::null_mut(), verb.as_ptr(), url.as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL)
        } as isize;
        if result <= 32 { return Err("não foi possível abrir o navegador".to_owned()); }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("xdg-open").arg(RELEASES_URL).status()
            .map_err(|error| format!("não foi possível abrir o navegador: {error}"))?;
        if status.success() { Ok(()) } else { Err("não foi possível abrir o navegador".to_owned()) }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    Err("plataforma não suportada".to_owned())
}
