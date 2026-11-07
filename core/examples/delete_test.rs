//! Does `d,*` (and `d,<id>`) work on SyncData? Inserts a dummy quick text in
//! slot 99, sends `d,*`, reads back, deletes the dummy by its ID, then writes
//! back any row that went missing, so SyncData ends as it started.
//! Prints types, short codes and IDs, never values.

use nimrag::device::{self, InReach};
use nimrag::plan::Item;
use nimrag::{csv, plan};

type Rows = Vec<(String, u32, u32, String)>; // ID, type, short code, value

fn rows(text: &str) -> Rows {
    csv::parse(text)
        .into_iter()
        .filter_map(|r| Some((r.first()?.clone(), r.get(1)?.parse().ok()?, r.get(2)?.parse().ok()?, r.get(3).cloned().unwrap_or_default())))
        .collect()
}

fn keys(r: &Rows) -> Vec<String> {
    r.iter().map(|(id, t, c, _)| format!("{t}/{c}={id}")).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let found = device::find(std::path::Path::new(device::SYSFS_HIDRAW)).into_iter().next().ok_or("no inReach")?;
    let mut dev = InReach::open(&found)?;
    if dev.app_mode()? != device::AppMode::App {
        return Err("the inReach must be on".into());
    }
    let t = plan::SYNC_DATA;
    let v = dev.table_version(t)?.ok_or("no SyncData")?;
    let before_text = dev.read_table(t, v)?;
    let saved = std::env::args().nth(1).ok_or("give a file to save SyncData to")?;
    std::fs::write(&saved, &before_text)?;
    let before = rows(&before_text);
    println!("1. before: {} rows", before.len());

    // With "restore <saved file>": clear SyncData with d,* (the device spares the
    // address book) and write back the saved rows, nothing else.
    if std::env::args().nth(2).as_deref() == Some("restore") {
        let keep = rows(&std::fs::read_to_string(std::env::args().nth(3).ok_or("give the saved file")?)?);
        dev.write_table(t, v, "d,*\r\n")?;
        let now = rows(&dev.read_table(t, v)?);
        let missing: Vec<String> = keep
            .iter()
            .filter(|b| !now.iter().any(|n| n.1 == b.1 && n.2 == b.2 && n.3 == b.3))
            .map(|(_, k, c, val)| Item { kind: *k, short_code: *c, value: val.clone() }.insert_line())
            .collect();
        dev.batch_write(t, v, &missing, device::max_script_len("inReach Mini", ""))?;
        let end = rows(&dev.read_table(t, v)?);
        let same = keys(&end) == keys(&keep) && end.iter().zip(&keep).all(|(a, b)| a.3 == b.3);
        println!("restored {} rows: {} rows now, identical to the saved file: {same}", missing.len(), end.len());
        return Ok(());
    }

    let dummy = Item { kind: plan::QUICK_TEXT, short_code: 99, value: "nimrag-sync test".into() };
    dev.write_table(t, v, &format!("{}\r\n", dummy.insert_line()))?;
    let with_dummy = rows(&dev.read_table(t, v)?);
    let dummy_id = with_dummy.iter().find(|r| r.1 == 2 && r.2 == 99).map(|r| r.0.clone());
    println!("2. dummy inserted: {} rows, dummy ID {:?}", with_dummy.len(), dummy_id);

    // With "single" as the second argument: skip d,* and delete only the dummy.
    let single = std::env::args().nth(2).as_deref() == Some("single");
    let answer = if single { Ok(()) } else { dev.write_table(t, v, "d,*\r\n") };
    let after_all = rows(&dev.read_table(t, v)?);
    println!("3. d,* answered {:?}: {} rows left", answer.as_ref().map(|_| "taken").map_err(|e| e.to_string()), after_all.len());

    if let Some(id) = &dummy_id
        && after_all.iter().any(|r| &r.0 == id)
    {
        let answer = dev.write_table(t, v, &format!("d,{id}\r\n"));
        let after_one = rows(&dev.read_table(t, v)?);
        let still = after_one.iter().any(|r| &r.0 == id);
        println!("4. d,{id} answered {:?}: dummy still there: {still}, {} rows", answer.map(|_| "taken").map_err(|e| e.to_string()), after_one.len());
    } else {
        println!("4. the dummy is gone already");
    }

    // Restore: write back every original row that is missing or changed.
    let now = rows(&dev.read_table(t, v)?);
    let missing: Vec<_> = before.iter().filter(|b| !now.iter().any(|n| n.1 == b.1 && n.2 == b.2 && n.3 == b.3)).collect();
    if !missing.is_empty() {
        let lines: Vec<String> = missing.iter().map(|(_, k, c, val)| Item { kind: *k, short_code: *c, value: val.clone() }.insert_line()).collect();
        dev.batch_write(t, v, &lines, device::max_script_len("inReach Mini", ""))?;
        println!("5. wrote back {} rows", missing.len());
    } else {
        println!("5. nothing to write back");
    }
    if let Some(id) = &dummy_id
        && rows(&dev.read_table(t, v)?).iter().any(|r| &r.0 == id)
    {
        dev.write_table(t, v, &format!("d,{id}\r\n"))?;
    }
    let end = rows(&dev.read_table(t, v)?);
    let same = keys(&end) == keys(&before) && end.iter().zip(&before).all(|(a, b)| a.3 == b.3);
    println!("6. end: {} rows, identical to the start: {same}", end.len());
    if !same {
        println!("   start: {:?}\n   end:   {:?}", keys(&before), keys(&end));
    }
    Ok(())
}
