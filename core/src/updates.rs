//! Software updates for an inReach that takes them as a USB drive (the inReach
//! Mini): Garmin's update server is asked with the drive's
//! `Garmin/GarminDevice.xml`, each file it offers is downloaded and checked
//! (size and MD5), copied to where it belongs on the drive, and the drive is
//! ejected; the device installs the file itself when it restarts. Garmin
//! Express does the same for its devices.
//!
//! The server answers without credentials. nimrag-sync asks it as itself,
//! not as Garmin Express (docs/server-observations.md).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::server::USER_AGENT;

pub const UPDATE_SERVER: &str = "https://omt.garmin.com";

/// One file Garmin offers for the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub part_number: String,
    pub product_name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub is_primary_firmware: bool,
    pub data_type: Option<String>,
    pub severity: Option<String>,
    pub secure_eula_url: Option<String>,
    pub eula_url: Option<String>,
    pub deliverable: Deliverable,
    pub installation: Installation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deliverable {
    pub secure_download_url: Option<String>,
    pub download_url: Option<String>,
    pub md5: String,
    pub size_in_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    #[serde(default)]
    pub is_reinstall: bool,
    pub path: String,
    pub file_name: String,
    #[serde(default)]
    pub order: i64,
}

impl Update {
    /// The HTTPS address of the file; none when Garmin gives only plain HTTP.
    pub fn url(&self) -> Option<&str> {
        self.deliverable.secure_download_url.as_deref().filter(|u| u.starts_with("https://"))
    }

    /// Where the file goes on the drive, relative to its root; `None` when
    /// Garmin's path or name would leave the drive.
    pub fn place(&self) -> Option<PathBuf> {
        let safe = |p: &str| {
            !p.is_empty()
                && !p.starts_with('/')
                && p.split(['/', '\\']).all(|c| !c.is_empty() && c != "." && c != "..")
        };
        let path = self.installation.path.trim_matches(['/', '\\']);
        let name = &self.installation.file_name;
        (safe(path) && safe(name) && !name.contains(['/', '\\'])).then(|| Path::new(path).join(name))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot reach Garmin's update server: {0}")]
    Network(String),
    #[error("Garmin's update server answered {0}")]
    Status(u16),
    #[error("Garmin's update server answered in a form nimrag-sync does not know: {0}")]
    Unexpected(String),
    #[error("the download of {name} {0}", name = .1)]
    Download(String, String),
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    updates: Vec<Update>,
}

/// The most a download may be (256 MB).
const MAX_DOWNLOAD: u64 = 256 << 20;

pub struct Client {
    agent: ureq::Agent,
    base: String,
    /// Downloads only over HTTPS; the tests' local server is plain HTTP.
    https_only: bool,
}

impl Default for Client {
    fn default() -> Self {
        Client::new(UPDATE_SERVER)
    }
}

impl Client {
    pub fn new(base: &str) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .build()
            .into();
        Client { agent, base: base.trim_end_matches('/').to_string(), https_only: true }
    }

    /// What Garmin offers for the device whose GarminDevice.xml is given; with
    /// `reinstall` also what the device has already (Garmin Express's "filter=all").
    pub fn check(&self, device_xml: &str, reinstall: bool) -> Result<Vec<Update>, Error> {
        let url = format!("{}/api/device-software/v1/updates", self.base);
        let mut request = self
            .agent
            .post(&url)
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US")
            .header("Content-Type", "application/json");
        if reinstall {
            request = request.query("filter", "all");
        }
        let body = serde_json::json!({ "garminDeviceXml": device_xml, "isUserInteractive": true, "clientInfo": {} });
        let mut response = request.send(body.to_string()).map_err(|e| Error::Network(e.to_string()))?;
        let status = response.status().as_u16();
        log::info!("POST {url} -> {status}");
        if status != 200 {
            return Err(Error::Status(status));
        }
        let text = response.body_mut().read_to_string().map_err(|e| Error::Network(e.to_string()))?;
        let answer: Answer = serde_json::from_str(&text).map_err(|e| Error::Unexpected(e.to_string()))?;
        let mut updates = answer.updates;
        updates.sort_by_key(|u| u.installation.order);
        Ok(updates)
    }

