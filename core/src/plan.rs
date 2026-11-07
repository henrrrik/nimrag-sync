//! What a sync would change on the device: the contacts, address book, quick
//! texts and preset messages from Garmin's service, compared with what the
//! device holds. Nothing here talks to the device or the service; `sync`
//! gathers both and hands them in.
//!
//! It does what inReach Sync 1.4.47 does (SyncManager.cs, SyncDevice.cs):
//! when Garmin answers for any of contacts, presets or quick texts, the whole
//! SyncData table is cleared and only what Garmin sent is written back; the
//! contacts Garmin put on the device (Synced = 1) are replaced and those made
//! on the device kept; settings 89 and 97 get the quick texts' and presets'
//! "last changed" times, and 73 the time of the sync. The one departure: a
//! preset too long for the device makes inReach Sync loop forever, so here its
//! last recipients are dropped until it fits, as that code meant to.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::csv;
use crate::server::{Contact, Preset, QuickText};

pub const CONTACTS: u8 = 1;
pub const SYNC_DATA: u8 = 9;

/// SyncData's types (inReachSE.SyncDataTypes).
pub const ADDRESS: u32 = 1;
pub const QUICK_TEXT: u32 = 2;
pub const PRESET: u32 = 3;
pub const BREADCRUMB: u32 = 4;
/// The short codes of the three presets.
pub const PRESET_SLOTS: [u32; 3] = [14, 15, 16];
/// inReach Sync never writes an address whose ID is this (DeviceSettings.DEVICE_FREEFORM_ADDRESS_CODE).
const FREEFORM_ADDRESS_ID: i64 = 2;

/// The columns of the Contacts table by version, ID first (inReachSE.Tables).
pub fn contact_columns(version: u8) -> Option<&'static [&'static str]> {
    const V5: &[&str] = &[
        "ID", "Synced", "First Name", "Last Name", "Organization", "Email", "Phone", "inReach Address", "IMEI",
        "TeamDisplayName", "TeamEst", "MembersID",
    ];
    match version {
        1 => Some(&["ID", "First Name", "Last Name", "Organization", "Email", "Phone", "inReach Address"]),
        2 => Some(&V5[..8]),
        3 => Some(&V5[..9]),
        4 => Some(&V5[..11]),
        5 => Some(V5),
        _ => None,
    }
}

/// A contact as both sides can tell it: what is compared.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ContactFields {
    pub first_name: String,
    pub last_name: String,
    pub organization: String,
    pub email: String,
    pub phone: String,
    pub inreach: String,
    pub imei: String,
}

impl ContactFields {
    pub fn from_server(c: &Contact) -> ContactFields {
        let address = |kind: i32| {
            c.addresses.iter().find(|a| a.kind == kind).map(|a| a.address.clone()).unwrap_or_default()
        };
        ContactFields {
            first_name: c.first_name.clone().unwrap_or_default(),
            last_name: c.last_name.clone().unwrap_or_default(),
            organization: c.organization.clone().unwrap_or_default(),
            email: address(0),
            phone: address(1),
            inreach: address(2),
            imei: address(4),
        }
    }

    fn from_row(columns: &[&str], row: &[String]) -> ContactFields {
        let get = |name: &str| {
            columns.iter().position(|c| *c == name).and_then(|i| row.get(i)).cloned().unwrap_or_default()
        };
        ContactFields {
            first_name: get("First Name"),
            last_name: get("Last Name"),
            organization: get("Organization"),
            email: get("Email"),
            phone: get("Phone"),
            inreach: get("inReach Address"),
            imei: get("IMEI"),
        }
    }

    /// The insert line inReach Sync writes for this contact
    /// (SyncDevice.GenerateRowForTableVersion): every column but the ID, `1` for Synced.
    pub fn insert_line(&self, columns: &[&str]) -> String {
        let fields: Vec<String> = columns
            .iter()
            .skip(1)
            .map(|c| match *c {
                "Synced" => "1".to_string(),
                "First Name" => csv::encode(&self.first_name),
                "Last Name" => csv::encode(&self.last_name),
                "Organization" => csv::encode(&self.organization),
                "Email" => csv::encode(&self.email),
                "Phone" => csv::encode(&self.phone),
                "inReach Address" => csv::encode(&self.inreach),
                "IMEI" => csv::encode(&self.imei),
                _ => String::new(),
            })
            .collect();
        format!("i,{}", fields.join(","))
    }
}

