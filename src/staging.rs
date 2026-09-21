use crate::config::FOLDERS_TO_INSTALL;
use crate::error::Result;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn extract_with_hotfixes<F>(prefix: &str, extract_fn: F) -> Result<TempDir>
where
    F: FnOnce(&Path) -> Result<()>,
{
    let dir = tempfile::Builder::new().prefix(prefix).tempdir()?;
    extract_fn(dir.path())?;

    if !cfg!(windows) {
        fix_malformed_archive(dir.path())?;
        fix_path_case(dir.path())?;
    }

    Ok(dir)
}

fn fix_malformed_archive(dir: &Path) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.contains('\\') {
            continue;
        }

        if entry.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
            continue;
        }

        let relative = name.replace('\\', "/");
        let target = dir.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&path, &target)?;
    }

    Ok(())
}

fn fix_path_case(root: &Path) -> Result<()> {
    let mut mismatched = Vec::new();
    collect_mismatched_dirs(root, &mut mismatched)?;

    for path in mismatched {
        let parent = match path.parent() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        for file in collect_files(&path) {
            let relative = match file.strip_prefix(&parent) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let relative_parent = relative.parent().unwrap_or_else(|| Path::new(""));
            let lowered = relative_parent.to_string_lossy().to_lowercase();
            let new_folder = parent.join(&lowered);

            fs::create_dir_all(&new_folder)?;

            if let Some(file_name) = file.file_name() {
                fs::rename(&file, new_folder.join(file_name))?;
            }
        }
    }

    Ok(())
}

fn collect_mismatched_dirs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        let lowered = name.to_lowercase();

        if FOLDERS_TO_INSTALL.contains(&lowered.as_str()) && name != lowered {
            out.push(path.clone());
        }

        collect_mismatched_dirs(&path, out)?;
    }

    Ok(())
}

fn collect_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect()
}
