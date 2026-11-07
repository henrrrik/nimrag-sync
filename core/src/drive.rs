//! The USB drive of an inReach in its drive mode: which block device belongs
//! to the inReach's USB device (from /sys only), where it is mounted, and
//! mounting and ejecting it through udisks (`udisksctl`), as the desktop does.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::device::KNOWN;

pub const SYSFS_BLOCK: &str = "/sys/class/block";

/// An inReach's drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// The block device, e.g. /dev/sda.
    pub device: PathBuf,
    pub vendor: u16,
    pub product: u16,
    /// Where it is mounted; `None` when it is not.
    pub mount: Option<PathBuf>,
}

/// The USB IDs of the device a block device belongs to, read from the
/// idVendor and idProduct files of the first ancestor in /sys that has them.
fn usb_ids(block: &Path) -> Option<(u16, u16)> {
    let mut dir = std::fs::canonicalize(block.join("device")).ok()?;
    loop {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        if let (Some(v), Some(p)) = (read("idVendor"), read("idProduct")) {
            return Some((u16::from_str_radix(v.trim(), 16).ok()?, u16::from_str_radix(p.trim(), 16).ok()?));
        }
        if !dir.pop() || dir.as_os_str().len() <= 1 {
            return None;
        }
    }
}

/// The mount point of `device` in a mounts table (/proc/self/mounts), its
/// spaces and other escapes (`\040`) undone.
pub fn mount_point(mounts: &str, device: &Path) -> Option<PathBuf> {
    let unescape = |s: &str| {
        let mut out = Vec::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b)) {
                out.push(u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 4]).ok().unwrap_or("0"), 8).unwrap_or(b'?'));
                i += 4;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    mounts.lines().find_map(|line| {
        let mut f = line.split_whitespace();
        let (source, target) = (f.next()?, f.next()?);
        (Path::new(&unescape(source)) == device).then(|| PathBuf::from(unescape(target)))
    })
}

/// The drives of inReach devices: whole disks and their partitions whose USB
/// device is one inReach Sync knows.
pub fn find(sysfs_block: &Path, mounts: &str) -> Vec<Drive> {
    let Ok(entries) = std::fs::read_dir(sysfs_block) else { return Vec::new() };
    let mut names: Vec<_> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let mut drives = Vec::new();
    for name in names {
        let Some((vendor, product)) = usb_ids(&sysfs_block.join(&name)) else { continue };
        if !KNOWN.iter().any(|(v, p, _)| *v == vendor && *p == product) {
            continue;
        }
        let device = Path::new("/dev").join(&name);
        let mount = mount_point(mounts, &device);
        drives.push(Drive { device, vendor, product, mount });
    }
    // A mounted one first: the disk itself on an inReach Mini (it has no partitions).
    drives.sort_by_key(|d| d.mount.is_none());
    drives
}

/// The drives here, from /sys and /proc/self/mounts.
pub fn find_here() -> Vec<Drive> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    find(Path::new(SYSFS_BLOCK), &mounts)
}

