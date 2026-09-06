use crate::{DiscoveryError, UsbDevice, stable_device_id};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use sysinfo::Disks;

const LSBLK: &str = "lsblk";
const LSBLK_COLUMNS: &str = "PATH,NAME,TYPE,TRAN,RM,SIZE,RO,MODEL,SERIAL,FSTYPE,MOUNTPOINTS";

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    path: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    tran: Option<String>,
    rm: bool,
    size: u64,
    ro: bool,
    model: Option<String>,
    serial: Option<String>,
    fstype: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

#[derive(Debug, Clone)]
struct MountInfo {
    available_bytes: u64,
    file_system: String,
}

pub(super) fn discover_removable_devices() -> Result<Vec<UsbDevice>, DiscoveryError> {
    let output = Command::new(LSBLK)
        .args(["--json", "--bytes", "--paths", "--output", LSBLK_COLUMNS])
        .output()
        .map_err(|source| DiscoveryError::Command {
            program: LSBLK,
            source,
        })?;

    if !output.status.success() {
        return Err(DiscoveryError::CommandFailed {
            program: LSBLK,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    let parsed: LsblkOutput =
        serde_json::from_slice(&output.stdout).map_err(|source| DiscoveryError::InvalidOutput {
            program: LSBLK,
            source,
        })?;
    Ok(devices_from_lsblk(parsed))
}

fn devices_from_lsblk(parsed: LsblkOutput) -> Vec<UsbDevice> {
    let mount_info = sysinfo_mounts();
    let mut devices = parsed
        .blockdevices
        .into_iter()
        .filter(|device| device.kind == "disk")
        // Exclui cartões, discos internos e SSDs externos não removíveis.
        .filter(|device| device.rm && device.tran.as_deref() == Some("usb"))
        .filter(|device| !contains_system_mount(device))
        .map(|device| to_usb_device(device, &mount_info))
        .collect::<Vec<_>>();

    devices.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.device_path.cmp(&right.device_path))
    });
    devices
}

fn to_usb_device(device: LsblkDevice, mount_info: &HashMap<String, MountInfo>) -> UsbDevice {
    let mut mount_points = Vec::new();
    collect_mount_points(&device, &mut mount_points);
    mount_points.sort();
    mount_points.dedup();

    let first_mount = mount_points.first().cloned().unwrap_or_default();
    let first_info = mount_info.get(&first_mount);
    let file_system = first_info
        .map(|info| info.file_system.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| first_filesystem(&device))
        .unwrap_or_default();
    let serial = clean_value(device.serial);
    let model = clean_value(device.model)
        .or_else(|| device.name.rsplit('/').next().map(str::to_owned))
        .unwrap_or_else(|| "Unidade USB".to_owned());

    UsbDevice {
        id: stable_device_id(&device.path, serial.as_deref(), device.size),
        name: model,
        device_path: device.path,
        mount_point: first_mount,
        mount_points,
        file_system,
        total_bytes: device.size,
        available_bytes: first_info.map_or(0, |info| info.available_bytes),
        // ISO9660 montado como somente leitura não torna o hardware protegido.
        read_only: device.ro,
        kind: "Removable".to_owned(),
        transport: "USB".to_owned(),
        serial,
    }
}

fn contains_system_mount(device: &LsblkDevice) -> bool {
    device
        .mountpoints
        .iter()
        .flatten()
        .any(|mount| matches!(mount.as_str(), "/" | "/boot" | "/boot/efi" | "/usr" | "/var" | "/home" | "[SWAP]"))
        || device.children.iter().any(contains_system_mount)
}

fn collect_mount_points(device: &LsblkDevice, output: &mut Vec<String>) {
    output.extend(
        device
            .mountpoints
            .iter()
            .flatten()
            .filter(|mount| mount.starts_with('/'))
            .cloned(),
    );
    for child in &device.children {
        collect_mount_points(child, output);
    }
}

fn first_filesystem(device: &LsblkDevice) -> Option<String> {
    clean_value(device.fstype.clone()).or_else(|| device.children.iter().find_map(first_filesystem))
}

fn clean_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn sysinfo_mounts() -> HashMap<String, MountInfo> {
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|disk| {
            (
                disk.mount_point().to_string_lossy().into_owned(),
                MountInfo {
                    available_bytes: disk.available_space(),
                    file_system: disk.file_system().to_string_lossy().to_uppercase(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{LsblkOutput, devices_from_lsblk};

    #[test]
    fn blocks_separate_system_partitions_and_keeps_rewritable_iso_media() {
        for mount in ["/", "/boot", "/boot/efi", "/home", "[SWAP]", "/run/media/user/LIVE"] {
            let json = serde_json::json!({"blockdevices": [{
                "path":"/dev/sdb", "name":"USB", "type":"disk", "tran":"usb",
                "rm":true, "size":16000000000u64, "ro":false, "model":"USB",
                "serial":"1", "fstype":"iso9660", "mountpoints":[mount]
            }]});
            let parsed = serde_json::from_value(json).unwrap();
            let devices = devices_from_lsblk(parsed);
            if mount == "/run/media/user/LIVE" {
                assert_eq!(devices.len(), 1);
                assert!(!devices[0].read_only);
            } else { assert!(devices.is_empty(), "{mount}"); }
        }
    }

    #[test]
    fn keeps_only_removable_usb_and_rejects_system_disk() {
        let json = r#"{
          "blockdevices": [
            {"path":"/dev/nvme0n1","name":"/dev/nvme0n1","type":"disk","tran":"nvme","rm":false,"size":512000,"ro":false,"model":"Internal","serial":"A","fstype":null,"mountpoints":[],"children":[
              {"path":"/dev/nvme0n1p1","name":"/dev/nvme0n1p1","type":"part","tran":"nvme","rm":false,"size":511000,"ro":false,"model":null,"serial":null,"fstype":"ext4","mountpoints":["/"],"children":[]}
            ]},
            {"path":"/dev/sdb","name":"/dev/sdb","type":"disk","tran":"usb","rm":true,"size":16000000000,"ro":false,"model":"Meraki USB","serial":"USB123","fstype":null,"mountpoints":[],"children":[
              {"path":"/dev/sdb1","name":"/dev/sdb1","type":"part","tran":"usb","rm":true,"size":15900000000,"ro":false,"model":null,"serial":null,"fstype":"vfat","mountpoints":["/run/media/test/USB"],"children":[]}
            ]},
            {"path":"/dev/sdc","name":"/dev/sdc","type":"disk","tran":"usb","rm":false,"size":1000000000,"ro":false,"model":"External SSD","serial":"SSD1","fstype":null,"mountpoints":[],"children":[]}
          ]
        }"#;
        let parsed: LsblkOutput = serde_json::from_str(json).unwrap();
        let devices = devices_from_lsblk(parsed);

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_path, "/dev/sdb");
        assert_eq!(devices[0].mount_points, ["/run/media/test/USB"]);
    }
}
