# inReach Sync device protocol (beyond framing)

What Garmin/DeLorme **inReach Sync 1.4.47.4776** does to an inReach over USB, worked out by reading the ilspycmd decompilation **statically**. No device was touched while writing this. The spec is meant for reimplementing the protocol in `nimrag-sync` (Rust, Linux).

Basic DelBin framing is already verified on a real inReach Mini and is **not** re-derived here:

- 64-byte HID reports with no report IDs.
- Output report: `[0x20][len][≤62 bytes]`.
- Message: `u16 0xFEDB, u16 code, u16 len, u16 hdr-cksum [, data, u16 data-cksum, u16 0xBCAD]`.
- Checksum: `0x10000 − Σ LE u16`.
- `DELBIN_VERSION`, `READ_APP_MODE` and `RESET_FOR_MASS_STORAGE` are confirmed working.

The framing code is at `DLB:886-1022`, for reference.

Every claim carries a `file:line` citation. Statements labelled **Observed** are read directly from the code. Statements labelled **Inferred** are interpretation and are not proven.

### File abbreviations

| Abbrev | Path (under `~/Downloads/inReachSync-extracted/4-decompiled/`) |
|---|---|
| `DLB` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb/DeLormeBinary.cs` |
| `DM` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb/DeviceManager.cs` |
| `DC` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb/DeviceConfig.cs` |
| `BW` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb/BatchWriter.cs` |
| `DV` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb/DeviceVersion.cs` |
| `MSG` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb.Interfaces.MsgCodes/DelBinMessages.cs` |
| `BLC` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb.Interfaces.MsgCodes/BootLoaderMessageCode.cs` |
| `AM` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb.Interfaces.MsgCodes/AppMode.cs` |
| `DIC` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb.Interfaces.MsgCodes/DeviceInfoConstants.cs` |
| `SEF` | `Common.Devices.Usb/DeLorme.Common.Devices.Usb/SEFormField.cs` |
| `WDEV` | `Common.Devices.Usb.Windows/DeLorme.Common.Devices.Usb.Windows/Device.cs` |
| `CD` | `Common.Devices.Usb.Windows/DeLorme.Common.Devices.Usb.Windows/ConnectedDevices.cs` |
| `NAT` | `Common.Devices.Usb.Windows/DeLorme.Common.Devices.Usb.Windows.MassStorage/Native.cs` |
| `SE` | `Common.Devices.Sync/Common.Devices.Sync/inReachSE.cs` |
| `CMD` | `Common.Devices.Sync/Common.Devices.Sync/Commands.cs` |
| `CSV` | `Common.Devices.Sync/inReach.Sync/CsvTools.cs` |
| `SD` | `inReach.Sync/inReach.Sync/SyncDevice.cs` |
| `DS` | `inReach.Sync/inReach.Sync/DeviceSettings.cs` |
| `DW` | `inReach.Sync/inReach.Sync/DeviceWaypoints.cs` |
| `DR` | `inReach.Sync/inReach.Sync/DeviceRoutes.cs` |
| `DT` | `inReach.Sync/inReach.Sync/DeviceTracks.cs` |
| `DMAP` | `inReach.Sync/inReach.Sync/DeviceMaps.cs` |
| `SM` | `inReach.Sync/inReach.Sync/SyncManager.cs` |
| `APP` | `inReach.Sync/inReach.Sync.UI/Application.cs` |
| `RC/*` | `SyncRestClient/SyncRestClient/*.cs` and `SyncRestClient/SyncRestClient.Models/*.cs` |

---

## 0. Message codes and transport notes

### 0.1 DelBin message codes (`MSG:3-21`)

| Code | Name | Payload sent | Reply | Used by Sync? |
|---|---|---|---|---|
| `0xA001` | `DELBIN_VERSION` | none | 180 B (see 0.3) | yes (`DLB:108-133`) |
| `0xA010` | `DELBIN_NAV_DATA_OUT` | – | unsolicited? | no (enum only) |
| `0xA020` | `DELBIN_SAT_INFO_OUT` | – | unsolicited? | no |
| `0xC020` | `DELBIN_MAPPKG_COUNT_REQ/OUT` | – | – | no |
| `0xFC00` | `DELBIN_FVT` | – | – | no |
| `0xFC01` | `DELBIN_TABLE_VERSION` | `[table u8]` | `[version u8]`; `0xFF` = table absent | yes (`DLB:135-151`, `DM:177-190`) |
| `0xFC02` | `DELBIN_READ_TABLE` | `[table u8][version u8]` | stream of `0xFC02` msgs with UTF-8 chunks, ended by a 0-length msg | yes (`DLB:153-182`) |
| `0xFC03` | `DELBIN_WRITE_TABLE` | `[table u8][version u8][UTF-8 script]` | `0xFC03`: no data = OK, data = UTF-8 error text | yes (`DLB:204-245`) |
| `0xFC04` | `DELBIN_RESET_DEVICE_FOR_FLASH` | none | none awaited; device reboots into bootloader | yes (`DLB:354-361`) |
| `0xFC05` | `DELBIN_READ_FLASH` | `[addr u32 LE][size u32 LE]` | stream of `0xFC05` chunks, ended by 0-length msg | **defined, never called** (`DLB:247-284`) |
| `0xFC06` | `DELBIN_APPLY_DEFAULT_SETTINGS` | none | none awaited | yes, after a device-type change (`DLB:345-352`, `DS:156-161`) |
| `0xAAFE` | `DELBIN_RESET_DEVICE_REQ` | none | none awaited; device reboots to app mode | yes (`DLB:363-370`) |
| `0xFC07` | `DELBIN_RESET_FOR_MASS_STORAGE` | none | none awaited | yes, maps only (`DLB:372-379`) |
| `0xFC08` | `DELBIN_READ_APP_MODE` | none | 1 byte `AppMode` | yes (`DLB:90-106`) |
| `0xFC09` | `DELBIN_RESET_ON_EJECT` | – | – | **never sent** (only in enum, `MSG:20`) |

Note that `0xAAFE` is outside the `0xFCxx` block, which is odd but is what the code says (`MSG:17`).

### 0.2 Request and response mechanics (Observed)

- **Default timeout.** Every request uses `DefaultTimeout` = **1800 ms** (`DLB:43`). `WRITE_TABLE` uses 8 × 1800 = **14 400 ms** (`DLB:214`, `DLB:234`).
- **Matching.** `SendRequestReceiveResponse` sends the request, then reads messages until one matches the request code (`DLB:841-870`). Messages with other codes, such as unsolicited NAV or SAT output, are **silently dropped** (`DLB:859-866`). A `Partial` match restarts the timeout stopwatch (`DLB:861-863`). This is how the streamed replies to `READ_TABLE` and `READ_FLASH` work.
- **Receive.** `Receive` discards reports that do not start with a valid header (`DLB:925-928`) and rejects `len > 4088` (`DLB:929-933`). A message whose data checksum or `0xBCAD` trailer is bad is silently dropped and reading continues (`DLB:955-961`). The maximum payload is therefore 4084 bytes.
- **Input report parsing.** The Windows input buffer includes a report-ID byte, so `Read` takes the length from byte `[2]` and the data from byte `[3]` (`DLB:966-981`). On hidraw with no report IDs this becomes length at `[1]` and data at `[2]`, which matches the verified framing.
- **Output fragmentation.** `Write` splits any byte string into `OutputReportLength − 3` = 62-byte chunks, each sent as `[0x00 reportID][0x20][n][chunk]` (`DLB:983-1007`). **Bootloader packets travel inside this same `0x20/len` HID wrapper.** There is no separate transport for them.
- **Input reports are gated.** `EnableInputReports` controls whether incoming reports are queued, and setting it to false flushes the queue (`WDEV:62-89`, `WDEV:339-342`). `ProcessInputReports` enables queuing only for the duration of a request (`DLB:872-884`). **Inferred:** anything the device sends while queuing is disabled is lost. A Rust port must likewise drain stale input before each request, as `DiscardReadData` does (`DLB:184-202`): it reads with a 2 s timeout until nothing arrives.
- **Device matching.** Accepted devices are VID `0x1163` (DeLorme, 4451) with PID `0x0300`, or VID `0x091E` (Garmin, 2334) with PID `0x0B89` (`CD:8-12`, `DIC:5-11`).
- **Device generation from `bcdDevice`.** The HID `VersionNumber` (`bcdDevice`) selects the device generation. `VersionMajor = VersionNumber >> 8` (`WDEV:56`, `DM:226-238`).

### 0.3 `DELBIN_VERSION` reply layout (`DLB:119-129`)

This is already verified; it is listed here for completeness.

| Off | Len | Field |
|---|---|---|
| 0 | 4 | `gpsFwMemAddress` u32 LE |
| 4 | 32 | company name, ASCII, NUL-padded |
| 36 | 32 | product name (drives all product detection, §6) |
| 68 | 32 | firmware version `"M.m.r[.b]"` |
| 100 | 48 | GPS firmware |
| 148 | 16 | serial number = IMEI. An IMEI of `"000000000000000"` means "off" (§6) |
| 164 | 16 | extra info |

The reply must be exactly 180 bytes, otherwise the call returns null (`DLB:119`).

---

## 1. Tables

### 1.1 Primitives (Observed)

- **`TableVersion(table)`** (`DLB:135-151`) sends `0xFC01 [table]`. The reply's `Data[0]` is the version. A reply with no data maps to 0.
  - `DM.GetTableVersion` maps **`0xFF` → "table not present"** (`DM:184-189`).
  - Sync always asks for the version first, then picks the matching `TableInfo` (`SD:216-236`). It throws `UnsupportedTableVersion` if it has no definition for that version (`SD:231-234`).
- **`ReadTable(table, version)`** (`DLB:153-182`) sends `0xFC02 [table][version]`.
  - The device replies with **N messages with code `0xFC02`**, each carrying a UTF-8 fragment, then **one `0xFC02` message with no data** as the terminator.
  - The fragments are concatenated. A message boundary may fall anywhere, including mid-line.
  - Each fragment restarts the 1800 ms timeout. A timeout returns null, and `DM.ReadTable` turns that into `ReadTableException` (`DM:355-368`).
- **`WriteTable(table, version, script)`** (`DLB:204-225`) sends `0xFC03 [table][version][UTF-8 script]` and waits up to 14.4 s for a `0xFC03` reply.
  - **Return value semantics:**

    | Device reply | `WriteTable` returns | Meaning |
    |---|---|---|
    | `0xFC03` with **no data** | `null` | **success** |
    | `0xFC03` with data | UTF-8 string of the data | **error text** from the device. Its content is not documented in the code. |
    | no reply within 14.4 s | the literal `"Response not received."` | failure |

  - Callers treat any non-null result as an error, or any non-whitespace result in the case of `BatchWriter` (`BW:102-106`, `SD:335-341`, `DC:244`, `DC:266`, `DC:274`).
- **`WriteTableBytes(bytes)`** (`DLB:227-245`) works the same way, but the caller supplies the whole payload including the two leading `[table][version]` bytes. It is used only for server-supplied "device commands" (§1.6).

### 1.2 Table IDs and column definitions (`SE:13-210`)

Column lists are the CSV columns of a row, in the order a `READ_TABLE` returns them and an `i,` insert supplies them. **Sync never sends a header row.** It parses read results with `hasHeaders:false` (for example `DS:59`, `SD:182`).

| ID | Name | Versions known | Columns (latest known version) | Sync reads? | Sync writes? |
|---|---|---|---|---|---|
| 1 | Contacts | 1–5 | v5: `ID, Synced, First Name, Last Name, Organization, Email, Phone, inReach Address, IMEI, TeamDisplayName, TeamEst, MembersID`. v4 drops MembersID, v3 drops Team*, v2 drops IMEI, v1 drops Synced (`SE:49-83`) | yes, to find old synced rows (`SD:279`, `SD:312-330`) | yes (`SD:271-302`) |
| 2 | TrackPoints | 1 | `ID, Created, Elevation, Latitude, Longitude, Speed, Course, GPS Status, Point Type` (`SE:84-90`) | no | no |
| 3 | Messages | 2, 3 | v3: `ID, GUID, Created, Received, Elevation, Latitude, Longitude, Speed, Course, GPS Status, Message Type, Sent to Iridium, Delivered to App, Sent by App, Send Failed, Waiting for GPS, Encrypted, Address, Message` (`SE:91-104`) | **no** | no |
| 4 | MessageThreads | 2 | (null) (`SE:105-111`) | no | no |
| 5 | Conversations | 1 | (null) (`SE:112-118`) | no | no |
| 6 | Words | 1 | (null) (`SE:119-125`) | no | no |
| 7 | Settings | 1 | `ID, Current Value, Default Value` (`SE:126-132`) | yes | yes |
| 8 | Configuration | 1 | `ID, Value1..Value9` (`SE:133-139`) | no | only via unused `DeviceConfig` |
| 9 | SyncData | 1 | `ID, Type, ShortCode, Value` (`SE:140-146`) | no | yes (address book, quick texts, presets, breadcrumb) |
| 10 | EventLog | 1 | `ID, Type, Time, Message` (`SE:147-153`) | **yes**, uploaded verbatim to the server (`SD:760-764`, `SM:1124-1134`) | no |
| 11 | – | – | (no definition) | – | – |
| 12 | Forms | 1 | `ID, Title, Destination, Prefix, Delimiter` (`SE:154-160`) | no | only via unused `DeviceConfig` |
| 13 | FormFields | 1 | `ID, FormID, Type, Min, Max, Required, Name, DefaultValue, PickList` (`SE:161-167`) | no | only via unused `DeviceConfig` |
| 14 | Waypoints | 1–3 | v3: `ID, SyncState, Hidden, Symbol, Latitude, Longitude, Created, Updated, Name` (`SE:168-188`) | yes (v3 only) | yes (v3 only) |
| 15 | TrackBlocks | 5 | `TrackID, Level, FragmentID, North, West, South, East, Lat, Lon, Time, Elevation, Speed, TimeBytes, LatBytes, LatScale, LonBytes, LonScale, ElevBytes, ElevScale, SpeedBytes, SpeedScale, IndexOfFirstPointInParent, NextFragmentID, DataBlock` (`SE:189-195`) | yes (v5 only) | yes (v5 only, route tracks) |
| 16 | Routes | 1 | `RouteID, TrackID, Created, Updated, Color, BorderColor, Deleted, VisibleOnMap, Name` (`SE:196-202`) | yes | yes |
| 17 | – | – | (no definition) | – | – |
| 18 | Locations | 1 | `LocationID, Lat, Lon, Name, Premium, Marine, Aviation` (`SE:203-209`) | no | no |

The `TableNames` enum (`SE:13-32`) numbers Locations as 17, but `TableInfo.TableID` is **18** (`SE:205`). **The protocol uses `TableInfo.TableID`**: every lookup goes through `Tables.First(...).TableID` (`DM:183`).

### 1.3 Script language (Observed)

A script is a UTF-8 string of **command lines terminated by `\r\n`**. `BatchWriter` appends `\r\n` when missing (`BW:16`, `BW:49-56`). Server-sent device commands use a bare `\r` terminator (`SD:519`), so the firmware apparently also accepts a bare `\r`.

| Command | Meaning (Observed usage; semantics Inferred) | Example source |
|---|---|---|
| `i,<f1>,<f2>,...` | insert or replace a row; the fields follow the table's column order | everywhere |
| `d,<id>` | delete the row whose first column equals `<id>` | `SD:327`, `DS:106`, `DS:175`, `DT:111-113` |
| `d,*` | delete all rows in the table | `SD:335`, `DC:265` |

**ID column on insert (Inferred, important).**
- **Waypoints, Routes and Settings:** inserts include the ID as the first field.
- **Contacts:** inserts omit the `ID` column. The first field is `Synced` (`SD:240-247`).
- **SyncData:** inserts appear to start at `Type`, which suggests the ID is omitted there too (`SD:288`, `SD:394`, `SD:466`, `CMD:10`).

This strongly suggests Contacts and SyncData have device-assigned auto-increment IDs. Contacts are later deleted by the ID read back from column 0 (`SD:319-327`).

**Field escaping** (`CSV:7-44`). The rules are RFC-4180-like.

- `EscapeField(x)` replaces `"` with `""`. It does **not** add quotes, and maps null to `""` (`CSV:15-22`).
- `CsvEncodeString(x)` escapes, then wraps the value in `"..."` **only if** it contains `,`, `"`, `\r` or `\n` (`CSV:24-44`). Null or whitespace-only values become the empty string, with no quotes (`CSV:26-30`, `CSV:35-38`).
- `CommaJoin` quotes every field (`CSV:9-13`). It is unused by the device paths.
- Some writers wrap values in `"..."` unconditionally, and some of those **do not escape** the contents:
  - setting values (`DS:99-103`)
  - preset recipients (`CMD:10`)
  - route and waypoint dates (none needed)

  These are listed per format below.
- Read-back parsing uses LumenWorks `CsvReader` with standard quote and `""` handling (`SD:182`, `DS:59`).

### 1.4 Size limits and batching (`BW:8-111`)

- **Max script length.** The limit is **2046 bytes** if the firmware version is ≥ `2.6.0.1` **or the device is a Mini**, and **1022 bytes** otherwise (`BW:12-14`, `BW:30-47`). An unparseable firmware version counts as `1.0` (`BW:37-41`). The two `[table][version]` header bytes come on top, giving 2048 or 1024 bytes of data (`BW:10`).
- **Batching.** `BatchWriteTable` peeks at each queued line, appends `\r\n`, and handles it as follows:
  - It throws if the line is empty or whitespace (`BW:79-82`).
  - It throws `"Line too long"` if a single line is longer than the max (`BW:83-86`).
  - It appends the line to the current batch while `lineBytes + batchBytes < Max`. The comparison is **strict** (`BW:87-95`).
  - Otherwise it flushes the batch with one `WriteTable` and starts a new one (`BW:96-109`).
  - The last line triggers a final flush (`BW:90-93`).
  - Any non-whitespace reply aborts with `"Error during batch write: <reply>"` (`BW:103-106`).
- **Edge-case bug (Observed).** A single line of exactly `Max` bytes fails the `<` test while the batch is still empty. The code then flushes an empty script and loops forever (`BW:87-109`).
- **Other limits:**
  - `inReachSE.MAX_SCRIPT_LENGTH = 2000` (`SE:45`).
  - Presets are capped so that `bytes(line) + 6 ≤ 2000` (`CMD:11`).
  - Each sub-command of a server device command must be ≤ 1999 bytes including the table and version bytes (`SD:520-525`).
- **Truncation bug in presets (Observed).** `Commands.RemoveLastContact` builds a new string and discards it, because `line` is passed by value (`CMD:18-23`). An oversized preset therefore makes `GetPresetWriteString` **loop forever** (`CMD:11-14`). A port should implement the intent instead: drop trailing recipients until the line fits.
- `SyncDevice.BatchTableWrite` requires `lastDeviceVersion`, which is set by a prior `GetIMEI`/`DELBIN_VERSION` (`SD:628-645`). It wraps any exception into a `DeviceError` event rather than throwing (`SD:641-644`). **Sync continues past write errors at the step level, but `ExecuteAllSteps` aborts the remaining steps once an error has been queued** (`SM:670-685`).

### 1.5 Line formats per data type

All formats below are Observed. Placeholders are in `<angle brackets>`.

#### Contacts (table 1)
The row is generated from the column names of the device's table version (`SD:238-269`):
1. Drop the `ID` column.
2. Emit `1` for `Synced`.
3. For each remaining column, match a `ContactModel` property whose name, with spaces removed, equals the column name: `FirstName`, `LastName` or `Organization` (`RC/ContactModel.cs:3-14`).
4. Otherwise use the first address whose `AddressType` string value equals the column name (`SD:258-262`). The mapping is `"Email"`→Email(0), `"Phone"`→SMS(1), `"inReach Address"`→inReach(2), `"IMEI"`→IMEI(4) (`RC/AddressType.cs:5-17`).
5. Otherwise emit an empty field. In practice TeamDisplayName, TeamEst and MembersID are always empty (`SD:263-266`).

Every value goes through `CsvEncodeString`.

```
i,1,<First>,<Last>,<Org>,<Email>,<Phone>,<inReachAddr>,<IMEI>,,,\r\n      (v5)
```
Sequence (`SD:271-302`):
1. `READ_TABLE` Contacts.
2. `d,<ID>` for every row whose column 1 (`Synced`) is `"1"`. This keeps device-created contacts.
3. Batch-insert all server contacts.
4. Write their addresses to SyncData (below).
5. Write the breadcrumb.

#### SyncData (table 9): `Type, ShortCode, Value` (ID omitted)
| Type | Meaning (`SE:5-11`) | Line | Source |
|---|---|---|---|
| 1 | Address | `i,1,<addressID>,"<EscapeField(address)>"` | `SD:284-290`. Skipped when `addressModel.ID == 2`. Compare `DEVICE_FREEFORM_ADDRESS_CODE = 2` (`DS:34`). |
| 2 | QuickText | `i,2,<quickTextID>,"<EscapeField(text)>"` | `SD:394` |
| 3 | Preset | `i,3,<index>,"<recipients>\|<EscapeField(message)>"` with index 14, 15, 16 (max 3 presets); recipients are the server's `Recipients[]` joined with `,` and **not escaped** | `SD:353-364`, `CMD:8-16` |
| 4 | Sync breadcrumb | `i,4,0,"sbc"` | `SD:460-474`, written after each of contacts, presets and quick texts |

Before contacts, presets or quick texts are written, a **`RemoveSyncData` step sends `d,*` to SyncData** (`SM:1334-1343`, `SD:205-209`, `SD:332-342`). **Inferred:** the meaning of the recipients list inside a preset (address IDs? raw addresses?) depends on server data and needs a capture.

#### Settings (table 7): `ID, Current Value, Default Value`
- Install: `i,<key>,"<value>"`, plus `,"<default>"` if a default is present. **The value is not escaped** (`DS:93-111`).
- Remove: `d,<key>` (`DS:105-107`).
- Raw single-field forms are used for internal keys: `i,73,<unixSecs>`, `i,89,<unixSecs>`, `i,97,<unixSecs>`, `i,16,<value>`, `i,1624,1`, `d,1192` (§2).

#### Waypoints (table 14, v3 only; `DW:149-173`)
```
i,<SyncId as u32>,0,0,<IconIndex>,<lat>,<lon>,<created>,<updated>,<CsvEncodeString(Name)>
```
- `SyncState` is set to 0 and `Hidden` to 0.
- lat and lon use .NET format `#.########` with the invariant culture (`DW:162-164`). **Quirk:** `0` formats as an empty string and `0.5` as `.5`.
- Dates use `yyyy-MM-ddTHH:mm:ssZ` in UTC, or an empty string if null (`SD:668-675`).
- Waypoints with `HiddenOnDevice` set are skipped (`DW:151`).
- Write sequence: `d,*`, then batch-insert everything (`DW:79-96`).

#### Routes (table 16, v1 only; `DR:104-125`)
```
i,<SyncId as u32>,<TrackId>,<created>,<updated>,<rgb565 color>,<rgb565 border>,0,1,<CsvEncodeString(Label)>\r\n
```
- `Deleted` is set to 0 and `VisibleOnMap` to 1.
- Color is 24-bit hex (`#RRGGBB` or `RRGGBB`) converted to RGB565 as a **decimal** u16: `(R/8)<<11 | (G/4)<<5 | (B/8)` (`DR:127-150`, `DR:213-226`).
- Read-back converts RGB565 to `#RRGGBB` with rounding (`DR:191-221`).
- Write sequence (`DR:78-102`, `DR:240-250`):
  1. If the device currently holds routes, send `d,*` to Routes, then `d,<oldTrackId>` to TrackBlocks for each old route.
  2. For each new route that is not deleted, write its track blocks immediately, **before** the route rows.
  3. Batch-insert the route rows at the end.

#### TrackBlocks (table 15, v5 only)
- Delete: `d,<trackId>`, sent only if the device has any track blocks (`DT:104-118`).
- Write: the server's `Route.TrackBlocks` string is split on `\r\n` and each line is queued **verbatim** (`DT:120-132`, `RC/Route.cs:19`). Sync never builds track-block lines itself. **Inferred:** the server already sends complete `i,...` lines, including the binary-ish `DataBlock` column. Their encoding is unknown.

#### Forms and FormFields (tables 12 and 13; only in the unused `DeviceConfig`)
- Each `<entry>` from an XML file becomes one `WriteTable` call with `i,<entry>`. Dictionaries produce `i,<key>,<value>`, or `d,<key>` when the value is null. Lists may be preceded by `d,*` (`DC:230-280`).
- Form line: `<id>,"<title>","<dest>","<prefix>","<delim>"` (`DC:293`).
- Field line: `<(formId<<16)|fieldId>,<formId>,<type 0=Text 1=Number 2=PickList>,<required 0/1>,<min>,<max>,"<name>","<default>","<picklist a,b,c>"` (`DC:310-321`, `SEF:7-12`, `SEF:37-40`).
- **Discrepancy (Observed):** the declared column order is `...,Type,Min,Max,Required,...` (`SE:166`), but `DeviceConfig` writes `Required` before `Min` (`DC:315-317`).
- `DeviceConfig` is never instantiated anywhere in the app. A grep finds only its own file.

### 1.6 Server "device commands" (raw `WRITE_TABLE`)
`ExecuteDeviceCommands` (`SD:476-545`):
1. Takes `DeviceCommandsModel.DeviceData` (`byte[]`, `RC/DeviceCommandsModel.cs`) and decodes it as UTF-8.
2. Splits it on `\r\r` outside double quotes (`SD:485-516`).
3. Sends each piece, re-terminated with `\r`, as the **entire** `WRITE_TABLE` payload (`SD:519-526`).

The piece itself must therefore start with the raw `[tableID][version]` bytes. **Inferred:** decoding these bytes as UTF-8 and re-encoding them is lossless only while both bytes are < `0x80`.

Any non-empty reply marks the command `Nacked`, records the error text, and stops processing that command (`SD:527-534`). Success marks it `Acked`, and the result is posted back to the server (`SM:1320-1328`).

`GetDeviceCommands` is called with the current tracking interval, read from setting 24 (`SM:1312-1317`, `SD:176-203`).

---

## 2. Settings (table 7)

### 2.1 Read path
`DeviceSettings` resolves the Settings table version when it is constructed (`DS:44-49`). `RefreshSettings` reads the whole table and parses rows as `ID, Current, Default` into a dictionary (`DS:56-77`). Missing fields become null or `""` (`DS:60`, `DS:67-68`). `GetSetting` refreshes lazily (`DS:79-91`). Values are always strings.

### 2.2 Write path
`SyncSettings(settingsModel)` (`DS:113-154`) handles a server `Dictionary<string,string>`:
1. If key `"16"` (device type) is present and differs from the device value, it sends `i,16,<value>` and then **`DELBIN_APPLY_DEFAULT_SETTINGS` (0xFC06)** (`DS:119-122`, `DS:156-161`, `DS:185-192`). **Inferred:** changing the device type resets the device's settings to the defaults for that type.
2. For every other integer key (key 16 is skipped), it emits `i,<key>,"<value>"[,"<default>"]`. The default is the value previously read from the device and is kept.
   - **Key 760:** the default is forced equal to the new value (`DS:141-144`).
   - **Keys 35 and 43:** an empty or whitespace value becomes `d,<key>` (`DS:145-148`).
3. All lines go out in one `BatchTableWrite` (`DS:153`).

### 2.3 Known setting IDs

| ID | Meaning (from code context) | Access | Cite |
|---|---|---|---|
| 16 | "device type" (`DEVICE_TYPE_FIELD`) | write `i,16,<v>` + APPLY_DEFAULT_SETTINGS | `DS:28-30`, `DS:156-161` |
| 24 | tracking interval (integer; passed to the server) | read | `SD:187` |
| 35, 43 | unknown; deleted when the server sends an empty value | write / delete | `DS:145-148` |
| 73 | last-sync timestamp (Unix seconds, UTC) | write `i,73,<secs>`, last step of the sync | `SD:412-426`, `SM:1344-1351` |
| 89 | `eSyncedQuickTextsTimestamp` (Unix secs of the server's last-modified time) | write | `SD:428-442` |
| 97 | `eSyncedPresetsTimestamp` | write | `SD:444-458` |
| 424 | firmware revision used by the "AmIAlive" logic | read; write = new revision **before** flashing, old revision restored on failure | `SM:964-988`, `SM:1373-1388` |
| 760 | product variant on HW25 hardware: `"1"`=SE, `"2"`=Explorer | read (product detection), write (value = default) | `SM:1030-1051`, `DS:141-144` |
| 1192 | activation pending (`"1"`) | read; cleared with `d,1192` after server `FactoryReset` | `DS:32`, `DS:163-177`, `SM:1194-1208` |
| 1624 | set to 1 after hunt map packages were deleted and the user has no hunt subscription | write `i,1624,1` (Explorer3 maps only) | `DMAP:190-196` |

Per-product: all products share the same table and ID scheme. The only product-specific reads are 760 (HW25 SE vs Explorer) and 1624 (Explorer3). Every other key comes opaquely from the server's `GetSettings` (`SM:1390-1409`).

---

## 3. Reading user data off the device

Observed: Sync **does** read user data, but only via `READ_TABLE`. `READ_FLASH` (0xFC05) and the bootloader `READFLASH` are never called. A grep finds callers only inside `DLB`.

| Table | Read when | What is done with it | Cite |
|---|---|---|---|
| Settings (7) | product detection, activation check, tracking interval, firmware rev | parsed locally | `DS:56-77`, `SD:176-203` |
| Contacts (1) | contacts sync | only to find rows with `Synced=="1"` to delete | `SD:312-330` |
| Waypoints (14, v3) | while planning the sync | each row is mapped to a `Waypoint` and **uploaded** with `UpdateWaypoints`; the server returns the merged list, which is written back | `DW:98-133`, `SM:1555-1560` |
| TrackBlocks (15, v5) | while planning the sync | the raw text is split on `\n` and trimmed. Rows starting with **`0,0`** (TrackID 0, Level 0 = "high-detail tracking") whose `Time` (column 9, parsed by `DateTimeOffset.Parse`) is ≥ the server's newest track point are **uploaded verbatim** with `InsertTracks` | `DT:85-97`, `DT:134-147`, `SM:1411-1458` |
| Routes (16, v1) | while planning the sync | mapped to `Route` (column 6 = Deleted, column 8 = Name; colours from RGB565) and uploaded with `UpdateRoutes`; the merged result is written back | `DR:157-189`, `SM:1523-1528` |
| EventLog (10) | after every sync (non-OneX) | the raw CSV text is posted to `/V1/App/EventLog` | `SD:760-764`, `SM:726-729`, `SM:1124-1134`, `RC/SyncRestClient.cs:206-209` |

Read formats:

- **Waypoint rows:** `ID(u32), SyncState(enum: 0=Synchronized, 1=SymbolNotSynced, 2=LocationNotSynced, 3=LabelNotSynced, 7=Unsynchronized), Hidden(0/1), Symbol(int), Lat(decimal), Lon(decimal), Created, Updated, Name`. Dates use `yyyy-MM-ddTHH:mm:ssZ` or are empty (`DW:116-142`, `RC/Waypoint.cs:5-12`, `SD:686-693`).
- **"Needs sync" checks:**
  - Waypoints: the table is empty, or any row is hidden or not Synchronized (`DW:66-77`, `DW:175-182`).
  - Routes: the table is empty, or any row is deleted (`DR:257-273`).
  - Tracks: there are new high-detail blocks (`DT:76-83`).

**Not read:** Messages (3), MessageThreads (4), Conversations (5), TrackPoints (2) and Locations (18) are defined but never touched. **Inferred:** they may be readable with the same `READ_TABLE` call. The Messages column list (`SE:91-104`) is a good starting point for a message export feature.

---

## 4. Firmware update (bootloader protocol)

### 4.1 Bootloader packet format (Observed)

**Host → device** (`DLB:762-779`), carried inside the normal `[0x20][len]` HID wrapper (`DLB:983-1007`):

| Byte | Value |
|---|---|
| 0 | `0x9C` |
| 1 | `cmd` |
| 2 | `0xFD` |
| 3 | `0x61 ^ cmd` (makes bytes 0..3 XOR to 0) |
| 4..n-1 | payload (optional) |
| n | XOR of the payload bytes (present only if there is a payload) |

The whole packet therefore XORs to `0x00`.

**Device → host** (`DLB:792-824`). Exactly one HID report is read and parsed as a response.

| Variant | Bytes | Command |
|---|---|---|
| "FD" form | `8C FD <cmd> <data...> <xor>` | byte 2 |
| "AC" form | `8C <cmd> AC <data...> <xor>` | byte 1 |

- The response must be at least 4 bytes, start with `0x8C`, and XOR to 0 over all its bytes (`DLB:795-802`, `DLB:836-839`).
- Data is bytes `[3 .. len-2]`, i.e. everything after the 3-byte header except the final XOR byte (`DLB:818-822`).
- `ReadBootLoaderResponse(cmd)` returns null if the next response has a different command (`DLB:826-834`). There is no skipping, so **any unexpected packet aborts the operation**.
- `SendBootLoaderCommand(cmd, waitForComplete)` sends the command, expects an echo of `cmd`, and when `waitForComplete` is set also expects a following `TASK_COMPLETE (0xC0)` (`DLB:781-790`).

### 4.2 Bootloader commands (`BLC:3-23`, plus the unnamed fast-write code)

| Code | Name | Payload | Expected response sequence | Used |
|---|---|---|---|---|
| `0xCA` | PING | none | the device **sends PINGs unsolicited** while in the bootloader; the host echoes PING to stop them | `AckPing` (`DLB:561-584`) |
| `0x51` | IMEI | none | `0x51` + 16 B ASCII NUL-padded, then `0xC0` | `DLB:515-522` |
| `0x55` | PRODUCT_NAME | none | `0x55` + 24 B ASCII, then `0xC0` | `DLB:524-531` |
| `0x5A` | FIRMWARE_VERSION | none | `0x5A` + 24 B ASCII, then `0xC0` | `DLB:533-540` |
| `0x5B` | BOOTLOADER_VERSION | none | `0x5B` + ASCII (any length), then `0xC0` | `DLB:542-549` (no callers) |
| `0xBC` | BLOCK_COUNT | none | `0xBC` + 8 B `[totalBlocks u32][unprotectedBlocks u32]`, then `0xC0`. **If present, enables "fast write"** | `DLB:586-603`, `DLB:639` |
| `0xED` | EDEFFLASH | none | `0xED`, then N × `0xEB` (BLOCK_ERASED), then `0xC0`. Erases the application area | `DLB:605-634` |
| `0xEA` | EALLFLASH | none | same as `0xED`, but **erases everything including the bootloader** | `DLB:607` |
| `0xFF` | PFLASH | none | `0xFF`, then 0..N × `0xEB`, then `0xFF` ("ready"). Starts programming of the application | `DLB:638`, `DLB:643-650`, `DLB:743-760` |
| `0xFB` | BFLASH | none | as PFLASH. Starts programming **of the bootloader-containing image** | `DLB:638` |
| `0x12` | *(unnamed; fast write)* | `[addr u32][len u16][data len≤512]` | `0xFF` or `0xFB` (echo of the active flash command) | `DLB:659-679`, `DLB:703` |
| `0x11` | WRITE (slow write) | none, then a raw 268-byte block (below) | `0xFF`/`0xFB` after the command, `0xFF`/`0xFB` after the block | `DLB:680-702` |
| `0xEF` | EOF | none | **nothing awaited** | `DLB:738` |
| `0x52` | RESTART | none | nothing awaited | `DLB:551-554` |
| `0x53` | SHUTDOWN | none | `0x53`, then `0xC0` | `DLB:556-559` (no callers) |
| `0x75` | READFLASH | `[key u32][addr u32][size u16 ≤256]` | `0x75`, then **raw** 268 B `[addr u32][addr ^ xor(data256) u32][data 256][size u32]`, then `0xC0` | `DLB:286-343` (no callers; `key` unknown) |
| `0xE0` | TASK_ERROR | – | never explicitly handled; treated as "unexpected", which means failure | `BLC:18` |
| `0xC0` | TASK_COMPLETE | – | terminator | – |
| `0xEB` | BLOCK_ERASED | – | progress tick during erase | – |

**Fast-write packet** (`DLB:663-677`). Total length is `len + 11` bytes. A 512-byte chunk is 523 bytes, which spans 9 HID reports.
```
9C 12 FD 73 | addr:u32 LE | len:u16 LE | data[len] | xor(bytes[4..end-1])
```

**Slow-write raw block** (`DLB:683-700`). It is sent **without** a bootloader header, right after `0x11` has been acked.

| Off | Len | Field |
|---|---|---|
| 0 | 4 | address u32 LE |
| 4 | 4 | `address ^ XOR(bytes 8..267)` as u32 LE (only the low byte changes; the length field is still zero when this is computed) |
| 8 | 256 | data, zero-padded |
| 264 | 4 | actual data length u32 LE |

### 4.3 Firmware file format (`DLB:1065-1115`)

The file is a **text** file. Each line has the form `<hexAddress> <hexByte> <hexByte> ...`, separated by spaces (`DLB:1074-1077`, `DLB:1107-1110`).

- A line whose address continues the current block is appended to it.
- A forward gap of < 8 bytes is padded with `0xFF` (`DLB:1078-1088`).
- Any other address starts a new `DataBlock{Address, Data}` (`DLB:1089-1102`).

`FirmwareContainsBootLoader` is true if the **first block's address is `0x80000000` or `0x30000000`** (`DLB:1056-1063`). This single flag selects `EALLFLASH` and `BFLASH` instead of `EDEFFLASH` and `PFLASH`.

**Download** (`SD:547-626`):
1. Download the file from the server URL.
2. Compute its MD5 and compare it, as lowercase hex, with `FirmewareResponseModel.CheckSum` (`RC/FirmewareResponseModel.cs`).

**If computing the MD5 throws `TargetInvocationException` (FIPS mode), the check is skipped and flashing proceeds** (`SD:615-619`).

### 4.4 Full flow

The sequence starts at `SM.addFirmwareInstallStep` (`SM:946-1012`), continues through `SD.InstallFirmware` (`SD:547-602`) and `DM.WriteFirmware` (`DM:378-408`), and ends in `DLB.WriteFirmware` (`DLB:1117-1181`).

1. **Pre-flash setting.** If the device is healthy and not a OneX, read setting 424 and write the new firmware revision into it (`SM:964-968`). On failure the old value is restored (`SM:983-988`).
2. **Decide whether to enter the bootloader.** `GetAppMode` is called first (`SM:969`). `inBootLoaderAlready = true` (skip step 3) when:
   - there is no version info, or
   - the device is possibly bricked and not in BlueLoader mode, or
   - the device is a OneX (`SM:970-981`).
3. **Enter the bootloader** (`DLB:381-426`):
   - If `AckPing()` succeeds or `GetDeviceProductName()` returns non-null, the device is already in the bootloader.
   - Otherwise send **`DELBIN_RESET_DEVICE_FOR_FLASH` (0xFC04)**.
   - On Windows, wait ≤ 15 s for a `DeviceAdded` event. On timeout, reopen the device and probe (`DLB:476-503`).
   - Then sleep 3 s, or 15 s on Windows 7 with an IR3 (`DLB:505-513`).
   - Reopen the device, then `AckPing()` or `GetDeviceProductName()` to confirm.
   - On macOS the code reinits USB up to 10 times, with 4 s waits (`DLB:428-449`).
   - Failure throws `"Device failed to enter in bootloader mode..."` (`DLB:1134-1137`).
4. **`GetBlockCount`** (`0xBC`) (`DLB:1138`). Its presence decides fast versus slow write and the progress step count (`DLB:1141-1146`).
5. **Parse the file** into blocks and compute `FirmwareContainsBootLoader` (`DLB:1139-1140`).
6. **Erase:** `0xEA` if the image contains the bootloader, otherwise `0xED`. The host consumes `0xEB` ticks until `0xC0`, with **1800 ms per response** (`DLB:1147-1153`, `DLB:605-634`). Failure throws `"Error erasing memory from device"`.
7. **Program** (`DLB:636-741`):
   1. Send `0xFB` (bootloader image) or `0xFF` (application), consume the echo, any `0xEB` ticks, and the second echo.
   2. For each block, for each chunk:
      - **fast:** send a 512-byte `0x12` packet and wait for the echo of `0xFF`/`0xFB`.
      - **slow:** send `0x11`, wait for the echo, send the raw 268-byte block, wait for the echo.
   3. **Retry:** if an echo is missing, disable input, sleep 2 s, resend `0xFF`/`0xFB` and consume the echoes, then rewind one chunk. Only **one retry per chunk**; a second failure aborts (`DLB:703-726`).
   4. Send **`0xEF` EOF without waiting for a reply** (`DLB:738`).
8. **Restart.** If writing returned true and the device is still open, send **`0x52` RESTART** (`DLB:1173-1180`).
9. **Wait for the reboot.** If the firmware's `VersionMajor == 1`, return immediately (`DM:395-399`). Otherwise `waitForFinishReboot()`: wait ≤ 7 s (30 s on macOS) for a `DeviceAdded` event, then sleep **5 s**, then reopen (`DM:400-433`).
10. Back in `SyncManager`, success posts `MarkFirmwareUpdated` (`SM:990-1007`). After all steps finish, a sync that contained a firmware step **re-schedules a full check** instead of completing (`SM:712-717`). While a firmware update is pending, data sync is **not** planned in the same pass (`SM:572-576`).

**There is no host-side read-back verification step.** `READFLASH` exists but is never used. **Inferred:** any verification happens on the device, at EOF or at boot.

### 4.5 Risk points (could leave the device unbootable)

| # | Step | Failure consequence | Cite |
|---|---|---|---|
| R1 | `RESET_DEVICE_FOR_FLASH` sent, but the host never re-finds the device | The device sits in the bootloader with the application intact. Recoverable with `0x52` RESTART, or by power-cycling (**Inferred**) | `DLB:403-421` |
| R2 | **Erase starts (`0xED`)** | From this point **the application is gone**. Any later failure leaves a bootloader-only device. Sync's recovery path is "PossibleBrickedDevice" (§4.6) | `DLB:605-634`, `DLB:1147-1153` |
| R3 | **`0xEA` + `0xFB`** for images whose first block is at `0x80000000` or `0x30000000` | **Erases and rewrites the bootloader itself.** An interruption here is a likely **hard brick**, with no USB recovery path in the code | `DLB:1056-1063`, `DLB:607`, `DLB:638` |
| R4 | Programming chunk ack timeout (1800 ms), with a single retry | Abort leaves a partial image, so the device is bricked to the bootloader | `DLB:703-726` |
| R5 | Exception mid-write (for example a HID write timeout raises `IOException`, `WDEV:255-263`) | It propagates out. The `finally` block restarts the device **only if** `flag` is true, so the device is left in the bootloader with a partial image | `DLB:1169-1180` |
| R6 | `0xEF` EOF is not acknowledged; `RESTART` is sent blind | A device-side EOF failure goes unnoticed. The host thinks it succeeded | `DLB:738`, `DLB:1175-1179` |
| R7 | MD5 check skipped under FIPS | A corrupt download could be flashed | `SD:615-619` |
| R8 | Wrong image for the hardware | The host does no compatibility check beyond what the server picked from IMEI, version and hardware minor (`SM:834-835`, `SM:861-895`) | – |
| R9 | `TASK_ERROR (0xE0)` from the device | Not decoded; it just counts as failure at whatever step it arrives | `BLC:18`, `DLB:626-629` |
| R10 | Pre-flash write of setting 424 | Harmless by itself, but it changes a setting before the flash succeeds | `SM:964-968` |

### 4.6 Recovery ("possibly bricked")

- **Detection.** The flag is set:
  - when `DELBIN_VERSION` returns null, which triggers `EnterBootLoader` and reading the IMEI, firmware version and product name from the bootloader with `possiblyCorruptFirmware = true` (`DM:239-260`, `DM:310-321`);
  - when the app mode is `BlueLoader` (`DM:234-237`, `DM:274-283`);
  - when `GetIMEI` returns nothing (`SM:588-591`).
- **User flow.** The UI asks the user, then calls `GetAndInstallFirmwareForBrickedDevice` (`APP:564-582`). That asks the server for the firmware matching the bootloader-reported version, or the "latest" when there is no info (`SM:897-930`), then runs the normal flash with `inBootLoaderAlready:true` (`SM:970-972`).
- **Older generations.** Devices with `bcdDevice == 1` or `2` are handled through the bootloader commands only (`DM:226-233`, `DM:285-308`).

### 4.7 Which products use this path

**Observed:** the same `InstallFirmware` path is used for **every** product type. The firmware step is product-agnostic, and only `inBootLoaderAlready` differs (`SM:963-981`).
- OneX is always flashed, even with the same version (`SM:841`), and always with `inBootLoaderAlready:true` (`SM:970`).
- The Mini's VID/PID is in the device filter (`CD:11`). `BatchWriter` has a Mini special case (`BW:42`).

**Inferred, unverified:** whether the server ever returns a DeLorme text image for the Mini. The Mini in mass-storage mode exposes Garmin `GUPDATE.GCD`, a Garmin format that this code never produces or parses. Mini firmware may therefore be updated by Garmin Express/GCD rather than by this bootloader. **Do not flash a Mini through this path without independent confirmation.**

---

## 5. Mode handling

### 5.1 App modes (`AM:3-11`)
| Value | Mode |
|---|---|
| 0 | App |
| 1 | Charger |
| 2 | MassStorage |
| 3 | Test |
| 4 | BlueLoader (**Inferred:** a secondary or "safe" loader with a working DelBin `VERSION`; treated as possibly corrupt firmware, `DM:274-283`) |
| 0xFF | Unknown, meaning no reply or a reply that is not exactly 1 byte (`DLB:101-105`) |

### 5.2 Operations (Observed)

- **`GetDeviceVersionInfo`** (`DM:207-245`). If the device is not open, sleep 2 s and open it.
  - `bcdDevice == 1`: OneX first generation. The product is set to `"D1"` and the host does `AckPing` ×3.
  - `bcdDevice == 2`: read IMEI, firmware version and name from the bootloader.
  - App mode BlueLoader: handled as `HandleBlueLoader`.
  - Otherwise: if `VersionMajor >= 3`, call **`ChangeToAppMode`**, then send `DELBIN_VERSION`.
- **`ChangeToAppMode`** (`DM:509-525`). Send `READ_APP_MODE`. If the mode is App or Unknown, do nothing. Otherwise call `Restart()`.
  - It is called at the start of every sync check (`SM:554`).
  - It is also called on attach of an "off" inReach 3, whose IMEI reads as `000000000000000` (`SM:387-390`).
  - **Inferred:** a device plugged in while switched off enumerates in Charger mode and is rebooted into App mode.
- **`Restart` / `RestartAppMode`** (`DM:449-463`, `DLB:363-370`). If the device is flagged as mass storage, re-enumerate first. Then open, send **`DELBIN_RESET_DEVICE_REQ` (0xAAFE)**, and `waitForFinishReboot`. This is also done **after every successful sync** for non-OneX devices (`SM:687-702`), so the device reloads its tables (**Inferred**).
- **`waitForFinishReboot`** (`DM:410-433`). Reset an event, wait ≤ 7 s (30 s on macOS) for any `DeviceAdded`, then on macOS `ReinitUsbAndWait`, then **sleep 5 s**. The timeout is not treated as an error.
- **`ReinitUsbAndWait`** (`DLB:451-474`). Close the HID handle and the USB stack, sleep 1 s (Windows) or 4 s (others), init again, then poll for connected devices every 1 s for up to 120 s. On Windows `InitUsb` and `CloseUsb` are no-ops (`WDEV:388-394`).
- **`EnableMassStorageMode`** (`DM:465-480`). Suppress events, set `MassStorageMode = true`, mark the device disconnected, send **0xFC07**, then `waitForFinishReboot`.
- **`EjectDrive`** (`DM:482-494`). This is pure OS-level eject. On Windows it uses `FSCTL_LOCK_VOLUME`, `FSCTL_DISMOUNT_VOLUME`, `IOCTL_STORAGE_MEDIA_REMOVAL` and `IOCTL_STORAGE_EJECT_MEDIA` (`NAT:24-30`). It then calls `waitForFinishReboot`. **No DelBin command is sent.** `DELBIN_RESET_ON_EJECT` is never used. **Inferred:** the firmware reboots itself to App mode when the host ejects the medium. On Linux the equivalent is unmount + `eject`/SCSI `START STOP UNIT`.

### 5.3 Maps over mass storage (`DMAP`; **Explorer3 only**, `SM:1479-1484`, `DMAP:289-294`)

This is a high-level summary only.

1. Snapshot the drive list, send **0xFC07**, and wait for the reboot. The new drive letter is found by diffing drive lists (`DMAP:137-148`). If no new drive appears, the host restarts the device and skips maps (`DMAP:150-155`).
2. Create `Garmin/maps/preload` and `Garmin/maps/download` if they are missing (`DMAP:95-97`, `DMAP:782-802`).
3. Unpack the embedded `BaseFiles.zip` (IR3) and copy any base files that are missing. Create a `.Trashes` file and delete `.Spotlight-V100` and `.fseventsd` (`DMAP:804-918`).
4. Download packages, stage each one in `Garmin/maps/<download|preload>/<type>/<package_id>/_<version>`, then rename the directory to `.../<version>` and delete other version directories (`DMAP:618-717`, `RC/MapPackage.cs:19-25`).
   - Unknown download packages are deleted (`DMAP:724-780`).
   - Deleting `ir_plss_vectors`, `ir_hunting_vectors` or `ir_parcel_vectors` sets `_huntMapsDeleted` (`DMAP:45`, `DMAP:756-760`).
5. If anything changed, delete `Garmin/maps/scanMarker` (`DMAP:251-275`). **Inferred:** this forces the device to rescan the maps.
6. Sleep 2 s, eject the drive (OS-level), and wait for the reboot (`DMAP:186-189`).
7. If hunt maps were deleted and the user has no hunt subscription, write Settings `i,1624,1` (`DMAP:190-196`).

---

## 6. Per-product differences

### 6.1 Product detection (`SM:1014-1063`)
| Product name test (on `DELBIN_VERSION` / bootloader name) | ProductType |
|---|---|
| `== "D1"`, `== "inReach Bluetooth"`, or null | **OneX** |
| contains `"inReach SE"` or `"inReach HW20"` | **SE** |
| contains `"inReach HW25"` | setting **760**: `"1"` → SE, `"2"` → Explorer, setting missing → Explorer. The setting is not read when the device is possibly corrupt |
| upper-case contains `"INREACH 3A"` | `"EX"` in the name → **Explorer3**, otherwise **SE3** |
| upper-case contains `"MINI"` | **Mini** |

Helper predicates in `DeviceVersion`:
- `IsMini()`: lower-case name **starts with** `"inreach mini"` (`DV:61-68`).
- `IsIR3()`: upper-case name contains `"INREACH 3"` (`DV:52-59`).
- `IsOffInReach3()`: `IsIR3()` and serial `== "000000000000000"` (`DV:43-50`).
- `BadImei()`: the serial does not parse as a long, or equals 0. `NoImei()`: the serial is empty, or `BadImei()` (`DV:24-41`).

The hardware minor is taken from `"Rev <n>"` in the product name and sent to the server for firmware selection (`SM:861-895`).

### 6.2 Behaviour matrix (Observed)
| Behaviour | OneX | SE / Explorer | SE3 / Explorer3 | Mini | Cite |
|---|---|---|---|---|---|
| Enumeration | VID 1163/PID 0300, bcdDevice 1 or 2 | VID 1163/PID 0300, bcdDevice major ≥ 3 (**Inferred**) | VID 1163/PID 0300 | VID 091E/PID 0B89 | `CD:11`, `DM:226-238` |
| IMEI source | user prompt (`NeedIMEI`) or bootloader | `DELBIN_VERSION` | `DELBIN_VERSION`; "off" IR3 is rebooted to App mode | `DELBIN_VERSION` | `SM:493-514`, `SM:387-390` |
| Table sync (contacts, settings, ...) | **no** | yes | yes | yes | `SM:573` |
| Script max length | 1022 / 2046 by firmware version | same | same | **always 2046** | `BW:42` |
| Firmware install | always, `inBootLoaderAlready` | via `0xFC04` | via `0xFC04` (Win7 waits 15 s) | via `0xFC04` (see §4.7 caveat) | `SM:970-981`, `DLB:505-513` |
| Firmware same-version reinstall | yes | only with `ForceFirmwareUpdate` | same | same | `SM:839-850` |
| Post-sync `RESET_DEVICE_REQ` | no | yes | yes | yes | `SM:690-696` |
| Event log upload | no | yes | yes | yes | `SM:726-729` |
| Maps via mass storage | no | no | **Explorer3 only** | no | `SM:1481` |
| HW25 SE/Explorer via setting 760 | – | yes | – | – | `SM:1030-1051` |

### 6.3 Sync step order (planning and execution)
**Device reads during planning.** `CheckAllSyncDataImpl` → `ChangeToAppMode` → `GetIMEI` (`SM:554-556`). Then `getSyncData` (`SM:1176-1371`) reads:
- Settings, for the activation check (`SM:1194`)
- Waypoints (`SM:1290`, `DW:52-58`)
- TrackBlocks (`SM:1294`, `DT:55-64`)
- Routes (`SM:1299`, `DR:59-67`)
- Settings, for tracking interval 24 (`SM:1315`)

**Steps executed in order** (`SM:637-686`):
1. [Activation pending: server `FactoryReset` + `d,1192`]
2. **RemoveSyncData** (`d,*` on table 9), inserted after step 1
3. FirmwareServerUpdate (REST only)
4. Contacts (+ SyncData addresses + breadcrumb)
5. Presets (+ breadcrumb + setting 97)
6. QuickTexts (+ breadcrumb + setting 89)
7. Settings
8. Waypoints
9. Tracks (upload only)
10. Routes (+ TrackBlocks)
11. DeviceCommands
12. **TimeStamp** (`i,73,<now>`)
13. Maps (Explorer3)

After the steps (`SM:687-729`):
- **`RESET_DEVICE_REQ` + wait**
- **EventLog read and upload**

Steps marked `RequiresAssignedDevice` are skipped for unassigned devices (`SM:651-655`).

---

## Open questions / unknowns

1. **Insert semantics for auto-ID tables.** Contacts and SyncData are written without an ID column (§1.3). This is not proven against the firmware. Also unconfirmed: whether `i,<id>,...` on an existing ID updates or duplicates the row, and what `d,<id>` matches on. The format of the device's **error strings** in `WRITE_TABLE` replies is unknown. A safe probe is `TABLE_VERSION`, then `READ_TABLE` of Settings and Waypoints on the real Mini, with no writes.
2. **Mini firmware path.** It is unknown whether the Mini uses the DeLorme bootloader (`0xFC04` / `0x9C…` packets) at all, or only Garmin `GUPDATE.GCD`. Also unknown: whether the Mini in bootloader mode keeps VID 091E/PID 0B89, and the Mini's `bcdDevice` (`lsusb -v` can show it without opening hidraw). What the server returns from `GetFirmwareByIMEI` for a Mini is also unknown. **Highest-risk unknown; do not test by flashing.**
3. **Fast-write opcode `0x12`** is not in the enum. Its exact device semantics (alignment, maximum length, whether `addr` must be erase-block aligned) are known only from this one call site.
4. The meaning of the **READFLASH `key`** parameter, and whether `DELBIN_READ_FLASH` (0xFC05) works or is locked in production firmware.
5. **`BlueLoader` (mode 4)**: what it is, and why it skips `EnterBootLoader` when flashing.
6. **TrackBlocks encoding.** The `DataBlock` column and the `*Bytes`/`*Scale` columns, and the exact text format of route track-block lines from the server. The server returns them ready to insert, so they need a capture or a decoder to round-trip.
7. **DeviceCommands payload.** The leading `[table][version]` bytes are inferred. A real `DeviceData` sample is needed. Re-encoding through UTF-8 would corrupt any byte ≥ `0x80`.
8. **Preset recipients format**, i.e. what `PresetMessageModel.Recipients[]` contains (address IDs vs strings). Also, the contacts code skips SyncData addresses with `ID == 2` rather than `Type == 2`, which looks like a bug.
9. **Setting IDs** other than those listed in §2.3. The server supplies them opaquely. The meanings of 35 and 43 are unknown.
10. **Unused codes:** `DELBIN_FVT` (0xFC00), `NAV_DATA_OUT`, `SAT_INFO_OUT`, `MAPPKG_COUNT` (0xC020) and `RESET_ON_EJECT` (0xFC09) have no payload definitions in this binary.
11. **Readability of the Messages, MessageThreads, Conversations, TrackPoints and Locations tables.** They are defined but never read, and their version on the Mini is unknown.
12. **Eject semantics.** Does the Mini reboot to App mode on a SCSI eject alone, as `EjectDrive` assumes, or does it need `RESET_ON_EJECT` or a replug?
