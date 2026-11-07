//! What the window and the command line show of the connected devices.

use std::path::Path;

use crate::device::{self, AppMode, Found, InReach, Version};

/// One connected inReach, with what it answered.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceStatus {
    pub node: String,
    pub name: &'static str,
    pub vendor: u16,
    pub product: u16,
    pub mode: Option<AppMode>,
    pub version: Option<Version>,
    /// Why the device could not be asked (no access, no answer…).
    pub error: Option<String>,
    /// The error is missing permission on the hidraw node (the udev rule).
    pub needs_permission: bool,
}

/// The inReach devices in `sysfs`; with `ask`, each is also asked for its mode
/// and version (up to a few seconds each), without `ask` only /sys is read.
pub fn devices(sysfs: &Path, ask: bool) -> Vec<DeviceStatus> {
    device::find(sysfs).into_iter().map(|f| status_of(f, ask)).collect()
}

fn status_of(found: Found, ask: bool) -> DeviceStatus {
    let mut s = DeviceStatus {
        node: found.node.display().to_string(),
        name: found.name,
        vendor: found.vendor,
        product: found.product,
        mode: None,
        version: None,
        error: None,
        needs_permission: false,
    };
    if !ask {
        return s;
    }
    let asked = InReach::open(&found).and_then(|mut dev| Ok((dev.app_mode()?, dev.version()?)));
    match asked {
        Ok((mode, version)) => {
            s.mode = Some(mode);
            s.version = Some(version);
        }
        Err(e) => {
            s.needs_permission = matches!(e, device::Error::Permission(_));
            s.error = Some(e.to_string());
        }
    }
    s
}

/// Restarts the inReach at `node` as a USB drive. `Ok(false)` when it is one already.
pub fn switch_to_drive(sysfs: &Path, node: &str) -> Result<bool, String> {
    let found = device::find(sysfs)
        .into_iter()
        .find(|f| f.node == Path::new(node))
        .ok_or_else(|| format!("no inReach at {node}"))?;
    let mut dev = InReach::open(&found).map_err(|e| e.to_string())?;
    if dev.app_mode().map_err(|e| e.to_string())? == AppMode::MassStorage {
        return Ok(false);
    }
    log::info!("{} ({:04x}:{:04x}) switched to its drive", found.name, found.vendor, found.product);
    dev.switch_to_drive().map_err(|e| e.to_string())?;
    Ok(true)
}