/// One SyncData row: what it is and what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    #[serde(rename = "type")]
    pub kind: u32,
    pub short_code: u32,
    pub value: String,
}

impl Item {
    /// The insert line (SyncData inserts leave out the ID, which the device derives).
    pub fn insert_line(&self) -> String {
        format!("i,{},{},\"{}\"", self.kind, self.short_code, self.value.replace('"', "\"\""))
    }
}

/// What happens to one kind of data.
#[derive(Debug, Clone, Serialize)]
pub struct Changes<T> {
    /// Garmin sent nothing for it: the device keeps what it has.
    pub left_alone: bool,
    pub unchanged: Vec<T>,
    pub added: Vec<T>,
    pub removed: Vec<T>,
}

// Not derived: that would ask T for Default too.
impl<T> Default for Changes<T> {
    fn default() -> Self {
        Changes { left_alone: false, unchanged: Vec::new(), added: Vec::new(), removed: Vec::new() }
    }
}

impl<T> Changes<T> {
    pub fn is_change(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub contacts: Changes<ContactFields>,
    /// Contacts made on the device (Synced = 0): always kept.
    pub device_contacts_kept: usize,
    pub addresses: Changes<Item>,
    pub quick_texts: Changes<Item>,
    pub presets: Changes<Item>,
    /// SyncData rows of other kinds, which clearing the table removes too.
    pub other_sync_data_removed: Vec<Item>,
    /// The settings a sync writes, as (ID, value).
    pub settings: Vec<(u32, String)>,
    /// Whether the sync clears SyncData before writing it.
    pub clears_sync_data: bool,
    /// The SyncData rows a sync writes, in order, after clearing the table.
    #[serde(skip)]
    pub sync_data: Vec<Item>,
    /// The contacts a sync writes, and the device IDs of the synced rows it deletes first.
    #[serde(skip)]
    pub contact_rows: Vec<ContactFields>,
    #[serde(skip)]
    pub contact_ids_to_delete: Vec<String>,
}

impl Plan {
    pub fn is_change(&self) -> bool {
        self.contacts.is_change()
            || self.addresses.is_change()
            || self.quick_texts.is_change()
            || self.presets.is_change()
            || !self.other_sync_data_removed.is_empty()
    }
}

/// The preset's SyncData value as inReach Sync writes it: the recipients as
/// Garmin sends them, joined with commas, a bar, the message (Commands.cs).
/// A line longer than the device takes (its bytes plus 6 over 2000) loses its
/// last recipients until it fits.
pub fn preset_value(p: &Preset) -> String {
    let mut recipients = p.recipients.clone();
    loop {
        let value = format!("{}|{}", recipients.join(","), p.message);
        let line = Item { kind: PRESET, short_code: 14, value: value.clone() }.insert_line();
        if line.len() + 2 + 6 <= 2000 || recipients.is_empty() {
            return value;
        }
        recipients.pop();
    }
}

/// What Garmin's service answered for one kind of data.
#[derive(Debug, Clone, Copy)]
pub enum Sent<'a, T> {
    /// No answer for it (`null`): inReach Sync makes no step of it.
    NotSent,
    /// An answer without its list: inReach Sync writes nothing of it.
    NoList,
    List(&'a [T]),
}

impl<'a, T> Sent<'a, T> {
    pub fn from(answer: Option<Option<&'a [T]>>) -> Self {
        match answer {
            None => Sent::NotSent,
            Some(None) => Sent::NoList,
            Some(Some(list)) => Sent::List(list),
        }
    }
    fn answered(&self) -> bool {
        !matches!(self, Sent::NotSent)
    }
    fn list(&self) -> Option<&'a [T]> {
        match self {
            Sent::List(l) => Some(l),
            _ => None,
        }
    }
}

pub struct FromServer<'a> {
    pub contacts: Sent<'a, Contact>,
    pub presets: Sent<'a, Preset>,
    pub quick_texts: Sent<'a, QuickText>,
    /// When the presets and quick texts last changed (Unix seconds), from the sync status.
    pub presets_modified: Option<i64>,
    pub quick_texts_modified: Option<i64>,
    /// The time of the sync (Unix seconds), for setting 73.
    pub now: i64,
}

