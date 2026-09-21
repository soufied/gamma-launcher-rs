use crate::error::{LauncherError, Result};
use crate::fsutil;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const DIRECTORY_MODE_MASK: u32 = 0o170000;
const DIRECTORY_MODE_FLAG: u32 = 0o040000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Rar,
    SevenZip,
}

pub fn detect_kind(path: &Path) -> Result<ArchiveKind> {
    let mut f = File::open(path)?;
    let mut buf = [0u8; 16];
    let n = f.read(&mut buf)?;
    let d = &buf[..n];

    if d.len() >= 4 && &d[..4] == b"PK\x03\x04" {
        return Ok(ArchiveKind::Zip);
    }
    if d.len() >= 3 && &d[..3] == b"Rar" {
        return Ok(ArchiveKind::Rar);
    }
    if d.len() >= 6 && d[..6] == [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
        return Ok(ArchiveKind::SevenZip);
    }

    Err(LauncherError::UnknownArchiveFormat(path.to_path_buf()))
}

fn extraction_failed(archive: &Path, message: impl Into<String>) -> LauncherError {
    LauncherError::ExtractionFailed {
        archive: archive.to_path_buf(),
        message: message.into(),
    }
}

fn run_external(tool: &str, args: &[&str], archive: &Path) -> Result<std::process::Output> {
    Command::new(tool).args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            LauncherError::CommandFailed {
                command: tool.to_string(),
                message: format!("{tool} is not installed or not in PATH"),
            }
        } else {
            extraction_failed(archive, e.to_string())
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryTarget {
    Path(PathBuf),
    Empty,
    Unsafe,
}

fn entry_target(name: &str) -> EntryTarget {
    let mut relative = PathBuf::new();

    for component in name.split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." || component.contains('\0') {
            return EntryTarget::Unsafe;
        }
        if relative.as_os_str().is_empty() && component.ends_with(':') {
            continue;
        }
        relative.push(component);
    }

    if relative.as_os_str().is_empty() {
        EntryTarget::Empty
    } else {
        EntryTarget::Path(relative)
    }
}

fn is_directory_entry(name: &str, mode: Option<u32>) -> bool {
    if name.ends_with('/') || name.ends_with('\\') {
        return true;
    }

    mode.map(|mode| mode & DIRECTORY_MODE_MASK == DIRECTORY_MODE_FLAG)
        .unwrap_or(false)
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| extraction_failed(archive, error.to_string()))?;

    fsutil::ensure_directory(dest)?;

    let mut skipped: Vec<String> = Vec::new();

    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| extraction_failed(archive, error.to_string()))?;

        let name = entry.name().to_string();
        let mode = entry.unix_mode();

        let relative = match entry_target(&name) {
            EntryTarget::Path(relative) => relative,
            EntryTarget::Empty => continue,
            EntryTarget::Unsafe => {
                skipped.push(name);
                continue;
            }
        };

        let target = dest.join(&relative);

        if is_directory_entry(&name, mode) {
            fsutil::ensure_directory(&target)?;
            continue;
        }

        fsutil::prepare_file_target(&target)?;

        let mut writer = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&target)
            .map_err(|error| {
                extraction_failed(archive, format!("{}: {error}", target.display()))
            })?;

        std::io::copy(&mut entry, &mut writer).map_err(|error| {
            extraction_failed(archive, format!("{}: {error}", target.display()))
        })?;

        drop(writer);
        fsutil::apply_archive_file_mode(&target, mode);
    }

    if !skipped.is_empty() {
        return Err(extraction_failed(
            archive,
            format!(
                "{} entry/entries escape the destination directory and were refused: {}",
                skipped.len(),
                skipped.join(", ")
            ),
        ));
    }

    Ok(())
}

