#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Mantém o executável desktop mínimo. A composição da aplicação vive em lib.rs,
// o que também permite reutilizar a mesma base nos targets móveis do Tauri.
fn main() {
    if let Some(exit_code) = meraki_flash_lib::elevated_helper_from_args() {
        std::process::exit(exit_code);
    }
    meraki_flash_lib::run();
}
