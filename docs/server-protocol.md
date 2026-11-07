# inReach Sync — server protocol specification

Source: Garmin "inReach Sync" 1.4.47.4776 (.NET, WPF), decompiled with ilspycmd.
Scope: everything the desktop client does over the network, plus the overall sync
sequence that ties network calls to device operations. USB/HID framing and table
formats are documented elsewhere and only referenced here.

Path abbreviations used in citations (all under
`/home/henrik/Downloads/inReachSync-extracted/4-decompiled/`):

| Abbrev | Path |
|---|---|
| `RHC` | `SyncRestClient/SyncRestClient/RestHttpClient.cs` |
| `SRC` | `SyncRestClient/SyncRestClient/SyncRestClient.cs` |
| `M/…` | `SyncRestClient/SyncRestClient.Models/…` |
| `R/…` | `SyncRestClient/SyncRestClient/…` (other files in that folder) |
| `SM` | `inReach.Sync/inReach.Sync/SyncManager.cs` |
| `SD` | `inReach.Sync/inReach.Sync/SyncDevice.cs` |
| `SW` | `inReach.Sync/inReach.Sync/SyncWeb.cs` |
| `DM` | `inReach.Sync/inReach.Sync/DeviceMaps.cs` |
| `DS` | `inReach.Sync/inReach.Sync/DeviceSettings.cs` |
| `DW` / `DT` / `DR` | `inReach.Sync/inReach.Sync/DeviceWaypoints.cs` / `DeviceTracks.cs` / `DeviceRoutes.cs` |
| `APP` | `inReach.Sync/inReach.Sync.UI/Application.cs` |
| `CRED` | `inReach.Sync/inReach.Sync.Core/Credentials.cs` |
| `UES` | `inReach.Sync/inReach.Sync.Utils/UserEnvironmentSplitter.cs` |
| `MW` | `SyncDesk/SyncDesk.Wpf/MainWindow.xaml.cs` |

Notation: **[obs]** = read directly in code; **[inf]** = my inference, not settled by code.

---

## 1. Transport basics (applies to every API call)

All JSON API calls go through `RestHttpClient` (base of `SyncRestClient`).

| Aspect | Behaviour | Cite |
|---|---|---|
| TLS | `SecurityProtocol |= Tls12` | RHC:43-46 |
| URL building | `new Uri(_serviceURL + operation)` — plain string concatenation of base URL and path. A trailing `/` on the base would produce `//`. | RHC:101, 157 |
| Query string rewrite (GET only) | If the operation contains `?`, the query is split on `&` and `=`, each value passed through `Uri.EscapeUriString`, values containing `,` are split into repeated keys, and the string is rebuilt as `?k=v&k2=v2&` — **always with a trailing `&`**. Keys with empty value stay as `k=&`. | RHC:92-137, 82-90 |
| POST URL | Query string is used as-is (no rewrite). | RHC:150-159 |
| HTTP client | New `HttpClient` per request; `AllowAutoRedirect=true`, `PreAuthenticate=true`. | RHC:256-269 |
| `Accept` | `application/json` | RHC:264-265 |
| `Authorization` | `Basic base64(UTF8(username + ":" + password))`, added pre-emptively **only if username is non-empty**. No token, no cookie, no challenge round-trip. | RHC:283-290 |
| `X-CultureCode` | UI culture name, e.g. `en-US` (from `-c <culture>` CLI arg or OS current culture). Sent whenever non-empty. | RHC:33, 271-281; `inReach.Sync.Resources/inReach.Sync.Resources/Localizer.cs:19-29`; `SyncDesk/SyncDesk.Wpf/App.xaml.cs:138-178` |
| `User-Agent` | **Not set** anywhere in the client (no code sets it; .NET Framework `HttpClient`/`WebClient` send none by default). | RHC:256-290 (absence) [obs], default-UA claim [inf] |
| App-version header | None. App version is only sent in the body of `/V1/App/Report`. | RHC (absence) |
| Proxy | `defaultProxy useDefaultCredentials="true"` | SyncDesk.exe.config `<system.net>` |
| Methods actually implemented | Only `GET` and `POST`. `PutRequest`/`DeleteRequest` exist but `SendRequest` only branches on GET/POST (and they are never called). | RHC:195-205, 161-181 |
| POST body | `JsonConvert.SerializeObject(content)` (Newtonsoft defaults: PascalCase property names as declared, enums as integers, `byte[]` as base64, `DateTime` ISO-8601), `Content-Type: application/json; charset=utf-8`, explicit `Content-Length`. | RHC:150-159, 199-205 |
| Response | Only HTTP **200** counts as success; body is `JsonConvert.DeserializeObject<T>`. A 200 with empty body or `null` yields `null` (callers treat `null` as "nothing to do"). | RHC:206, 239 |

### 1.1 Quirk: the `"{}"` body

Several "mark"/"ack" POSTs pass the C# **string** `"{}"` as content
(SRC:95, 99, 105, 110, 115, 135). Because the client JSON-serializes the content
object (RHC:153-156), the wire body is the JSON string literal

```
"{}"
```

i.e. 4 bytes `0x22 0x7B 0x7D 0x22`, not an empty object. [obs] Whether the server
cares is unknown — a reimplementation should send exactly this to be safe.