    /// Downloads an update's file, `progress` told the bytes so far, and
    /// checks its size and MD5 against what Garmin said.
    pub fn download(&self, update: &Update, progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<u8>, Error> {
        let name = update.installation.file_name.clone();
        let fail = |why: String| Error::Download(why, name.clone());
        let url = if self.https_only {
            update.url().ok_or_else(|| fail("has no HTTPS address".into()))?
        } else {
            update.deliverable.secure_download_url.as_deref().ok_or_else(|| fail("has no address".into()))?
        };
        let mut response = self.agent.get(url).call().map_err(|e| fail(format!("failed: {e}")))?;
        let status = response.status().as_u16();
        log::info!("GET {url} -> {status}");
        if status != 200 {
            return Err(fail(format!("was answered {status}")));
        }
        let expected = update.deliverable.size_in_bytes;
        // An inReach's software is a few megabytes; more is a wrong answer.
        if expected > MAX_DOWNLOAD {
            return Err(fail(format!("would be {expected} bytes, more than an inReach's software can be")));
        }
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(expected.saturating_add(1))
            .reader();
        let mut data = Vec::with_capacity(expected.min(16 << 20) as usize);
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| fail(format!("broke off: {e}")))?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..n]);
            progress(data.len() as u64, expected);
        }
        if data.len() as u64 != expected {
            return Err(fail(format!("has {} bytes, not the {expected} Garmin said", data.len())));
        }
        let md5 = format!("{:x}", md5::compute(&data));
        if !md5.eq_ignore_ascii_case(update.deliverable.md5.trim()) {
            return Err(fail(format!("has MD5 {md5}, not the {} Garmin said", update.deliverable.md5)));
        }
        Ok(data)
    }
}

/// What the device's GarminDevice.xml says of it, for showing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    pub description: String,
    pub part_number: String,
    /// "390" in the file is software 3.90.
    pub software_version: String,
}

/// The first `<tag>…</tag>` of a GarminDevice.xml (whose few fields need no parser).
fn field(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    xml.split_once(&open)
        .and_then(|(_, rest)| rest.split_once(&format!("</{tag}>")))
        .map(|(v, _)| v.trim().to_string())
        .unwrap_or_default()
}

pub fn device_info(xml: &str) -> DeviceInfo {
    let raw = field(xml, "SoftwareVersion");
    let version = raw
        .parse::<u32>()
        .map(|v| format!("{}.{:02}", v / 100, v % 100))
        .unwrap_or(raw);
    DeviceInfo { description: field(xml, "Description"), part_number: field(xml, "PartNumber"), software_version: version }
}

