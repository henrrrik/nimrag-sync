//! An inReach on its USB HID interface (Linux hidraw).
//!
//! Only messages verified on a real inReach Mini are sent here: the version,
//! the app mode, and the restart into its USB drive mode.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::frame::{self, Assembler, Message, Pushed};

pub const SYSFS_HIDRAW: &str = "/sys/class/hidraw";

/// inReach Sync's DefaultTimeout.
pub const TIMEOUT: Duration = Duration::from_millis(1800);
/// WRITE_TABLE waits eight times as long (DeLormeBinary.WriteTable).
pub const WRITE_TIMEOUT: Duration = Duration::from_millis(1800 * 8);

pub const VERSION: u16 = 0xA001;
pub const TABLE_VERSION: u16 = 0xFC01;
pub const READ_TABLE: u16 = 0xFC02;
pub const WRITE_TABLE: u16 = 0xFC03;
pub const APPLY_DEFAULT_SETTINGS: u16 = 0xFC06;
pub const RESET_FOR_MASS_STORAGE: u16 = 0xFC07;
pub const READ_APP_MODE: u16 = 0xFC08;
pub const RESET_DEVICE_REQ: u16 = 0xAAFE;

/// The tables inReach Sync knows (inReachSE.Tables), by their ID on the device.
pub const TABLES: &[(u8, &str)] = &[
    (1, "Contacts"),
    (2, "TrackPoints"),
    (3, "Messages"),
    (4, "MessageThreads"),
    (5, "Conversations"),
    (6, "Words"),
    (7, "Settings"),
    (8, "Configuration"),
    (9, "SyncData"),
    (10, "EventLog"),
    (12, "Forms"),
    (13, "FormFields"),
    (14, "Waypoints"),
    (15, "TrackBlocks"),
    (16, "Routes"),
    (18, "Locations"),
];

/// The USB IDs inReach Sync looks for (ConnectedDevices.cs).
pub const KNOWN: &[(u16, u16, &str)] = &[
    (0x091E, 0x0B89, "inReach Mini"),
    (0x1163, 0x0300, "inReach"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppMode {
    App,
    Charger,
    MassStorage,
    Test,
    Bootloader,
    Unknown(u8),
}

impl From<u8> for AppMode {
    fn from(v: u8) -> Self {
        match v {
            0 => AppMode::App,
            1 => AppMode::Charger,
            2 => AppMode::MassStorage,
            3 => AppMode::Test,
            4 => AppMode::Bootloader,
            other => AppMode::Unknown(other),
        }
    }
}

/// An inReach found in sysfs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Found {
    pub node: PathBuf,
    pub name: &'static str,
    pub vendor: u16,
    pub product: u16,
}

/// How many bytes one table script may have (BatchWriter): 2046 on an inReach
/// Mini or software 2.6.0.1 and later, else 1022 (an unreadable version counts as 1.0).
pub fn max_script_len(product: &str, firmware: &str) -> usize {
    let mini = product.to_lowercase().starts_with("inreach mini");
    let parts: Vec<u64> = firmware.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let version = |i: usize| parts.get(i).copied().unwrap_or(0);
    let new_enough = (version(0), version(1), version(2), version(3)) >= (2, 6, 0, 1);
    if mini || new_enough { 2046 } else { 1022 }
}

/// The 180-byte answer to VERSION (DeLormeBinary.GetDeviceVersion).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Version {
    pub company: String,
    pub product: String,
    pub firmware: String,
    pub gps_firmware: String,
    pub serial: String,
    pub extra: String,
}

