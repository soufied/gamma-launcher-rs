mod archive;
mod commands;
mod config;
mod detect;
mod error;
mod fsutil;
mod hash;
mod mods;
mod report;
mod runner;
mod staging;
mod ui;
mod userltx;

fn main() -> eframe::Result<()> {
    ui::run()
}
