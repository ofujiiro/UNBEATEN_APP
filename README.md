# UNBEATEN Launcher

A desktop companion for **[UNBEATABLE](https://www.unbeatablegame.com/)** and
**[UNBEATEN](https://unbeaten.cc)**, the custom-chart leaderboard. Windows and
Linux.

- **Sign in** with your UNBEATEN account. The launcher shows a code, you
  approve it on unbeaten.cc (you're signed in with Discord there as usual), and
  you're done.
- **Play**: starts UNBEATABLE through Steam (Proton on Linux).
- **Scores sync on their own.** A few seconds after the results screen, new
  scores from the game's `arcade-highscores.json` reach UNBEATEN, matched to
  the exact chart you played by its content, even if you renamed the folder.
  You see the PP you gained and your new rank.
- **Ranked maps in one click.** Browse every ranked map, install it into the
  game's `CustomSongs` folder, and update it when a new version is ranked.
  Outdated or edited copies are flagged. Any folder the launcher replaces goes
  to a backup folder; nothing is deleted.
- Lives in the tray, can start with your computer, and needs no game mod.

> Fan project, not affiliated with D-CELL GAMES or Playstack. UNBEATABLE™ is
> property of D-CELL GAMES.

## Install

Download the latest release from
[Releases](https://github.com/ofujiiro/UNBEATEN_APP/releases/latest):

| System | File |
|--------|------|
| Windows 10/11 | `UNBEATEN Launcher_x.y.z_x64-setup.exe` (or the `.msi`) |
| Linux | `.AppImage` (`chmod +x` it and run it), or the `.deb` / `.rpm` |

The Windows installer isn't code-signed yet, so SmartScreen may warn about it
("More info" → "Run anyway").

Then open the launcher and click **Sign in**.

### Where it looks for the game

The launcher finds Steam, UNBEATABLE and the game's data folder on its own:

| Setup | Data folder |
|-------|-------------|
| Windows | `%USERPROFILE%\AppData\LocalLow\D-CELL GAMES\UNBEATABLE` |
| Linux, through Proton (usual) | `<Steam library>/steamapps/compatdata/2240620/pfx/drive_c/users/steamuser/AppData/LocalLow/D-CELL GAMES/UNBEATABLE` |
| Linux, native build | `~/.config/unity3d/D-CELL GAMES/UNBEATABLE` |

Steam is looked for in the registry on Windows, and in `~/.steam`,
`~/.local/share/Steam`, the Flatpak and the Snap locations on Linux, including
every library listed in `libraryfolders.vdf`. `arcade-highscores.json` is
searched for inside the data folder (it may sit in a profile subfolder), and
`CustomSongs` is next to it. If something isn't found, set it in **Settings**.

## How it works

### Sign-in
A device link, like signing in a TV app: the launcher asks UNBEATEN for a code
and opens `unbeaten.cc/link?code=…`; you approve it there; the launcher gets
its own token. That token can only sync scores and read your profile and the
map list. It can't change your profile, and you can sign it out from
UNBEATEN's **Settings → Connected launchers**. On your computer it's kept in
the system's credential store (Windows Credential Manager, or the Secret
Service on Linux), or in a private file if there is none.

### Score sync
The launcher reads the save file (it never writes it) when it changes, and when
the game closes. It sends the custom-chart entries that changed since the last
sync, with a hash of each chart in the song's folder, to
`POST /api/launcher/sync`:

- **Failed runs** (`cleared: false`) are never sent.
- The server matches each score to the ranked chart whose **hash** is in your
  folder, so renamed copies work. If your chart is an **older version** or an
  **edited** copy, the score isn't stored and the launcher tells you to update
  the map.
- When a map's charts change on your disk (an update, a reinstall), the save's
  existing records for it were set on the old chart. They're marked as such
  and not sent; your next play on the new chart is.
- On the very first sync, everything in the save is sent once, like a web
  import.

The hash (`ubh1`) covers the chart's `[Difficulty]`, `[TimingPoints]` and
`[HitObjects]` sections, so editing a title or tags doesn't change it. It's
defined in UNBEATEN's `docs/SCORING.md` and both implementations are tested
against the same vectors (`crates/core/tests/fixtures/chart_hash_vectors.json`).

### Maps
`GET /api/launcher/manifest` lists every ranked map with the hashes of its
current charts and of older versions. The launcher hashes your `CustomSongs`
folders to show each map as installed, needing an update, differing, or not
installed. Installing downloads the map, unpacks it into
`UNBEATEN Launcher/staging` next to `CustomSongs`, checks every chart hash, and
only then moves it into place. A folder it replaces goes to
`UNBEATEN Launcher/backups`. A renamed copy is updated where it is, so your
save keys don't change. Maps aren't installed while the game is running.

## Command line: `ubl`

The same engine without a window, also in the releases (`ubl-*`):

```
ubl detect                 where Steam, the game, the save and CustomSongs are
ubl login                  sign in (prints the code)
ubl maps [--todo]          ranked maps and their state here
ubl install <id>           install or update one map
ubl update-all             update every outdated map
ubl sync                   sync new scores once
ubl watch                  sync whenever the save changes (e.g. Steam Deck Game Mode)
ubl hash <chart.txt>...    print ubh1 hashes
```

Global options: `--server`, `--save`, `--custom-songs`, `--state-dir`,
`--token-file`. The CLI and the app share settings, sign-in and sync state.

## Development

Requirements: Rust (stable), Node 20+, and on Linux Tauri's packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libdbus-1-dev
```

```bash
npm ci
npm run tauri dev          # the app with hot reload
cargo test --workspace     # core tests (detection, hashing, saves, sync, installs)
npm run build              # type-check and build the UI
npx tauri build            # installers for this OS, in target/release/bundle/
```

To use a local UNBEATEN instead of unbeaten.cc, change the server on the
sign-in screen (or in Settings), e.g. `http://localhost:8081`.

### End-to-end check
`scripts/e2e.py` runs the real UNBEATEN backend (from a checkout of
[UNBEATEN](https://github.com/ofujiiro/UNBEATEN)) on SQLite, ranks some songs,
fakes a game install, and drives `ubl` through sign-in, installs, verified
syncs, failed runs, edited and outdated charts, an update with backup, and
watch mode with a fake game process:

```bash
cargo build -p ubl
pip install -r ../UNBEATEN/backend/requirements-dev.txt
python3 scripts/e2e.py --unbeaten ../UNBEATEN --charts scripts/e2e-fixtures/charts \
    --save scripts/e2e-fixtures/arcade-highscores.json --ubl target/debug/ubl
```

CI runs the Rust checks on Linux and Windows on every push. The end-to-end job
needs to read UNBEATEN, which is private: add a repository secret
`UNBEATEN_READ_TOKEN` (a fine-grained token with read-only access to
`ofujiiro/UNBEATEN` contents) and it runs too; without it, it's skipped.

### Releasing
Bump `version` in `Cargo.toml` (workspace) and `package.json`, then push a tag:

```bash
git tag v0.2.0 && git push origin v0.2.0
```

`.github/workflows/release.yml` builds the Windows and Linux installers and the
`ubl` binaries into a **draft** release; review it and publish. The app checks
the latest published release and shows an "update available" banner.

If a release changes something the server relies on, raise
`LAUNCHER_MIN_VERSION` on UNBEATEN: older launchers then get a "please update"
message instead of failing.

**Not set up yet** (both need secrets only you can create):
- *Signed automatic updates*: generate a key with `npx tauri signer generate`,
  add the private key and password as the `TAURI_SIGNING_PRIVATE_KEY` /
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets, put the public key
  in `tauri.conf.json` under `plugins.updater`, and add `tauri-plugin-updater`.
- *Windows code signing*, which removes the SmartScreen warning.

## Layout

```
crates/core/   unbeaten-core: detection, ubh1 hashing, save reading, API client,
               map status and installs, sync engine (no UI; unit-tested)
crates/cli/    ubl
src-tauri/     the desktop app: commands, background worker, tray
src/           the window (React + TypeScript, styled like unbeaten.cc)
scripts/       e2e.py and its fixtures
```