impl Version {
    pub fn parse(data: &[u8]) -> Option<Version> {
        if data.len() != 180 {
            return None;
        }
        let text = |offset: usize, size: usize| {
            let field = &data[offset..offset + size];
            let end = field.iter().position(|&b| b == 0).unwrap_or(size);
            String::from_utf8_lossy(&field[..end]).into_owned()
        };
        Some(Version {
            company: text(4, 32),
            product: text(36, 32),
            firmware: text(68, 32),
            gps_firmware: text(100, 48),
            serial: text(148, 16),
            extra: text(164, 16),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no access to {0}; allow it with the udev rule in the README, then plug the device in again")]
    Permission(PathBuf),
    #[error("cannot open {0}: {1}")]
    Open(PathBuf, std::io::Error),
    #[error("the device stopped responding: {0}")]
    Io(#[from] std::io::Error),
    #[error("the device did not answer")]
    NoAnswer,
    #[error("the device refused a write: {0}")]
    Refused(String),
    #[error("a line is too long for the device ({0} bytes)")]
    LineTooLong(usize),
    #[error("another nimrag-sync is using the inReach; try again when it is done")]
    Busy,
}

/// Fails as `batch_write` would on a line too long for one script of `max`
/// bytes, before anything is written.
pub fn check_lines<'a>(lines: impl IntoIterator<Item = &'a String>, max: usize) -> Result<(), Error> {
    for line in lines {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        let len = line.len() + 2;
        if len > max {
            return Err(Error::LineTooLong(len));
        }
    }
    Ok(())
}

/// The inReach devices with a hidraw node, read from `sysfs` (normally
/// /sys/class/hidraw) only.
pub fn find(sysfs: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(sysfs) else { return Vec::new() };
    let mut names: Vec<_> = entries.flatten().map(|e| e.file_name()).collect();
    names.sort();
    let mut found = Vec::new();
    for name in names {
        let Ok(uevent) = std::fs::read_to_string(sysfs.join(&name).join("device/uevent")) else {
            continue;
        };
        let Some(id) = uevent.lines().find_map(|l| l.strip_prefix("HID_ID=")) else { continue };
        let parts: Vec<_> = id.split(':').collect();
        let [_, vendor, product] = parts[..] else { continue };
        let (Ok(vendor), Ok(product)) =
            (u32::from_str_radix(vendor, 16), u32::from_str_radix(product, 16))
        else {
            continue;
        };
        if let Some(&(v, p, label)) =
            KNOWN.iter().find(|(v, p, _)| *v as u32 == vendor && *p as u32 == product)
        {
            found.push(Found { node: Path::new("/dev").join(&name), name: label, vendor: v, product: p });
        }
    }
    found
}

/// The two ends a conversation needs; a hidraw file, or a fake in tests.
pub trait Transport {
    /// One input report, or `None` when none comes within `timeout`.
    fn read_report(&mut self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>>;
    fn write_report(&mut self, report: &[u8]) -> std::io::Result<()>;
}

pub struct Hidraw {
    file: File,
}

/// How long an open waits for another nimrag-sync to let go of the device.
const LOCK_WAIT: Duration = Duration::from_secs(10);

impl Hidraw {
    /// Opens the node and locks it, so that no other nimrag-sync (another
    /// window, the command line) talks to the device meanwhile and takes its
    /// answers. Waits up to `LOCK_WAIT` for one that has it.
    pub fn open(node: &Path) -> Result<Hidraw, Error> {
        let file = match OpenOptions::new().read(true).write(true).open(node) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => return Err(Error::Permission(node.into())),
            Err(e) => return Err(Error::Open(node.into(), e)),
        };
        lock_exclusive(&file, LOCK_WAIT).map_err(|e| match e.kind() {
            ErrorKind::WouldBlock => Error::Busy,
            _ => Error::Open(node.into(), e),
        })?;
        Ok(Hidraw { file })
    }
}

/// An exclusive `flock` on `file`, waiting up to `wait` for it; a
/// `WouldBlock` error when it stays taken. Released when the file is closed.
pub fn lock_exclusive(file: &File, wait: Duration) -> std::io::Result<()> {
    let start = Instant::now();
    loop {
        // SAFETY: a valid descriptor, owned by `file` for the call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        match e.kind() {
            ErrorKind::Interrupted => continue,
            ErrorKind::WouldBlock if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(100)),
            _ => return Err(e),
        }
    }
}

impl Transport for Hidraw {
    fn read_report(&mut self, timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd for the duration of the call.
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            return if e.kind() == ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        if n == 0 {
            return Ok(None);
        }
        let mut buf = [0u8; frame::REPORT_LEN];
        let got = self.file.read(&mut buf)?;
        Ok(Some(buf[..got].to_vec()))
    }

