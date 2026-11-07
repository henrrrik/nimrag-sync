//! `nimrag-sync <command>`: everything the window does, without a display,
//! for scripts and agents. Each command takes `--json`, exits 0 or 1, and
//! writes errors to stderr.

use std::io::Write;
use std::path::Path;

use crate::{device, server, status};

const USAGE: &str = "\
Usage: nimrag-sync [open]                  open the window
       nimrag-sync devices [--json]          the connected inReach devices, their mode and software
       nimrag-sync drive-mode [--apply] [--json]
                                             restart an inReach as a USB drive (a trial run without --apply)
       nimrag-sync app-mode [--apply] [--json]
                                             restart an inReach in its normal mode (a trial run without --apply)
       nimrag-sync tables [--json]           the tables the inReach has, and their versions (read only)
       nimrag-sync read-table <id> [--json]  the rows of one table as the inReach sends them (read only)
       nimrag-sync server [<imei>] [--json]  what Garmin's inReach service holds for the device (read only;
                                             the IMEI comes from the device, in app mode, when not given)
       nimrag-sync help                      this text
";

/// Runs the command in `args` (args[0] is the program). Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run_in(Path::new(device::SYSFS_HIDRAW), args, out, err)
}

pub fn run_in(sysfs: &Path, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    let rest: Vec<&str> = args.iter().skip(2).map(String::as_str).collect();
    let (flags, operands): (Vec<&str>, Vec<&str>) = rest.iter().partition(|a| a.starts_with("--"));
    let json = flags.contains(&"--json");
    let apply = flags.contains(&"--apply");
    if let Some(unknown) = flags.iter().find(|f| !["--json", "--apply"].contains(f)) {
        let _ = writeln!(err, "nimrag-sync: unknown option {unknown}\n\n{USAGE}");
        return 1;
    }
    match command {
        "devices" => devices(sysfs, json, out, err),
        "drive-mode" => drive_mode(sysfs, apply, json, out, err),
        "app-mode" => app_mode(sysfs, apply, json, out, err),
        "tables" => tables(sysfs, json, out, err),
        "read-table" => match operands.first().and_then(|t| t.parse::<u8>().ok()) {
            Some(id) => read_table(sysfs, id, json, out, err),
            None => {
                let _ = writeln!(err, "nimrag-sync: read-table needs a table ID (see `nimrag-sync tables`)");
                1
            }
        },
        "server" => server(sysfs, operands.first().copied(), json, out, err),
        "help" | "--help" | "-h" => {
            let _ = write!(out, "{USAGE}");
            0
        }
        other => {
            let _ = writeln!(err, "nimrag-sync: unknown command {other}\n\n{USAGE}");
            1
        }
    }
}

fn devices(sysfs: &Path, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let list = status::devices(sysfs, true);
    if json {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&list).unwrap_or_default());
    } else if list.is_empty() {
        let _ = writeln!(out, "No inReach is connected over USB.");
    } else {
        for d in &list {
            let _ = writeln!(out, "{} ({}, {:04x}:{:04x})", d.name, d.node, d.vendor, d.product);
            if let Some(v) = &d.version {
                let _ = writeln!(out, "  product:  {}\n  software: {}", v.product, v.firmware);
            }
            if let Some(m) = &d.mode {
                let _ = writeln!(out, "  mode:     {}", mode_name(m));
            }
            if let Some(e) = &d.error {
                let _ = writeln!(err, "  error:    {e}");
            }
        }
    }
    if list.iter().any(|d| d.error.is_some()) { 1 } else { 0 }
}

fn drive_mode(sysfs: &Path, apply: bool, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let list = status::devices(sysfs, true);
    let Some(d) = list.first() else {
        let _ = writeln!(err, "No inReach is connected over USB.");
        return 1;
    };
    if let Some(e) = &d.error {
        let _ = writeln!(err, "{e}");
        return 1;
    }
    let already = d.mode == Some(device::AppMode::MassStorage);
    let switched = if apply && !already {
        match status::switch_to_drive(sysfs, &d.node) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(err, "{e}");
                return 1;
            }
        }
    } else {
        false
    };
    if json {
        let answer = serde_json::json!({
            "device": d, "already_a_drive": already, "applied": apply, "switched": switched,
        });
        let _ = writeln!(out, "{answer:#}");
    } else if already {
        let _ = writeln!(out, "{} is a USB drive already.", d.name);
    } else if switched {
        let _ = writeln!(out, "{} restarts as a USB drive.", d.name);
    } else {
        let _ = writeln!(
            out,
            "{} is in {} mode. With --apply it restarts as a USB drive; if it is switched on, \
             it stops what it is doing, tracking too.",
            d.name,
            d.mode.as_ref().map(mode_name).unwrap_or("an unknown")
        );
    }
    0
}

