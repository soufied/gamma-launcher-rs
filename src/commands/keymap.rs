use crate::error::{LauncherError, Result};
use crate::report::Reporter;
use crate::userltx::UserLtx;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeymapLayout {
    #[default]
    Azerty,
    Dvorak,
}

impl KeymapLayout {
    pub fn label(&self) -> &'static str {
        match self {
            KeymapLayout::Azerty => "AZERTY",
            KeymapLayout::Dvorak => "DVORAK",
        }
    }
}

pub struct SwitchKeymap;

impl SwitchKeymap {
    pub fn run(anomaly: &Path, layout: KeymapLayout, reporter: &Reporter) -> Result<()> {
        let user_config = anomaly.join("appdata").join("user.ltx");
        let mut config = UserLtx::open(user_config.as_path())?;

        if config.bind().get("forward") != Some("kW") {
            return Err(LauncherError::Other(
                "user.ltx does not seem to be in QWERTY ... Aborting".to_string(),
            ));
        }

        reporter.info(format!(
            "[+] Switching user.ltx keymap to the {} layout",
            layout.label()
        ));

        match layout {
            KeymapLayout::Azerty => config.bind().to_azerty_layout(),
            KeymapLayout::Dvorak => config.bind().to_dvorak_layout(),
        }

        config.save(None)
    }
}
