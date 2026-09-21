use crate::error::Result;
use crate::report::Reporter;
use std::fs;
use std::path::Path;

const RESHADE_FILES: [&str; 7] = [
    "d3d9.dll",
    "dxgi.dll",
    "dxgi.log",
    "G.A.M.M.A.Reshade.ini",
    "ReShade.ini",
    "ReShade.log",
    "reshade-shaders",
];

pub struct PurgeShaderCache;

impl PurgeShaderCache {
    pub fn run(anomaly: &Path, reporter: &Reporter) -> Result<()> {
        let cache = anomaly.join("appdata").join("shaders_cache");

        if !cache.is_dir() {
            reporter.info("[*] No shader cache to purge");
            return Ok(());
        }

        reporter.info(format!("[+] Purging shader cache in {}", cache.display()));
        fs::remove_dir_all(&cache)?;
        Ok(())
    }
}

pub struct RemoveReshade;

impl RemoveReshade {
    pub fn run(anomaly: &Path, reporter: &Reporter) -> Result<()> {
        reporter.info("[+] Removing ReShade from the Anomaly bin directory");

        for name in RESHADE_FILES {
            let file = anomaly.join("bin").join(name);

            if !file.exists() {
                continue;
            }

            reporter.info(format!("  - Removing {}", file.display()));
            if file.is_dir() {
                fs::remove_dir_all(&file)?;
            } else {
                fs::remove_file(&file)?;
            }
        }

        PurgeShaderCache::run(anomaly, reporter)
    }
}