### 1.2 Quirk: `POST /V1/Sync/Reset` with a null body

`FactoryReset` passes `null` content (SRC:211-214) → `content2 = null` (RHC:152-158)
→ `Encoding.UTF8.GetByteCount(null)` throws (RHC:201) **before any request is
sent**; the exception is re-thrown as a generic "failed to read server response"
(RHC:241-253). So, as decompiled, the client never actually sends this request and
the activation-pending step always fails. [obs, but possibly a decompiler artefact —
see Open questions]

### 1.3 Error handling

`SendRequest<T, E>` (RHC:183-254):

| Status | Behaviour |
|---|---|
| 200 | deserialize body as `T` |
| 401 | body deserialized as `AuthFailureModel` → `throw ServerError(AuthFailureModel, url)` (RHC:216-219) |
| 404 | records `ResourceNotFoundException(uri)`, **then still** tries to deserialize body as `E`; if that succeeds (incl. empty body → null) a `ServerError` is thrown; only if JSON parsing throws (e.g. HTML 404 page) is `ResourceNotFoundException` thrown (RHC:212-233). [obs] |
| other non-200 (incl. 204) | body deserialized as `E` → `throw ServerError(E, url)` (RHC:220-223, 234-237) |
| network/parse failure | `throw new Exception(SYNCAPP_FAILED_TO_READ_SERVER_RESPONSE)` (RHC:241-253) |

`E` is `WebApiError` for account-API calls and `SyncErrorModel` for sync-API calls (SRC:50-214).

Error body models:

| Model | Fields | Cite |
|---|---|---|
| `AuthFailureModel` | `DisplayMessage: string`, `ReasonCode: int enum {0 GeneralError, 1 AccountLockedGeneral, 2 AccountLockedBilling, 3 EarthmateAppExpired, 4 IncorrectUsername, 5 IncorrectPassword, 6 LoginsAreBeingThrottled, 7 ProvisionedUserNeedsCreds}`, `ActionCode: int enum {0 Nothing, 1 ShowLogin, 2 DisableAutoSync}` | M/AuthFailureModel.cs:3-29 |
| `SyncErrorModel` | `Code: SyncErrorCode`, `Description: string`, `Message: string`, `URL: string` | R/SyncErrorModel.cs:6-14 |
| `SyncErrorCode` | `1 InternalError, 2 AuthenticationError, 3 UnknownDeviceError, 4 UnOwnedDeviceError, 5 UnassignedDeviceError` | R/SyncErrorCode.cs:6-18 |
| `WebApiError` | `Code: ErrorCode`, `Message`, `Description`, `URL` (strings) | R/WebApiError.cs:7-15 |
| `ErrorCode` | `1 InternalError, 2 TooManyRequestsError, 3 AuthenticationError, 4 UnknownDeviceError, 5 UnknownUserError, 6 InvalidTimeStampError, 7 InvalidUserError, 8 InvalidDeviceError, 9 NotImplemented, 10 UnknownPlanError, 11 InvalidBillingInfoError, 12 InvalidSettingsError` | R/ErrorCode.cs:5-31 |

UI reaction (APP:473-562):
- `AuthFailureModel` → login dialog; if `ActionCode == DisableAutoSync` the dialog is not shown, only `DisplayMessage` as error (MW:355-364). `ProvisionedUserNeedsCreds+ShowLogin` shows login without "failed" flag (APP:489-498).
- `SyncErrorModel.Code == AuthenticationError` → login dialog with failed flag (APP:505-508).
- `SyncErrorModel.Code == UnassignedDeviceError` → "device not assigned" message (APP:509-512). During data gathering this code is caught and turns the device into "unassigned" mode instead (SM:1304-1311, see §4).
- otherwise `Description` is shown.

---

## 2. Authentication

### 2.1 Method [obs]

- Plain **username (e-mail) + password** typed into a native WPF dialog
  (`LoginWindow`: `Email`, `PasswordBox`, "save credentials" checkbox default `true`)
  — `SyncDesk/SyncDesk.Wpf/LoginWindow.xaml.cs:17-113`. No browser, no OAuth, no SSO.
- There is **no login request**. Credentials are simply attached as HTTP Basic auth
  to every sync-API request (RHC:283-290). A wrong password surfaces as a 401
  (`AuthFailureModel`) or a `SyncErrorModel{Code=2}` on the first authenticated call
  (normally `GET /V1/Sync/Device`, SM:1219).
- No tokens, no session, no refresh, no expiry logic anywhere in the code.
- Credentials are optional until the server says otherwise: `GET /V1/SyncURL` returns
  `Provisioned`; if `Provisioned == true` and no credentials are known, the client
  synthesises `AuthFailureModel{ReasonCode=ProvisionedUserNeedsCreds, ActionCode=ShowLogin}`
  and shows the login dialog (SM:609-627). Non-provisioned devices get firmware only
  (SM:573-580).

Which calls carry credentials [obs]:

| Call | Auth | Cite |
|---|---|---|
| `GET /V1/SyncURL`, `GET /V1/Application` (account API) | none (client constructed without user/pass) | SW:26, 50 |
| `GET /V1/Sync/Firmware/Get` for non-OneX devices | none | SM:830-835 |
| `GET /V1/Sync/Firmware/Get` + `POST /V1/Sync/Device` for OneX | Basic | SM:825-829 |
| `GET /V1/Sync/Firmware/LatestFirmware` (bricked path) | none | SM:902-906 |
| everything in `getSyncData`, Report, EventLog, Firmware/Current | Basic | SM:1001, 1128, 1173, 1190 |

