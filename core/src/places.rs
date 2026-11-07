//! Waypoints, routes and tracks, as inReach Sync 1.4.47 syncs them
//! (DeviceWaypoints.cs, DeviceRoutes.cs, DeviceTracks.cs):
//!
//! - waypoints go both ways: all of the device's go to Garmin, Garmin answers
//!   with the merged list, which replaces the device's;
//! - routes the same, each with its track as TrackBlocks rows;
//! - tracks only go to Garmin: the high-detail blocks (rows starting `0,0`)
//!   from the newest point Garmin has on.
//!
//! Numbers, dates and colours are written as inReach Sync writes them.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Number;

use crate::csv;
use crate::server::unix_seconds;

pub const WAYPOINTS: u8 = 14;
pub const TRACK_BLOCKS: u8 = 15;
pub const ROUTES: u8 = 16;
/// The only table versions inReach Sync syncs.
pub const WAYPOINTS_VERSION: u8 = 3;
pub const TRACK_BLOCKS_VERSION: u8 = 5;
pub const ROUTES_VERSION: u8 = 1;

/// A waypoint as Garmin's service and inReach Sync have it (Models/Waypoint.cs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Waypoint {
    pub sync_id: i64,
    /// 0 synchronized, 1 symbol, 2 location, 3 label not synced, 7 unsynchronized.
    #[serde(default)]
    pub sync_status: i64,
    #[serde(default)]
    pub hidden_on_device: bool,
    #[serde(default)]
    pub icon_index: i64,
    pub name: Option<String>,
    pub latitude: Number,
    pub longitude: Number,
    pub create_date: Option<String>,
    pub modified_date: Option<String>,
}

/// A route (Models/Route.cs); its track comes as TrackBlocks rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Route {
    pub label: Option<String>,
    pub color_hex: Option<String>,
    pub border_color_hex: Option<String>,
    pub created_date: Option<String>,
    pub modified_date: Option<String>,
    pub track_blocks: Option<String>,
    pub sync_id: i64,
    #[serde(default)]
    pub deleted_on_device: bool,
    pub track_id: i64,
}

/// A decimal as .NET's `decimal.Parse` takes it, as a JSON number: ".5" is 0.5.
fn number(text: &str) -> Result<Number, String> {
    let t = text.trim();
    let fixed = if let Some(rest) = t.strip_prefix("-.") {
        format!("-0.{rest}")
    } else if let Some(rest) = t.strip_prefix('.') {
        format!("0.{rest}")
    } else if t.is_empty() {
        // inReach Sync writes 0 as nothing ("#.########"); reading it back it
        // would fail, so here it is 0 again.
        "0".into()
    } else {
        t.to_string()
    };
    Number::from_str(&fixed).map_err(|_| format!("{text:?} is not a number"))
}

/// A number as .NET writes a decimal with "#.########": up to eight decimals,
/// rounded half away from zero, without trailing zeros, without a 0 before the
/// point, and nothing at all for zero.
pub fn format_8(n: &Number) -> String {
    let text = n.to_string();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, text.clone()),
    };
    if digits.contains(['e', 'E']) {
        // Not how Garmin or the device write them; good enough for the odd case.
        let v = n.as_f64().unwrap_or(0.0);
        return format_8(&Number::from_str(&format!("{v:.12}")).unwrap_or_else(|_| Number::from(0)));
    }
    let (int, frac) = digits.split_once('.').unwrap_or((&digits, ""));
    let mut int: Vec<u8> = int.bytes().map(|b| b - b'0').collect();
    let mut frac: Vec<u8> = frac.bytes().map(|b| b - b'0').collect();
    if frac.len() > 8 {
        let round_up = frac[8] >= 5;
        frac.truncate(8);
        if round_up {
            // Carry through the eight decimals, then the integer digits.
            let mut i = frac.len();
            let mut carry = true;
            while carry && i > 0 {
                i -= 1;
                frac[i] += 1;
                carry = frac[i] == 10;
                if carry {
                    frac[i] = 0;
                }
            }
            let mut j = int.len();
            while carry && j > 0 {
                j -= 1;
                int[j] += 1;
                carry = int[j] == 10;
                if carry {
                    int[j] = 0;
                }
            }
            if carry {
                int.insert(0, 1);
            }
        }
    }
    while frac.last() == Some(&0) {
        frac.pop();
    }
    let int: String = int.iter().map(|d| (b'0' + d) as char).collect::<String>().trim_start_matches('0').to_string();
    let frac: String = frac.iter().map(|d| (b'0' + d) as char).collect();
    if int.is_empty() && frac.is_empty() {
        return String::new();
    }
    let sign = if negative { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{int}") } else { format!("{sign}{int}.{frac}") }
}

