use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// Disco físico removível que pode ser apresentado ao frontend.
///
/// `id` é adequado para seleção, mas o helper elevado ainda precisa revalidar
/// caminho, capacidade e serial imediatamente antes de abrir o bloco bruto.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsbDevice {
    pub id: String,
    pub name: String,
    pub device_path: String,
    pub mount_point: String,
    pub mount_points: Vec<String>,
    pub file_system: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub read_only: bool,
    pub kind: String,
    pub transport: String,
    pub serial: Option<String>,
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("não foi possível executar {program}: {source}")]
    Command {
        program: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} retornou erro: {message}")]
    CommandFailed {
        program: &'static str,
        message: String,
    },
    #[error("resposta inválida de {program}: {source}")]
    InvalidOutput {
        program: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("a descoberta segura de USB não é suportada nesta plataforma")]
    UnsupportedPlatform,
}

pub fn discover_removable_devices() -> Result<Vec<UsbDevice>, DiscoveryError> {
    #[cfg(target_os = "linux")]
    {
        return linux::discover_removable_devices();
    }

    #[cfg(target_os = "windows")]
    {
        return windows::discover_removable_devices();
    }

    #[allow(unreachable_code)]
    Err(DiscoveryError::UnsupportedPlatform)
}

pub fn find_removable_device(id: &str) -> Result<Option<UsbDevice>, DiscoveryError> {
    Ok(discover_removable_devices()?
        .into_iter()
        .find(|device| device.id == id))
}

pub fn validate_iso_for_device(
    iso_path: impl AsRef<Path>,
    device: &UsbDevice,
) -> Result<(PathBuf, u64), String> {
    let iso_path = iso_path
        .as_ref()
        .canonicalize()
        .map_err(|error| format!("não foi possível acessar a ISO: {error}"))?;
    let metadata = iso_path
        .metadata()
        .map_err(|error| format!("não foi possível ler a ISO: {error}"))?;

    if !metadata.is_file() {
        return Err("a imagem selecionada não é um arquivo regular".to_owned());
    }
    if !iso_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"))
    {
        return Err("o arquivo selecionado precisa ter a extensão .iso".to_owned());
    }
    if metadata.len() == 0 {
        return Err("a imagem ISO está vazia".to_owned());
    }
    if metadata.len() > device.total_bytes {
        return Err(format!(
            "a ISO possui {} bytes, mas o dispositivo comporta apenas {} bytes",
            metadata.len(),
            device.total_bytes
        ));
    }

    if device.mount_points.iter().any(|mount_point| {
        let mount = Path::new(mount_point);
        mount.is_absolute() && mount != Path::new("/") && iso_path.starts_with(mount)
    }) {
        return Err("a ISO está armazenada no próprio dispositivo de destino".to_owned());
    }

    Ok((iso_path, metadata.len()))
}

pub(crate) fn stable_device_id(path: &str, serial: Option<&str>, size: u64) -> String {
    format!("{path}::{}::{size}", serial.unwrap_or("sem-serial"))
}
