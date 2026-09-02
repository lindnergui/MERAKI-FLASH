use meraki_flash_core::{UsbDevice, discover_removable_devices};

/// Ponte fina entre a interface Tauri e a crate de domínio testável.
#[tauri::command]
pub async fn list_usb_devices() -> Result<Vec<UsbDevice>, String> {
    tauri::async_runtime::spawn_blocking(discover_removable_devices)
        .await
        .map_err(|error| format!("falha ao consultar dispositivos: {error}"))?
        .map_err(|error| error.to_string())
}
