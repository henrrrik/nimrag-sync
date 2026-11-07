//! A sync, start to end: the device and Garmin's service gathered into a
//! `plan::Plan`. Only reading so far: the preview of what a sync would change.

use std::path::Path;

use serde::Serialize;

use crate::credentials::{self, Keyring};
use crate::device::{self, AppMode, InReach};
use crate::plan::{self, FromServer, Plan, Sent};
use crate::server::{self, Client};

#[derive(Debug, Serialize)]
pub struct Preview {
    pub device: String,
    pub firmware: String,
    pub sync_url: String,
    pub last_synced: Option<String>,
    pub plan: Plan,
}

/// What a sync of the first connected inReach would change. Reads the device
/// (it must be on: app mode) and Garmin's service, writes nothing.
pub fn preview(sysfs: &Path, config: &Path, keyring: &Keyring, client: &Client) -> Result<Preview, String> {
    let found = device::find(sysfs).into_iter().next().ok_or("No inReach is connected over USB.")?;
    let mut dev = InReach::open(&found).map_err(|e| e.to_string())?;
    let mode = dev.app_mode().map_err(|e| e.to_string())?;
    if mode != AppMode::App {
        return Err("The inReach must be on to sync (it is charging or a USB drive): switch it on, or run \
                    `nimrag-sync app-mode --apply`."
            .into());
    }
    let version = dev.version().map_err(|e| e.to_string())?;
    let imei = dev.imei().map_err(|e| e.to_string())?.ok_or("The inReach did not tell its IMEI.")?;
    let contacts_version = dev
        .table_version(plan::CONTACTS)
        .map_err(|e| e.to_string())?
        .ok_or("The inReach has no Contacts table.")?;
    let contacts_text = dev.read_table(plan::CONTACTS, contacts_version).map_err(|e| e.to_string())?;
    let sync_data_version = dev
        .table_version(plan::SYNC_DATA)
        .map_err(|e| e.to_string())?
        .ok_or("The inReach has no SyncData table.")?;
    let sync_data_text = dev.read_table(plan::SYNC_DATA, sync_data_version).map_err(|e| e.to_string())?;
    drop(dev);

    let info = client.sync_info(&imei).map_err(|e| e.to_string())?;
    if !info.provisioned {
        return Err("This inReach is not on a Garmin account, so Garmin's service has nothing for it.".into());
    }
    let host = info.sync_url.split_once("://").map(|(_, r)| r).unwrap_or(&info.sync_url);
    let host = host.split('/').next().unwrap_or(host);
    let who = credentials::load(config, keyring, host).map_err(|e| e.to_string())?;
    let status = client.device_sync(&info, &imei, &who).map_err(|e| e.to_string())?;
    let contacts = client.contacts(&info, &imei, &who).map_err(|e| e.to_string())?;
    let presets = client.presets(&info, &imei, &who).map_err(|e| e.to_string())?;
    let quick_texts = client.quick_texts(&info, &imei, &who).map_err(|e| e.to_string())?;

    let modified = |kind: i32| {
        status
            .sync_data
            .iter()
            .find(|h| h.data_type == kind)
            .and_then(|h| h.last_modified.as_deref())
            .and_then(server::unix_seconds)
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let from = FromServer {
        contacts: Sent::from(contacts.as_ref().map(|c| c.as_deref())),
        presets: Sent::from(presets.as_ref().map(|c| c.as_deref())),
        quick_texts: Sent::from(quick_texts.as_ref().map(|c| c.as_deref())),
        // DataTypeEnum: 1 quick texts, 2 presets.
        presets_modified: modified(2),
        quick_texts_modified: modified(1),
        now,
    };
    let plan = plan::plan(contacts_version, &contacts_text, &sync_data_text, &from)?;
    Ok(Preview {
        device: version.product,
        firmware: version.firmware,
        sync_url: info.sync_url,
        last_synced: status.last_synced,
        plan,
    })
}

/// The default places: /sys, ~/.config/nimrag-sync, the desktop keyring, Garmin.
pub fn preview_here() -> Result<Preview, String> {
    preview(
        Path::new(device::SYSFS_HIDRAW),
        &credentials::config_dir(),
        &Keyring::default(),
        &server::Client::default(),
    )
}
