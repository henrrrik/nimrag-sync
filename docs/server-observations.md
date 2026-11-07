# What Garmin's inReach servers answered

Read-only requests, 2026-11-07, for an inReach Mini on a current Garmin account. Only `GET`s were sent:
no acknowledgements (`POST …?synced=true`), so nothing changed on the account. No personal data is kept
here: the shape of the answers only.

## Finding the sync server

`GET https://account-api.inreach.garmin.com/V1/SyncURL?IMEI=<imei>&`, without credentials, with
`Accept: application/json` and `X-CultureCode: en-US`:

1. `301` to `https://region.inreachapp.com/ProvisionInfo/InReachIMEI/<imei>` (HTML body). .NET's
   `HttpClient` follows it by default; a client must follow redirects here.
2. That answers `200 application/json`: `{"SyncURL":"https://sync.explore.garmin.com","Provisioned":true}`.

So the sync base is `https://sync.explore.garmin.com`, without a trailing slash or path; the client's
`SyncURL + "/V1/…"` works as is. The IMEI comes from setting 9 on the device (see
`device-observations.md`).

## Basic authentication works

`GET https://sync.explore.garmin.com/V1/Sync/Device?imei=<imei>` with HTTP Basic authentication (the
Garmin account's email and password; the account has no two-step verification) answered `200`. Basic
authentication as inReach Sync sends it is still accepted.

## Answers

All `200 application/json`, in the models the spec lists:

| Request | Shape |
|---|---|
| `GET /V1/Sync/Device?imei=` | `{Synced: bool, LastSynced: ISO time, SyncData: [{DataType: 0–5, LastModified: ISO time or null}]}` — one high-water mark per data type (0 Contacts, 1 QuickTexts, 2 Presets, 3 Waypoints, 4 Routes, 5 DeviceCommands) |
| `GET /V1/Sync/Contacts?imei=` | `{Contacts: [{ID: int, FirstName, LastName: string, Organization: null, Addresses: [{ID: int, Type: 0–2, Address: string}]}]}` |
| `GET /V1/Sync/Presets?imei=` | `{Presets: [{ID: 1–3, Recipients: [string], Message: string, Facebook, Twitter, MapShare: bool}]}` |
| `GET /V1/Sync/QuickTexts?imei=` | `{QuickTexts: [{ID: int, Text: string}]}` |

The counts matched the device: as many contacts as it has rows with `Synced = 1`, 3 presets, and as many
quick texts as SyncData has type-2 rows.

## Presets: recipients are addresses, and MapShare is a recipient

- A preset's `Recipients` are the addresses themselves (email addresses, phone numbers), not address IDs.
- On the device, each preset's recipient list was the server's list **plus a final `ms`**, and all three
  presets had `MapShare: true` on the server. The messages were identical.
- inReach Sync's code never writes `ms` (it joins `Recipients` as they come), so the device's presets were
  last written by something else, most likely Garmin's phone app. **Inferred:** `ms` is how the device is
  told to post the preset to MapShare; a writer should append it when `MapShare` is true. What Facebook and
  Twitter would add is not known (both are false here, and those services are long gone from inReach).

## High-water marks for waypoints and routes move by themselves

Two `GET /V1/Sync/Device` five minutes apart, with nothing changed in between, gave the same marks for
contacts, quick texts and presets, but **new times for waypoints (3) and routes (4)**, close to the time of
the request. Those two marks cannot tell whether anything changed; the others can.

## What a sync as inReach Sync does would change on this Mini

`nimrag-sync preview`, which replicates inReach Sync 1.4.47, on the Mini above:

- Contacts, quick texts: unchanged; the contacts made on the device are kept.
- Address book: the entries that are no Explore contact's address are removed, since clearing SyncData
  removes them and only Explore contacts' addresses are written back. On this device that included phone
  numbers used by its presets, its own contacts and its messages.
- Presets: rewritten without the trailing `ms`, since inReach Sync writes Garmin's recipients as they come.
  Whatever wrote `ms` there (most likely the phone app) would have to write it again for MapShare.
- Settings 89 and 97 hold the quick texts' and presets' "last changed" times from Garmin, in Unix seconds,
  and matched Garmin's to the second; 73 gets the time of the sync.
