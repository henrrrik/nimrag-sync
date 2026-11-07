//! A sync, start to end, as inReach Sync 1.4.47 runs it (SyncManager.cs):
//! gather the device and Garmin's service into a `plan::Plan`, show it
//! (`preview`), or carry it out (`apply`) and read the device back to check.
//!
//! Departures from inReach Sync, each because nimrag-sync cannot or should not
//! do the same: it stops when Garmin offers a firmware update (inReach Sync
//! flashes it first; an inReach Mini takes its update as a USB drive), it
//! sends no usage report (`/V1/App/Report`, which names itself "SyncApp") and
//! no copy of the device's event log (`/V1/App/EventLog`), and it keeps a
//! backup of the tables it changes.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::credentials::{self, Keyring};
use crate::device::{self, AppMode, Found, InReach};
use crate::plan::{self, FromServer, Plan, Sent};
use crate::server::{self, Client, Credentials, FirmwareOffer, SyncInfo};

pub const SETTINGS: u8 = 7;

#[derive(Debug, Serialize)]
pub struct Preview {
    pub device: String,
    pub firmware: String,
    pub sync_url: String,
    pub last_synced: Option<String>,
    /// A firmware Garmin offers that the device does not have yet.
    pub firmware_update: Option<FirmwareOffer>,
    pub plan: Plan,
}

/// Everything a sync needs, read from the device and Garmin's service.
struct Gathered {
    found: Found,
    product: String,
    firmware: String,
    version: [i64; 4],
    imei: String,
    contacts_version: u8,
    sync_data_version: u8,
    settings_version: u8,
    tables: Vec<(u8, String)>,
    info: SyncInfo,
    who: Credentials,
    last_synced: Option<String>,
    firmware_update: Option<FirmwareOffer>,
    plan: Plan,
}

/// The firmware version as inReach Sync reads it (ParseHardwareVersion): the
/// first three numbers of "3.90.1101380", and the number after "Rev " in the
/// product name ("inReach Mini Rev 0") as the hardware minor.
pub fn version_numbers(firmware: &str, product: &str) -> [i64; 4] {
    let mut parts = firmware.split('.').map(|p| p.trim().parse::<i64>().unwrap_or(0));
    let hardware = product
        .split("Rev ")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    [parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0), hardware]
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn open_on(sysfs: &Path) -> Result<(Found, InReach<device::Hidraw>), String> {
    let found = device::find(sysfs).into_iter().next().ok_or("No inReach is connected over USB.")?;
    let mut dev = InReach::open(&found).map_err(|e| e.to_string())?;
    if dev.app_mode().map_err(|e| e.to_string())? != AppMode::App {
        return Err("The inReach must be on to sync (it is charging or a USB drive): switch it on, or run \
                    `nimrag-sync app-mode --apply`."
            .into());
    }
    Ok((found, dev))
}

fn read(dev: &mut InReach<device::Hidraw>, table: u8, name: &str) -> Result<(u8, String), String> {
    let version = dev
        .table_version(table)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("The inReach has no {name} table."))?;
    Ok((version, dev.read_table(table, version).map_err(|e| e.to_string())?))
}

