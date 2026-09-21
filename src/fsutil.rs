use crate::error::Result;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const DIRECTORY_MODE: u32 = 0o755;
pub const FILE_MODE: u32 = 0o644;
pub const EXECUTABLE_MODE: u32 = 0o755;

const MINIMUM_DIRECTORY_BITS: u32 = 0o700;
const MINIMUM_FILE_BITS: u32 = 0o600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOutcome {
    Unchanged,
    Changed,
    Rejected,
}

#[cfg(unix)]
fn permission_bits(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    fs::symlink_metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o7777)
}

#[cfg(unix)]
pub fn raise_permissions(path: &Path, bits: u32) {
    use std::os::unix::fs::PermissionsExt;

    let current = match permission_bits(path) {
        Some(current) => current,
        None => return,
    };

    if current & bits == bits {
        return;
    }

    let _ = fs::set_permissions(path, fs::Permissions::from_mode(current | bits));
}

#[cfg(unix)]
pub fn force_permissions(path: &Path, mode: u32) -> PermissionOutcome {
    use std::os::unix::fs::PermissionsExt;

    if permission_bits(path) == Some(mode) {
        return PermissionOutcome::Unchanged;
    }

    match fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        Ok(()) => PermissionOutcome::Changed,
        Err(_) => PermissionOutcome::Rejected,
    }
}

#[cfg(not(unix))]
pub fn raise_permissions(_path: &Path, _bits: u32) {}

#[cfg(not(unix))]
pub fn force_permissions(_path: &Path, _mode: u32) -> PermissionOutcome {
    PermissionOutcome::Unchanged
}

pub fn ensure_directory(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }

    if path.is_dir() {
        raise_permissions(path, MINIMUM_DIRECTORY_BITS);
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        ensure_directory(parent)?;
    }

    if path.is_symlink() || path.exists() {
        fs::remove_file(path)?;
    }

    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }

    raise_permissions(path, MINIMUM_DIRECTORY_BITS);
    Ok(())
}

pub fn prepare_file_target(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_directory(parent)?;
    }

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    if metadata.is_dir() {
        relax_tree_permissions(path);
        fs::remove_dir_all(path)?;
        return Ok(());
    }

    raise_permissions(path, MINIMUM_FILE_BITS);
    fs::remove_file(path)?;
    Ok(())
}

pub fn apply_archive_file_mode(path: &Path, archived_mode: Option<u32>) {
    let executable = archived_mode
        .map(|mode| mode & 0o111 != 0)
        .unwrap_or(false);

    let mode = if executable {
        EXECUTABLE_MODE
    } else {
        FILE_MODE
    };

    let _ = force_permissions(path, mode);
}

pub fn relax_tree_permissions(root: &Path) {
    if !root.exists() && !root.is_symlink() {
        return;
    }

    raise_permissions(root, MINIMUM_DIRECTORY_BITS);

    for entry in WalkDir::new(root).into_iter().filter_map(|entry| entry.ok()) {
        let file_type = entry.file_type();

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            raise_permissions(entry.path(), MINIMUM_DIRECTORY_BITS);
        } else {
            raise_permissions(entry.path(), MINIMUM_FILE_BITS);
        }
    }
}

pub fn directory_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

pub fn entry_size(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => directory_size(path),
        Ok(metadata) => metadata.len(),
        Err(_) => 0,
    }
}

pub fn remove_path(path: &Path) -> Result<u64> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };

    if metadata.is_dir() {
        let size = directory_size(path);
        relax_tree_permissions(path);
        fs::remove_dir_all(path)?;
        return Ok(size);
    }

    if let Some(parent) = path.parent() {
        raise_permissions(parent, MINIMUM_DIRECTORY_BITS);
    }

    let size = metadata.len();
    fs::remove_file(path)?;
    Ok(size)
}

pub fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    ensure_directory(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_symlink() {
            copy_symlink(&entry.path(), &target)?;
        } else {
            prepare_file_target(&target)?;
            fs::copy(entry.path(), &target)?;
            raise_permissions(&target, MINIMUM_FILE_BITS);
        }
    }

    Ok(())
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    if let Ok(link_target) = fs::read_link(src) {
        let _ = fs::remove_file(dst);
        std::os::unix::fs::symlink(&link_target, dst)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    fs::copy(src, dst)?;
    Ok(())
}

pub fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&byte| byte as char).collect(),
    };
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

pub fn which(binary: &str) -> Option<PathBuf> {
    if binary.is_empty() {
        return None;
    }

    let direct = Path::new(binary);
    if direct.components().count() > 1 && direct.is_file() {
        return Some(direct.to_path_buf());
    }

    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

pub fn tool_available(binary: &str) -> bool {
    which(binary).is_some()
}

pub fn directory_has_entries(path: &Path) -> bool {
    match fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_some(),
        Err(_) => false,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_read_only_directory_becomes_writable_again() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("locked").join("deeper");

        ensure_directory(&nested).unwrap();
        let _ = force_permissions(&nested, 0o500);
        ensure_directory(&nested).unwrap();

        let file = nested.join("payload.txt");
        prepare_file_target(&file).unwrap();
        fs::write(&file, b"ok").unwrap();

        assert_eq!(fs::read(&file).unwrap(), b"ok");
    }

    #[test]
    fn an_existing_read_only_file_is_replaced_instead_of_refused() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("meta.ini");

        fs::write(&file, b"old").unwrap();
        let _ = force_permissions(&file, 0o444);

        prepare_file_target(&file).unwrap();
        fs::write(&file, b"new").unwrap();

        assert_eq!(fs::read(&file).unwrap(), b"new");
    }

    #[test]
    fn a_file_blocking_a_directory_path_is_cleared() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("gamedata");

        fs::write(&blocker, b"not a directory").unwrap();
        ensure_directory(&blocker).unwrap();

        assert!(blocker.is_dir());
    }

    #[test]
    fn archive_modes_never_drop_the_user_write_bit() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("locked.dds");

        fs::write(&file, b"payload").unwrap();
        apply_archive_file_mode(&file, Some(0o444));

        assert_eq!(permission_bits(&file), Some(FILE_MODE));
    }

    #[test]
    fn archive_modes_keep_the_executable_bit() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("tool.sh");

        fs::write(&file, b"payload").unwrap();
        apply_archive_file_mode(&file, Some(0o555));

        assert_eq!(permission_bits(&file), Some(EXECUTABLE_MODE));
    }

    #[test]
    fn removing_a_read_only_tree_succeeds() {
        let root = tempfile::tempdir().unwrap();
        let tree = root.path().join("mods");

        ensure_directory(&tree.join("inner")).unwrap();
        fs::write(tree.join("inner").join("a.txt"), b"1234").unwrap();
        let _ = force_permissions(&tree.join("inner"), 0o500);

        assert_eq!(remove_path(&tree).unwrap(), 4);
        assert!(!tree.exists());
    }
}
