use crate::commands::CommandArgs;
use crate::error::Result;
use crate::fsutil::{copy_tree, read_text};
use crate::report::Reporter;
use std::fs;
use std::path::Path;

pub struct Usvfs;

impl Usvfs {
    fn read_modlist(modlist: &Path) -> Result<Vec<String>> {
        let text = read_text(modlist)?;

        let mut mods: Vec<String> = text
            .split('\n')
            .filter(|line| line.starts_with('+'))
            .map(|line| line.trim_start_matches('+').trim().to_string())
            .collect();

        mods.reverse();
        Ok(mods)
    }

    pub fn run(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
        let gamma_dir = args.gamma_dir()?;
        let anomaly_dir = args.anomaly_dir()?;
        let install_dir = args.final_dir()?;

        fs::create_dir_all(&install_dir)?;

        reporter.info("[+] Copying the Anomaly directory to the install directory...");
        copy_tree(&anomaly_dir, &install_dir)?;

        reporter.info("[+] Applying mods...");
        let modlist = gamma_dir
            .join("profiles")
            .join("G.A.M.M.A")
            .join("modlist.txt");

        let mods = Self::read_modlist(&modlist)?;
        let total = mods.len();

        for (index, name) in mods.iter().enumerate() {
            reporter.overall_progress(index, total, name);
            reporter.info(format!("  Installing {name}"));

            if let Err(error) = copy_tree(&gamma_dir.join("mods").join(name), &install_dir) {
                reporter.warn(format!("    --> Failed: {error}"));
            }
        }
        reporter.overall_progress(total, total, "Mods applied");

        reporter.info("[+] Reapplying the Anomaly binary directory to the install directory");
        copy_tree(&anomaly_dir.join("bin"), &install_dir.join("bin"))
    }
}
