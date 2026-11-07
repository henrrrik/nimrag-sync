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
use crate::commands::{self, DeviceCommand};
use crate::places::{self, Route, Waypoint};
use crate::plan::{self, FromServer, Plan, Sent};
use crate::server::{self, Client, Credentials, FirmwareOffer, SyncInfo};

pub const SETTINGS: u8 = 7;

/// Waypoints, routes and tracks on the device, as a sync would take them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Places {
    /// Each is `None` when the device's table is not a version inReach Sync syncs.
    pub waypoints: Option<usize>,
    pub waypoints_need_sync: bool,
    pub routes: Option<usize>,
    pub routes_need_sync: bool,
    /// The high-detail track blocks Garmin does not have yet.
    pub tracks_to_send: Option<usize>,
    pub newest_track_point: Option<String>,
}

/// The device's waypoints, routes and track blocks, read for a sync.
#[derive(Default)]
struct PlaceTables {
    waypoints: Option<(u8, Vec<Waypoint>)>,
    routes: Option<(u8, Vec<Route>)>,
    track_blocks: Option<(u8, String)>,
    newest_track_point: Option<i64>,
    tracks_to_send: Vec<String>,
}

impl PlaceTables {
    fn summary(&self) -> Places {
        Places {
            waypoints: self.waypoints.as_ref().map(|w| w.1.len()),
            waypoints_need_sync: self.waypoints.as_ref().is_some_and(|w| places::waypoints_need_sync(&w.1)),
            routes: self.routes.as_ref().map(|r| r.1.len()),
            routes_need_sync: self.routes.as_ref().is_some_and(|r| places::routes_need_sync(&r.1)),
            tracks_to_send: self.track_blocks.as_ref().map(|_| self.tracks_to_send.len()),
            newest_track_point: self.newest_track_point.map(places::utc_text),
        }
    }
}

/// One of Garmin's device commands, as the preview shows it: not its contents.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSummary {
    pub transaction_id: Option<i64>,
    /// The tables its sub-commands write (each one's first byte).
    pub tables: Vec<u8>,
    pub bytes: usize,
}

