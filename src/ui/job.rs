use crate::commands::{
    maintenance, AnomalyInstall, CheckAnomaly, CheckMd5, CommandArgs, FullInstall, GammaSetup,
    KeymapLayout, PurgeShaderCache, RemoveReshade, SwitchKeymap, TestModMaker, Usvfs,
};
use crate::config::AppConfig;
use crate::error::{LauncherError, Result};
use crate::mods::downloader::base::configure_http_client;
use crate::process::SharedProcessRegistry;
use crate::report::{Reporter, TaskEvent};
use crate::runner::{self, JobHandle, LaunchTarget, ProcessState, Waker};
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    FullInstall,
    AnomalyInstall,
    GammaSetup,
    CheckMd5,
    CheckAnomaly,
    PurgeShaderCache,
    RemoveReshade,
    SwitchKeymap(KeymapLayout),
    UsvfsWorkaround,
    TestModMaker,
    PurgeDownloads,
    ClearTempCache,
    PruneIncompleteDownloads,
    KillProcesses,
    ResetWinePrefix,
    ResetGraphicsState,
    RebuildModCache,
    SyncModOrganizerIni,
    RepairModOrganizerPaths,
    ReindexPresets,
    FixPermissions,
    Launch(LaunchTarget),
}

impl Job {
    pub fn label(&self) -> String {
        match self {
            Job::FullInstall => "Sync / Update".to_string(),
            Job::AnomalyInstall => "Anomaly install".to_string(),
            Job::GammaSetup => "GAMMA setup".to_string(),
            Job::CheckMd5 => "MD5 check".to_string(),
            Job::CheckAnomaly => "Anomaly verification".to_string(),
            Job::PurgeShaderCache => "Shader cache clean".to_string(),
            Job::RemoveReshade => "ReShade removal".to_string(),
            Job::SwitchKeymap(layout) => format!("Keymap switch to {}", layout.label()),
            Job::UsvfsWorkaround => "USVFS workaround".to_string(),
            Job::TestModMaker => "Mod maker test".to_string(),
            Job::PurgeDownloads => "Purge downloads".to_string(),
            Job::ClearTempCache => "Temp cache clean".to_string(),
            Job::PruneIncompleteDownloads => "Incomplete download prune".to_string(),
            Job::KillProcesses => "Process termination".to_string(),
            Job::ResetWinePrefix => "Wine prefix reset".to_string(),
            Job::ResetGraphicsState => "DXVK and D3D reset".to_string(),
            Job::RebuildModCache => "Mod cache rebuild".to_string(),
            Job::SyncModOrganizerIni => "ModOrganizer.ini sync".to_string(),
            Job::RepairModOrganizerPaths => "ModOrganizer.ini path repair".to_string(),
            Job::ReindexPresets => "Preset re-index".to_string(),
            Job::FixPermissions => "Permission repair".to_string(),
            Job::Launch(target) => format!("Launch {}", target.label()),
        }
    }

    pub fn needs_network(&self) -> bool {
        matches!(
            self,
            Job::FullInstall | Job::AnomalyInstall | Job::GammaSetup | Job::CheckMd5
        )
    }
}

fn repair_mod_organizer_paths(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
    let repaired = maintenance::sanitize_mod_organizer_ini(args, reporter)?;

    if repaired == 0 {
        reporter.info("[+] Every Wine path in ModOrganizer.ini is already valid, nothing to repair");
    }

    Ok(())
}