/// Writes `data` to `root/place` as a whole or not at all: beside it first,
/// then renamed over it, and flushed to the drive.
pub fn put_on_drive(root: &Path, place: &Path, data: &[u8]) -> std::io::Result<PathBuf> {
    let target = root.join(place);
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = target.with_extension("partial");
    {
        let mut file = std::fs::File::create(&partial)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    std::fs::rename(&partial, &target)?;
    if let Some(dir) = target.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    const ANSWER: &str = r#"{"$id":"1","updates":[{"$id":"2","partNumber":"006-B2953-00","productName":"inReach Mini",
        "version":"3.91","isPrimaryFirmware":true,"dataType":"Firmware","severity":"Recommended",
        "eulaUrl":"http://example/license.txt","secureEulaUrl":"https://example/license.txt",
        "deliverable":{"$id":"3","downloadUrl":"http://HOST/GUPDATE.GCD","md5":"MD5","sizeInBytes":SIZE,
            "secureDownloadUrl":"SECURE/GUPDATE.GCD"},
        "installation":{"$id":"4","isProductInstalled":true,"isReinstall":false,"path":"Garmin","fileName":"GUPDATE.GCD","order":100}}],
        "accessLevel":"live"}"#;

    /// A server answering the check with ANSWER and the file with `file`.
    fn serve(file: &'static [u8], md5: &'static str) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let base_in = base.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                let length = head
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                    .unwrap_or(0);
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                log.lock().unwrap().push(format!("{head}\n{}", String::from_utf8_lossy(&body)));
                let line = head.lines().next().unwrap_or("").to_string();
                let (status, payload): (u16, Vec<u8>) = if line.starts_with("POST /api/device-software/v1/updates") {
                    let a = ANSWER.replace("SECURE", &base_in).replace("MD5", md5).replace("SIZE", &file.len().to_string());
                    (200, a.into_bytes())
                } else if line.starts_with("GET /GUPDATE.GCD") {
                    (200, file.to_vec())
                } else {
                    (404, Vec::new())
                };
                let mut out = stream;
                let _ = write!(out, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
                let _ = out.write_all(&payload);
            }
        });
        (base, seen)
    }

    // Not the MD5 of b"firmware".
    const FIRMWARE_MD5: &str = "7f8b1f4ae1a3c5a85b2e0d0e1b1a4a0b";

    #[test]
    fn check_download_and_verify() {
        let md5 = Box::leak(format!("{:x}", md5::compute(b"firmware")).into_boxed_str());
        let (base, seen) = serve(b"firmware", md5);
        let client = Client { https_only: false, ..Client::new(&base) };
        let updates = client.check("<Device/>", false).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].version.as_deref(), Some("3.91"));
        assert_eq!(updates[0].place().unwrap(), Path::new("Garmin/GUPDATE.GCD"));
        let mut seen_progress = 0;
        let data = client.download(&updates[0], &mut |n, _| seen_progress = n).unwrap();
        assert_eq!(data, b"firmware");
        assert_eq!(seen_progress, 8);
        let requests = seen.lock().unwrap();
        // Asked as nimrag-sync, with the device's file, and no credentials.
        assert!(requests[0].contains("nimrag-sync/") && requests[0].contains("\"garminDeviceXml\":\"<Device/>\""));
        assert!(!requests[0].to_lowercase().contains("authorization") && !requests[0].contains("Garmin-Client"));
        assert!(!requests[0].starts_with("POST /api/device-software/v1/updates?filter=all"));
        drop(requests);
        client.check("<Device/>", true).unwrap();
        assert!(seen.lock().unwrap().last().unwrap().starts_with("POST /api/device-software/v1/updates?filter=all"));
    }

    #[test]
    fn a_wrong_md5_is_refused() {
        let (base, _) = serve(b"firmware", FIRMWARE_MD5);
        let client = Client { https_only: false, ..Client::new(&base) };
        let updates = client.check("<Device/>", false).unwrap();
        let err = client.download(&updates[0], &mut |_, _| {}).unwrap_err().to_string();
        assert!(err.contains("MD5"), "{err}");
    }

    #[test]
    fn places_stay_on_the_drive() {
        let mut u: Update = serde_json::from_str(
            &ANSWER.replace("SECURE", "https://x").replace("MD5", "m").replace("SIZE", "1"),
        )
        .map(|a: Answer| a.updates[0].clone())
        .unwrap();
        assert_eq!(u.place(), Some(PathBuf::from("Garmin/GUPDATE.GCD")));
        u.installation.path = "../etc".into();
        assert_eq!(u.place(), None);
        u.installation.path = "Garmin".into();
        u.installation.file_name = "a/../../b".into();
        assert_eq!(u.place(), None);
        u.installation.file_name = "GUPDATE.GCD".into();
        u.deliverable.secure_download_url = Some("http://x/plain".into());
        assert_eq!(u.url(), None);
    }

    #[test]
    fn device_info_from_the_xml() {
        let xml = "<Device><Model><PartNumber>006-B2953-00</PartNumber><SoftwareVersion>390</SoftwareVersion>\
                   <Description>inReach Mini</Description></Model><Id>1</Id></Device>";
        assert_eq!(
            device_info(xml),
            DeviceInfo { description: "inReach Mini".into(), part_number: "006-B2953-00".into(), software_version: "3.90".into() }
        );
    }

    #[test]
    fn put_on_drive_whole_or_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let target = put_on_drive(dir.path(), Path::new("Garmin/GUPDATE.GCD"), b"firmware").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"firmware");
        assert!(!dir.path().join("Garmin/GUPDATE.partial").exists());
    }
}
