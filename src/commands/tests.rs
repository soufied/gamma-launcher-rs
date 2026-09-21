use crate::commands::CommandArgs;
use crate::error::{LauncherError, Result};
use crate::mods::{modpack_data_dir, read_mod_maker};
use crate::report::Reporter;

pub struct TestModMaker;

impl TestModMaker {
    pub fn run(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
        let gamma_dir = args.gamma_dir()?;
        let entries = read_mod_maker(&modpack_data_dir(&gamma_dir), reporter)?;

        let mut errors = 0;
        for entry in &entries {
            let info = entry.info();
            let subdirs = match &info.subdirs {
                Some(subdirs) => subdirs,
                None => continue,
            };

            for subdir in subdirs {
                if subdir.trim().is_empty() {
                    reporter.warn(format!("{}: empty subdirs directive", info.name));
                    errors += 1;
                }
            }
        }

        reporter.info(format!(
            "[+] Verified {} mod definition(s), {errors} issue(s) found",
            entries.len()
        ));

        if errors == 0 {
            Ok(())
        } else {
            Err(LauncherError::Other(format!(
                "{errors} malformed subdirs directive(s) detected"
            )))
        }
    }
}