async fn execute(
    job: Job,
    config: AppConfig,
    reporter: &Reporter,
    process_state: &ProcessState,
    adopted_processes: &SharedProcessRegistry,
    bg_reporter: Reporter,
    wake: Waker,
) -> Result<()> {
    let args = CommandArgs::from_config(&config);

    match job {
        Job::FullInstall => FullInstall::run(&args, reporter).await,
        Job::AnomalyInstall => AnomalyInstall::run(&args, reporter).await,
        Job::GammaSetup => GammaSetup::run(&args, reporter).await,
        Job::CheckMd5 => CheckMd5::run(&args, reporter).await,
        Job::CheckAnomaly => CheckAnomaly::run(&args.anomaly_dir()?, reporter),
        Job::PurgeShaderCache => PurgeShaderCache::run(&args.anomaly_dir()?, reporter),
        Job::RemoveReshade => RemoveReshade::run(&args.anomaly_dir()?, reporter),
        Job::SwitchKeymap(layout) => SwitchKeymap::run(&args.anomaly_dir()?, layout, reporter),
        Job::UsvfsWorkaround => Usvfs::run(&args, reporter),
        Job::TestModMaker => TestModMaker::run(&args, reporter),
        Job::PurgeDownloads => maintenance::purge_downloads(&args, reporter),
        Job::ClearTempCache => maintenance::clear_temp_cache(reporter),
        Job::PruneIncompleteDownloads => maintenance::prune_incomplete_downloads(&args, reporter),
        Job::KillProcesses => maintenance::kill_game_processes(reporter, adopted_processes),
        Job::ResetWinePrefix => maintenance::reset_wine_prefix(&config, reporter),
        Job::ResetGraphicsState => maintenance::reset_graphics_state(&args, &config, reporter),
        Job::RebuildModCache => maintenance::rebuild_mod_cache(&args, reporter),
        Job::SyncModOrganizerIni => maintenance::sync_mod_organizer_ini(&args, &config, reporter),
        Job::RepairModOrganizerPaths => repair_mod_organizer_paths(&args, reporter),
        Job::ReindexPresets => maintenance::reindex_presets(&args, reporter),
        Job::FixPermissions => maintenance::fix_permissions(&args, reporter),
        Job::Launch(target) => runner::launch(
            &config,
            target,
            reporter,
            process_state,
            adopted_processes,
            bg_reporter,
            wake,
        ),
    }
}

fn clean_partial_downloads(config: &AppConfig, reporter: &Reporter) {
    let args = CommandArgs::from_config(config);

    if args.downloads.is_none() {
        return;
    }

    reporter.info("[*] Removing the partial transfers left behind by the cancelled job");

    if let Err(error) = maintenance::prune_incomplete_downloads(&args, reporter) {
        reporter.warn(format!("[!] Could not prune the partial transfers: {error}"));
    }
}

pub fn spawn(
    job: Job,
    config: AppConfig,
    process_state: ProcessState,
    adopted_processes: SharedProcessRegistry,
    bg_reporter: Reporter,
    control: JobHandle,
    ctx: egui::Context,
) -> Receiver<TaskEvent> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let cleanup_config = config.clone();
        let reporter = Reporter::new(sender.clone(), control.clone());
        let cleanup_reporter = Reporter::detached(sender);
        let wake_ctx = ctx.clone();
        let wake = Waker::new(move || wake_ctx.request_repaint());
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                reporter.error(format!("Could not start the async runtime: {error}"));
                control.finish();
                reporter.job_finished(false);
                ctx.request_repaint();
                return;
            }
        };

        reporter.info(format!("[+] Starting: {}", job.label()));

        if job.needs_network() {
            if let Err(error) = configure_http_client(&config.proxy, &reporter) {
                reporter.error(format!("Could not build the HTTP client: {error}"));
                control.finish();
                reporter.job_finished(false);
                ctx.request_repaint();
                return;
            }
        }

        let outcome = runtime.block_on(execute(
            job,
            config,
            &reporter,
            &process_state,
            &adopted_processes,
            bg_reporter,
            wake,
        ));

        match &outcome {
            Ok(()) => reporter.info(format!("[+] Finished: {}", job.label())),
            Err(LauncherError::Cancelled) => {
                cleanup_reporter.warn(format!("[!] Cancelled: {}", job.label()));
                clean_partial_downloads(&cleanup_config, &cleanup_reporter);
            }
            Err(error) => reporter.error(error.to_string()),
        }

        control.finish();
        cleanup_reporter.job_finished(outcome.is_ok());
        ctx.request_repaint();
    });

    receiver
}