/// The first inReach, opened, with an error line written when there is none.
fn open_first(sysfs: &Path, err: &mut dyn Write) -> Option<device::InReach<device::Hidraw>> {
    let Some(found) = device::find(sysfs).into_iter().next() else {
        let _ = writeln!(err, "No inReach is connected over USB.");
        return None;
    };
    match device::InReach::open(&found) {
        Ok(dev) => Some(dev),
        Err(e) => {
            let _ = writeln!(err, "{e}");
            None
        }
    }
}

fn app_mode(sysfs: &Path, apply: bool, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(mut dev) = open_first(sysfs, err) else { return 1 };
    let mode = match dev.app_mode() {
        Ok(m) => m,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    let already = mode == device::AppMode::App;
    let restarted = apply && !already;
    if restarted && let Err(e) = dev.restart_app_mode() {
        let _ = writeln!(err, "{e}");
        return 1;
    }
    if json {
        let answer = serde_json::json!({ "mode": mode, "already_on": already, "applied": apply, "restarted": restarted });
        let _ = writeln!(out, "{answer:#}");
    } else if already {
        let _ = writeln!(out, "The inReach is on (app mode) already.");
    } else if restarted {
        let _ = writeln!(out, "The inReach restarts in its normal mode; it switches on.");
    } else {
        let _ = writeln!(
            out,
            "The inReach is in {} mode. With --apply it restarts in its normal mode and switches on.",
            mode_name(&mode)
        );
    }
    0
}

fn tables(sysfs: &Path, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(mut dev) = open_first(sysfs, err) else { return 1 };
    let mut rows = Vec::new();
    for &(id, name) in device::TABLES {
        match dev.table_version(id) {
            Ok(version) => rows.push(serde_json::json!({ "id": id, "name": name, "version": version })),
            Err(e) => {
                let _ = writeln!(err, "table {id} ({name}): {e}");
                return 1;
            }
        }
    }
    if json {
        let _ = writeln!(out, "{:#}", serde_json::Value::Array(rows));
    } else {
        for r in &rows {
            let version = r["version"].as_u64().map(|v| format!("version {v}")).unwrap_or_else(|| "absent".into());
            let _ = writeln!(out, "{:>3}  {:<15} {version}", r["id"], r["name"].as_str().unwrap_or(""));
        }
    }
    0
}

fn read_table(sysfs: &Path, id: u8, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(mut dev) = open_first(sysfs, err) else { return 1 };
    let version = match dev.table_version(id) {
        Ok(Some(v)) => v,
        Ok(None) => {
            let _ = writeln!(err, "The inReach has no table {id}.");
            return 1;
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    match dev.read_table(id, version) {
        Ok(text) if json => {
            let rows: Vec<&str> = text.split(['\r', '\n']).filter(|l| !l.is_empty()).collect();
            let answer = serde_json::json!({ "id": id, "version": version, "rows": rows });
            let _ = writeln!(out, "{answer:#}");
            0
        }
        Ok(text) => {
            let _ = write!(out, "{}", text.replace("\r\n", "\n"));
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            1
        }
    }
}

/// Until the credentials live in the keyring: a netrc file, $NIMRAG_NETRC or
/// ~/.config/nimrag-sync/test.netrc.
fn credentials_for(host: &str) -> Result<server::Credentials, String> {
    let path = std::env::var_os("NIMRAG_NETRC").map(std::path::PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var_os("HOME").unwrap_or_default();
        Path::new(&home).join(".config/nimrag-sync/test.netrc")
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    server::netrc_credentials(&text, host)
        .ok_or_else(|| format!("{} has no login and password for {host}", path.display()))
}

fn server(sysfs: &Path, imei: Option<&str>, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let imei = match imei {
        Some(i) => i.to_string(),
        None => {
            let Some(mut dev) = open_first(sysfs, err) else { return 1 };
            match dev.imei() {
                Ok(Some(i)) => i,
                Ok(None) => {
                    let _ = writeln!(err, "The inReach did not tell its IMEI.");
                    return 1;
                }
                Err(e) => {
                    let _ = writeln!(err, "{e} (the IMEI is read in app mode: nimrag-sync app-mode --apply)");
                    return 1;
                }
            }
        }
    };
    let client = server::Client::default();
    let result = (|| -> Result<serde_json::Value, String> {
        let info = client.sync_info(&imei).map_err(|e| e.to_string())?;
        let host = info.sync_url.trim_start_matches("https://").trim_start_matches("http://");
        let host = host.split('/').next().unwrap_or(host).to_string();
        let who = credentials_for(&host)?;
        let status = client.device_sync(&info, &imei, &who).map_err(|e| e.to_string())?;
        let count = |n: Option<usize>| n.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null);
        let contacts = client.contacts(&info, &imei, &who).map_err(|e| e.to_string())?;
        let presets = client.presets(&info, &imei, &who).map_err(|e| e.to_string())?;
        let quick_texts = client.quick_texts(&info, &imei, &who).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "sync_url": info.sync_url,
            "provisioned": info.provisioned,
            "synced": status.synced,
            "last_synced": status.last_synced,
            "changed": status.sync_data,
            "contacts": count(contacts.as_ref().map(Vec::len)),
            "presets": count(presets.as_ref().map(Vec::len)),
            "quick_texts": count(quick_texts.as_ref().map(Vec::len)),
        }))
    })();
    match result {
        Ok(v) if json => {
            let _ = writeln!(out, "{v:#}");
            0
        }
        Ok(v) => {
            let names = ["contacts", "quick texts", "presets", "waypoints", "routes", "device commands"];
            let _ = writeln!(out, "Sync server:  {}", v["sync_url"].as_str().unwrap_or(""));
            let _ = writeln!(out, "Last synced:  {}", v["last_synced"].as_str().unwrap_or("never"));
            let _ = writeln!(out, "Up to date:   {}", if v["synced"].as_bool() == Some(true) { "yes" } else { "no" });
            for key in ["contacts", "presets", "quick_texts"] {
                let n = v[key].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "none sent".into());
                let _ = writeln!(out, "{:<13} {n}", format!("{}:", key.replace('_', " ")));
            }
            for h in v["changed"].as_array().into_iter().flatten() {
                let name = names.get(h["DataType"].as_u64().unwrap_or(99) as usize).unwrap_or(&"other");
                let when = h["LastModified"].as_str().unwrap_or("never");
                let _ = writeln!(out, "  {name} changed: {when}");
            }
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            1
        }
    }
}

