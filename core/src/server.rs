//! Garmin's inReach service, as inReach Sync 1.4.47 uses it (docs/server-protocol.md,
//! checked against the real service in docs/server-observations.md).
//!
//! The account API tells which sync server serves a device (no credentials);
//! the sync server answers with HTTP Basic authentication, the Garmin
//! account's email and password, on every request. Only reading requests are
//! here: nothing acknowledges a sync or changes the account.

use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

pub const ACCOUNT_API: &str = "https://account-api.inreach.garmin.com";
pub const USER_AGENT: &str = concat!("nimrag-sync/", env!("CARGO_PKG_VERSION"));

/// The Garmin account's email address and password.
#[derive(Clone)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("email", &self.email).finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot reach Garmin's inReach service: {0}")]
    Network(String),
    #[error("Garmin refused the email address or password")]
    Unauthorized,
    #[error("Garmin's inReach service answered {status} to {what}")]
    Status { status: u16, what: String },
    #[error("Garmin's inReach service answered {what} in a form nimrag-sync does not know: {detail}")]
    Unexpected { what: String, detail: String },
}

/// The answer to `GET /V1/SyncURL`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct SyncInfo {
    #[serde(rename = "SyncURL")]
    pub sync_url: String,
    #[serde(rename = "Provisioned")]
    pub provisioned: bool,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceSync {
    pub synced: bool,
    pub last_synced: Option<String>,
    #[serde(default)]
    pub sync_data: Vec<HighWater>,
}

/// When one kind of data last changed on the service.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct HighWater {
    /// 0 contacts, 1 quick texts, 2 presets, 3 waypoints, 4 routes, 5 device commands.
    pub data_type: i32,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Contact {
    #[serde(rename = "ID")]
    pub id: i64,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub organization: Option<String>,
    #[serde(default)]
    pub addresses: Vec<Address>,
}

