#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{is_elevated, logical_sector_size, prepare_device};
#[cfg(target_os = "windows")]
pub use windows::{is_elevated, logical_sector_size, prepare_device};

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!("o motor de gravação suporta apenas Linux e Windows");