fn mode_name(m: &device::AppMode) -> &'static str {
    match m {
        device::AppMode::App => "app",
        device::AppMode::Charger => "charger",
        device::AppMode::MassStorage => "USB drive",
        device::AppMode::Test => "test",
        device::AppMode::Bootloader => "bootloader",
        device::AppMode::Unknown(_) => "an unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(sysfs: &Path, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = std::iter::once("nimrag-sync").chain(args.iter().copied()).map(String::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_in(sysfs, &args, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn without_devices() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(run_args(dir.path(), &["devices"]), (0, "No inReach is connected over USB.\n".into(), "".into()));
        assert_eq!(run_args(dir.path(), &["devices", "--json"]).1, "[]\n");
        let (code, _, err) = run_args(dir.path(), &["drive-mode", "--apply"]);
        assert_eq!(code, 1);
        assert!(err.contains("No inReach"));
    }

    #[test]
    fn unknown_commands_and_options_fail_with_the_usage() {
        let dir = tempfile::tempdir().unwrap();
        let (code, _, err) = run_args(dir.path(), &["flash"]);
        assert_eq!(code, 1);
        assert!(err.contains("unknown command flash") && err.contains("Usage"));
        assert_eq!(run_args(dir.path(), &["devices", "--force"]).0, 1);
        assert_eq!(run_args(dir.path(), &["help"]).0, 0);
        let (code, _, err) = run_args(dir.path(), &["read-table"]);
        assert_eq!(code, 1);
        assert!(err.contains("needs a table ID"));
        for command in ["tables", "app-mode"] {
            let (code, _, err) = run_args(dir.path(), &[command]);
            assert_eq!(code, 1);
            assert!(err.contains("No inReach"));
        }
    }
}
