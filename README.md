# gamma-launcher-rust

Native Rust port of `gamma-launcher`, with an `egui`/`eframe` desktop UI targeting
CachyOS / Arch Linux (KDE Plasma 6, Wayland or X11).

## Build dependencies

```
sudo pacman -S --needed rust pkgconf fontconfig libxkbcommon wayland \
                        libxcb libxrandr libxi libxcursor mesa
```

## Runtime dependencies

| Tool | Used for |
| --- | --- |
| `git` | GitHub mod definitions and Git resources (falls back to HTTP archives when absent) |
| `p7zip` | 7z extraction fallback and archive listing |
| `unrar` | RAR extraction |
| `umu-launcher` | Proton runtime (`umu-run`) |
| `gamemode` | optional `gamemoderun` wrapper |

```
sudo pacman -S --needed git p7zip unrar gamemode
paru -S umu-launcher
```

## Build and run

```
cargo build --release
./target/release/gamma-launcher-rust
```

The build is warning free, so it can be gated in CI:

```
RUSTFLAGS="--deny warnings" cargo build --release
```

## Tests

```
cargo test
```

## Configuration

Settings live in `config.toml` **next to the launcher executable**, never in
`$XDG_CONFIG_HOME`, so a build directory stays self contained and can be moved or
deleted as a unit. The file is written before every job and whenever a field
loses focus.

Defaults are resolved from the running system rather than hardcoded:

| Setting | Default |
| --- | --- |
| Proton build | `proton-cachyos-slr`, then any CachyOS build, then GE-Proton, then the first compatibility tool found |
| GameMode | on when `gamemoderun` is in `PATH` |
| UMU runner | on when `umu-run` is in `PATH` |
| `OMP_NUM_THREADS` | detected CPU core count |
| `DXVK_CONFIG` | compiler threads set to the core count, `maxTessFactor` capped at 8 |

## Auto-detection

`Auto-Detect Paths` does not stop at the first executable it finds. It gathers
every candidate under the configured folders, the home directory, Steam libraries
and mounted drives, then ranks them:

| Signal | Weight |
| --- | --- |
| path contains `g.a.m.m.a` | +400 |
| path contains `stalker_gamma` / `stalker-gamma` | +350 |
| path contains `gamma` | +300 |
| a whole directory is named after GAMMA | +60 |
| `.Grok's Modpack Installer` sits next to `ModOrganizer.exe` | +90 |
| complete MO2 instance layout (`mods` + `profiles`) | +60 |
| real Anomaly layout (`bin` + `gamedata`/`db`/`appdata`) | +40 |
| a GAMMA install sits in the same parent folder | +70 |
| shares a parent with the selected GAMMA install | +45 |
| inside a folder you already configured | +45 |
| renderer preference (DX11AVX > DX11 > DX10 > DX9) | +3 to +14 |
| looks like a backup, copy or archive | −120 |
| generic or temporary location | −40 |
| per path component | −2 |

Because the GAMMA tiers outweigh every other signal combined, an install such as
`/Games/STALKER_gamma` always wins over `/Games/STALKER_soufied`. The selected
candidate, its score, the reasons behind it and the top rejected runners-up are
all written to the console log. The scan runs on a background thread, so the
interface stays responsive.

## Maintenance

The dashboard groups every maintenance action by what it touches, in cards that
reflow into three, two or one column depending on the window width. Every button
carries an icon, and a hover text that states what the action does, when it
should be run and whether it can be undone.

| Group | Actions |
| --- | --- |
| Cache & Downloads | purge downloads, clear the launcher staging folders left in `TMPDIR`, prune interrupted `.part` transfers and zero byte archives, clean the compiled shader cache |
| Game & Prefix Diagnostics | terminate every Anomaly, Mod Organizer 2, `usvfs_proxy`, `umu-run` and `wineserver` process, move the wine prefix aside for a clean rebuild, verify the Anomaly install against `tools/checksums.md5`, restore the DXVK and D3D overrides, remove ReShade |
| Mods & Configuration | rebuild the mod index, synchronise `ModOrganizer.ini`, re-index the GAMMA nominated presets, repair file permissions, run the USVFS workaround, switch the in-game keymap |

The three destructive cache actions display the space they can reclaim,
measured on a background thread when the window opens and refreshed after every
job or on demand.

Process termination sends `SIGTERM` to each match, waits up to three seconds,
then escalates to `SIGKILL` for anything still alive. It stays available while a
tracked process is running, since that is exactly when it is needed. Resetting
the wine prefix never deletes anything: the old prefix is renamed to
`<name>.bak-<timestamp>` and left next to the new one. Synchronising
`ModOrganizer.ini` rewrites only the path and profile keys, translating them into
`Z:` drive paths for Wine, and preserves every other key in the file, including
Nexus credentials. `Fix File Permissions` restores `755` on directories and `644`
on files across `mods/` and `downloads/`, follows a symlinked downloads folder to
its real location, skips symlinks and reports the entries that a filesystem
without POSIX modes refused.

## Archive extraction

Archives are unpacked entry by entry instead of through the convenience
extractor of the `zip` crate, which applies the modes recorded inside the
archive to the directories it creates. A directory entry carrying `0o555`, or a
DOS read-only flag, produced a directory without the user write bit and the next
file written into it failed with `Permission denied (os error 13)`.

The extractor now guarantees, for every entry:

- parent directories are created recursively and forced to at least `0o700`
  before anything is written into them, and the whole destination tree is
  relaxed again once extraction finishes, so write access is never revoked
  half way through
- an existing target is removed before the file is opened, whether it is a
  read-only file, a symlink or a directory sitting where a file belongs
- extracted files end up at `0o644`, or `0o755` when the archive marks them
  executable, which strips the Windows read-only attribute
- `..` components are refused, absolute paths and `C:` style prefixes are
  rebased inside the destination, and backslash separators become real nested
  directories
- permission changes that the filesystem rejects, as NTFS, exFAT and FAT mounts
  do, are ignored instead of failing the extraction

The same relaxation pass runs after the 7z and RAR paths, and `copy_tree` applies
the same treatment, since copying over an existing read-only file used to fail
the same way when a mod was reinstalled.

## Process locking

While Mod Organizer 2, the Anomaly launcher or the game is running, every launch
and maintenance control is disabled and an animated badge shows which process is
alive and under which PID. A watcher thread blocks on the child process and
publishes the exit through a channel that lives for the whole session, so an exit
notification can never be dropped and the controls always re-enable themselves.
A generation counter makes sure a stale watcher can never clear the state of a
newer process.

## Networking

Downloads go through a shared HTTP client rebuilt before every networked job.
The optional SOCKS5 proxy uses the `socks5h://` scheme, so host names are
resolved by the proxy rather than locally, and supports both anonymous and
authenticated connections. Transfer speed is reported as an exponentially
smoothed rate instead of a cumulative average, so the readout reacts to real
throughput changes.
