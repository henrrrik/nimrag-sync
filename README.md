# nimrag-sync

![nimrag-sync showing what a sync of an inReach Mini would change](docs/screenshot.png)

Keeps a Garmin/DeLorme inReach in sync on Linux, as Garmin's inReach Sync does on Windows and macOS:
its contacts, preset messages, quick texts, settings, waypoints, routes and tracks, with the Garmin
account it is on, and its software up to date. An app for [Omarchy](https://omarchy.org), with a
command line for everything the window does.

## Install

Packages for Arch Linux and Omarchy, x86_64 and aarch64, are on the
[releases page](https://github.com/henrrrik/nimrag-sync/releases). Download one, check it against the
release's `SHA256SUMS`, and install the file:

```sh
curl -LO https://github.com/henrrrik/nimrag-sync/releases/download/v0.2.0/nimrag-sync-0.2.0-1-x86_64.pkg.tar.zst
curl -LO https://github.com/henrrrik/nimrag-sync/releases/download/v0.2.0/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
sudo pacman -U nimrag-sync-0.2.0-1-x86_64.pkg.tar.zst
```

(`aarch64` in place of `x86_64` on ARM, such as an Asahi Mac.) The packages are not signed, so pacman
installs them from a file you have checked, not straight from their address: with Arch's default
settings `pacman -U <address>` asks for a signature and stops without one. The package brings a udev
rule that lets the user at the computer reach the inReach, so no `sudo` is needed afterwards.
`packaging/PKGBUILD` builds the same package from a release's source.

## Using it

Plug the inReach in with a USB cable that carries data and open nimrag-sync (Super + Space).

- **Sync.** Sign in with the Garmin account the inReach is on, the one of explore.garmin.com. With the
  inReach on, nimrag-sync shows what a sync would change; **Sync now** does it. A charging inReach can be
  switched on from the window; one that is a USB drive must be ejected first.
- **Software updates.** **Switch to USB drive mode**: as a drive, the inReach is checked for updates by
  itself, and **Install update** downloads the update, checks it, copies it onto the drive and ejects it.
  The inReach installs it when it restarts; leave it plugged in until it is done.

The password is kept in the desktop's keyring (the Secret Service, through `secret-tool`), the email
address in `~/.config/nimrag-sync/account`. Before a sync writes anything, the device's tables as they
were are kept in `~/.local/share/nimrag-sync/backups/<IMEI>/<time>/`, readable by you only.

## What a sync does

It does what inReach Sync 1.4.47 does, step for step and in its order (`docs/server-protocol.md`):

1. tells Garmin which software the inReach has, and stops if Garmin offers newer software for it
   (see below);
2. writes the contacts, address book, preset messages and quick texts from Garmin, keeping the contacts
   made on the device;
3. writes Garmin's settings for the device;
4. sends the device's waypoints to Garmin and writes back Garmin's merged list;
5. sends the device's new high-detail track to Garmin;
6. sends the device's routes and writes back Garmin's, with their tracks;
7. runs the device commands Garmin has for it (changes of plan and the like);
8. writes the time of the sync and restarts the inReach.

Along the way it tells Garmin, as inReach Sync does, which steps went through. Before the restart it reads
the device back and says whether it matches Garmin.

### Where it differs from inReach Sync

- It does not install software through the device's bootloader, as inReach Sync does for the older DeLorme
  models; when Garmin's inReach service offers new software during a sync, it stops and says so. An inReach
  Mini takes its software as a USB drive instead (`nimrag-sync update`).
- It sends Garmin no usage report and no copy of the device's event log.
- It keeps the backup above.

As inReach Sync does, a sync writes Garmin's data over the device's. The preview shows what changes; things
worth knowing:

- Preset messages get Garmin's recipients as they are. Garmin's phone app adds a MapShare recipient (`ms`)
  that inReach Sync does not, so presets written by a sync no longer post to MapShare.
- Garmin's device commands come back on every sync, and set the settings they name back to Garmin's values.
- An inReach Mini keeps address-book entries that no Explore contact has: the step that clears them
  leaves the address book alone on it.

## Devices

| Device | USB ID | Status |
|---|---|---|
| inReach Mini (1st generation) | `091e:0b89` | sync and software updates: tried on a real device |
| inReach SE, Explorer, SE+, Explorer+ | `1163:0300` | found, and synced as inReach Sync does; untested, and no software updates |

The inReach Mini 2, Messenger and other Garmin-made devices are not inReach Sync devices: they use Garmin
Express.

## Access to the device

nimrag-sync talks to the inReach's USB HID interface (`/dev/hidraw*`), which only root may open by
default. The package installs a udev rule that lets the user at the computer open it; when building from
source, install it yourself:

```sh
sudo install -m644 pkgbuild/70-nimrag-sync.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw
```

Software updates mount and eject the inReach's drive through udisks (`udisksctl`, `gdbus`), as the file
manager does.

## Shortcuts

| Key | Action |
|---|---|
| Space, R | Ask the devices again |
| C | Check with Garmin what a sync would change |
| ? | Show or hide the shortcuts |
| Q | Quit |

## Languages

The window follows the language picked under Omarchy's Setup > Language, live: English and Swedish so
far. A language is a catalog in `src/i18n/<code>.json` (English text to translation), listed in
`src/resources.qrc`; `bin/test` fails when a catalog misses a text the window shows. Messages from the
core (errors, the command line) are in English.

## Command line

Everything the window does also works without a display, for scripts and agents. Each command takes
`--json`, exits 0 or 1, and writes errors to stderr. Commands that change something are a trial run
without `--apply`.

| Command | What it does |
|---|---|
| `nimrag-sync` or `nimrag-sync open` | open the window |
| `nimrag-sync devices` | the connected inReach devices, their mode and software |
| `nimrag-sync login <email>`, `logout` | sign in (the password, typed or piped to stdin, goes to the keyring) and out |
| `nimrag-sync preview` | what a sync would change on the inReach (reads only; the inReach must be on) |
| `nimrag-sync sync [--apply]` | sync as inReach Sync does; without `--apply` the preview |
| `nimrag-sync update [--apply] [--reinstall]` | software updates for an inReach in USB drive mode: asks Garmin's update server with the drive's `GarminDevice.xml`; `--apply` downloads the update, checks its size and MD5, copies it onto the drive and ejects it; `--reinstall` offers the version it has too |
| `nimrag-sync drive-mode [--apply]` | restart an inReach as a USB drive (if it is on, it stops what it is doing, tracking too) |
| `nimrag-sync app-mode [--apply]` | restart an inReach in its normal mode: it switches on |
| `nimrag-sync eject` | eject an inReach's drive as the file manager does (it then charges, or installs an update) |
| `nimrag-sync server [<imei>]` | what Garmin's inReach service holds for the device: sync server, last sync, contacts, presets and quick texts (reads only) |
| `nimrag-sync tables` | the tables the inReach has, and their versions (reads only; the inReach must be on) |
| `nimrag-sync read-table <id>` | the rows of one table as the inReach sends them (reads only; the inReach must be on) |
| `nimrag-sync --version`, `help` | the version; the commands |

## Building

A Rust core (`core/`, built with cargo as a static library) under a Qt Quick window (`src/`, built with
qmake6), linked into one binary. It needs Rust (cargo), Qt 6 (base, declarative, wayland), a C++
compiler and make; at run time also libsecret and udisks2.

```sh
bin/build      # cargo + qmake6 + make into build/nimrag-sync
bin/test       # the Rust tests and the Qt tests (offscreen)
bin/install    # makepkg -fsi: the app lands in the launcher
bin/screenshot [language] [out.png] [state.json]
               # the window with made-up state, captured on screen
```

`bin/screenshot` runs the real window with `tools/demo/`'s `FakeBackend` instead of the Rust core, the
state in `tools/demo/state.json`, the default accent and the language given (`en` when none), floats it
beside the pointer for a few seconds and saves it with `grim` (to `docs/screenshot.png` by default). It
needs Hyprland: Qt's offscreen platform does not draw Material's button backgrounds. `bin/test` checks
that `FakeBackend` keeps up with `Backend`.

### Tests and releases

GitHub runs `bin/test` on every push in Arch Linux containers, x86_64 and aarch64 (Arch Linux ARM).
Pushing a tag `v<version>` builds the packages for both, tests them, and publishes a release with them,
their checksums and the notes in `docs/releases/v<version>.md`; the version goes in `core/Cargo.toml`,
`src/main.cpp` and both PKGBUILDs first, and `packaging/PKGBUILD` gets the new source tarball's checksum
after. Started by hand, the release workflow builds and gathers the packages without releasing anything.

## Protocol notes

`docs/device-protocol.md` and `docs/server-protocol.md` describe what inReach Sync 1.4.47 does over USB
and with Garmin's servers, read from its code. `docs/device-observations.md` and
`docs/server-observations.md` record what a real inReach Mini and Garmin's servers answered.
`docs/account-api-swagger.json` is the published description of Garmin's inReach account API.

## Not Garmin's

nimrag-sync is not made by or affiliated with Garmin. inReach, Explore and MapShare are Garmin's
trademarks. It talks to Garmin's inReach service with your own account, as inReach Sync does, and asks
Garmin's update server for software as itself.

## License

MIT, see [LICENSE](LICENSE). `src/theme.*`, `src/translator.*` and the project layout follow Omacut and
Monologue (MIT, David Heinemeier Hansson).