/// The settings a sync writes, as (ID, value).
pub const QUICK_TEXTS_TIME: u32 = 89;
pub const PRESETS_TIME: u32 = 97;
pub const SYNC_TIME: u32 = 73;

/// The plan for a device whose Contacts table (`contacts_version`, its text)
/// and SyncData text are given.
pub fn plan(contacts_version: u8, contacts_text: &str, sync_data_text: &str, server: &FromServer) -> Result<Plan, String> {
    let columns = contact_columns(contacts_version)
        .ok_or_else(|| format!("the device's Contacts table has version {contacts_version}, which nimrag-sync does not know"))?;
    let synced_at = columns.iter().position(|c| *c == "Synced");

    // Contacts: only the rows Garmin's service put there (Synced = 1) are its to change.
    let rows = csv::parse(contacts_text);
    let (synced, made_here): (Vec<_>, Vec<_>) =
        rows.iter().partition(|r| synced_at.is_some_and(|i| r.get(i).map(String::as_str) == Some("1")));
    let mut contacts = Changes::default();
    let mut contact_rows = Vec::new();
    let mut contact_ids_to_delete = Vec::new();
    match server.contacts {
        Sent::NotSent => {
            contacts.left_alone = true;
            contacts.unchanged = synced.iter().map(|r| ContactFields::from_row(columns, r)).collect();
        }
        // inReach Sync would delete the synced contacts, then fail on the missing list.
        Sent::NoList => return Err("Garmin's answer for the contacts had no list; nothing is synced.".into()),
        Sent::List(list) => {
            let mut have: Vec<ContactFields> = synced.iter().map(|r| ContactFields::from_row(columns, r)).collect();
            contact_ids_to_delete = synced.iter().filter_map(|r| r.first().cloned()).collect();
            for c in list {
                let fields = ContactFields::from_server(c);
                if let Some(i) = have.iter().position(|h| *h == fields) {
                    have.remove(i);
                    contacts.unchanged.push(fields.clone());
                } else {
                    contacts.added.push(fields.clone());
                }
                contact_rows.push(fields);
            }
            contacts.removed = have;
        }
    }

    // SyncData, by kind. Any answer for contacts, presets or quick texts clears
    // the table (SyncStep.RemoveSyncData); then each kind with a list is written.
    let clears = server.contacts.answered() || server.presets.answered() || server.quick_texts.answered();
    let mut current: BTreeMap<u32, Vec<Item>> = BTreeMap::new();
    for r in csv::parse(sync_data_text) {
        let (Some(kind), Some(code)) = (r.get(1).and_then(|v| v.parse().ok()), r.get(2).and_then(|v| v.parse().ok())) else {
            continue;
        };
        current.entry(kind).or_default().push(Item { kind, short_code: code, value: r.get(3).cloned().unwrap_or_default() });
    }
    let addresses_wanted = server.contacts.list().map(|list| {
        list.iter()
            .flat_map(|c| &c.addresses)
            .filter(|a| a.id != FREEFORM_ADDRESS_ID && a.id >= 0)
            .map(|a| Item { kind: ADDRESS, short_code: a.id as u32, value: a.address.clone() })
            .collect::<Vec<_>>()
    });
    let quick_texts_wanted = server.quick_texts.list().map(|list| {
        list.iter()
            .filter(|q| q.id >= 0)
            .map(|q| Item { kind: QUICK_TEXT, short_code: q.id as u32, value: q.text.clone() })
            .collect::<Vec<_>>()
    });
    let presets_wanted = server.presets.list().map(|list| {
        list.iter()
            .zip(PRESET_SLOTS)
            .map(|(p, slot)| Item { kind: PRESET, short_code: slot, value: preset_value(p) })
            .collect::<Vec<_>>()
    });

    let mut sync_data = Vec::new();
    let mut kind_changes = |kind: u32, wanted: Option<Vec<Item>>| {
        let mut have = current.remove(&kind).unwrap_or_default();
        let mut changes = Changes::default();
        if !clears {
            changes.left_alone = true;
            changes.unchanged = have;
            return changes;
        }
        for item in wanted.unwrap_or_default() {
            if let Some(i) = have.iter().position(|h| *h == item) {
                have.remove(i);
                changes.unchanged.push(item.clone());
            } else {
                changes.added.push(item.clone());
            }
            sync_data.push(item);
        }
        changes.removed = have;
        changes
    };
    let addresses = kind_changes(ADDRESS, addresses_wanted);
    let quick_texts = kind_changes(QUICK_TEXT, quick_texts_wanted);
    let presets = kind_changes(PRESET, presets_wanted);
    let wrote_any = server.contacts.list().is_some() || server.presets.list().is_some() || server.quick_texts.list().is_some();
    let breadcrumb = current.remove(&BREADCRUMB).unwrap_or_default();
    let mut other_sync_data_removed: Vec<Item> = Vec::new();
    if clears {
        other_sync_data_removed = current.into_values().flatten().collect();
        if wrote_any {
            sync_data.push(Item { kind: BREADCRUMB, short_code: 0, value: "sbc".into() });
        } else {
            other_sync_data_removed.extend(breadcrumb);
        }
    }

    let mut settings = Vec::new();
    if server.quick_texts.list().is_some() && let Some(t) = server.quick_texts_modified {
        settings.push((QUICK_TEXTS_TIME, t.to_string()));
    }
    if server.presets.list().is_some() && let Some(t) = server.presets_modified {
        settings.push((PRESETS_TIME, t.to_string()));
    }
    if clears {
        settings.push((SYNC_TIME, server.now.to_string()));
    }

    Ok(Plan {
        contacts,
        device_contacts_kept: made_here.len(),
        addresses,
        quick_texts,
        presets,
        other_sync_data_removed,
        settings,
        clears_sync_data: clears,
        sync_data,
        contact_rows,
        contact_ids_to_delete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Address;

    fn contact(id: i64, first: &str, email: &str, email_id: i64) -> Contact {
        Contact {
            id,
            first_name: Some(first.into()),
            last_name: Some("Lovelace".into()),
            organization: None,
            addresses: vec![Address { id: email_id, kind: 0, address: email.into() }],
        }
    }

    fn preset(message: &str, map_share: bool) -> Preset {
        Preset { id: 1, recipients: vec!["ada@example.com".into(), "+46700000000".into()], message: message.into(), facebook: false, twitter: false, map_share }
    }

    // A device like the one in docs/device-observations.md, with made-up data.
    const CONTACTS_V5: &str = "1,0,\"\",\"\",\"\",\"test@example.com\",\"\",\"\",\"\",\"\",\"0\",\"0\"\r\n\
        30,1,\"Ada\",\"Lovelace\",\"\",\"ada@example.com\",\"\",\"\",\"\",\"\",\"0\",\"0\"\r\n\
        31,1,\"Old\",\"Friend\",\"\",\"old@example.com\",\"\",\"\",\"\",\"\",\"0\",\"0\"\r\n";
    const SYNC_DATA: &str = "65547,1,11,\"ada@example.com\"\r\n65548,1,12,\"old@example.com\"\r\n\
        65539,1,3,\"+46700000000\"\r\n\
        131102,2,30,\"I'm fine\"\r\n\
        196622,3,14,\"ada@example.com,+46700000000,ms|All good\"\r\n\
        262144,4,0,\"sbc\"\r\n";

    fn from<'a>(contacts: Sent<'a, Contact>, presets: Sent<'a, Preset>, quick_texts: Sent<'a, QuickText>) -> FromServer<'a> {
        FromServer { contacts, presets, quick_texts, presets_modified: Some(1600500092), quick_texts_modified: Some(1600355483), now: 1789300000 }
    }

    #[test]
    fn as_inreach_sync_does_with_a_device_like_the_mini() {
        let contacts = [
            contact(7, "Ada", "ada@example.com", 11),
            Contact { last_name: Some("Friend".into()), ..contact(8, "Old", "old@example.com", 12) },
        ];
        let quick = [QuickText { id: 30, text: "I'm fine".into() }];
        let presets = [preset("All good", true)];
        let p = plan(5, CONTACTS_V5, SYNC_DATA, &from(Sent::List(&contacts), Sent::List(&presets), Sent::List(&quick))).unwrap();
        assert_eq!(p.device_contacts_kept, 1);
        assert_eq!(p.contacts.unchanged.len(), 2);
        assert!(!p.contacts.is_change() && !p.quick_texts.is_change());
        // The synced rows are deleted by ID before the contacts are written again.
        assert_eq!(p.contact_ids_to_delete, ["30", "31"]);
        // An address that is no Garmin contact's goes, as clearing SyncData has it.
        assert_eq!(p.addresses.removed.iter().map(|i| i.short_code).collect::<Vec<_>>(), [3]);
        // inReach Sync writes the recipients as Garmin sends them: no `ms`.
        assert_eq!(p.presets.removed[0].value, "ada@example.com,+46700000000,ms|All good");
        assert_eq!(p.presets.added[0].value, "ada@example.com,+46700000000|All good");
        assert!(p.clears_sync_data);
        assert_eq!(p.sync_data.last().unwrap().kind, BREADCRUMB);
        assert_eq!(p.settings, [(89, "1600355483".to_string()), (97, "1600500092".to_string()), (73, "1789300000".to_string())]);
    }

    #[test]
    fn additions_and_removals() {
        let contacts = [contact(7, "Ada", "ada@example.com", 11), contact(9, "Grace", "grace@example.com", 13)];
        let p = plan(5, CONTACTS_V5, SYNC_DATA, &from(Sent::List(&contacts), Sent::NotSent, Sent::NotSent)).unwrap();
        assert_eq!(p.contacts.added.iter().map(|c| c.first_name.as_str()).collect::<Vec<_>>(), ["Grace"]);
        assert_eq!(p.contacts.removed.iter().map(|c| c.first_name.as_str()).collect::<Vec<_>>(), ["Old"]);
        assert_eq!(p.addresses.added[0].short_code, 13);
        // Presets and quick texts were not answered, but clearing SyncData removes them all the same.
        assert_eq!(p.presets.removed.len(), 1);
        assert_eq!(p.quick_texts.removed.len(), 1);
        assert_eq!(p.settings, [(73, "1789300000".to_string())]);
    }

    #[test]
    fn without_any_answer_nothing_is_touched() {
        let p = plan(5, CONTACTS_V5, SYNC_DATA, &from(Sent::NotSent, Sent::NotSent, Sent::NotSent)).unwrap();
        assert!(!p.is_change() && !p.clears_sync_data && p.settings.is_empty());
        assert!(p.contacts.left_alone && p.presets.left_alone && p.presets.unchanged.len() == 1);
    }

    #[test]
    fn an_answer_without_its_list() {
        // Presets answered without a list: cleared, nothing written, no time for them.
        let p = plan(5, CONTACTS_V5, SYNC_DATA, &from(Sent::NotSent, Sent::NoList, Sent::NotSent)).unwrap();
        assert!(p.clears_sync_data && p.presets.removed.len() == 1 && p.sync_data.is_empty());
        assert_eq!(p.settings, [(73, "1789300000".to_string())]);
        // Contacts without a list would cost the synced contacts: refused.
        assert!(plan(5, CONTACTS_V5, SYNC_DATA, &from(Sent::NoList, Sent::NotSent, Sent::NotSent)).is_err());
    }

    #[test]
    fn a_long_preset_loses_its_last_recipients() {
        let many: Vec<String> = (0..300).map(|i| format!("person{i}@example.com")).collect();
        let p = Preset { recipients: many, ..preset("Hi", false) };
        let value = preset_value(&p);
        let line = Item { kind: PRESET, short_code: 14, value: value.clone() }.insert_line();
        assert!(line.len() + 2 + 6 <= 2000);
        assert!(value.starts_with("person0@example.com,person1@example.com,") && value.ends_with("|Hi"));
    }

    #[test]
    fn insert_lines_follow_the_table_version() {
        let fields = ContactFields::from_server(&Contact { organization: Some("Analytical, Ltd".into()), ..contact(7, "Ada", "ada@example.com", 11) });
        assert_eq!(fields.insert_line(contact_columns(5).unwrap()), "i,1,Ada,Lovelace,\"Analytical, Ltd\",ada@example.com,,,,,,");
        assert_eq!(fields.insert_line(contact_columns(1).unwrap()), "i,Ada,Lovelace,\"Analytical, Ltd\",ada@example.com,,");
        let item = Item { kind: PRESET, short_code: 14, value: "a@b,ms|Say \"hi\"".into() };
        assert_eq!(item.insert_line(), "i,3,14,\"a@b,ms|Say \"\"hi\"\"\"");
        assert!(plan(9, "", "", &from(Sent::NotSent, Sent::NotSent, Sent::NotSent)).is_err());
    }
}