/// One way to reach a contact. `kind`: 0 email, 1 SMS, 2 inReach, 4 IMEI.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct Address {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Type")]
    pub kind: i32,
    #[serde(rename = "Address")]
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Preset {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(default)]
    pub recipients: Vec<String>,
    pub message: String,
    #[serde(default)]
    pub facebook: bool,
    #[serde(default)]
    pub twitter: bool,
    #[serde(default)]
    pub map_share: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuickText {
    #[serde(rename = "ID")]
    pub id: i64,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContactsResponse {
    contacts: Option<Vec<Contact>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PresetsResponse {
    presets: Option<Vec<Preset>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct QuickTextsResponse {
    quick_texts: Option<Vec<QuickText>>,
}

/// A connection to the inReach service for one device.
pub struct Client {
    agent: ureq::Agent,
    account_api: String,
}

impl Default for Client {
    fn default() -> Self {
        Client::new(ACCOUNT_API)
    }
}

impl Client {
    /// A client of the account API at `account_api` (tests point it elsewhere).
    pub fn new(account_api: &str) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            // The account API redirects SyncURL to region.inreachapp.com.
            .max_redirects(5)
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .build()
            .into();
        Client { agent, account_api: account_api.trim_end_matches('/').to_string() }
    }

    /// Which sync server serves the device with this IMEI, and whether it is
    /// on an account (then its data needs the account's credentials).
    pub fn sync_info(&self, imei: &str) -> Result<SyncInfo, Error> {
        let url = format!("{}/V1/SyncURL", self.account_api);
        self.get_json(&url, &[("IMEI", imei)], None, "the sync server lookup")
    }

    pub fn device_sync(&self, sync: &SyncInfo, imei: &str, who: &Credentials) -> Result<DeviceSync, Error> {
        self.sync_get(sync, "Device", imei, who, "the sync status")
    }

    pub fn contacts(&self, sync: &SyncInfo, imei: &str, who: &Credentials) -> Result<Option<Vec<Contact>>, Error> {
        let r: ContactsResponse = self.sync_get(sync, "Contacts", imei, who, "the contacts")?;
        Ok(r.contacts)
    }

    pub fn presets(&self, sync: &SyncInfo, imei: &str, who: &Credentials) -> Result<Option<Vec<Preset>>, Error> {
        let r: PresetsResponse = self.sync_get(sync, "Presets", imei, who, "the preset messages")?;
        Ok(r.presets)
    }

    pub fn quick_texts(&self, sync: &SyncInfo, imei: &str, who: &Credentials) -> Result<Option<Vec<QuickText>>, Error> {
        let r: QuickTextsResponse = self.sync_get(sync, "QuickTexts", imei, who, "the quick texts")?;
        Ok(r.quick_texts)
    }

    fn sync_get<T: DeserializeOwned>(
        &self,
        sync: &SyncInfo,
        what: &str,
        imei: &str,
        who: &Credentials,
        label: &str,
    ) -> Result<T, Error> {
        let url = format!("{}/V1/Sync/{what}", sync.sync_url.trim_end_matches('/'));
        self.get_json(&url, &[("imei", imei)], Some(who), label)
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
        who: Option<&Credentials>,
        what: &str,
    ) -> Result<T, Error> {
        let mut request = self
            .agent
            .get(url)
            .header("Accept", "application/json")
            .header("X-CultureCode", "en-US");
        for (k, v) in query {
            request = request.query(*k, *v);
        }
        if let Some(who) = who {
            request = request.header("Authorization", basic(who));
        }
        let mut response = request.call().map_err(|e| Error::Network(e.to_string()))?;
        let status = response.status().as_u16();
        log::info!("GET {url} -> {status}");
        if status == 401 {
            return Err(Error::Unauthorized);
        }
        if status != 200 {
            return Err(Error::Status { status, what: what.into() });
        }
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Network(e.to_string()))?;
        serde_json::from_str(&body).map_err(|e| Error::Unexpected { what: what.into(), detail: e.to_string() })
    }
}

fn basic(who: &Credentials) -> String {
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", who.email, who.password));
    format!("Basic {token}")
}

/// The credentials in a netrc file for the sync server's host: the stand-in
/// until they live in the keyring.
pub fn netrc_credentials(text: &str, host: &str) -> Option<Credentials> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        if words[i] == "machine" && words.get(i + 1) == Some(&host) {
            let (mut login, mut password) = (None, None);
            let mut j = i + 2;
            while j + 1 < words.len() && words[j] != "machine" {
                match words[j] {
                    "login" => login = Some(words[j + 1]),
                    "password" => password = Some(words[j + 1]),
                    _ => {}
                }
                j += 2;
            }
            return Some(Credentials { email: login?.into(), password: password?.into() });
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A small HTTP server on localhost: answers each request with what
    /// `route` returns for its request line, and keeps the requests.
    fn serve(route: fn(&str) -> (u16, String, Option<String>)) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
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
                let request_line = head.lines().next().unwrap_or("").to_string();
                log.lock().unwrap().push(head);
                let (status, body, location) = route(&request_line);
                let mut out = stream;
                let location = location.map(|l| format!("Location: {l}\r\n")).unwrap_or_default();
                let _ = write!(
                    out,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{location}Connection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (base, seen)
    }

    fn who() -> Credentials {
        Credentials { email: "ada@example.com".into(), password: "secret".into() }
    }

    #[test]
    fn sync_info_follows_the_redirect() {
        let (base, seen) = serve(|line| {
            if line.starts_with("GET /V1/SyncURL?IMEI=300434000000001 ") {
                (301, String::new(), Some("/ProvisionInfo/InReachIMEI/300434000000001".into()))
            } else if line.starts_with("GET /ProvisionInfo/InReachIMEI/300434000000001 ") {
                (200, r#"{"SyncURL":"https://sync.example","Provisioned":true}"#.into(), None)
            } else {
                (404, String::new(), None)
            }
        });
        let info = Client::new(&base).sync_info("300434000000001").unwrap();
        assert_eq!(info, SyncInfo { sync_url: "https://sync.example".into(), provisioned: true });
        let requests = seen.lock().unwrap();
        // The lookup goes without credentials.
        assert!(requests.iter().all(|r| !r.to_lowercase().contains("authorization")));
    }

    #[test]
    fn sync_data_with_basic_auth() {
        let (base, seen) = serve(|line| match line.split(' ').nth(1).unwrap_or("") {
            "/V1/Sync/Contacts?imei=300434000000001" => (
                200,
                r#"{"Contacts":[{"ID":7,"FirstName":"Ada","LastName":"Lovelace","Organization":null,
                    "Addresses":[{"ID":11,"Type":0,"Address":"ada@example.com"}]}]}"#
                    .into(),
                None,
            ),
            "/V1/Sync/Presets?imei=300434000000001" => (
                200,
                r#"{"Presets":[{"ID":1,"Recipients":["ada@example.com"],"Message":"All good",
                    "Facebook":false,"Twitter":false,"MapShare":true}]}"#
                    .into(),
                None,
            ),
            "/V1/Sync/QuickTexts?imei=300434000000001" => (200, r#"{"QuickTexts":null}"#.into(), None),
            "/V1/Sync/Device?imei=300434000000001" => (
                200,
                r#"{"Synced":false,"LastSynced":"2022-09-22T19:48:50Z","SyncData":[{"DataType":5,"LastModified":null}]}"#
                    .into(),
                None,
            ),
            _ => (404, String::new(), None),
        });
        let client = Client::new("http://unused.invalid");
        let sync = SyncInfo { sync_url: format!("{base}/"), provisioned: true };
        let imei = "300434000000001";
        let contacts = client.contacts(&sync, imei, &who()).unwrap().unwrap();
        assert_eq!(contacts[0].addresses[0].address, "ada@example.com");
        assert_eq!(contacts[0].organization, None);
        let presets = client.presets(&sync, imei, &who()).unwrap().unwrap();
        assert!(presets[0].map_share);
        // null and an empty list differ: an empty list empties the device's table.
        assert_eq!(client.quick_texts(&sync, imei, &who()).unwrap(), None);
        let status = client.device_sync(&sync, imei, &who()).unwrap();
        assert!(!status.synced && status.sync_data[0].last_modified.is_none());
        let requests = seen.lock().unwrap();
        // "ada@example.com:secret" in base64, on every request to the sync server.
        assert!(requests.iter().all(|r| r.contains("Basic YWRhQGV4YW1wbGUuY29tOnNlY3JldA==")));
        assert!(requests.iter().all(|r| r.contains("nimrag-sync/")));
    }

    #[test]
    fn refusals_and_odd_answers() {
        let (base, _) = serve(|line| {
            if line.contains("/Contacts") {
                (401, String::new(), None)
            } else if line.contains("/Presets") {
                (500, String::new(), None)
            } else {
                (200, "<html>".into(), None)
            }
        });
        let client = Client::new("http://unused.invalid");
        let sync = SyncInfo { sync_url: base, provisioned: true };
        assert!(matches!(client.contacts(&sync, "1", &who()), Err(Error::Unauthorized)));
        assert!(matches!(client.presets(&sync, "1", &who()), Err(Error::Status { status: 500, .. })));
        assert!(matches!(client.quick_texts(&sync, "1", &who()), Err(Error::Unexpected { .. })));
    }

    #[test]
    fn credentials_never_show_their_password() {
        assert!(!format!("{:?}", who()).contains("secret"));
    }

    #[test]
    fn netrc() {
        let text = "machine other login x password y\nmachine sync.explore.garmin.com login ada@example.com password s3cr3t\n";
        let c = netrc_credentials(text, "sync.explore.garmin.com").unwrap();
        assert_eq!((c.email.as_str(), c.password.as_str()), ("ada@example.com", "s3cr3t"));
        assert!(netrc_credentials(text, "nowhere").is_none());
        assert!(netrc_credentials("machine sync.explore.garmin.com login x", "sync.explore.garmin.com").is_none());
    }
}
