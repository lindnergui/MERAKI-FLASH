use meraki_flash_core::UsbDevice;
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::{ffi::OsStr, ptr};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    DISK_GEOMETRY, FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_DISK_GET_DRIVE_GEOMETRY,
};
use windows_sys::Win32::UI::Shell::IsUserAnAdmin;

pub struct PreparedDevice {
    pub file: File,
    _volume_locks: Vec<File>,
}

pub fn is_elevated() -> bool {
    // SAFETY: IsUserAnAdmin não recebe ponteiros e apenas consulta o token atual.
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn prepare_device(device: &UsbDevice) -> Result<PreparedDevice, String> {
    if !device
        .device_path
        .to_ascii_lowercase()
        .starts_with(r"\\.\physicaldrive")
    {
        return Err("caminho de dispositivo Windows inválido".to_owned());
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&device.device_path)
        .map_err(|error| format!("não foi possível abrir {}: {error}", device.device_path))?;

    let mut volume_locks = Vec::new();
    for mount in &device.mount_points {
        let volume_path = raw_volume_path(mount)
            .ok_or_else(|| format!("ponto de montagem Windows inválido: {mount}"))?;
        volume_locks.push(lock_and_dismount_volume(&volume_path)?);
    }

    Ok(PreparedDevice {
        file,
        _volume_locks: volume_locks,
    })
}

pub fn logical_sector_size(file: &File) -> Result<u32, String> {
    let mut geometry = DISK_GEOMETRY::default();
    let mut returned = 0u32;
    // SAFETY: o handle pertence a `file`, o buffer tem exatamente o tamanho
    // de DISK_GEOMETRY e permanece válido durante DeviceIoControl.
    let success = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            IOCTL_DISK_GET_DRIVE_GEOMETRY,
            ptr::null(),
            0,
            (&mut geometry as *mut DISK_GEOMETRY).cast(),
            std::mem::size_of::<DISK_GEOMETRY>() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(format!(
            "não foi possível consultar o tamanho de setor do dispositivo: {}",
            std::io::Error::last_os_error()
        ));
    }
    let sector_size = geometry.BytesPerSector;
    ((512..=4096).contains(&sector_size) && sector_size.is_power_of_two())
        .then_some(sector_size)
        .ok_or_else(|| format!("tamanho de setor não suportado: {sector_size}"))
}

fn raw_volume_path(mount: &str) -> Option<String> {
    let bytes = mount.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        .then(|| format!(r"\\.\{}:", bytes[0] as char))
}

fn lock_and_dismount_volume(volume_path: &str) -> Result<File, String> {
    let wide = wide_string(volume_path);
    // SAFETY: wide é terminada em NUL, os demais ponteiros opcionais são nulos
    // e o handle é convertido exatamente uma vez em File ou fechado no erro.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!(
            "não foi possível bloquear {volume_path}: {}",
            std::io::Error::last_os_error()
        ));
    }

    for (control_code, action) in [
        (FSCTL_LOCK_VOLUME, "bloquear"),
        (FSCTL_DISMOUNT_VOLUME, "desmontar"),
    ] {
        let mut returned = 0u32;
        // SAFETY: handle é válido; estes FSCTLs não usam buffers de entrada/saída.
        let success = unsafe {
            DeviceIoControl(
                handle,
                control_code,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if success == 0 {
            let error = std::io::Error::last_os_error();
            // SAFETY: o handle ainda não foi transferido para File.
            unsafe { CloseHandle(handle) };
            return Err(format!(
                "não foi possível {action} {volume_path}: {error}. Feche aplicativos que estejam usando o pendrive"
            ));
        }
    }

    // SAFETY: File passa a ser o único proprietário do handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn wide_string(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
