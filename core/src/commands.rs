//! Garmin's device commands (`/V1/Sync/Plans`): raw table scripts Garmin's
//! service has for the device, such as a change of plan or of the tracking
//! interval. inReach Sync runs them as they come (SyncDevice.ExecuteDeviceCommands)
//! and tells Garmin how each went; so does this.

use serde::{Deserialize, Serialize};

/// One command (Models/DeviceCommandsModel.cs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceCommand {
    pub transaction_id: Option<i64>,
    /// 0 no response, 1 acked, 2 nacked, 3 completed, 4 cancelled, 5 waiting for ack.
    #[serde(default)]
    pub status_code: i64,
    /// The script, base64 (a C# byte[] in Newtonsoft's JSON).
    pub device_data: Option<String>,
    pub error_msg: Option<String>,
}

pub const ACKED: i64 = 1;
pub const NACKED: i64 = 2;
/// A sub-command longer than this (with its `\r`) is refused, as inReach Sync refuses it.
pub const MAX_SUBCOMMAND: usize = 1999;

impl DeviceCommand {
    /// The bytes of the script; `None` when there are none or they are not base64.
    pub fn data(&self) -> Option<Vec<u8>> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(self.device_data.as_deref()?).ok()
    }
}

/// The sub-commands of a script, as ExecuteDeviceCommands cuts it: at `\r\r`
/// outside quotes (a quote next to another is an escaped one), each without
/// its surrounding `\r`s. Each is written as it is, its first two bytes being
/// the table and its version.
pub fn subcommands(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let piece = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    let mut list = Vec::new();
    if chars.len() > 2 && chars.contains(&'"') {
        let mut quoted = false;
        let mut start = 0;
        for i in 0..chars.len() {
            if chars[i] == '"' {
                let after_quote = i > 0 && chars[i - 1] == '"';
                let before_quote = i < chars.len() - 1 && chars[i + 1] == '"';
                if !after_quote && !before_quote {
                    quoted = !quoted;
                }
            } else if i < chars.len() - 1 && chars[i] == '\r' && chars[i + 1] == '\r' && !quoted {
                list.push(piece(start, i).trim_start_matches('\r').to_string());
                start = i;
            }
        }
        if start + 2 < chars.len() {
            list.push(piece(start, chars.len()).trim_matches('\r').to_string());
        }
    } else {
        list.extend(text.split("\r\r").filter(|p| !p.is_empty()).map(|p| p.trim_matches('\r').to_string()));
    }
    list
}

/// A sub-command's bytes as they are written: ending with one `\r`.
pub fn subcommand_bytes(item: &str) -> Vec<u8> {
    let mut bytes = item.as_bytes().to_vec();
    if !item.ends_with('\r') {
        bytes.push(b'\r');
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_scripts_split_at_double_returns() {
        assert_eq!(subcommands("\x07\x01i,24,600\r\r\x07\x01i,25,324\r"), ["\x07\x01i,24,600", "\x07\x01i,25,324"]);
        assert_eq!(subcommands("\x07\x01i,24,600\r"), ["\x07\x01i,24,600"]);
        assert_eq!(subcommands(""), Vec::<String>::new());
    }

    #[test]
    fn quotes_keep_their_double_returns() {
        let text = "\x07\x01i,18,\"a\r\rb\"\r\r\x07\x01i,26,\"x\"\"y\"\r";
        assert_eq!(subcommands(text), ["\x07\x01i,18,\"a\r\rb\"", "\x07\x01i,26,\"x\"\"y\""]);
    }

    #[test]
    fn three_returns_leave_an_empty_command_as_inreach_sync_does() {
        // ExecuteDeviceCommands cuts at each pair, overlapping: "\r\r\r" makes an empty one.
        let text = "\x07\x01i,1,\"a\"\r\r\r\x07\x01i,2,\"b\"";
        assert_eq!(subcommands(text), ["\x07\x01i,1,\"a\"", "", "\x07\x01i,2,\"b\""]);
    }

    #[test]
    fn bytes_end_with_one_return() {
        assert_eq!(subcommand_bytes("\x07\x01i,24,600"), b"\x07\x01i,24,600\r");
        assert_eq!(subcommand_bytes("x\r"), b"x\r");
    }

    #[test]
    fn data_is_base64() {
        let c = DeviceCommand { transaction_id: Some(7), status_code: 5, device_data: Some("BwFpLDI0LDYwMA0=".into()), error_msg: None };
        assert_eq!(c.data().unwrap(), b"\x07\x01i,24,600\r");
        assert_eq!(DeviceCommand { device_data: None, ..c.clone() }.data(), None);
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"TransactionId":7,"StatusCode":5,"DeviceData":"BwFpLDI0LDYwMA0=","ErrorMsg":null}"#
        );
    }
}
