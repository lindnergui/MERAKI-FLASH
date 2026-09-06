mod commands;
mod elevation;
mod flash;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(flash::FlashManager::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::usb::list_usb_devices,
            commands::flash::start_flash,
            commands::updates::open_releases_page
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Meraki Flash");
}

pub fn elevated_helper_from_args() -> Option<i32> {
    let mut arguments = std::env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next()) {
        (Some(flash::HELPER_FLAG), Some(request)) => Some(flash::run_elevated_helper(&request)),
        _ => None,
    }
}
