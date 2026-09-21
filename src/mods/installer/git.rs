use crate::error::Result;
use crate::fsutil::copy_tree;
use crate::mods::info::ModInfo;
use crate::mods::installer::base::BaseInstaller;
use crate::report::Reporter;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct GitResourceInstaller {
    base: BaseInstaller,
    find_gamedata: bool,
}

impl GitResourceInstaller {
    pub fn new(info: ModInfo, find_gamedata: bool) -> Self {
        Self {
            base: BaseInstaller::new(info),
            find_gamedata,
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

    pub fn gamedata_iterator(root: &Path) -> Vec<PathBuf> {
        WalkDir::new(root)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_dir() && entry.file_name().to_string_lossy() == "gamedata")
            .map(|entry| entry.into_path())
            .collect()
    }

    pub fn toplevel_dir_iterator(root: &Path) -> Result<Vec<PathBuf>> {
        let mut dirs = Vec::new();
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
        Ok(dirs)
    }

    pub fn install(&self, to: &Path, reporter: &Reporter) -> Result<()> {
        reporter.info(format!(
            "[+] Installing Git Resource mod: {}",
            self.base.info().url
        ));
        fs::create_dir_all(to)?;

        let staged = tempfile::Builder::new()
            .prefix("gamma-launcher-modinstall-")
            .tempdir()?;
        self.base.extract(staged.path())?;

        let sources = if self.find_gamedata {
            Self::gamedata_iterator(staged.path())
        } else {
            Self::toplevel_dir_iterator(staged.path())?
        };

        for source in sources {
            let name = match source.file_name() {
                Some(name) => name.to_os_string(),
                None => continue,
            };
            copy_tree(&source, &to.join(name))?;
        }

        Ok(())
    }
}
