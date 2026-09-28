use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Icon, ToolTip, Tray};
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy)]
pub enum TrayCommand {
    RestoreWindow,
    LaunchGame,
    KillAll,
    QuitKeepRunning,
    QuitEverything,
}

pub struct LauncherTray {
    commands: Sender<TrayCommand>,
    active_process_count: usize,
    kill_enabled: bool,
}

impl Tray for LauncherTray {
    fn id(&self) -> String {
        "gamma-launcher-rust".to_string()
    }

    fn title(&self) -> String {
        "Gamma Launcher".to_string()
    }

    fn icon_name(&self) -> String {
        "applications-games".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        Vec::new()
    }

    fn tool_tip(&self) -> ToolTip {
        let description = if self.active_process_count > 0 {
            format!("{} Zone process(es) active", self.active_process_count)
        } else {
            "No Zone process active".to_string()
        };

        ToolTip {
            title: "Gamma Launcher".to_string(),
            description,
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let open_commands = self.commands.clone();
        let launch_commands = self.commands.clone();
        let kill_commands = self.commands.clone();
        let quit_keep_commands = self.commands.clone();
        let quit_all_commands = self.commands.clone();

        vec![
            StandardItem {
                label: "Open Launcher".to_string(),
                activate: Box::new(move |_| {
                    let _ = open_commands.send(TrayCommand::RestoreWindow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Launch Game".to_string(),
                activate: Box::new(move |_| {
                    let _ = launch_commands.send(TrayCommand::LaunchGame);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!(
                    "Kill S.T.A.L.K.E.R. & MO2 ({} active)",
                    self.active_process_count
                ),
                enabled: self.kill_enabled,
                activate: Box::new(move |_| {
                    let _ = kill_commands.send(TrayCommand::KillAll);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Launcher (Keep Game Running)".to_string(),
                activate: Box::new(move |_| {
                    let _ = quit_keep_commands.send(TrayCommand::QuitKeepRunning);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit Everything".to_string(),
                activate: Box::new(move |_| {
                    let _ = quit_all_commands.send(TrayCommand::QuitEverything);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub type TrayHandle = ksni::blocking::Handle<LauncherTray>;

pub fn spawn(commands: Sender<TrayCommand>) -> Result<TrayHandle, ksni::Error> {
    let tray = LauncherTray {
        commands,
        active_process_count: 0,
        kill_enabled: false,
    };

    tray.spawn()
}

pub fn update_counts(handle: &TrayHandle, active_process_count: usize) {
    handle.update(|tray| {
        tray.active_process_count = active_process_count;
        tray.kill_enabled = active_process_count > 0;
    });
}