### 2.2 Environment selection

The login "username" field doubles as an environment selector (`UserEnvironmentSplitter`, UES:37-79, used at APP:600-614):

- `user@domain:ENV` → username `user@domain`, environment `ENV` (everything after the first `:`).
- `user@domain-test` / `-dev` / `-int` / `-local` (suffix on the domain, case-insensitive) → suffix stripped from the username, environment = `test`/`dev`/`int`/`local`.
- Otherwise environment is empty.

Fallback: app setting `SyncEnvironment` (commented out in shipped config) (APP:246-250; SyncDesk.exe.config).

Account-API base by environment (SM:157-180, lowercased at SM:279-298; enum R/AccountApiServer.cs:5-17):

| Environment | Account API base |
|---|---|
| empty (production) | `https://account-api.inreach.garmin.com` |
| `stage` | `https://account-api-stage.delorme.com` |
| `test` | `https://account-api-test.delorme.com` |
| `dev`, `int` | `https://account-api-dev.delorme.com` |
| `local` | `http://localhost:53540` |
| any other string `X` | `http://X:53540` |

The **sync** API host is never hard-coded: it is whatever `SyncURL` the account API
returns (§3.1).

### 2.3 Local storage (`CRED`)

- Stored via .NET `ApplicationSettingsBase` user-scoped settings `UserName`,
  `UserPassword`, `Entropy`, `Environment` (`inReach.Sync/inReach.Sync.Settings/SyncDeskSettings.cs:10-74`), i.e. the per-user `user.config` [inf for exact file path].
- `Platform` enum: `UNKNOWN=0, WINDOWS=1, OSX=2` (`Common.Devices.Usb/DeLorme.Common.Devices.Usb/Platform.cs:3-8`); the test `(uint)(platform-1) <= 1` is true for both Windows and Mac (CRED:46, 83).
- Password: UTF-8 bytes → `ProtectedData.Protect(bytes, entropy, CurrentUser)` (DPAPI) with 20 random bytes of entropy; both ciphertext and entropy stored base64 (CRED:107-124). Username and environment are stored in clear.
- Saved only if the "save" checkbox is ticked (APP:609-612). Logout clears UserName/UserPassword/Environment (CRED:126-132; APP:181-186).
- A corrupt settings file is deleted and re-read once (CRED:134-155).

A Linux reimplementation needs only "username + password + optional environment"; use the Secret Service/keyring instead of DPAPI. [inf]

### 2.4 Hard-coded keys / IDs

None. No API keys, client IDs, client secrets or signing keys appear in
SyncRestClient, inReach.Sync or SyncDesk. The only hard-coded identifiers are:
- application name `"SyncApp"` (SM:107, 801, 1140) — **hard-coded string**;
- the account-API host list above — **hard-coded**;
- the single-instance mutex/IPC name `com.delorme.inreachSyncApp` (local only; `inReach.Sync/inReach.Sync.Utils/Synchronization.cs:21,108`).

---

## 3. Endpoints

