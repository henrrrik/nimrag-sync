//! `nimrag-sync <command>`: everything the window does, without a display,
//! for scripts and agents. Each command takes `--json`, exits 0 or 1, and
//! writes errors to stderr.

use std::io::Write;
use std::path::Path;

use crate::{credentials, device, server, status};

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
       nimrag-sync preview [--json]          what a sync would change on the inReach: contacts, address
                                             book, quick texts, preset messages (reads only; device on)
       nimrag-sync sync [--apply] [--json]   sync as inReach Sync does: contacts, address book, presets, quick
                                             texts, settings (a trial run, the preview, without --apply)
       nimrag-sync update [--apply] [--reinstall] [--json]
                                             software updates for an inReach in USB drive mode (a trial run
                                             without --apply; --reinstall offers its own version too)
       nimrag-sync eject                     eject an inReach's USB drive, as the file manager does
       nimrag-sync login <email>             sign in to the Garmin account; the password (typed, or piped
                                             to stdin) goes to the keyring
       nimrag-sync logout                    sign out; the password leaves the keyring
       nimrag-sync --version                 the version
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
    if let Some(unknown) = flags.iter().find(|f| !["--json", "--apply", "--reinstall"].contains(f)) {
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
        "update" => update(apply, flags.contains(&"--reinstall"), json, out, err),
        "eject" => match crate::drive::find_here().into_iter().next() {
            Some(d) => match crate::drive::eject(&d) {
                Ok(()) => {
                    let _ = writeln!(out, "Ejected {}.", d.device.display());
                    0
                }
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    1
                }
            },
            None => {
                let _ = writeln!(err, "No inReach is a USB drive now.");
                1
            }
        },
        "login" => login(operands.first().copied(), out, err),
        "preview" => preview(json, out, err),
        "sync" => sync(apply, json, out, err),
        "logout" => logout(out, err),
        "version" | "--version" => {
            let _ = writeln!(out, "nimrag-sync {}", env!("CARGO_PKG_VERSION"));
            0
        }
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

/// The password from stdin: typed without echo at a terminal, else read
/// whole from a pipe (`--password-stdin` style), without its last newline.
fn read_password(err: &mut dyn Write) -> std::io::Result<String> {
    use std::io::{BufRead, Read};
    let fd = libc::STDIN_FILENO;
    // SAFETY: isatty only inspects the descriptor.
    if unsafe { libc::isatty(fd) } == 1 {
        let _ = write!(err, "Password: ");
        let _ = err.flush();
        // SAFETY: a zeroed termios is filled by tcgetattr before use.
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        let have = unsafe { libc::tcgetattr(fd, &mut saved) } == 0;
        if have {
            let mut quiet = saved;
            quiet.c_lflag &= !libc::ECHO;
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &quiet) };
        }
        let mut line = String::new();
        let read = std::io::stdin().lock().read_line(&mut line);
        if have {
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
        }
        let _ = writeln!(err);
        read?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text.strip_suffix('\n').map(|t| t.strip_suffix('\r').unwrap_or(t)).unwrap_or(&text).to_string())
}

fn login(email: Option<&str>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(email) = email else {
        let _ = writeln!(err, "nimrag-sync: login needs the Garmin account's email address");
        return 1;
    };
    let password = match read_password(err) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    match credentials::sign_in(&credentials::config_dir(), &credentials::Keyring::default(), email, &password) {
        Ok(()) => {
            let _ = writeln!(out, "Signed in as {}; the password is in the keyring.", email.trim());
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            1
        }
    }
}

fn logout(out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match credentials::sign_out(&credentials::config_dir(), &credentials::Keyring::default()) {
        Ok(Some(email)) => {
            let _ = writeln!(out, "Signed out {email}; its password left the keyring.");
            0
        }
        Ok(None) => {
            let _ = writeln!(out, "Not signed in.");
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            1
        }
    }
}