fn gather(sysfs: &Path, config: &Path, keyring: &Keyring, client: &Client) -> Result<Gathered, String> {
    let (found, mut dev) = open_on(sysfs)?;
    let v = dev.version().map_err(|e| e.to_string())?;
    let (settings_version, settings_text) = read(&mut dev, SETTINGS, "Settings")?;
    let imei = crate::csv::parse(&settings_text)
        .into_iter()
        .find(|r| r.first().map(String::as_str) == Some("9"))
        .and_then(|r| r.get(1).cloned())
        .filter(|i| i.len() == 15 && i.bytes().all(|b| b.is_ascii_digit()))
        .ok_or("The inReach did not tell its IMEI.")?;
    let (contacts_version, contacts_text) = read(&mut dev, plan::CONTACTS, "Contacts")?;
    let (sync_data_version, sync_data_text) = read(&mut dev, plan::SYNC_DATA, "SyncData")?;
    drop(dev);

    let info = client.sync_info(&imei).map_err(|e| e.to_string())?;
    if !info.provisioned {
        return Err("This inReach is not on a Garmin account, so Garmin's service has nothing for it.".into());
    }
    let host = info.sync_url.split_once("://").map(|(_, r)| r).unwrap_or(&info.sync_url);
    let host = host.split('/').next().unwrap_or(host);
    let who = credentials::load(config, keyring, host).map_err(|e| e.to_string())?;
    let version = version_numbers(&v.firmware, &v.product);
    let offer = client.firmware_offer(&info, &imei, &version).map_err(|e| e.to_string())?;
    // inReach Sync skips an offer of the version the device has.
    let firmware_update = offer.filter(|o| [o.version_major, o.version_minor, o.version_rev] != version[..3]);
    let status = client.device_sync(&info, &imei, &who).map_err(|e| e.to_string())?;
    let contacts = client.contacts(&info, &imei, &who).map_err(|e| e.to_string())?;
    let presets = client.presets(&info, &imei, &who).map_err(|e| e.to_string())?;
    let quick_texts = client.quick_texts(&info, &imei, &who).map_err(|e| e.to_string())?;
    let settings = client.settings(&info, &imei, &who).map_err(|e| e.to_string())?;

    let modified = |kind: i32| {
        status
            .sync_data
            .iter()
            .find(|h| h.data_type == kind)
            .and_then(|h| h.last_modified.as_deref())
            .and_then(server::unix_seconds)
    };
    let from = FromServer {
        contacts: Sent::from(contacts.as_ref().map(|c| c.as_deref())),
        presets: Sent::from(presets.as_ref().map(|c| c.as_deref())),
        quick_texts: Sent::from(quick_texts.as_ref().map(|c| c.as_deref())),
        // DataTypeEnum: 1 quick texts, 2 presets.
        presets_modified: modified(2),
        quick_texts_modified: modified(1),
        now: now(),
        settings: settings.as_deref(),
    };
    let plan = plan::plan(contacts_version, &contacts_text, &sync_data_text, &settings_text, &from)?;
    Ok(Gathered {
        found,
        product: v.product,
        firmware: v.firmware,
        version,
        imei,
        contacts_version,
        sync_data_version,
        settings_version,
        tables: vec![(plan::CONTACTS, contacts_text), (plan::SYNC_DATA, sync_data_text), (SETTINGS, settings_text)],
        info,
        who,
        last_synced: status.last_synced,
        firmware_update,
        plan,
    })
}

/// What a sync of the first connected inReach would change. Reads the device
/// (it must be on: app mode) and Garmin's service, writes nothing.
pub fn preview(sysfs: &Path, config: &Path, keyring: &Keyring, client: &Client) -> Result<Preview, String> {
    let g = gather(sysfs, config, keyring, client)?;
    Ok(Preview {
        device: g.product,
        firmware: g.firmware,
        sync_url: g.info.sync_url,
        last_synced: g.last_synced,
        firmware_update: g.firmware_update,
        plan: g.plan,
    })
}

/// `$XDG_DATA_HOME/nimrag-sync`, or `~/.local/share/nimrag-sync`.
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".local/share"))
        .join("nimrag-sync")
}

/// Keeps the tables a sync changes, as the device sent them, in
/// `dir/backups/<imei>/<time>/table-<id>.csv`, readable by the user only.
fn backup(dir: &Path, imei: &str, tables: &[(u8, String)]) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let folder = dir.join("backups").join(imei).join(now().to_string());
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)
        .map_err(|e| format!("cannot keep a backup in {}: {e}", folder.display()))?;
    for (id, text) in tables {
        let path = folder.join(format!("table-{id}.csv"));
        std::fs::write(&path, text).map_err(|e| format!("cannot keep a backup in {}: {e}", path.display()))?;
    }
    Ok(folder)
}