/// A time as inReach Sync writes it to the device (ToDeviceUtcString):
/// "yyyy-MM-ddTHH:mm:ssZ" in UTC, or nothing.
pub fn device_time(text: Option<&str>) -> String {
    let Some(t) = text.filter(|t| !t.trim().is_empty()) else { return String::new() };
    match unix_seconds(t.trim()) {
        Some(s) => utc_text(s),
        None => String::new(),
    }
}

/// A device time ("yyyy-MM-ddTHH:mm:ssZ") as .NET's `DateTimeOffset.ToString("O")`
/// gives it, which is how inReach Sync sends it to Garmin.
fn round_trip_time(device: &str) -> Option<String> {
    let s = unix_seconds(device.trim())?;
    let utc = utc_text(s);
    Some(format!("{}.0000000+00:00", utc.trim_end_matches('Z')))
}

/// Seconds since 1970 as "yyyy-MM-ddTHH:mm:ssZ".
pub fn utc_text(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    // civil_from_days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// A device ID ("uint" on the device) as inReach Sync keeps it: an `int`.
fn id(text: &str) -> Result<i64, String> {
    text.trim().parse::<u32>().map(|v| v as i32 as i64).map_err(|_| format!("{text:?} is not an ID"))
}

/// The waypoints in the device's Waypoints table (version 3).
pub fn waypoints_from_device(text: &str) -> Result<Vec<Waypoint>, String> {
    csv::parse(text)
        .into_iter()
        .map(|r| {
            let field = |i: usize| r.get(i).map(String::as_str).unwrap_or("");
            let date = |i: usize| (!field(i).trim().is_empty()).then(|| round_trip_time(field(i))).flatten();
            Ok(Waypoint {
                sync_id: id(field(0))?,
                // Enum.TryParse: a number or a name, anything else Synchronized.
                sync_status: field(1).trim().parse().unwrap_or(0),
                hidden_on_device: field(2).trim().parse::<i64>().map_err(|_| format!("{:?} is not a number", field(2)))? != 0,
                icon_index: field(3).trim().parse().map_err(|_| format!("{:?} is not a number", field(3)))?,
                latitude: number(field(4))?,
                longitude: number(field(5))?,
                create_date: date(6),
                modified_date: date(7),
                name: Some(field(8).to_string()),
            })
        })
        .collect()
}

/// The insert lines for Garmin's waypoints (EncodeWaypointRecord): hidden
/// ones are left out, the rest written as synchronized and shown.
pub fn waypoint_lines(list: &[Waypoint]) -> Vec<String> {
    list.iter()
        .filter(|w| !w.hidden_on_device)
        .map(|w| {
            format!(
                "i,{},0,0,{},{},{},{},{},{}",
                w.sync_id as i32 as u32,
                w.icon_index,
                format_8(&w.latitude),
                format_8(&w.longitude),
                device_time(w.create_date.as_deref()),
                device_time(w.modified_date.as_deref()),
                csv::encode(w.name.as_deref().unwrap_or(""))
            )
        })
        .collect()
}

/// A waypoint needs a sync when it is hidden or not synchronized; and
/// inReach Sync syncs when the device has none at all.
pub fn waypoints_need_sync(list: &[Waypoint]) -> bool {
    list.is_empty() || list.iter().any(|w| w.hidden_on_device || w.sync_status != 0)
}

/// 16-bit RGB 5-6-5 as the device keeps colours, to "#RRGGBB" (DeviceRgbToHexString).
pub fn device_rgb_to_hex(text: &str) -> String {
    let Ok(v) = text.trim().parse::<u16>() else { return "#000000".into() };
    // (byte)((double)(255f * x / 31f) + 0.5), in single precision as .NET has it.
    let five = |x: u16| ((255f32 * x as f32 / 31f32) as f64 + 0.5) as u8;
    let six = |x: u16| ((255f32 * x as f32 / 63f32) as f64 + 0.5) as u8;
    format!("#{:02X}{:02X}{:02X}", five(v >> 11), six((v >> 5) & 0x3F), five(v & 0x1F))
}

/// "#RRGGBB" (or "0xRRGGBB") to the device's 16-bit colour (RgbTwentyFourBitToDeviceRgb).
pub fn hex_to_device_rgb(hex: Option<&str>) -> u16 {
    let Some(h) = hex.map(str::trim).filter(|h| !h.is_empty()) else { return 0 };
    let digits = h.trim_start_matches('#').trim_start_matches("0x");
    let v = u32::from_str_radix(digits, 16).unwrap_or(0);
    let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
    ((r as u16 / 8) << 11) + ((g as u16 / 4) << 5) + b as u16 / 8
}

/// The routes in the device's Routes table (version 1).
pub fn routes_from_device(text: &str) -> Result<Vec<Route>, String> {
    csv::parse(text)
        .into_iter()
        .map(|r| {
            let field = |i: usize| r.get(i).map(String::as_str).unwrap_or("");
            // ParseDate: an empty date is 1970.
            let date = |i: usize| {
                if field(i).trim().is_empty() {
                    Some("1970-01-01T00:00:00.0000000+00:00".to_string())
                } else {
                    round_trip_time(field(i))
                }
            };
            Ok(Route {
                sync_id: id(field(0))?,
                track_id: id(field(1))?,
                created_date: date(2),
                modified_date: date(3),
                color_hex: Some(device_rgb_to_hex(field(4))),
                border_color_hex: Some(device_rgb_to_hex(field(5))),
                deleted_on_device: field(6).trim().parse::<i64>().map_err(|_| format!("{:?} is not a number", field(6)))? != 0,
                label: Some(field(8).to_string()),
                track_blocks: None,
            })
        })
        .collect()
}

/// The Routes insert line and the TrackBlocks rows of a route Garmin sent
/// (EncodeRouteAndWriteTrack); nothing for one deleted on the device.
pub fn route_lines(route: &Route) -> Option<(String, Vec<String>)> {
    if route.deleted_on_device {
        return None;
    }
    let line = format!(
        "i,{},{},{},{},{},{},0,1,{}",
        route.sync_id as i32 as u32,
        route.track_id,
        device_time(route.created_date.as_deref()),
        device_time(route.modified_date.as_deref()),
        hex_to_device_rgb(route.color_hex.as_deref()),
        hex_to_device_rgb(route.border_color_hex.as_deref()),
        csv::encode(route.label.as_deref().unwrap_or(""))
    );
    let blocks = route
        .track_blocks
        .as_deref()
        .unwrap_or("")
        .split("\r\n")
        .filter(|b| !b.is_empty())
        .map(str::to_string)
        .collect();
    Some((line, blocks))
}

/// inReach Sync syncs routes when the device has none, or one deleted on it.
pub fn routes_need_sync(list: &[Route]) -> bool {
    list.is_empty() || list.iter().any(|r| r.deleted_on_device)
}

/// The track blocks to send Garmin: the device's rows, as they are, that are
/// high-detail (`0,0` first) and from Garmin's newest point on (`newest`, Unix seconds).
pub fn tracks_to_send(track_blocks_text: &str, newest: i64) -> Vec<String> {
    track_blocks_text
        .split('\n')
        .map(str::trim)
        .filter(|r| !r.is_empty() && r.starts_with("0,0"))
        .filter(|r| r.split(',').nth(9).and_then(unix_seconds).is_some_and(|t| t >= newest))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Number {
        Number::from_str(s).unwrap()
    }

    #[test]
    fn numbers_as_dotnet_formats_them() {
        assert_eq!(format_8(&n("61.234567")), "61.234567");
        assert_eq!(format_8(&n("14.7654320")), "14.765432");
        assert_eq!(format_8(&n("0.5")), ".5");
        assert_eq!(format_8(&n("-0.25")), "-.25");
        assert_eq!(format_8(&n("0")), "");
        assert_eq!(format_8(&n("10")), "10");
        assert_eq!(format_8(&n("1.123456785")), "1.12345679");
        assert_eq!(format_8(&n("1.123456784")), "1.12345678");
        assert_eq!(format_8(&n("9.999999995")), "10");
        assert_eq!(format_8(&n("-179.999999999")), "-180");
    }

    #[test]
    fn times() {
        assert_eq!(device_time(Some("2021-07-03T10:20:30Z")), "2021-07-03T10:20:30Z");
        assert_eq!(device_time(Some("2023-05-06T07:08:09.123+02:00")), "2023-05-06T05:08:09Z");
        assert_eq!(device_time(None), "");
        assert_eq!(round_trip_time("2021-07-03T10:20:30Z").unwrap(), "2021-07-03T10:20:30.0000000+00:00");
        assert_eq!(utc_text(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_text(1600355483), "2020-09-17T15:11:23Z");
    }

    const WAYPOINTS_V3: &str = "170000001,0,0,40,61.234567,14.765432,2021-07-03T10:20:30Z,2023-05-06T07:08:09Z,\"Tent\"\r\n\
        3000000000,7,1,0,.5,-12.25,,,\"Hidden, far\"\r\n";

    #[test]
    fn waypoints_off_and_onto_the_device() {
        let list = waypoints_from_device(WAYPOINTS_V3).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].sync_id, 170000001);
        assert_eq!(list[0].create_date.as_deref(), Some("2021-07-03T10:20:30.0000000+00:00"));
        // A device ID over i32 is kept as .NET's (int)uint has it.
        assert_eq!(list[1].sync_id, 3000000000u32 as i32 as i64);
        assert_eq!(list[1].latitude.to_string(), "0.5");
        assert!(list[1].hidden_on_device && list[1].create_date.is_none());
        let json = serde_json::to_string(&list[0]).unwrap();
        assert_eq!(
            json,
            r#"{"SyncId":170000001,"SyncStatus":0,"HiddenOnDevice":false,"IconIndex":40,"Name":"Tent","Latitude":61.234567,"Longitude":14.765432,"CreateDate":"2021-07-03T10:20:30.0000000+00:00","ModifiedDate":"2023-05-06T07:08:09.0000000+00:00"}"#
        );
        // Written back: hidden ones left out, the rest synchronized and shown.
        assert_eq!(
            waypoint_lines(&list),
            ["i,170000001,0,0,40,61.234567,14.765432,2021-07-03T10:20:30Z,2023-05-06T07:08:09Z,Tent"]
        );
        assert!(waypoints_need_sync(&list) && waypoints_need_sync(&[]) && !waypoints_need_sync(&list[..1]));
        let negative = Waypoint { sync_id: 3000000000u32 as i32 as i64, hidden_on_device: false, ..list[1].clone() };
        assert_eq!(waypoint_lines(&[negative]), ["i,3000000000,0,0,0,.5,-12.25,,,\"Hidden, far\""]);
    }

    #[test]
    fn colours_both_ways() {
        assert_eq!(device_rgb_to_hex("65535"), "#FFFFFF");
        assert_eq!(device_rgb_to_hex("63488"), "#FF0000");
        assert_eq!(device_rgb_to_hex(""), "#000000");
        assert_eq!(hex_to_device_rgb(Some("#FF0000")), 63488);
        assert_eq!(hex_to_device_rgb(Some("0x00FF00")), 2016);
        assert_eq!(hex_to_device_rgb(Some("#123456")), (2 << 11) + (13 << 5) + 10);
        assert_eq!(hex_to_device_rgb(None), 0);
    }

    #[test]
    fn routes_off_and_onto_the_device() {
        let list = routes_from_device("5,77,2021-01-02T03:04:05Z,,63488,0,1,1,\"Up\"\r\n").unwrap();
        assert_eq!(list[0].track_id, 77);
        assert_eq!(list[0].modified_date.as_deref(), Some("1970-01-01T00:00:00.0000000+00:00"));
        assert_eq!(list[0].color_hex.as_deref(), Some("#FF0000"));
        assert!(list[0].deleted_on_device && routes_need_sync(&list));
        assert_eq!(route_lines(&list[0]), None);
        let from_garmin = Route { deleted_on_device: false, track_blocks: Some("77,0,1,a\r\n77,1,2,b\r\n".into()), ..list[0].clone() };
        let (line, blocks) = route_lines(&from_garmin).unwrap();
        assert_eq!(line, "i,5,77,2021-01-02T03:04:05Z,1970-01-01T00:00:00Z,63488,0,0,1,Up");
        assert_eq!(blocks, ["77,0,1,a", "77,1,2,b"]);
    }

    #[test]
    fn tracks_from_the_newest_point_on() {
        let row = |t: &str, lead: &str| format!("{lead},1,0,0,0,0,0,0,{t},1,2,3,4,5,6,7,8,9,10,11,12,13,14\r\n");
        let text = [
            row("2026-11-01T10:00:00Z", "0,0"),
            row("2026-11-07T10:00:00Z", "0,0"),
            row("2026-11-07T10:00:00Z", "3,1"),
        ]
        .concat();
        let newest = unix_seconds("2026-11-05T00:00:00Z").unwrap();
        let sent = tracks_to_send(&text, newest);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].starts_with("0,0,1") && sent[0].contains("2026-11-07") && !sent[0].ends_with('\r'));
        assert_eq!(tracks_to_send(&text, 0).len(), 2);
    }
}
