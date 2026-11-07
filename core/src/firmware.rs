//! Software updates for an inReach in its USB drive mode, from the drive to
//! Garmin's update server and back (see `updates`).

use std::path::PathBuf;

use serde::Serialize;

use crate::drive::{self, Drive};
use crate::updates::{self, DeviceInfo, Update};

#[derive(Debug, Serialize)]
pub struct Check {
    pub device: DeviceInfo,
    /// Where the drive is mounted.
    pub drive: PathBuf,
    pub free_bytes: Option<u64>,
    pub updates: Vec<Update>,
}

/// Free space on the file system at `path`.
fn free_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid C path and a statvfs to fill.
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

/// The inReach's drive, mounted.
fn mounted_drive() -> Result<(Drive, PathBuf), String> {
    let Some(d) = drive::find_here().into_iter().next() else {
        let on_usb = !crate::device::find(std::path::Path::new(crate::device::SYSFS_HIDRAW)).is_empty();
        return Err(if on_usb {
            "The inReach is not a USB drive now. Software updates go onto its drive: switch it to USB drive mode \
             first (nimrag-sync drive-mode --apply, or the button in the window)."
                .into()
        } else {
            "No inReach is connected over USB.".into()
        });
    };
    let root = drive::mount(&d)?;
    Ok((d, root))
}

/// What Garmin offers for the inReach on its drive; with `reinstall` also what it has already.
pub fn check(client: &updates::Client, reinstall: bool) -> Result<Check, String> {
    let (_, root) = mounted_drive()?;
    let (_, xml) = drive::device_xml(&root).ok_or("The inReach's drive has no Garmin/GarminDevice.xml.")?;
    let updates = client.check(&xml, reinstall).map_err(|e| e.to_string())?;
    Ok(Check { device: updates::device_info(&xml), free_bytes: free_bytes(&root), drive: root, updates })
}

#[derive(Debug, Serialize)]
pub struct Installed {
    /// The files written to the drive.
    pub files: Vec<PathBuf>,
    pub ejected: bool,
}

/// Downloads, checks and puts Garmin's updates on the inReach's drive, then
/// ejects it; the inReach installs them when it restarts. `step` hears what it
/// is doing; `progress` the bytes of each download.
pub fn install(
    client: &updates::Client,
    reinstall: bool,
    step: &mut dyn FnMut(&str),
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Installed, String> {
    step("Asking Garmin for software updates");
    let (d, root) = mounted_drive()?;
    let (_, xml) = drive::device_xml(&root).ok_or("The inReach's drive has no Garmin/GarminDevice.xml.")?;
    let offered = client.check(&xml, reinstall).map_err(|e| e.to_string())?;
    if offered.is_empty() {
        return Ok(Installed { files: Vec::new(), ejected: false });
    }
    // All of them first, so nothing is written when one cannot be had.
    let mut files = Vec::new();
    for u in &offered {
        let place = u.place().ok_or_else(|| format!("Garmin's place for {} is not on the drive", u.installation.file_name))?;
        step(&format!("Downloading {} {}", u.product_name.as_deref().unwrap_or(&u.part_number), u.version.as_deref().unwrap_or("")));
        let data = client.download(u, progress).map_err(|e| e.to_string())?;
        files.push((place, data));
    }
    let needed: u64 = files.iter().map(|(_, d)| d.len() as u64).sum();
    if let Some(free) = free_bytes(&root)
        && free < needed
    {
        return Err(format!("The inReach's drive has {free} bytes free; the updates need {needed}."));
    }
    let mut written = Vec::new();
    for (place, data) in &files {
        step("Copying the update to the inReach");
        written.push(updates::put_on_drive(&root, place, data).map_err(|e| format!("cannot write {}: {e}", place.display()))?);
    }
    step("Ejecting the inReach's drive");
    drive::eject(&d)?;
    Ok(Installed { files: written, ejected: true })
}