fn preview(json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let p = match crate::sync::preview_here() {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    if json {
        let _ = writeln!(out, "{:#}", serde_json::to_value(&p).unwrap_or_default());
        return 0;
    }
    let _ = writeln!(out, "{} (software {}), last synced {}", p.device, p.firmware, p.last_synced.as_deref().unwrap_or("never"));
    if let Some(f) = &p.firmware_update {
        let _ = writeln!(
            out,
            "Garmin offers software {}.{}.{}: inReach Sync installs it first, nimrag-sync cannot yet, so it will not sync.",
            f.version_major, f.version_minor, f.version_rev
        );
    }
    show_plan(out, &p.plan);
    show_places(out, &p.places);
    if p.device_commands.is_empty() {
        let _ = writeln!(out, "Device commands: none from Garmin");
    } else {
        for c in &p.device_commands {
            let tables: Vec<String> = c.tables.iter().map(u8::to_string).collect();
            let _ = writeln!(
                out,
                "Device command {}: {} bytes for table {} (written as Garmin sent it)",
                c.transaction_id.map(|t| t.to_string()).unwrap_or_else(|| "?".into()),
                c.bytes,
                tables.join(", ")
            );
        }
    }
    let _ = writeln!(
        out,
        "{}",
        if p.plan.is_change() { "A sync would make these changes: nimrag-sync sync --apply" } else { "The inReach is up to date." }
    );
    0
}

fn show_plan(out: &mut dyn Write, plan: &crate::plan::Plan) {
    let contact = |c: &crate::plan::ContactFields| {
        let name = format!("{} {}", c.first_name, c.last_name).trim().to_string();
        let reach: Vec<&str> = [&c.email, &c.phone, &c.inreach].into_iter().map(String::as_str).filter(|s| !s.is_empty()).collect();
        format!("{name} <{}>", reach.join(", "))
    };
    section(out, "Contacts from Garmin", &plan.contacts, contact);
    let _ = writeln!(out, "  {} made on the device, kept", plan.device_contacts_kept);
    section(out, "Address book", &plan.addresses, |i| i.value.clone());
    if !plan.addresses_kept.is_empty() {
        let _ = writeln!(out, "  {} that are no Garmin contact's stay (the device keeps its address book)", plan.addresses_kept.len());
    }
    section(out, "Quick texts", &plan.quick_texts, |i| i.value.clone());
    section(out, "Preset messages", &plan.presets, |i| {
        let (to, message) = i.value.split_once('|').unwrap_or(("", &i.value));
        format!("{message}  (to {to})")
    });
    if !plan.other_sync_data_removed.is_empty() {
        let _ = writeln!(out, "Other sync data: {} rows cleared", plan.other_sync_data_removed.len());
    }
    let st = &plan.settings;
    if st.answered {
        let _ = writeln!(out, "Settings from Garmin: {} unchanged, {} changed", st.unchanged, st.changed.len());
        for c in &st.changed {
            let from = c.from.as_deref().unwrap_or("(none)");
            let to = c.to.as_deref().unwrap_or("(deleted)");
            let _ = writeln!(out, "  ~ {}: {from} -> {to}", c.id);
        }
        if let Some(c) = &st.device_type_change {
            let _ = writeln!(
                out,
                "  ! device type (setting 16) {} -> {}: the inReach resets ALL its settings to their defaults first",
                c.from.as_deref().unwrap_or("?"),
                c.to.as_deref().unwrap_or("?")
            );
        }
    } else {
        let _ = writeln!(out, "Settings from Garmin: none");
    }
    for (id, value) in &plan.timestamps {
        let what = match *id {
            89 => "quick texts changed on Garmin at",
            97 => "presets changed on Garmin at",
            73 => "synced at",
            _ => "value",
        };
        let _ = writeln!(out, "Setting {id} ({what}): {value}");
    }
}

fn show_places(out: &mut dyn Write, p: &crate::sync::Places) {
    match p.waypoints {
        Some(n) => {
            let _ = writeln!(out, "Waypoints: the device's {n} go to Garmin; Garmin's merged list replaces them on the device");
        }
        None => {
            let _ = writeln!(out, "Waypoints: not synced (the device's table is not a version inReach Sync syncs)");
        }
    }
    match p.routes {
        Some(n) => {
            let _ = writeln!(out, "Routes: the device's {n} go to Garmin; Garmin's routes, with their tracks, replace them");
        }
        None => {
            let _ = writeln!(out, "Routes: not synced");
        }
    }
    match p.tracks_to_send {
        Some(n) => {
            let since = p.newest_track_point.as_deref().unwrap_or("the start");
            let _ = writeln!(out, "Tracks: {n} high-detail blocks go to Garmin (Garmin has points up to {since})");
        }
        None => {
            let _ = writeln!(out, "Tracks: not synced");
        }
    }
}

fn sync(apply: bool, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if !apply {
        return preview(json, out, err);
    }
    let mut steps = Vec::new();
    let result = crate::sync::apply_here(&mut |s: &str| {
        if !json {
            println!("{s}…");
        }
        steps.push(s.to_string());
    });
    match result {
        Ok(applied) => {
            if json {
                let _ = writeln!(out, "{:#}", serde_json::json!({ "steps": steps, "backup": applied.backup, "left_over": applied.left_over }));
            } else {
                let _ = writeln!(out, "Synced. The tables as they were are kept in {}", applied.backup.display());
                let _ = match applied.mode_after_restart {
                    Some(device::AppMode::MassStorage) => writeln!(out, "The inReach restarted as a USB drive."),
                    Some(device::AppMode::App) => writeln!(out, "The inReach restarted and is on."),
                    Some(m) => writeln!(out, "The inReach restarted in {} mode.", mode_name(&m)),
                    None => writeln!(out, "The inReach restarted; it has not answered since."),
                };
                if let Some(n) = applied.waypoints_from_garmin {
                    let _ = writeln!(out, "Waypoints: Garmin sent {n}, now on the device.");
                }
                if let Some(n) = applied.routes_from_garmin {
                    let _ = writeln!(out, "Routes: Garmin sent {n}.");
                }
                for c in &applied.device_commands {
                    let status = if c.status_code == crate::commands::ACKED { "ran" } else { "failed" };
                    let _ = writeln!(out, "Device command {}: {status}.", c.transaction_id.map(|t| t.to_string()).unwrap_or_else(|| "?".into()));
                }
                if let Some(n) = applied.tracks_taken {
                    let _ = writeln!(out, "Tracks: Garmin took {n} blocks.");
                }
                if applied.left_over.is_change() {
                    let _ = writeln!(out, "Read back before the restart, the inReach still differs from Garmin:");
                    show_plan(out, &applied.left_over);
                } else {
                    let _ = writeln!(out, "Read back before the restart, the inReach matches Garmin.");
                }
            }
            if applied.left_over.is_change() { 1 } else { 0 }
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            if !steps.is_empty() {
                let _ = writeln!(err, "Stopped during: {}", steps.last().unwrap());
            }
            1
        }
    }
}

fn section<T>(out: &mut dyn Write, title: &str, c: &crate::plan::Changes<T>, show: impl Fn(&T) -> String) {
    if c.left_alone {
        let _ = writeln!(out, "{title}: not touched ({} on the device)", c.unchanged.len());
        return;
    }
    let _ = writeln!(out, "{title}: {} unchanged, {} to add, {} to remove", c.unchanged.len(), c.added.len(), c.removed.len());
    for item in &c.added {
        let _ = writeln!(out, "  + {}", show(item));
    }
    for item in &c.removed {
        let _ = writeln!(out, "  - {}", show(item));
    }
}

fn update(apply: bool, reinstall: bool, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let client = crate::updates::Client::default();
    let check = match crate::firmware::check(&client, reinstall) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    if !apply {
        if json {
            let _ = writeln!(out, "{:#}", serde_json::to_value(&check).unwrap_or_default());
            return 0;
        }
        let d = &check.device;
        let _ = writeln!(out, "{} ({}), software {}", d.description, d.part_number, d.software_version);
        if check.updates.is_empty() {
            let _ = writeln!(out, "No software update: it is up to date.");
            return 0;
        }
        for u in &check.updates {
            let _ = writeln!(
                out,
                "{} {} ({}, {:.1} MB){} -> {}",
                u.product_name.as_deref().unwrap_or(&u.part_number),
                u.version.as_deref().unwrap_or("?"),
                u.severity.as_deref().unwrap_or(""),
                u.deliverable.size_in_bytes as f64 / 1e6,
                if u.installation.is_reinstall { ", reinstall" } else { "" },
                u.place().map(|p| p.display().to_string()).unwrap_or_else(|| "?".into())
            );
            if let Some(eula) = u.secure_eula_url.as_deref().or(u.eula_url.as_deref()) {
                let _ = writeln!(out, "  License: {eula}");
            }
        }
        let _ = writeln!(
            out,
            "With --apply it is downloaded, checked, copied to the inReach's drive and the drive ejected; the \
             inReach installs it when it restarts. Installing means accepting Garmin's license above."
        );
        return 0;
    }
    let result = crate::firmware::install(
        &client,
        reinstall,
        &mut |s: &str| {
            if !json {
                println!("{s}…");
            }
        },
        &mut |_, _| {},
    );
    match result {
        Ok(done) if json => {
            let _ = writeln!(out, "{:#}", serde_json::to_value(&done).unwrap_or_default());
            0
        }
        Ok(done) if done.files.is_empty() => {
            let _ = writeln!(out, "No software update: it is up to date.");
            0
        }
        Ok(done) => {
            for f in &done.files {
                let _ = writeln!(out, "Copied {}", f.display());
            }
            let _ = writeln!(out, "The drive is ejected: the inReach installs the update when it restarts. Leave it plugged in until it is done.");
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            1
        }
    }
}

/// The sync server's host name, for the credentials.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
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
        let who = credentials::load(&credentials::config_dir(), &credentials::Keyring::default(), &host_of(&info.sync_url))
            .map_err(|e| e.to_string())?;
        let status = client.device_sync(&info, &imei, &who).map_err(|e| e.to_string())?;
        let count = |n: Option<Option<usize>>| n.flatten().map(serde_json::Value::from).unwrap_or(serde_json::Value::Null);
        let contacts = client.contacts(&info, &imei, &who).map_err(|e| e.to_string())?;
        let presets = client.presets(&info, &imei, &who).map_err(|e| e.to_string())?;
        let quick_texts = client.quick_texts(&info, &imei, &who).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "sync_url": info.sync_url,
            "provisioned": info.provisioned,
            "synced": status.synced,
            "last_synced": status.last_synced,
            "changed": status.sync_data,
            "contacts": count(contacts.as_ref().map(|c| c.as_ref().map(Vec::len))),
            "presets": count(presets.as_ref().map(|c| c.as_ref().map(Vec::len))),
            "quick_texts": count(quick_texts.as_ref().map(|c| c.as_ref().map(Vec::len))),
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
        assert_eq!(run_args(dir.path(), &["--version"]), (0, format!("nimrag-sync {}\n", env!("CARGO_PKG_VERSION")), String::new()));
        let (code, _, err) = run_args(dir.path(), &["read-table"]);
        assert_eq!(code, 1);
        assert!(err.contains("needs a table ID"));
        assert_eq!(host_of("https://sync.explore.garmin.com"), "sync.explore.garmin.com");
        assert_eq!(host_of("https://sync.example/x/"), "sync.example");
        let (code, _, err) = run_args(dir.path(), &["login"]);
        assert_eq!(code, 1);
        assert!(err.contains("email address"));
        for command in ["tables", "app-mode"] {
            let (code, _, err) = run_args(dir.path(), &[command]);
            assert_eq!(code, 1);
            assert!(err.contains("No inReach"));
        }
    }
}