fn udisksctl(args: &[&str]) -> Result<String, String> {
    // Only the commands that act take --no-user-interaction; `info` refuses it.
    let acts = matches!(args.first(), Some(&"mount" | &"unmount" | &"power-off"));
    let out = Command::new("udisksctl")
        .args(args)
        .args(acts.then_some("--no-user-interaction"))
        .output()
        .map_err(|e| format!("udisksctl: {e} (is udisks2 installed?)"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("udisksctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Mounts the drive where the desktop mounts drives; where it went.
pub fn mount(drive: &Drive) -> Result<PathBuf, String> {
    if let Some(m) = &drive.mount {
        return Ok(m.clone());
    }
    let device = drive.device.to_string_lossy();
    udisksctl(&["mount", "-b", &device])?;
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    mount_point(&mounts, &drive.device).ok_or_else(|| format!("{device} was mounted, but not found mounted"))
}

/// The UDisks object of the drive a block device is on, from `udisksctl info`
/// ("Drive: '/org/freedesktop/UDisks2/drives/…'").
pub fn udisks_drive(info: &str) -> Option<String> {
    info.lines().find_map(|l| {
        let v = l.trim().strip_prefix("Drive:")?.trim().trim_matches('\'');
        (v.starts_with("/org/freedesktop/UDisks2/drives/")).then(|| v.to_string())
    })
}

/// Ejects the drive as the file manager does: unmounted, then UDisks's
/// Drive.Eject. An inReach takes that as the end of its drive mode and comes
/// back on USB by itself. (Not `power-off`: that has the kernel drop the USB
/// device, which then stays away until it is plugged in again.)
pub fn eject(drive: &Drive) -> Result<(), String> {
    let device = drive.device.to_string_lossy();
    if mount_point(&std::fs::read_to_string("/proc/self/mounts").unwrap_or_default(), &drive.device).is_some() {
        udisksctl(&["unmount", "-b", &device])?;
    }
    let info = udisksctl(&["info", "-b", &device])?;
    let object = udisks_drive(&info).ok_or_else(|| format!("udisks does not name the drive of {device}"))?;
    let out = Command::new("gdbus")
        .args(["call", "--system", "--dest", "org.freedesktop.UDisks2", "--object-path", &object])
        .args(["--method", "org.freedesktop.UDisks2.Drive.Eject", "{'auth.no_user_interaction': <true>}"])
        .output()
        .map_err(|e| format!("gdbus: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("ejecting {device}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The drive's `Garmin/GarminDevice.xml`, its folder in any letter case.
pub fn device_xml(root: &Path) -> Option<(PathBuf, String)> {
    let entries = std::fs::read_dir(root).ok()?;
    for folder in entries.flatten() {
        if !folder.file_name().to_string_lossy().eq_ignore_ascii_case("garmin") {
            continue;
        }
        for file in std::fs::read_dir(folder.path()).ok()?.flatten() {
            if file.file_name().to_string_lossy().eq_ignore_ascii_case("GarminDevice.xml") {
                let text = std::fs::read_to_string(file.path()).ok()?;
                return Some((file.path(), text));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A /sys/class/block with an inReach Mini's disk and a pendrive, as links
    /// into a /sys/devices tree, as the kernel has it.
    fn sysfs(root: &Path) -> PathBuf {
        let devices = root.join("devices");
        let add = |usb: &str, vendor: &str, product: &str, disk: &str| {
            let usb_dir = devices.join(usb);
            let scsi = usb_dir.join(format!("{usb}:1.1/host0/target0:0:0/0:0:0:0"));
            std::fs::create_dir_all(&scsi).unwrap();
            std::fs::write(usb_dir.join("idVendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(usb_dir.join("idProduct"), format!("{product}\n")).unwrap();
            let block = devices.join(format!("block/{disk}"));
            std::fs::create_dir_all(&block).unwrap();
            std::os::unix::fs::symlink(&scsi, block.join("device")).unwrap();
        };
        add("1-1", "091e", "0b89", "sda");
        add("1-2", "0781", "5581", "sdb");
        let class = root.join("class/block");
        std::fs::create_dir_all(&class).unwrap();
        for d in ["sda", "sdb"] {
            std::os::unix::fs::symlink(devices.join(format!("block/{d}")), class.join(d)).unwrap();
        }
        class
    }

    #[test]
    fn finds_the_inreach_drive_and_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let class = sysfs(dir.path());
        let mounts = "/dev/sdb /run/media/ada/STICK vfat rw 0 0\n/dev/sda /run/media/ada/GARMIN\\040X vfat rw 0 0\n";
        let drives = find(&class, mounts);
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].device, Path::new("/dev/sda"));
        assert_eq!((drives[0].vendor, drives[0].product), (0x091E, 0x0B89));
        assert_eq!(drives[0].mount.as_deref(), Some(Path::new("/run/media/ada/GARMIN X")));
        assert_eq!(find(&class, "")[0].mount, None);
        assert!(find(&dir.path().join("missing"), "").is_empty());
    }

    #[test]
    fn the_udisks_drive_of_a_block_device() {
        let info = "/org/freedesktop/UDisks2/block_devices/sda:\n  org.freedesktop.UDisks2.Block:\n    Device:                     /dev/sda\n    Drive:                      '/org/freedesktop/UDisks2/drives/Garmin_inReach_0000001234567890'\n";
        assert_eq!(udisks_drive(info).as_deref(), Some("/org/freedesktop/UDisks2/drives/Garmin_inReach_0000001234567890"));
        assert_eq!(udisks_drive("    Drive:                      '/'\n"), None);
    }

    #[test]
    fn reads_garmin_device_xml_in_any_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("GARMIN")).unwrap();
        std::fs::write(dir.path().join("GARMIN/garmindevice.XML"), "<Device/>").unwrap();
        let (path, text) = device_xml(dir.path()).unwrap();
        assert!(path.ends_with("GARMIN/garmindevice.XML"));
        assert_eq!(text, "<Device/>");
        assert!(device_xml(&dir.path().join("none")).is_none());
    }
}