/// Waits for the device after its restart as inReach Sync does
/// (DeviceManager.waitForFinishReboot): up to 7 s for it to come back on USB,
/// then 5 s more. Returns the mode it came back in, if it answers. An
/// inReach Mini restarted from app mode while plugged in comes back as a USB
/// drive.
fn wait_after_restart(sysfs: &Path) -> Option<AppMode> {
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(2));
    while start.elapsed() < Duration::from_secs(7) {
        if !device::find(sysfs).is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    std::thread::sleep(Duration::from_secs(5));
    let found = device::find(sysfs).into_iter().next()?;
    InReach::open(&found).ok()?.app_mode().ok()
}

/// What `apply` did.
#[derive(Debug, Serialize)]
pub struct Applied {
    pub backup: PathBuf,
    /// The plan of a second look after the writes, before the restart: what still differs.
    pub left_over: Plan,
    /// The mode the device came back in after its restart (`None`: not seen).
    pub mode_after_restart: Option<AppMode>,
}

/// Syncs the first connected inReach as inReach Sync does, telling `step`
/// what it is doing. Keeps a backup first and reads the device back after.
pub fn apply(
    sysfs: &Path,
    config: &Path,
    data: &Path,
    keyring: &Keyring,
    client: &Client,
    step: &mut dyn FnMut(&str),
) -> Result<Applied, String> {
    step("Reading the inReach and Garmin's service");
    let g = gather(sysfs, config, keyring, client)?;
    if let Some(offer) = &g.firmware_update {
        return Err(format!(
            "Garmin offers software {}.{}.{} for this inReach. inReach Sync installs it before anything else; \
             nimrag-sync does not install software this way yet. Install it first (on an inReach Mini: \
             nimrag-sync drive-mode --apply, then openExpress or Garmin's own tools), then sync.",
            offer.version_major, offer.version_minor, offer.version_rev
        ));
    }
    let backup_dir = backup(data, &g.imei, &g.tables)?;
    let p = &g.plan;
    let max = device::max_script_len(&g.product, &g.firmware);
    let (sync_data, contacts, settings) = (plan::SYNC_DATA, plan::CONTACTS, SETTINGS);
    let (sdv, cv, sv) = (g.sync_data_version, g.contacts_version, g.settings_version);
    let columns = plan::contact_columns(cv).ok_or("unknown Contacts table version")?;
    let lines = |items: &[plan::Item]| items.iter().map(plan::Item::insert_line).collect::<Vec<_>>();
    let e = |e: device::Error| e.to_string();
    let s = |e: server::Error| e.to_string();

    // FirmwareServerUpdate: Garmin hears which firmware the device has.
    step("Telling Garmin the inReach's software version");
    client.report_firmware(&g.info, &g.imei, &g.version, &g.who).map_err(s)?;

    let mut dev = InReach::open(&g.found).map_err(e)?;
    if p.clears_sync_data {
        step("Clearing the sync data");
        dev.write_table(sync_data, sdv, "d,*\r\n").map_err(e)?;
    }
    let breadcrumb = vec![plan::Item { kind: plan::BREADCRUMB, short_code: 0, value: "sbc".into() }.insert_line()];
    let by_kind = |kind: u32| p.sync_data.iter().filter(|i| i.kind == kind).cloned().collect::<Vec<_>>();

    if !p.contacts.left_alone {
        step("Writing the contacts");
        let deletes: Vec<String> = p.contact_ids_to_delete.iter().map(|id| format!("d,{id}")).collect();
        dev.batch_write(contacts, cv, &deletes, max).map_err(e)?;
        let inserts: Vec<String> = p.contact_rows.iter().map(|c| c.insert_line(columns)).collect();
        dev.batch_write(contacts, cv, &inserts, max).map_err(e)?;
        dev.batch_write(sync_data, sdv, &lines(&by_kind(plan::ADDRESS)), max).map_err(e)?;
        dev.batch_write(sync_data, sdv, &breadcrumb, max).map_err(e)?;
        client.mark_synced(&g.info, "Contacts", &g.imei, &g.who).map_err(s)?;
    }
    let time = |id: u32| p.timestamps.iter().find(|(i, _)| *i == id).map(|(i, v)| vec![format!("i,{i},{v}")]);
    // SyncPresets: nothing at all when the answer had no list; no acknowledgement either way.
    if p.presets_step && p.presets_listed {
        step("Writing the preset messages");
        dev.batch_write(sync_data, sdv, &lines(&by_kind(plan::PRESET)), max).map_err(e)?;
        dev.batch_write(sync_data, sdv, &breadcrumb, max).map_err(e)?;
        if let Some(t) = time(plan::PRESETS_TIME) {
            dev.batch_write(settings, sv, &t, max).map_err(e)?;
        }
    }
    // SyncQuickTexts: writes only with a list, but reports success either way,
    // and inReach Sync then tells Garmin the address book is synced.
    if p.quick_texts_step {
        step("Writing the quick texts");
        if p.quick_texts_listed {
            dev.batch_write(sync_data, sdv, &lines(&by_kind(plan::QUICK_TEXT)), max).map_err(e)?;
            dev.batch_write(sync_data, sdv, &breadcrumb, max).map_err(e)?;
            if let Some(t) = time(plan::QUICK_TEXTS_TIME) {
                dev.batch_write(settings, sv, &t, max).map_err(e)?;
            }
        }
        client.mark_synced(&g.info, "QuickTexts", &g.imei, &g.who).map_err(s)?;
    }
    if p.settings.answered {
        step("Writing the settings");
        if let Some(change) = &p.settings.device_type_change {
            let to = change.to.clone().unwrap_or_default();
            dev.write_table(settings, sv, &format!("i,16,{to}\r\n")).map_err(e)?;
            dev.apply_default_settings().map_err(e)?;
        }
        dev.batch_write(settings, sv, &p.settings.lines, max).map_err(e)?;
        client.mark_synced(&g.info, "Settings", &g.imei, &g.who).map_err(s)?;
    }
    if let Some(t) = time(plan::SYNC_TIME) {
        step("Writing the time of the sync");
        dev.batch_write(settings, sv, &t, max).map_err(e)?;
    }

    drop(dev);

    // Not a step of inReach Sync's: a second look while the device is still
    // in app mode, which it may not be after the restart.
    step("Reading the inReach back");
    let after = gather(sysfs, config, keyring, client)?;

    step("Restarting the inReach");
    InReach::open(&g.found).and_then(|mut d| d.restart_app_mode()).map_err(e)?;
    let mode_after_restart = wait_after_restart(sysfs);
    Ok(Applied { backup: backup_dir, left_over: after.plan, mode_after_restart })
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

pub fn apply_here(step: &mut dyn FnMut(&str)) -> Result<Applied, String> {
    apply(
        Path::new(device::SYSFS_HIDRAW),
        &credentials::config_dir(),
        &data_dir(),
        &Keyring::default(),
        &server::Client::default(),
        step,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_numbers_as_inreach_sync_parses_them() {
        assert_eq!(version_numbers("3.90.1101380", "inReach Mini Rev 0"), [3, 90, 1101380, 0]);
        assert_eq!(version_numbers("2.7.5", "inReach SE Rev 2"), [2, 7, 5, 2]);
        assert_eq!(version_numbers("", "inReach"), [0, 0, 0, 0]);
    }

    #[test]
    fn backups_keep_the_tables_privately() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = backup(dir.path(), "300434000000001", &[(1, "1,1,Ada\r\n".into()), (9, String::new())]).unwrap();
        assert_eq!(std::fs::read_to_string(folder.join("table-1.csv")).unwrap(), "1,1,Ada\r\n");
        assert!(folder.join("table-9.csv").exists());
        assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