    fn write_report(&mut self, report: &[u8]) -> std::io::Result<()> {
        self.file.write_all(report)
    }
}

/// A conversation with an inReach.
pub struct InReach<T: Transport> {
    transport: T,
}

impl InReach<Hidraw> {
    pub fn open(found: &Found) -> Result<Self, Error> {
        Ok(InReach { transport: Hidraw::open(&found.node)? })
    }
}

impl<T: Transport> InReach<T> {
    pub fn new(transport: T) -> Self {
        InReach { transport }
    }

    /// Reads and drops whatever the device sent before a request.
    fn drain(&mut self) -> Result<(), Error> {
        while self.transport.read_report(Duration::from_millis(100))?.is_some() {}
        Ok(())
    }

    pub fn send(&mut self, code: u16, data: &[u8]) -> Result<(), Error> {
        for report in frame::reports(&frame::encode(code, data)) {
            self.transport.write_report(&report)?;
        }
        Ok(())
    }

    /// The next message `code`; others are passed over. `None` when none comes in time.
    pub fn receive(&mut self, code: u16, timeout: Duration) -> Result<Option<Message>, Error> {
        let end = Instant::now() + timeout;
        let mut assembler = Assembler::default();
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let Some(report) = self.transport.read_report(left)? else { return Ok(None) };
            let Some(part) = frame::part_of(&report) else { continue };
            match assembler.push(part) {
                Pushed::Done(m) if m.code == code => return Ok(Some(m)),
                Pushed::Done(m) => log::debug!("message 0x{:04x} passed over", m.code),
                Pushed::Corrupt(c) => log::info!("message 0x{c:04x} with a bad checksum, skipped"),
                Pushed::Pending => {}
            }
        }
    }

    pub fn request(&mut self, code: u16, data: &[u8]) -> Result<Message, Error> {
        self.drain()?;
        self.send(code, data)?;
        self.receive(code, TIMEOUT)?.ok_or(Error::NoAnswer)
    }

    pub fn app_mode(&mut self) -> Result<AppMode, Error> {
        let m = self.request(READ_APP_MODE, &[])?;
        match m.data[..] {
            [mode] => Ok(AppMode::from(mode)),
            _ => Err(Error::NoAnswer),
        }
    }

    pub fn version(&mut self) -> Result<Version, Error> {
        let m = self.request(VERSION, &[])?;
        Version::parse(&m.data).ok_or(Error::NoAnswer)
    }

    /// The version of a table; `None` when the device has no such table (0xFF).
    pub fn table_version(&mut self, table: u8) -> Result<Option<u8>, Error> {
        let m = self.request(TABLE_VERSION, &[table])?;
        // DeLormeBinary.TableVersion takes a reply without data as version 0.
        match m.data.first().copied().unwrap_or(0) {
            0xFF => Ok(None),
            v => Ok(Some(v)),
        }
    }

    /// The rows of a table as the device sends them: CSV text, `\r\n`-separated,
    /// without a header row. The device streams it in READ_TABLE messages and
    /// ends with one without data; each part restarts the timeout.
    pub fn read_table(&mut self, table: u8, version: u8) -> Result<String, Error> {
        self.drain()?;
        self.send(READ_TABLE, &[table, version])?;
        let mut text = Vec::new();
        loop {
            let m = self.receive(READ_TABLE, TIMEOUT)?.ok_or(Error::NoAnswer)?;
            if m.data.is_empty() {
                return Ok(String::from_utf8_lossy(&text).into_owned());
            }
            text.extend_from_slice(&m.data);
        }
    }

    /// The device's IMEI: setting 9 in the Settings table (the serial in the
    /// VERSION reply is zeros on an inReach Mini). Needs app mode.
    pub fn imei(&mut self) -> Result<Option<String>, Error> {
        const SETTINGS: u8 = 7;
        let Some(version) = self.table_version(SETTINGS)? else { return Ok(None) };
        let text = self.read_table(SETTINGS, version)?;
        Ok(crate::csv::parse(&text)
            .into_iter()
            .find(|row| row.first().map(String::as_str) == Some("9"))
            .and_then(|row| row.get(1).cloned())
            .filter(|imei| imei.len() == 15 && imei.bytes().all(|b| b.is_ascii_digit())))
    }

