# Gamma Launcher Rust

**A native Linux installer, updater, and launcher for the S.T.A.L.K.E.R. Anomaly / G.A.M.M.A. modpack.**

Built purely to make the installation and setup hassle-free, so anyone can easily install and jump straight into the Zone.

> ⚠️ **Notice:** Steam integration is currently **non-functional** (launching via Spacewar AppID 480 could not be successfully implemented).

---

## Overview

[S.T.A.L.K.E.R. G.A.M.M.A.](https://github.com/Grokitach/gamma_setup) is a modpack for **S.T.A.L.K.E.R. Anomaly**. Officially, installation requires Windows-only PowerShell/Python scripts.

**Gamma Launcher Rust** is a standalone, single-binary Linux application that handles the entire G.A.M.M.A. setup, patching, mod verification, and launching via **Wine, Proton, or UMU** natively. This project is based on [Mord3rca/gamma-launcher](https://github.com/Mord3rca/gamma-launcher), rewritten and reimplemented in Rust.

### Architecture
1. **Installer & Updater Pipeline** (`src/commands/`, `src/mods/`, `src/archive.rs`, `src/config.rs`) — downloads Anomaly 1.5.3, parses G.A.M.M.A. mod manifests, downloads individual mods (ModDB, GitHub, direct mirrors), verifies checksums, extracts archives (`.zip`, `.7z`, `.rar`), patches the base game, and provisions MO2 profiles.
2. **Runtime & Launcher** (`src/runner.rs`, ~~`src/steam_identity.rs`~~, `src/process.rs`, `src/mo2.rs`, `src/tray.rs`) — runs MO2 or the game through Wine/Proton/[UMU](https://github.com/Open-Wine-Components/umu-launcher), configures DXVK/sync variables, ~~spoofs Steam "Spacewar" (AppID 480) identity for multiplayer~~, monitors `/proc` to manage game processes, and integrates with the system tray.

---

## Supported Platforms

Linux-only (relies directly on `/proc` and Linux-specific tooling).

| Platform | Support | Notes |
|---|---|---|
| **Arch Linux / CachyOS / Manjaro** | ✅ Primary | Defaults tuned for CachyOS and native package layout. |
| **Debian / Ubuntu / Pop!_OS** | ⚠️ Untested | Expected to work. Requires manual package installation (`wine`, `umu-launcher`, etc.). |
| **Fedora / openSUSE** | ⚠️ Untested | Expected to work. Install distribution equivalents for build and runtime tools. |
| **Windows** | ❌ Unsupported | Use official launcher. |
| **macOS** | ❌ Unsupported | Not planned. |

---

## Key Features

### 🧩 Installer & Modpack Pipeline
- **`AnomalyInstall`** — Downloads and extracts Anomaly 1.5.3 from ModDB with MD5 verification.
- **`GammaSetup`** — Sets up directory structures, pulls Mod Organizer 2 releases, and fetches installer definitions.
- **`FullInstall`** — Orchestrates the full process: patches Anomaly, downloads/unpacks all mods, configures MO2 profiles, and copies addons.
- **Download Management** — Handles ModDB scraping, GitHub releases, and direct HTTP downloads with retries, resume support, and SOCKS5 proxy routing.
- **Archive Extraction** — Extracts `.zip` and `.7z` natively; delegates `.rar` to external `unrar`.

### 🎮 Runtime & Launching
- **Flexible Execution** — Launches MO2, the Anomaly Launcher, or the game binary directly (prioritizing DX11 AVX builds), with headless MO2 shortcut support.
- **Tuning & Optimization** — Manages DLL overrides, DXVK configurations, GameMode, MangoHud, and Fsync/Esync.
- ~~**Steam "Spacewar" Spoofing (`src/steam_identity.rs`)** — Generates `steam_appid.txt` (AppID `480`), sets Goldberg emulator configs, and syncs nicknames into `user.ltx`.~~
- **Process Management (`src/process.rs`)** — Tracks game and Wine processes via `/proc` with graceful termination support.
- **System Tray (`src/tray.rs`)** — Tray icon (`ksni`) for quick launch, status checks, and process shutdown.

### 🛠️ Maintenance Tools
- Disk cleanup for downloads, partial files, and graphics shader caches.
- Wine prefix reset and DXVK cache cleanup.
- POSIX permission fixing across the installation tree.
- USVFS flattening into a standalone directory.

### 🖥️ GUI (`src/ui/`)
Built with `egui`/`eframe` and driven by Tokio async background jobs:
- **Dashboard** — One-click launch, update, and install actions with live log streaming.
- **Paths** — Configured and auto-detected directories with status indicators.
- **Runtime** — Runner selection (Wine/Proton/UMU), GameMode, MangoHud, and sync flags.
- **Tweaks** — Git revision targeting, proxy configuration, and ~~Steam Spacewar settings~~.

---

## Dependencies & Installation

### Requirements
- **Rust 1.75+** and standard build tools (`gcc`/`clang`, `pkg-config`, `openssl`).
- **Runtime tools:** `wine` (or Proton), `unrar`, and optionally `umu-launcher`, `gamemode`, `mangohud`.

#### Arch Linux / CachyOS
```bash
# Build tools & runtime
sudo pacman -S --needed base-devel rustup wine wine-mono wine-gecko unrar \
                        libxkbcommon wayland libxcursor libxrandr libxi

# Optional integrations
sudo pacman -S --needed gamemode lib32-gamemode mangohud lib32-mangohud
paru -S umu-launcher-git
# sudo pacman -S --needed steam
```

#### Debian / Ubuntu
```bash
sudo apt update
sudo apt install -y build-essential curl pkg-config libssl-dev wine wine64 \
                    unrar libxkbcommon-dev libwayland-dev libxcursor-dev libxi-dev
```

### Building

```bash
git clone https://github.com/soufied/gamma-launcher-rs.git gamma-launcher-rust
cd gamma-launcher-rust
cargo build --release
```

The compiled binary will be located at `target/release/gamma-launcher-rust`.

---

## Configuration (`config.toml`)

Generated automatically on first run alongside the executable:

```toml
anomaly_path = "/path/to/S.T.A.L.K.E.R. Anomaly"
gamma_path = "/path/to/S.T.A.L.K.E.R. Gamma"
cache_path = "/tmp"
custom_gamma_repository = "Grokitach/Stalker_GAMMA"
update_gamma_definition = true
patch_anomaly = true
anomaly_verify = true

[runner]
proton_path = "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr"
wine_prefix = "$HOME/.local/share/wineprefixes/stalker_anomaly_gamma"
use_umu = true
use_gamemode = true
wine_dll_overrides_enabled = true

[proxy]
enabled = false
host = ""
port = 1080

# [spacewar]
# steam_spacewar_mode = false
# player_nickname = "Stalker"

[tray]
minimize_to_tray = true
close_action = "Exit"
```

---

## Credits & Acknowledgements

This project is based on and inspired by the original work in [gamma-launcher](https://github.com/Mord3rca/gamma-launcher) by **Mord3rca**, rewritten and expanded natively in Rust.

---

## License

GPL-3.0-only. See `Cargo.toml`.
