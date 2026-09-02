use meraki_flash_core::UsbDevice;
use nix::errno::Errno;
use nix::mount::umount;
use nix::unistd::Uid;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub struct PreparedDevice {
    pub file: File,
}

pub fn is_elevated() -> bool {
    Uid::effective().is_root()
}

pub fn prepare_device(device: &UsbDevice) -> Result<PreparedDevice, String> {
    if !device.device_path.starts_with("/dev/") || device.device_path.contains("..") {
        return Err("caminho de dispositivo Linux inválido".to_owned());
    }
    if device.mount_points.iter().any(|mount| mount == "/") {
        return Err("o disco que contém o sistema operacional foi bloqueado".to_owned());
    }

    // Abrir antes da desmontagem fixa o descritor no mesmo dispositivo físico;
    // nenhum byte é escrito até todos os volumes terem sido desmontados.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&device.device_path)
        .map_err(|error| format!("não foi possível abrir {}: {error}", device.device_path))?;

    let mut mounts = device.mount_points.clone();
    mounts.sort_by_key(|mount| std::cmp::Reverse(Path::new(mount).components().count()));
    for mount in mounts {
        match umount(Path::new(&mount)) {
            Ok(()) | Err(Errno::EINVAL) => {}
            Err(error) => {
                return Err(format!(
                    "não foi possível desmontar {mount}: {error}. Feche arquivos e janelas que estejam usando o pendrive"
                ));
            }
        }
    }

    Ok(PreparedDevice { file })
}

pub fn logical_sector_size(file: &File) -> Result<u32, String> {
    let mut sector_size = 0i32;
    // SAFETY: BLKSSZGET escreve um inteiro no ponteiro fornecido e o descritor
    // permanece válido durante a chamada.
    if unsafe { libc::ioctl(file.as_raw_fd(), libc::BLKSSZGET, &mut sector_size) } != 0 {
        return Err(format!(
            "não foi possível consultar o tamanho de setor do dispositivo: {}",
            std::io::Error::last_os_error()
        ));
    }
    u32::try_from(sector_size)
        .ok()
        .filter(|size| (512..=4096).contains(size) && size.is_power_of_two())
        .ok_or_else(|| format!("tamanho de setor não suportado: {sector_size}"))
}