    /// Runs a script on a table: `\r\n`-terminated `i,…` and `d,…` lines. The
    /// device answers without data when it took it, with its error text when not.
    pub fn write_table(&mut self, table: u8, version: u8, script: &str) -> Result<(), Error> {
        let mut data = vec![table, version];
        data.extend_from_slice(script.as_bytes());
        self.drain()?;
        self.send(WRITE_TABLE, &data)?;
        let answer = self.receive(WRITE_TABLE, WRITE_TIMEOUT)?.ok_or(Error::NoAnswer)?;
        let text = String::from_utf8_lossy(&answer.data).trim().to_string();
        if text.is_empty() { Ok(()) } else { Err(Error::Refused(text)) }
    }

    /// Writes a script whose first two bytes are already the table and its
    /// version (WriteTableBytes, for Garmin's device commands).
    pub fn write_table_bytes(&mut self, data: &[u8]) -> Result<(), Error> {
        self.drain()?;
        self.send(WRITE_TABLE, data)?;
        let answer = self.receive(WRITE_TABLE, WRITE_TIMEOUT)?.ok_or(Error::NoAnswer)?;
        let text = String::from_utf8_lossy(&answer.data).trim().to_string();
        if text.is_empty() { Ok(()) } else { Err(Error::Refused(text)) }
    }

    /// Writes `lines` in as few scripts as fit in `max` bytes each, as
    /// BatchWriter.BatchWriteTable does (a line joins a script while the two
    /// stay under `max`). A line alone longer than that is refused; one exactly
    /// `max` long, which makes inReach Sync loop forever, goes alone.
    pub fn batch_write(&mut self, table: u8, version: u8, lines: &[String], max: usize) -> Result<usize, Error> {
        let mut script = String::new();
        let mut writes = 0;
        for line in lines {
            let mut line = line.trim_end_matches(['\r', '\n']).to_string();
            if line.trim().is_empty() {
                continue;
            }
            line.push_str("\r\n");
            if line.len() > max {
                return Err(Error::LineTooLong(line.len()));
            }
            if !script.is_empty() && script.len() + line.len() >= max {
                self.write_table(table, version, &script)?;
                writes += 1;
                script.clear();
            }
            script.push_str(&line);
        }
        if !script.is_empty() {
            self.write_table(table, version, &script)?;
            writes += 1;
        }
        Ok(writes)
    }

    /// The device resets its settings to their defaults (no answer comes).
    pub fn apply_default_settings(&mut self) -> Result<(), Error> {
        self.drain()?;
        self.send(APPLY_DEFAULT_SETTINGS, &[])
    }

    /// The device restarts in its normal (app) mode, as inReach Sync has it do
    /// before a sync; no answer comes, it leaves the bus.
    pub fn restart_app_mode(&mut self) -> Result<(), Error> {
        self.drain()?;
        self.send(RESET_DEVICE_REQ, &[])
    }

