# nimrag-sync

Keeps a Garmin/DeLorme inReach in sync on Linux: its software, contacts,
preset messages and settings, as Garmin's inReach Sync does on Windows and
macOS. An app for [Omarchy](https://omarchy.org).

Early days: it finds a connected inReach, shows its software version and mode,
and restarts it as a USB drive. Syncing with Garmin's inReach service is being
written from the protocol notes in `docs/`.

## Devices

| Device | USB ID | Status |
|---|---|---|
| inReach Mini (1st generation) | `091e:0b89` | found, version and mode, USB drive mode — tried on a real device |
| inReach SE, Explorer, SE+, Explorer+ | `1163:0300` | found; untested |

The inReach Mini 2, Messenger and other Garmin-made devices are not inReach
Sync devices: they use Garmin Express.

## Access to the device

nimrag-sync talks to the inReach's USB HID interface (`/dev/hidraw*`), which
only root may open by default. The package installs a udev rule that lets the
user at the computer open it; when building from source, install it yourself:

```sh
sudo install -m644 pkgbuild/70-nimrag-sync.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw
```

## Shortcuts

| Key | Action |
|---|---|
| Space, R | Ask the devices again |
| ? | Show or hide the shortcuts |
| Q | Quit |

## Command line

Everything the window does also works without a display, for scripts and
agents. Each command takes `--json`, exits 0 or 1, and writes errors to stderr.

| Command | What it does |
|---|---|
| `nimrag-sync` or `nimrag-sync open` | open the window |
| `nimrag-sync devices [--json]` | the connected inReach devices, their mode and software |
| `nimrag-sync drive-mode [--apply] [--json]` | restart an inReach as a USB drive; a trial run without `--apply` |
| `nimrag-sync app-mode [--apply] [--json]` | restart an inReach in its normal mode (it switches on); a trial run without `--apply` |
| `nimrag-sync tables [--json]` | the tables the inReach has and their versions (read only; needs app mode) |
| `nimrag-sync read-table <id> [--json]` | the rows of one table as the inReach sends them (read only; needs app mode) |

## Building

A Rust core (`core/`, built with cargo as a static library) under a Qt Quick
window (`src/`, built with qmake6), linked into one binary.

```sh
bin/build      # cargo + qmake6 + make into build/nimrag-sync
bin/test       # the Rust tests and the Qt tests (offscreen)
bin/install    # makepkg -fsi: the app lands in the launcher
```

## Protocol notes

`docs/device-protocol.md` and `docs/server-protocol.md` describe what inReach
Sync 1.4.47 does over USB and with Garmin's servers, read from its code.
`docs/device-observations.md` and `docs/server-observations.md` record what a
real inReach Mini and Garmin's servers answered.
`docs/account-api-swagger.json` is the published description of Garmin's
inReach account API.