fn extract_sevenzip(archive: &Path, dest: &Path) -> Result<()> {
    fsutil::ensure_directory(dest)?;

    if sevenz_rust2::decompress_file(archive, dest).is_ok() {
        return Ok(());
    }

    fsutil::relax_tree_permissions(dest);

    let dest_arg = format!("-o{}", dest.display());
    let archive_str = archive.to_string_lossy().to_string();
    let output = run_external("7z", &["x", "-y", &dest_arg, &archive_str], archive)?;

    if !output.status.success() {
        return Err(extraction_failed(
            archive,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

fn extract_rar(archive: &Path, dest: &Path) -> Result<()> {
    fsutil::ensure_directory(dest)?;

    let archive_str = archive.to_string_lossy().to_string();
    let dest_str = format!("{}{}", dest.to_string_lossy(), std::path::MAIN_SEPARATOR);

    if let Ok(output) = run_external("unrar", &["x", "-y", "-o+", &archive_str, &dest_str], archive)
    {
        if output.status.success() {
            return Ok(());
        }
    }

    fsutil::relax_tree_permissions(dest);

    let dest_arg = format!("-o{}", dest.display());
    let output = run_external("7z", &["x", "-y", &dest_arg, &archive_str], archive)?;

    if !output.status.success() {
        return Err(extraction_failed(
            archive,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

pub fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    fsutil::ensure_directory(dest)?;

    let outcome = match detect_kind(archive)? {
        ArchiveKind::Zip => extract_zip(archive, dest),
        ArchiveKind::SevenZip => extract_sevenzip(archive, dest),
        ArchiveKind::Rar => extract_rar(archive, dest),
    };

    fsutil::relax_tree_permissions(dest);
    outcome
}

pub fn list_archive_content(archive: &Path) -> Result<Vec<String>> {
    match detect_kind(archive)? {
        ArchiveKind::Zip => {
            let file = File::open(archive)?;
            let mut zip = zip::ZipArchive::new(file)
                .map_err(|error| extraction_failed(archive, error.to_string()))?;

            let mut names = Vec::with_capacity(zip.len());
            for i in 0..zip.len() {
                if let Ok(entry) = zip.by_index(i) {
                    names.push(entry.name().to_string());
                }
            }
            Ok(names)
        }
        ArchiveKind::SevenZip => {
            let output = run_external("7z", &["l", "-ba", &archive.to_string_lossy()], archive)?;
            let text = String::from_utf8_lossy(&output.stdout);
            Ok(text
                .lines()
                .filter_map(|l| l.split_whitespace().last().map(|s| s.to_string()))
                .collect())
        }
        ArchiveKind::Rar => {
            let output = run_external("unrar", &["lb", &archive.to_string_lossy()], archive)?;
            let text = String::from_utf8_lossy(&output.stdout);
            Ok(text.lines().map(|l| l.to_string()).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_separators_become_nested_directories() {
        assert_eq!(
            entry_target("gamedata\\meshes\\actor.ogf"),
            EntryTarget::Path(PathBuf::from("gamedata/meshes/actor.ogf"))
        );
    }

    #[test]
    fn traversal_entries_are_refused() {
        assert_eq!(entry_target("../../etc/passwd"), EntryTarget::Unsafe);
        assert_eq!(entry_target("gamedata/../../escape.ltx"), EntryTarget::Unsafe);
    }

    #[test]
    fn absolute_and_drive_prefixed_entries_are_rebased() {
        assert_eq!(
            entry_target("/gamedata/configs/system.ltx"),
            EntryTarget::Path(PathBuf::from("gamedata/configs/system.ltx"))
        );
        assert_eq!(
            entry_target("C:\\gamedata\\weapons.ltx"),
            EntryTarget::Path(PathBuf::from("gamedata/weapons.ltx"))
        );
    }

    #[test]
    fn empty_entries_are_ignored_without_failing_the_archive() {
        assert_eq!(entry_target("./"), EntryTarget::Empty);
        assert_eq!(entry_target("/"), EntryTarget::Empty);
    }

    #[test]
    fn directory_entries_are_recognised_by_name_and_by_mode() {
        assert!(is_directory_entry("gamedata/", None));
        assert!(is_directory_entry("gamedata\\", None));
        assert!(is_directory_entry("gamedata", Some(0o040555)));
        assert!(!is_directory_entry("gamedata/a.ltx", Some(0o100444)));
    }
}
