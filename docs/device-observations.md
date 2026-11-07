# What a real inReach Mini answered

Read-only checks with `nimrag-sync tables` and `nimrag-sync read-table <id>` on an inReach Mini (1st
generation, `091e:0b89`, software 3.90.1101380), 2026-11-07. Only messages that read were sent, apart
from the restart into app mode that inReach Sync also sends before each sync (`0xAAFE`). No personal data
is kept here: the shape of the tables only.

## Modes

- In **mass storage** (USB drive) and **charger** mode the device answers `VERSION` and `READ_APP_MODE`,
  but **not** `TABLE_VERSION`: tables need **app mode**.
- `RESET_DEVICE_REQ` (`0xAAFE`) from charger mode: the device left the bus and came back in app mode
  (switched on) within about 4 s.

## Tables present

Every table inReach Sync defines is present. Two are newer than the versions inReach Sync knows:

| ID | Table | Version on the device | Versions inReach Sync knows |
|---|---|---|---|
| 1 | Contacts | 5 | 1–5 |
| 2 | TrackPoints | 1 | 1 |
| 3 | Messages | 3 | 2, 3 |
| 4 | MessageThreads | **3** | 2 |
| 5 | Conversations | 1 | 1 |
| 6 | Words | **0** | 1 |
| 7 | Settings | 1 | 1 |
| 8 | Configuration | 1 | 1 |
| 9 | SyncData | 1 | 1 |
| 10 | EventLog | 1 | 1 |
| 12 | Forms | 1 | 1 |
| 13 | FormFields | 1 | 1 |
| 14 | Waypoints | 3 | 1–3 |
| 15 | TrackBlocks | 5 | 5 |
| 16 | Routes | 1 | 1 |
| 18 | Locations | **2** | 1 |

## READ_TABLE

`READ_TABLE` streams CSV without a header row, rows separated by `\r\n`, as the spec says. Text fields are
quoted, numbers are not, empty fields are `""` or nothing. UTF-8 (Swedish letters came through intact).

- **Settings (7)**: 160 rows `ID,Current[,Default]`. Some values are long hex blobs (IDs 11, 19, 83, 91).
  - ID 9 is a 15-digit number starting with `3004340`, the Iridium IMEI range: **the device's IMEI** (the
    serial in the `VERSION` reply is all zeros on this Mini).
  - ID 26 is the inReach address (`…@inreach.garmin.com`); 18 and 42 the plan's name and description.
- **Contacts (1, v5)**: `ID,Synced,First,Last,Org,Email,Phone,inReachAddr,IMEI,TeamDisplayName,TeamEst,MembersID`.
  Rows with `Synced = 1` came from Explore (inReach Sync replaces only those); rows with `Synced = 0` were
  made on the device and must be kept.
- **SyncData (9)**: `ID,Type,ShortCode,Value`. The ID is `(Type << 16) | ShortCode` (e.g. `0x3000E` = type
  3, short code 14), so the device derives it, which is why inserts leave it out. Present: addresses (type
  1), quick texts (type 2, short codes from 30), the three presets (type 3, short codes 14, 15, 16) and the
  sync breadcrumb (type 4, short code 0, value `sbc`).
- **Messages (3, v3)**: one row per message sent or received, with GUID, created and received times (ISO
  8601, UTC), position, flags, address and text, in the columns of the spec's v3. **inReach Sync never
  reads this table**; it is readable, so messages can be backed up and exported.
- **MessageThreads (4, v3)**: 7 numeric columns per row, one row per message; the meaning of the columns is
  not known yet (version 3 is newer than inReach Sync's definition).
- **Waypoints (14, v3)**: `ID,SyncState,Hidden,Symbol,Lat,Lon,Created,Updated,Name`, as the spec says.
- **Routes (16)**: empty on this device.