fn summarize(c: &DeviceCommand) -> CommandSummary {
    let data = c.data().unwrap_or_default();
    let text = String::from_utf8_lossy(&data);
    let mut tables: Vec<u8> = commands::subcommands(&text).iter().filter_map(|s| s.as_bytes().first().copied()).collect();
    tables.dedup();
    CommandSummary { transaction_id: c.transaction_id, tables, bytes: data.len() }
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub device: String,
    pub firmware: String,
    pub sync_url: String,
    pub last_synced: Option<String>,
    /// A firmware Garmin offers that the device does not have yet.
    pub firmware_update: Option<FirmwareOffer>,
    pub plan: Plan,
    pub places: Places,
    pub device_commands: Vec<CommandSummary>,
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
    places: PlaceTables,
    device_commands: Vec<DeviceCommand>,
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

/// A table, when the device has it in `version`; `None` otherwise (inReach
/// Sync skips the step then, TableIsSupported).
fn read_if(dev: &mut InReach<device::Hidraw>, table: u8, version: u8) -> Result<Option<String>, String> {
    match dev.table_version(table).map_err(|e| e.to_string())? {
        Some(v) if v == version => Ok(Some(dev.read_table(table, v).map_err(|e| e.to_string())?)),
        _ => Ok(None),
    }
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
    let waypoints_text = read_if(&mut dev, places::WAYPOINTS, places::WAYPOINTS_VERSION)?;
    let track_blocks_text = read_if(&mut dev, places::TRACK_BLOCKS, places::TRACK_BLOCKS_VERSION)?;
    // Routes need the TrackBlocks table too (DeviceRoutes.AreSupported).
    let routes_text = match &track_blocks_text {
        Some(_) => read_if(&mut dev, places::ROUTES, places::ROUTES_VERSION)?,
        None => None,
    };
    // Kept in the backup only: Garmin's device commands may write it.
    let configuration_text = match dev.table_version(8).map_err(|e| e.to_string())? {
        Some(v) => Some(dev.read_table(8, v).map_err(|e| e.to_string())?),
        None => None,
    };
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
    let mut place_tables = PlaceTables::default();
    if let Some(text) = &waypoints_text {
        place_tables.waypoints = Some((places::WAYPOINTS_VERSION, places::waypoints_from_device(text)?));
    }
    if let Some(text) = &track_blocks_text {
        let newest = client.newest_track_point(&info, &imei, &who).map_err(|e| e.to_string())?;
        place_tables.newest_track_point = Some(newest);
        place_tables.tracks_to_send = places::tracks_to_send(text, newest);
        place_tables.track_blocks = Some((places::TRACK_BLOCKS_VERSION, text.clone()));
    }
    if let Some(text) = &routes_text {
        place_tables.routes = Some((places::ROUTES_VERSION, places::routes_from_device(text)?));
    }
    // GetCurrentTrackingInterval: setting 24, when it reads as a number.
    let tracking_interval = crate::csv::parse(&settings_text)
        .into_iter()
        .find(|r| r.first().map(|v| v.trim()) == Some("24"))
        .and_then(|r| r.get(1).and_then(|v| v.trim().parse::<i32>().ok()))
        .map(i64::from);
    let device_commands = client
        .device_commands(&info, &imei, tracking_interval, &who)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

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
        tables: [
            Some((plan::CONTACTS, contacts_text)),
            Some((plan::SYNC_DATA, sync_data_text)),
            Some((SETTINGS, settings_text)),
            waypoints_text.map(|t| (places::WAYPOINTS, t)),
            track_blocks_text.map(|t| (places::TRACK_BLOCKS, t)),
            routes_text.map(|t| (places::ROUTES, t)),
            configuration_text.map(|t| (8, t)),
        ]
        .into_iter()
        .flatten()
        .collect(),
        info,
        who,
        last_synced: status.last_synced,
        firmware_update,
        plan,
        places: place_tables,
        device_commands,
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
        places: g.places.summary(),
        device_commands: g.device_commands.iter().map(summarize).collect(),
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
    pub places_after: Places,
    /// What Garmin sent back: its waypoints and routes, the track blocks it took.
    pub waypoints_from_garmin: Option<usize>,
    pub routes_from_garmin: Option<usize>,
    pub tracks_taken: Option<i64>,
    /// Garmin's device commands run, as acknowledged (each one's status).
    pub device_commands: Vec<DeviceCommand>,
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
    // Waypoints both ways (SyncWaypoints): Garmin's merged list replaces the device's.
    let mut waypoints_from_garmin = None;
    if let Some((version, mine)) = &g.places.waypoints {
        step("Syncing the waypoints");
        let merged = client.update_waypoints(&g.info, &g.imei, mine, &g.who).map_err(s)?;
        dev.write_table(places::WAYPOINTS, *version, "d,*\r\n").map_err(e)?;
        dev.batch_write(places::WAYPOINTS, *version, &places::waypoint_lines(&merged), max).map_err(e)?;
        waypoints_from_garmin = Some(merged.len());
    }
    // Tracks to Garmin only (SyncTracks).
    let mut tracks_taken = None;
    if let Some((version, _)) = &g.places.track_blocks {
        step("Sending the tracks");
        if !g.places.tracks_to_send.is_empty() {
            tracks_taken = Some(client.insert_tracks(&g.info, &g.imei, *version, &g.places.tracks_to_send, &g.who).map_err(s)?);
        }
    }
    // Routes both ways (SyncRoutes): the device's go, Garmin's replace them with their tracks.
    let mut routes_from_garmin = None;
    if let (Some((version, mine)), Some((tb_version, tb_text))) = (&g.places.routes, &g.places.track_blocks) {
        step("Syncing the routes");
        let merged = client.update_routes(&g.info, &g.imei, *tb_version, mine, &g.who).map_err(s)?;
        if !mine.is_empty() {
            dev.write_table(places::ROUTES, *version, "d,*\r\n").map_err(e)?;
        }
        if !tb_text.trim().is_empty() {
            for route in mine {
                dev.batch_write(places::TRACK_BLOCKS, *tb_version, &[format!("d,{}", route.track_id)], max).map_err(e)?;
            }
        }
        let mut route_rows = Vec::new();
        for route in &merged {
            if let Some((line, blocks)) = places::route_lines(route) {
                route_rows.push(line);
                dev.batch_write(places::TRACK_BLOCKS, *tb_version, &blocks, max).map_err(e)?;
            }
        }
        dev.batch_write(places::ROUTES, *version, &route_rows, max).map_err(e)?;
        routes_from_garmin = Some(merged.len());
    }

    // Device commands (ExecuteDeviceCommands, then AcknowledgeDeviceCommands):
    // each sub-command written as Garmin sent it; Garmin hears how each went,
    // and a failed one stops the sync here, before its time and the restart.
    let mut ran = g.device_commands.clone();
    if !ran.is_empty() {
        step("Running Garmin's device commands");
        let mut failures = Vec::new();
        for c in &mut ran {
            let Some(data) = c.data() else {
                failures.push(format!("{:?}: no data", c.transaction_id));
                continue;
            };
            let text = String::from_utf8_lossy(&data).into_owned();
            let mut ok = true;
            for item in commands::subcommands(&text) {
                let bytes = commands::subcommand_bytes(&item);
                if bytes.len() > commands::MAX_SUBCOMMAND {
                    failures.push(format!("{:?}: a sub-command is too long ({} bytes)", c.transaction_id, bytes.len()));
                    ok = false;
                    break;
                }
                let refusal = match dev.write_table_bytes(&bytes) {
                    Ok(()) => None,
                    Err(device::Error::Refused(t)) => Some(t),
                    Err(device::Error::NoAnswer) => Some("Response not received.".to_string()),
                    Err(other) => return Err(other.to_string()),
                };
                if let Some(t) = refusal {
                    c.status_code = commands::NACKED;
                    c.error_msg = Some(t.clone());
                    failures.push(format!("{:?}: {t}", c.transaction_id));
                    ok = false;
                    break;
                }
            }
            if ok {
                c.status_code = commands::ACKED;
            }
        }
        client.acknowledge_device_commands(&g.info, &g.imei, &ran, &g.who).map_err(s)?;
        if !failures.is_empty() {
            return Err(format!(
                "Garmin's device commands did not all run ({}). inReach Sync stops a sync here, so the time of \
                 the sync was not written and the inReach was not restarted. Its tables as they were are kept in {}.",
                failures.join("; "),
                backup_dir.display()
            ));
        }
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
    Ok(Applied {
        backup: backup_dir,
        left_over: after.plan,
        places_after: after.places.summary(),
        waypoints_from_garmin,
        routes_from_garmin,
        tracks_taken,
        device_commands: ran,
        mode_after_restart,
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