`{ACCT}` = account-API base (§2.2). `{SYNC}` = `SyncInfo.SyncURL` returned by `/V1/SyncURL`.
All `imei` values are decimal `long` (the device's `serialNum`, SM:479).
"Auth" = Basic auth header present.

### 3.1 Account API

| # | Method & URL | Auth | Request | Response (200) | Err model | Cite |
|---|---|---|---|---|---|---|
| A1 | `GET {ACCT}/V1/SyncURL?IMEI=<imei>&` | no | — (`IMEI` empty if unknown) | `SyncInfo { SyncURL: string, Provisioned: bool }` | WebApiError | SRC:9, 55-59; R/SyncInfo.cs:5-9; SW:24-46 |
| A2 | `GET {ACCT}/V1/Application?name=SyncApp&platform=<Windows\|Mac>&` | no | — | `ApplicationInfo { Name, Platform, VersionMajor:int, VersionMinor:int, VersionRev:int, LastUpdate:DateTime, Location:string, Active:bool, Required:bool }` | WebApiError | SRC:50-53; R/ApplicationInfo.cs:6-26; SM:798-815 |

Notes:
- A1: the public swagger lists params `imei` (required), `username`, `guid` and documents only a `204` response (`docs/account-api-swagger.json`, path `/V1/SyncURL`). The client sends upper-case `IMEI` only and **requires 200 + JSON** (a 204 would raise `ServerError`, RHC:206-237). Swagger also declares Basic auth security for the API, but the client sends none for A1/A2. If the response is null the client throws "No sync info was available for {imei}" (SW:45).
- A1 is called once per device connection (cached in `_syncInfo`; reset on disconnect SM:369-378 and after login APP:594).
- A2: `ApplicationInfo.Version` = `VersionMajor.VersionMinor.build.revision` where `VersionRev` is zero-padded to 8 digits and split 4+4 (R/ApplicationInfo.cs:28-43). Compared to the entry assembly version `1.4.47.4776` (`SyncDesk/Properties/AssemblyInfo.cs`); if newer the user is offered `Location` (opened with `Process.Start`), and if `Required` the sync button is disabled (APP:439-471; MW:241-253). Called once at start-up (APP:263; SM:516-533).

### 3.2 Sync API (`{SYNC}`)

| # | Method & path | Auth | Query | Body | Response (200) | Cite |
|---|---|---|---|---|---|---|
| S1 | `GET /V1/Sync/Device` | yes | `imei` | — | `DeviceSync { Synced: bool, LastSynced: DateTime, SyncData: [DataTypeSyncHighwater { DataType: int enum, LastModified: DateTime? }] }` | SRC:123-126; M/DeviceSync.cs; M/DataTypeSyncHighwater.cs; DataTypeEnum `0 Contacts,1 QuickTexts,2 Presets,3 Waypoints,4 Routes,5 DeviceCommands,6 Social` (M/DataTypeEnum.cs) |
| S2 | `POST /V1/Sync/Device` ("CheckDeviceOwned") | OneX: yes; unassigned path: yes | `imei` | `"{}"` | ignored | SRC:103-106; SM:828, 1330-1333 |
| S3 | `GET /V1/Sync/Contacts` | yes | `imei` | — | `ContactsResponseModel { Contacts: [ContactModel { ID:int, FirstName, LastName, Organization: string, Addresses: [AddressModel { ID:int, Type:int enum, Address:string }] }] }` | SRC:61-64; R/ContactsResponseModel.cs; R/ContactModel.cs; R/AddressModel.cs |
| S4 | `POST /V1/Sync/Contacts?synced=true&imei=<imei>` | yes | | `"{}"` | ignored | SRC:108-111 |
| S5 | `GET /V1/Sync/Presets` | yes | `imei` | — | `PresetsResponseModel { Presets: [PresetMessageModel { ID:int, Recipients: string[], Message: string, Facebook, Twitter, MapShare: bool }] }` | SRC:66-69; R/PresetsResponseModel.cs; R/PresetMessageModel.cs |
| S6 | `GET /V1/Sync/QuickTexts` | yes | `imei` | — | `QuickTextsResponseModel { QuickTexts: [QuickTextModel { ID:int, Text:string }] }` | SRC:71-74; R/QuickTextsResponseModel.cs; R/QuickTextModel.cs |
| S7 | `POST /V1/Sync/QuickTexts?synced=true&imei=<imei>` ("MarkDeviceAddressBookSynced") | yes | | `"{}"` | ignored | SRC:113-116 |
| S8 | `GET /V1/Sync/Settings` | yes | `imei` | — | `SettingsModel { Settings: { "<int key>": <any JSON value> } }` — each value is converted to a string via `JToken.ToString()` | SRC:128-131; M/SettingsModel.cs; M/StringDictionaryConverter.cs:15-27 |
| S9 | `POST /V1/Sync/Settings?synced=true&imei=<imei>` | yes | | `"{}"` | ignored | SRC:133-136 |
| S10 | `GET /V1/Sync/Plans` | yes | `imei`, optional `trackingInterval` (int, device setting 24) | — | `DeviceCommandsResponseModel { DeviceCommands: [DeviceCommandsModel { TransactionId:int?, StatusCode:int enum, DeviceData: byte[] (base64), ErrorMsg:string }] }` | SRC:138-145; M/DeviceCommands*.cs; SD:176-203 |
| S11 | `POST /V1/Sync/Plans?imei=<imei>` ("AcknowledgeDeviceCommands") | yes | | the same `DeviceCommandsResponseModel` with `StatusCode` updated (`1 Acked` / `2 Nacked`) and `ErrorMsg` set on failure | ignored | SRC:118-121; SD:476-545; StatusCode enum `0 NoResponse,1 Acked,2 Nacked,3 Completed,4 Cancelled,5 WaitingForAck` (M/DeviceCommandStatusCode.cs) |
| S12 | `POST /V1/Sync/Waypoints` | yes | — | `{ "imei": long, "waypoints": [Waypoint] }` (lower-case keys from anonymous object) | `Waypoint[]` | SRC:152-160 |
| S13 | `POST /V1/Sync/Routes` | yes | — | `{ "imei": long, "trackBlockVersion": int, "routes": [Route] }` | `Route[]` | SRC:162-171 |
| S14 | `GET /V1/Sync/Tracks` | yes | `imei` | — | `NewestTrackPointTime { Date: string }` (parsed with `DateTimeOffset.Parse`, invariant culture; empty/invalid ⇒ 1970-01-01Z) | SRC:189-204; M/NewestTrackPointTime.cs |
| S15 | `POST /V1/Sync/Tracks` | yes | — | `Tracks { imei: long, tableVersion: int, trackBlocks: string[] }` (lower-case property names as declared) | `InsertTrackResponse { insertCount: int }` (logged only) | SRC:183-187; M/Tracks.cs; M/InsertTrackResponse.cs |
| S16 | `GET /V1/Sync/Maps/SelectedMaps` | yes | `imei` | — | `AvailableMapResponse { packages: [MapPackage { files: [MapFile { url, size:long, md5_hash, name }], name, version:int, type, package_id }] }` | SRC:173-176; M/AvailableMapResponse.cs; M/MapPackage.cs; M/MapFile.cs |
| S17 | `GET /V1/Sync/Maps/PreloadMaps` | yes | `imei`, `alwaysIncludeWorldMaps=true` | — | `AvailableMapResponse` | SRC:178-181 |
| S18 | `GET /V1/Sync/Maps/UserHasHunt` | yes | `imei` | — | bare JSON `bool` | SRC:147-150 |
| S19 | `GET /V1/Sync/Firmware/Get` | non-OneX: **no**; OneX: yes | `imei`, `major`, `minor`, `revision`, `hMinor` (ints, may be empty `k=&`) | — | `FirmewareResponseModel { FirmwareRevId:int, URL:string, CheckSum:string, Name:string, ReleaseDate:DateTime, VersionMajor, VersionMinor, VersionRev:int }` or `null` = no update | SRC:76-79; R/FirmewareResponseModel.cs |
| S20 | `GET /V1/Sync/Firmware/LatestFirmware` | no | — | `FirmewareResponseModel` | SRC:81-84; SM:904-907 |
| S21 | `POST /V1/Sync/Firmware/Current?imei=&major=&minor=&revision=[&firmwareRevId=]` | yes | as shown | `"{}"` | ignored | SRC:91-101 |
| S22 | `POST /V1/Sync/Reset?imei=<imei>` | yes | | `null` → never sent (§1.2) | — | SRC:211-214; SM:1194-1208 |
| S23 | `POST /V1/App/Report` | yes | — | `AppReportModel` (below) | ignored | SRC:86-89; SM:1136-1174 |
| S24 | `POST /V1/App/EventLog` | yes | — | `{ "log": string, "imei": long? }` | ignored | SRC:206-209; SM:1124-1134 |

Every sync-API error body is parsed as `SyncErrorModel`.

#### Body models in detail

`Waypoint` (M/Waypoint.cs:3-31) — PascalCase JSON:

| Field | Type | From device column (DW:116-131) |
|---|---|---|
| `SyncId` | int (device stores uint, cast) | 0 |
| `SyncStatus` | int enum `0 Synchronized, 1 SymbolNotSynced, 2 LocationNotSynced, 3 LabelNotSynced, 7 Unsynchronized` | 1 |
| `HiddenOnDevice` | bool | 2 (≠0) |
| `IconIndex` | int | 3 |
| `Name` | string | 8 |
| `Latitude`, `Longitude` | decimal | 4, 5 |
| `CreateDate`, `ModifiedDate` | string, ISO-8601 round-trip (`"O"`) or null | 6, 7 (`yyyy-MM-ddTHH:mm:ssZ` on device) |

`Route` (M/Route.cs:3-26): `Label`, `ColorHex` (`#RRGGBB`), `BorderColorHex`, `CreatedDate`, `ModifiedDate` (strings, `"O"` format), `TrackBlocks` (string; **null when uploading**, CRLF-separated TrackBlocks-table rows when returned), `SyncId:int`, `DeletedOnDevice:bool`, `TrackId:int`. Device colours are RGB565 converted to/from hex (DR:127-226).

`AppReportModel` (M/AppReportModel.cs:5-40), as filled at SM:1136-1173:

| Field | Value sent |
|---|---|
| `ID` | null |
| `ApplicationName` | `"SyncApp"` |
| `ApplicationVersion` | `VersionInfo.AssemblyDescription` = AssemblyProduct (`""`) + `" "` + AssemblyDescription → `" int : 1.474776"` (note leading space) (`inReach.Sync/inReach.Sync.Utils/VersionInfo.cs:10-22`; `inReach.Sync/Properties/AssemblyInfo.cs`; APP:627) |
| `Platform` | `Platform.ToString()` → `"WINDOWS"` / `"OSX"` |
| `OSVersion` | e.g. `"Windows 10"` or `sw_vers -productVersion` output (`inReach.Sync/inReach.Sync.Utils/OSInformation.cs`) |
| `IMEI` | long? |
| `ContainsFirmwareInformation` | device version known |
| `FirmwareMajor/Minor/Revision`, `DeviceVersion` | from device `firmwareVer` "a.b.c" |
| `EmailAddress` | the login username |
| `IsErrorReport` | true for error reports |
| `ExceptionMessage`, `ExceptionStackTrace` | error reports only |
| `Logfile` | error reports only: last 50 000 chars of the local log file |
| `TimeOfReport` | never set (null) |

### 3.3 Non-API downloads

| What | How | Auth | Integrity | Cite |
|---|---|---|---|---|
| Firmware image | `WebClient.DownloadData(FirmewareResponseModel.URL)` (whole file in memory) | none | MD5 of bytes, lower-case hex, compared **case-sensitively** with `CheckSum`; mismatch ⇒ abort install. If the MD5 provider throws `TargetInvocationException` (FIPS mode) the check is skipped. | SD:547-626 |
| Map files | `WebClient.DownloadFileAsync(MapFile.url)` into `%LOCALAPPDATA%\SyncDesk\Garmin\maps\download\<type>\<package_id>\<version>\<name>` | none | **size only** (`FileInfo.Length == MapFile.size`); `md5_hash` is never used | DM:925-1026; grep: `md5_hash` only referenced in M/MapFile.cs |
| App installer | `Process.Start(ApplicationInfo.Location)` — handed to the OS | — | — | MW:241-253 |

---

## 4. Sync sequence

### 4.1 Connect and plan (`CheckAllSyncDataImpl`, SM:535-607)

1. **Startup**: load stored credentials, pick environment, construct `SyncManager`, `GET {ACCT}/V1/Application` (A2) in background, start USB device manager (APP:113-130, 244-281).
2. **Device attached** → `GetIMEIAsync` reads the device version record (`serialNum`, `firmwareVer`, `productName`) (SM:342-367; SD:131-174). Inreach-3 in bootloader is switched to app mode first (SM:387-391). OneX devices without IMEI ask the user (or take `imei=` from a URL passed on the command line / IPC from the website, MW:390-399; APP:188-206) (SM:493-514).
3. `_imei = long.Parse(serialNum)`; **A1 `GET /V1/SyncURL?IMEI=`** → `{SyncURL, Provisioned}`. If `Provisioned` and no credentials ⇒ login dialog (SM:454-491, 609-627).
4. Product type is derived from `productName` (SM:1014-1063): `D1`/`inReach Bluetooth`/null ⇒ OneX; contains `inReach SE`/`inReach HW20` ⇒ SE; `inReach HW25` ⇒ SE/Explorer by device setting 760; upper-case contains `INREACH 3A` ⇒ Explorer3 if also `EX` else SE3; contains `MINI` ⇒ Mini. D1 with no firmware version asks for device colour (SM:564-568).
5. **Firmware check** (SM:822-859): parse `firmwareVer` → major/minor/revision; `hMinor` = number after `"Rev "` in `productName` (or the chosen colour for D1) (SM:861-895). For OneX first **S2 `POST /V1/Sync/Device`** with auth, then **S19** with auth; for others **S19** without auth. Non-null response with a *different* version (or same version and OneX / `ForceFirmwareUpdate` config) ⇒ add a `Firmware` step and report `UpdateAvailable`.
6. If **no firmware step** and product ≠ OneX and `Provisioned` ⇒ `getSyncData` (step 7). If a firmware step exists, data sync is deferred until after the reboot (SM:572-580, 712-718).
7. **`getSyncData`** (SM:1176-1371), in this order; network calls happen *now*, device writes happen later in step 9:
   1. Read device setting 1192; if `"1"` (activation pending) add step *Settings/FactoryReset* → S22 then delete setting 1192 (SM:1194-1208; DS:163-177).
   2. Explorer3 only: **S17** PreloadMaps and **S16** SelectedMaps, prepare map tasks (SM:1209-1214; DM:289-421).
   3. Add step *FirmwareServerUpdate* → **S21** with the device's current version, no `firmwareRevId` (SM:932-944, 1215-1218).
   4. **S1 `GET /V1/Sync/Device`** (highwater marks) (SM:1219-1224).
   5. **S3** Contacts → step *Contacts* (non-null response only).
   6. **S5** Presets → step *Presets*.
   7. **S6** QuickTexts → step *QuickTexts*.
   8. **S8** Settings → step *Settings* (only if ≥1 entry) (SM:1390-1409).
   9. Read device Waypoints table (v3 only) → step *Waypoints* (SM:1536-1553).
   10. **S14** newest track point → read TrackBlocks table (v5 only) → step *Tracks* (SM:1411-1437).
   11. Read Routes table (v1 only, needs Tracks support) → step *Routes* (SM:1460-1477).
   - If any of 4–11 returns `SyncErrorModel{Code=5 UnassignedDeviceError}`, the device is marked *unassigned*: remaining fetches in that block are skipped and later all `RequiresAssignedDevice` steps (Contacts, Presets, QuickTexts, Waypoints, Tracks, Routes) are skipped (SM:1304-1311, 651-655).
   12. Read device setting 24 (tracking interval) → **S10 `GET /V1/Sync/Plans`** → step *DeviceCommands* if non-empty (SM:1312-1329).
   13. If unassigned and (firmware step or commands present) → **S2** (SM:1330-1333).
   14. If Contacts/Presets/QuickTexts were non-null: insert *RemoveSyncData* (wipe SyncData table) before step 3 above, and append *TimeStamp* (setting 73 = now) (SM:1334-1352).
   15. Explorer3 and not `SkipMaps` config ⇒ append *Maps* step (SM:1479-1521).

Resulting step order: `[Firmware]`, `FactoryReset?`, `RemoveSyncData?`, `FirmwareServerUpdate`, `Contacts?`, `Presets?`, `QuickTexts?`, `Settings?`, `Waypoints?`, `Tracks?`, `Routes?`, `DeviceCommands?`, `TimeStamp?`, `Maps?`.

8. If nothing blocks auto-continue (no required app update, no error), steps run immediately; otherwise on Sync button (SM:581-586; APP:137-165).

### 4.2 Execute (`ExecuteAllSteps`, SM:637-749)

9. Each step in order (aborts on the first device error and uploads an error report per error, SM:670-686):

| Step | Device side | Network side | Cite |
|---|---|---|---|
| Firmware | non-OneX: save setting 424, write 424 = new `VersionRev` ("DisableNewAmIAlive"); download + MD5 + flash; on failure restore 424 | download `URL`; on success and `Provisioned`: **S21** with `firmwareRevId` (auth; failure ignored) | SM:946-1012, 1373-1388; SD:547-626 |
| FactoryReset | delete setting 1192 | S22 (see §1.2) | SM:1197-1206 |
| RemoveSyncData | `d,*` on SyncData table | — | SD:205-209 |
| FirmwareServerUpdate | — | S21 (current version) | SM:932-944 |
| Contacts | delete synced contacts, write Contacts + SyncData type-1 address rows | then **S4** | SM:1237-1248; SD:271-302 |
| Presets | write up to 3 presets as SyncData type-3 indices 14–16; setting 97 = `LastModified(Presets)` from S1 | — (no "synced" POST) | SM:1257-1266; SD:344-378 |
| QuickTexts | SyncData type-2 rows; setting 89 = `LastModified(QuickTexts)` | then **S7** | SM:1275-1287; SD:380-410 |
| Settings | write each key/value to Settings table (key 16 = device type ⇒ apply defaults; empty 35/43 ⇒ delete) | then **S9** if no device error | SM:1397-1408; DS:113-161 |
| Waypoints | — | **S12** upload all device waypoints, receive merged list | SM:1555-1566 |
|  | wipe Waypoints table, write returned non-hidden waypoints | | DW:79-96, 149-173 |
| Tracks | — | **S15** upload rows starting `0,0` (high-detail) whose field 9 date ≥ S14 date; `tableVersion` = TrackBlocks table version | SM:1439-1458; DT:134-147 |
| Routes | — | **S13** upload device routes (no track blocks) with `trackBlockVersion` | SM:1523-1534 |
|  | delete old routes + their track blocks; write returned routes and their `TrackBlocks` rows | | DR:78-125, 240-250 |
| DeviceCommands | split each `DeviceData` (UTF-8) on `\r\r`, write each script (≤1999 bytes) raw to the device; set `StatusCode` Acked/Nacked | then **S11** with the updated list | SM:1320-1328; SD:476-545 |
| TimeStamp | setting 73 = unix now | — | SD:412-426 |
| Maps | mass-storage mode; copy BaseFiles; download/copy packages; delete unknown packages; eject; if hunt maps deleted and **S18** false ⇒ setting 1624 = 1 | S18 (twice), map file downloads | SM:1490-1520; DM:126-206 |

`ResourceNotFoundException` (404 with non-JSON body) on Waypoints/Tracks/Routes is swallowed — older servers without these endpoints are tolerated (SM:1432-1436, 1454-1457, 1530-1533, 1562-1565).

10. **Reboot** device (non-OneX, no error) (SM:687-702).
11. If a Firmware step ran: clear steps and **go back to 4.1 step 3** (re-read version, re-check firmware, then data) (SM:712-718). Otherwise **S23** sync report (`IsErrorReport=false`) and status *Complete* (SM:719-723; OneX SM:705-709).
12. Non-OneX: **S24** upload the device's EventLog table (raw CSV) with IMEI — done after every run, success or failure (SM:726-729, 1124-1134).
13. Any queued errors → **S23** error report with log tail (SM:744-748, 776-785, 1088-1110).

### 4.3 Bricked-device path

If the device version cannot be read (`PossibleBrickedDevice`), the user is asked to reflash; `GetAndInstallFirmwareForBrickedDevice`: A1 → S20 (no device info) or S19 → Firmware step (bootloader mode), plus `getSyncData` if provisioned, then execute (SM:897-930; APP:564-582).

---

## 5. Data direction per data type

| Data | Source of truth | Direction | Endpoints |
|---|---|---|---|
| Contacts | server (Explore website) | server → device; then "synced" ack | S3, S4 |
| Preset messages (3) | server | server → device (no ack) | S5 |
| Quick texts | server | server → device; ack | S6, S7 |
| Device settings | server | server → device; ack | S8, S9 |
| Device commands ("plans": opaque table scripts, e.g. service-plan/tracking changes [inf]) | server | server → device; status posted back | S10, S11 |
| Waypoints | merged | **device → server → device** (two-way; server returns the merged set, device table rewritten) | S12 |
| Routes | merged | **device → server → device** (device sends metadata incl. `DeletedOnDevice`; server returns routes with track blocks) | S13 |
| Tracks | device | **device → server only** (high-detail track blocks newer than the server's newest point) | S14, S15 |
| Maps (Explorer3 only) | server selection | server → device (file download to USB mass storage) | S16, S17, S18 |
| Device event log | device | device → server | S24 |
| Firmware version | device | device → server | S21 |
| Messages (sent/received) | — | **not synced** by this client at all (no endpoint, no table read) | — |

Highwater marks in S1 are used only to stamp Presets/QuickTexts timestamps into device settings 97/89; S1.`Synced` only changes the UI status (SM:1220-1224, 1263, 1281).

### Per-product differences in the server flow [obs]

| Product | Differences | Cite |
|---|---|---|
| OneX (inReach 1.x, D1, "inReach Bluetooth") | IMEI may come from user/URL; S2 + authenticated S19 before firmware; same-version firmware is still offered; **no data sync** (`getSyncData` skipped); no reboot; no event-log upload; flash always "in bootloader already" | SM:493-514, 573, 690, 705-709, 726, 825-829, 841, 970-977 |
| SE / Explorer (HW20/HW25) | standard flow; HW25 SE vs Explorer via setting 760 | SM:1026-1052 |
| SE3 / Explorer3 ("inReach 3A") | standard flow; **Explorer3 only** gets maps (S16–S18) | SM:1053-1056, 1481; DM:291, 370 |
| Mini | standard flow; nothing Mini-specific in client code | SM:1057-1060 |

Which data steps actually run on a given device is decided by **device table presence/version**, not product type (Waypoints v3, TrackBlocks v5, Routes v1) (DW:61-64; DT:71-74; DR:69-76).

---

## 6. Other things a reimplementation must replicate

- **"AmIAlive" setting 424** — before flashing (non-OneX, healthy device) the client writes device setting 424 = target `VersionRev` and restores the previous value if flashing fails (SM:964-968, 983-988, 1373-1388). Device-side meaning not visible in client code.
- **Activation pending (setting 1192)** — triggers the S22 reset step; see §1.2.
- **Reboot after sync** for non-OneX (SM:690-696).
- **Post-firmware re-run**: after a firmware step the whole check is re-run before data sync (SM:712-718).
- **S21 every sync** with the current firmware version (FirmwareServerUpdate step).
- **Telemetry**: S23 success report after every successful sync; S23 error report (with up to 50 000 chars of local log, username as `EmailAddress`) on every error; S24 event-log upload after every non-OneX run. Whether the server requires these is unknown — they are fire-and-forget (failures only logged) (SM:1112-1134).
- **App version check** (A2) — the server can mark a version `Required`, which disables sync in the official app. Not enforced server-side by anything visible.
- **DisableSpotlightTask** — macOS-only: runs AppleScript `do shell script "mdutil -i off -d <volume>"` up to 10 times, 3 s apart (`inReach.Sync/inReach.Sync.Tasks/DisableSpotlightTask.cs:36-64`; DM:872-878). It is only reachable from `DeviceMaps.DisableSpotlight()` (DM:208-249), which has **no callers** — dead code. `ReplaceMissingBaseFiles` does create `.Trashes` and delete `.Spotlight-V100` / `.fseventsd` on the device volume during map sync (DM:835-864). Not relevant to the server protocol; a Linux client can ignore Spotlight but should avoid leaving desktop-indexer junk on the volume.
- **Map cache layout and package version directories** — `<type>/<package_id>/<version>` with `_<version>` temp staging and deletion of older versions; `scanMarker` in `Garmin/maps` deleted when maps changed (MapPackage paths M/MapPackage.cs:18-25; DM:251-275, 618-717). Free-space check needs total size + 500 MiB locally (DM:502-513).
- **Hunt map filtering**: if S18 is false, packages whose `type` is one of `ir_plss_vectors`, `ir_hunting_vectors`, `ir_parcel_vectors` are dropped (DM:45, 395-402).
- **No retries/backoff** on API calls; one HttpClient per call; error report submission retried once (SM:1088-1110).

---

## 7. Open questions / unknowns

1. **SyncURL value and format.** The sync host is only known at runtime from A1. Unknown: actual production hostname, whether it carries a path prefix or trailing slash (client concatenates `SyncURL + "/V1/..."`), and whether it differs per IMEI/region. Needs one real A1 response (outside the scope of static reading).
2. **A1 contract mismatch**: swagger says `imei` (required, lower-case) + optional `username`/`guid`, response `204`; client sends `IMEI=` only and needs `200 + {SyncURL, Provisioned}`. Presumably the swagger is stale/incomplete and ASP.NET query binding is case-insensitive [inf].
3. **Sync-API authentication details**: is Basic auth the only scheme the server accepts today (Garmin SSO migration)? Does the server still accept this client's credentials for Garmin-account (post-DeLorme) users? Code shows only Basic.
4. **`POST /V1/Sync/Reset` with null body** (§1.2): as decompiled, the request is never sent and the step throws. Could be an ilspy artefact or a real bug; verify against IL or traffic before relying on it.
5. **`"{}"` body** (§1.1): unknown whether the server parses it; safest to replicate byte-for-byte.
6. **Response nullability**: when is S3/S5/S6/S8 null vs. empty list? The client treats null as "no step" and any non-null (even empty) as "rewrite device table" — for contacts an empty list would delete all synced contacts. Server semantics (e.g. "null when nothing changed since last ack") are not visible.
7. **Device command payloads** (`DeviceData`): opaque device table scripts generated server-side; their content and the meaning of `trackingInterval` for the server are not documented in the client.
8. **Required vs optional telemetry**: whether skipping S21/S23/S24 has any server-side effect (e.g. the "last synced" shown on Explore, firmware compliance) is unknown.
9. **`DateTime` formats on the wire** for `DeviceSync.LastSynced`, `DataTypeSyncHighwater.LastModified`, `FirmewareResponseModel.ReleaseDate`, `ApplicationInfo.LastUpdate` — Newtonsoft accepts ISO-8601 and `/Date(…)/`; exact server format unknown.
10. **Enum encoding** in responses (`SyncErrorModel.Code`, `DataType`, `StatusCode`, `AddressModel.Type`): client sends integers and Newtonsoft would accept either integers or names; the server's choice is unknown.
11. **`hMinor`** semantics for 2.x/3.x devices: taken from `"Rev N"` in `productName`; unknown whether the server needs it for Mini firmware selection.
12. **User-Agent**: client sends none; whether the server/WAF rejects requests with a custom UA is unknown.
