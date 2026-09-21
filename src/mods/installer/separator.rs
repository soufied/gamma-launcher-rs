use crate::error::Result;
use crate::mods::info::ModInfo;
use crate::mods::installer::base::BaseInstaller;
use crate::report::Reporter;
use std::fs;
use std::path::Path;

const SEPARATOR_META: &str = "[General]\n\
modid=0\n\
version=\n\
newestVersion=\n\
category=0\n\
installationFile=\n\
\n\
[installedFiles]\n\
size=0\n";

#[derive(Debug, Clone)]
pub struct SeparatorInstaller {
    base: BaseInstaller,
}

impl SeparatorInstaller {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            base: BaseInstaller::new(ModInfo::separator(name)),
        }
    }

    pub fn base(&self) -> &BaseInstaller {
        &self.base
    }

    pub fn base_mut(&mut self) -> &mut BaseInstaller {
        &mut self.base
    }

    pub fn into_base(self) -> BaseInstaller {
        self.base
    }

    pub fn info(&self) -> &ModInfo {
        self.base.info()
    }

    pub fn install(&self, to: &Path, reporter: &Reporter) -> Result<()> {
        let install_dir = to.join(&self.base.info().name);

        reporter.info(format!(
            "[+] Installing separator: {}",
            self.base.info().name
        ));
        fs::create_dir_all(&install_dir)?;
        fs::write(install_dir.join("meta.ini"), SEPARATOR_META)?;

        Ok(())
    }
}