    /// The device restarts as a USB drive; no answer comes, it leaves the bus.
    pub fn switch_to_drive(&mut self) -> Result<(), Error> {
        self.drain()?;
        self.send(RESET_FOR_MASS_STORAGE, &[])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A device that answers each request with prepared messages.
    #[derive(Default)]
    pub struct Fake {
        pub incoming: VecDeque<Vec<u8>>,
        pub written: Vec<Vec<u8>>,
        pub answers: Vec<(u16, Vec<u8>)>,
        /// Several messages of the request's code, queued once it is written.
        pub streams: Vec<(u16, Vec<Vec<u8>>)>,
    }

    impl Fake {
        pub fn queue(&mut self, code: u16, data: &[u8]) {
            for r in frame::reports(&frame::encode(code, data)) {
                self.incoming.push_back(r[1..].to_vec());
            }
        }
    }

    impl Transport for Fake {
        fn read_report(&mut self, _t: Duration) -> std::io::Result<Option<Vec<u8>>> {
            Ok(self.incoming.pop_front())
        }
        fn write_report(&mut self, report: &[u8]) -> std::io::Result<()> {
            self.written.push(report.to_vec());
            // A request is one report here; answer it once it is written.
            let code = u16::from_le_bytes([report[5], report[6]]);
            if let Some(i) = self.answers.iter().position(|(c, _)| *c == code) {
                let (c, d) = self.answers.remove(i);
                self.queue(c, &d);
            }
            if let Some(i) = self.streams.iter().position(|(c, _)| *c == code) {
                let (c, parts) = self.streams.remove(i);
                self.queue(0x2222, b"unrelated");
                for d in parts {
                    self.queue(c, &d);
                }
            }
            Ok(())
        }
    }

    fn version_bytes() -> Vec<u8> {
        let mut d = vec![0u8; 180];
        let put = |d: &mut Vec<u8>, off: usize, s: &str| d[off..off + s.len()].copy_from_slice(s.as_bytes());
        put(&mut d, 4, "DeLorme");
        put(&mut d, 36, "inReach Mini Rev 0");
        put(&mut d, 68, "3.90.1101380");
        put(&mut d, 100, "GNSSLIB_7.3.14.95_GNU");
        put(&mut d, 148, "000000000000000");
        d
    }

    #[test]
    fn version_and_mode() {
        let mut fake = Fake::default();
        fake.queue(0x1111, b"stale"); // drained before the request
        fake.answers.push((VERSION, version_bytes()));
        fake.answers.push((READ_APP_MODE, vec![1]));
        let mut dev = InReach::new(fake);
        let v = dev.version().unwrap();
        assert_eq!(v.product, "inReach Mini Rev 0");
        assert_eq!(v.firmware, "3.90.1101380");
        assert_eq!(v.gps_firmware, "GNSSLIB_7.3.14.95_GNU");
        assert_eq!(dev.app_mode().unwrap(), AppMode::Charger);
    }

    #[test]
    fn table_version_and_absent_tables() {
        let mut fake = Fake::default();
        fake.answers.push((TABLE_VERSION, vec![5]));
        fake.answers.push((TABLE_VERSION, vec![0xFF]));
        let mut dev = InReach::new(fake);
        assert_eq!(dev.table_version(1).unwrap(), Some(5));
        assert_eq!(dev.table_version(11).unwrap(), None);
        let sent = &dev.transport.written;
        assert_eq!(sent[0][3 + 8], 1); // the table byte after the 8-byte header
        assert_eq!(sent[1][3 + 8], 11);
    }

    #[test]
    fn read_table_puts_the_stream_together() {
        let mut fake = Fake::default();
        fake.queue(0x1111, b"stale"); // drained before the request
        // Once the request is written: another message, two parts and the end.
        fake.streams.push((READ_TABLE, vec![b"1,1,Ada,".to_vec(), b"Lovelace\r\n".to_vec(), vec![]]));
        let mut dev = InReach::new(fake);
        assert_eq!(dev.read_table(1, 5).unwrap(), "1,1,Ada,Lovelace\r\n");
        let request = &dev.transport.written[0];
        assert_eq!(&request[3 + 8..3 + 10], &[1, 5]);
    }

    #[test]
    fn read_table_without_its_end_is_no_answer() {
        let mut fake = Fake::default();
        fake.streams.push((READ_TABLE, vec![b"1,1,Ada".to_vec()]));
        let mut dev = InReach::new(fake);
        assert!(matches!(dev.read_table(1, 5), Err(Error::NoAnswer)));
    }

    #[test]
    fn imei_is_setting_9() {
        let mut fake = Fake::default();
        fake.answers.push((TABLE_VERSION, vec![1]));
        fake.streams.push((READ_TABLE, vec![b"8,20041,\r\n9,300434000000001,\r\n11,89AB\r\n".to_vec(), vec![]]));
        let mut dev = InReach::new(fake);
        assert_eq!(dev.imei().unwrap().as_deref(), Some("300434000000001"));
    }

    #[test]
    fn write_table_ok_and_refused() {
        let mut fake = Fake::default();
        fake.answers.push((WRITE_TABLE, vec![]));
        fake.answers.push((WRITE_TABLE, b"Bad row".to_vec()));
        let mut dev = InReach::new(fake);
        dev.write_table(9, 1, "d,*\r\n").unwrap();
        let first = dev.transport.written[0].clone();
        assert_eq!(&first[3 + 8..3 + 8 + 2 + 5], b"\x09\x01d,*\r\n");
        assert!(matches!(dev.write_table(9, 1, "i,x\r\n"), Err(Error::Refused(t)) if t == "Bad row"));
    }

    #[test]
    fn batches_follow_batchwriter() {
        let mut fake = Fake::default();
        for _ in 0..3 {
            fake.answers.push((WRITE_TABLE, vec![]));
        }
        let mut dev = InReach::new(fake);
        // 10 bytes a line with \r\n; under 21 bytes a script: two lines each.
        let lines: Vec<String> = (0..5).map(|i| format!("i,1,{i},ab")).collect();
        assert_eq!(dev.batch_write(9, 1, &lines, 21).unwrap(), 3);
        let scripts: Vec<String> = dev.transport.written.iter().map(|r| {
            let msg = &r[3..3 + r[2] as usize];
            String::from_utf8_lossy(&msg[10..msg.len() - 4]).into_owned()
        }).collect();
        assert_eq!(scripts, ["i,1,0,ab\r\ni,1,1,ab\r\n", "i,1,2,ab\r\ni,1,3,ab\r\n", "i,1,4,ab\r\n"]);
        assert!(matches!(dev.batch_write(9, 1, &["x".repeat(30)], 21), Err(Error::LineTooLong(32))));
    }

    #[test]
    fn script_length_by_device() {
        assert_eq!(max_script_len("inReach Mini Rev 0", "1.0"), 2046);
        assert_eq!(max_script_len("inReach SE", "2.6.0.1"), 2046);
        assert_eq!(max_script_len("inReach SE", "2.6.0.0"), 1022);
        assert_eq!(max_script_len("inReach SE", "junk"), 1022);
    }

    #[test]
    fn no_answer_is_an_error() {
        let mut dev = InReach::new(Fake::default());
        assert!(matches!(dev.app_mode(), Err(Error::NoAnswer)));
    }

    #[test]
    fn switch_to_drive_sends_only_that_message() {
        let mut dev = InReach::new(Fake::default());
        dev.switch_to_drive().unwrap();
        let written = &dev.transport.written;
        assert_eq!(written.len(), 1);
        assert_eq!(u16::from_le_bytes([written[0][5], written[0][6]]), RESET_FOR_MASS_STORAGE);
    }

    #[test]
    fn find_reads_hid_id() {
        let dir = tempfile::tempdir().unwrap();
        let add = |name: &str, id: &str| {
            let d = dir.path().join(name).join("device");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("uevent"), format!("DRIVER=hid-generic\nHID_ID={id}\n")).unwrap();
        };
        add("hidraw0", "0003:0000046D:0000C52B");
        add("hidraw4", "0003:0000091E:00000B89");
        add("hidraw5", "0003:00001163:00000300");
        std::fs::create_dir_all(dir.path().join("hidraw6")).unwrap();
        let found = find(dir.path());
        let names: Vec<_> = found.iter().map(|f| (f.node.to_str().unwrap(), f.name)).collect();
        assert_eq!(names, [("/dev/hidraw4", "inReach Mini"), ("/dev/hidraw5", "inReach")]);
        assert!(find(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn long_lines_are_found_before_anything_is_written() {
        let ok = vec!["i,1,a".to_string(), String::new()];
        assert!(check_lines(&ok, 20).is_ok());
        let long = vec!["i,1,a".to_string(), "x".repeat(19)];
        assert!(matches!(check_lines(&long, 20), Err(Error::LineTooLong(21))));
    }

    #[test]
    fn a_second_holder_of_the_lock_waits_then_gives_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node");
        let first = File::create(&path).unwrap();
        lock_exclusive(&first, Duration::ZERO).unwrap();
        let second = File::open(&path).unwrap();
        let err = lock_exclusive(&second, Duration::from_millis(150)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WouldBlock);
        drop(first);
        lock_exclusive(&second, Duration::ZERO).unwrap();
    }
}
