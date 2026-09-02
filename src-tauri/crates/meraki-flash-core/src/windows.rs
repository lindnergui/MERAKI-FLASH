use crate::{DiscoveryError, UsbDevice, stable_device_id};
use serde::Deserialize;
use std::process::Command;

const POWERSHELL: &str = "powershell.exe";
const DISCOVERY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$result = @(
  foreach ($disk in Get-CimInstance Win32_DiskDrive) {
    if ($disk.InterfaceType -ne 'USB' -or $disk.MediaType -notmatch 'Removable') { continue }
    $logical = @(
      Get-CimAssociatedInstance -InputObject $disk -Association Win32_DiskDriveToDiskPartition |
        ForEach-Object { Get-CimAssociatedInstance -InputObject $_ -Association Win32_LogicalDiskToPartition }
    )
    $mounts = @($logical | ForEach-Object { $_.DeviceID + '\' })
    $diskInfo = Get-Disk -Number $disk.Index -ErrorAction SilentlyContinue
    [PSCustomObject]@{
      DevicePath = [string]$disk.DeviceID
      Name = [string]$disk.Model
      Serial = [string]$disk.SerialNumber
      Size = [uint64]$disk.Size
      ReadOnly = [bool]($diskInfo -and $diskInfo.IsReadOnly)
      MountPoints = $mounts
      FileSystem = [string](($logical | Select-Object -First 1).FileSystem)
      AvailableBytes = [uint64](($logical | Measure-Object -Property FreeSpace -Sum).Sum)
      IsSystem = [bool]($mounts -contains ($env:SystemDrive + '\'))
    }
  }
)
ConvertTo-Json -InputObject $result -Compress -Depth 5
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsDisk {
    device_path: String,
    name: String,
    serial: String,
    size: u64,
    read_only: bool,
    #[serde(default)]
    mount_points: Vec<String>,
    file_system: String,
    available_bytes: u64,
    is_system: bool,
}

pub(super) fn discover_removable_devices() -> Result<Vec<UsbDevice>, DiscoveryError> {
    let output = Command::new(POWERSHELL)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            DISCOVERY_SCRIPT,
        ])
        .output()
        .map_err(|source| DiscoveryError::Command {
            program: POWERSHELL,
            source,
        })?;

    if !output.status.success() {
        return Err(DiscoveryError::CommandFailed {
            program: POWERSHELL,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    let parsed: Vec<WindowsDisk> =
        serde_json::from_slice(&output.stdout).map_err(|source| DiscoveryError::InvalidOutput {
            program: POWERSHELL,
            source,
        })?;
    let mut devices = parsed
        .into_iter()
        .filter(|disk| !disk.is_system && disk.size > 0)
        .map(|disk| {
            let serial = clean_value(disk.serial);
            let first_mount = disk.mount_points.first().cloned().unwrap_or_default();
            UsbDevice {
                id: stable_device_id(&disk.device_path, serial.as_deref(), disk.size),
                name: clean_value(disk.name).unwrap_or_else(|| "Unidade USB".to_owned()),
                device_path: disk.device_path,
                mount_point: first_mount,
                mount_points: disk.mount_points,
                file_system: disk.file_system.trim().to_uppercase(),
                total_bytes: disk.size,
                available_bytes: disk.available_bytes,
                read_only: disk.read_only,
                kind: "Removable".to_owned(),
                transport: "USB".to_owned(),
                serial,
            }
        })
        .collect::<Vec<_>>();

    devices.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    Ok(devices)
}

fn clean_value(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}
