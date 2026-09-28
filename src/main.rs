mod archive;
mod commands;
mod config;
mod detect;
mod diagnostics;
mod error;
mod fsutil;
mod hash;
mod mo2;
mod mods;
mod process;
mod report;
mod runner;
mod staging;
mod steam_identity;
mod tray;
mod ui;
mod userltx;

fn main() -> eframe::Result<()> {
    ui::run()
}
