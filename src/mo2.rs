use crate::error::{LauncherError, Result};
use crate::userltx::UserLtx;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Mo2Profile {
    pub name: String,
    pub path: PathBuf,
}

impl Mo2Profile {
    pub fn user_ltx_path(&self) -> PathBuf {
        self.path.join("user.ltx")
    }
}

pub fn parse_selected_profile(ini_text: &str) -> Option<String> {
    let mut in_general = false;

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_general = line.eq_ignore_ascii_case("[General]");
            continue;
        }

        if !in_general {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        if !key.trim().eq_ignore_ascii_case("selected_profile") {
            continue;
        }

        return Some(decode_ini_value(value.trim()));
    }

    None
}

fn decode_ini_value(value: &str) -> String {
    let trimmed = value.trim();

    if let Some(inner) = trimmed
        .strip_prefix("@ByteArray(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return inner.to_string();
    }

    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }

    trimmed.to_string()
}

pub fn read_selected_profile_name(mo2_path: &Path) -> Result<String> {
    let ini_path = mo2_path.join("ModOrganizer.ini");

    let text = std::fs::read_to_string(&ini_path).map_err(|error| {
        LauncherError::RunnerConfig(format!(
            "could not read {}: {}",
            ini_path.display(),
            error
        ))
    })?;

    parse_selected_profile(&text).ok_or_else(|| {
        LauncherError::RunnerConfig(format!(
            "{} does not contain [General] -> selected_profile",
            ini_path.display()
        ))
    })
}

pub fn resolve_active_profile(mo2_path: &Path) -> Result<Mo2Profile> {
    let name = read_selected_profile_name(mo2_path)?;
    let path = mo2_path.join("profiles").join(&name);

    if !path.is_dir() {
        return Err(LauncherError::RunnerConfig(format!(
            "the active MO2 profile directory does not exist: {}",
            path.display()
        )));
    }

    Ok(Mo2Profile { name, path })
}

pub fn load_active_profile_user_ltx(mo2_path: &Path) -> Result<(Mo2Profile, UserLtx)> {
    let profile = resolve_active_profile(mo2_path)?;
    let ltx_path = profile.user_ltx_path();

    let ltx = if ltx_path.is_file() {
        UserLtx::open(&ltx_path)?
    } else {
        if let Some(parent) = ltx_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&ltx_path, "")?;
        UserLtx::open(&ltx_path)?
    };

    Ok((profile, ltx))
}

pub fn sync_identity_into_active_profile(
    mo2_path: &Path,
    persona_name: &str,
    network_player_name: &str,
) -> Result<Mo2Profile> {
    let (profile, mut ltx) = load_active_profile_user_ltx(mo2_path)?;

    if !persona_name.trim().is_empty() {
        ltx.set("cl_name", persona_name.trim());
    }

    if !network_player_name.trim().is_empty() {
        ltx.set("net_player_name", network_player_name.trim());
    }

    ltx.save(Some(&profile.user_ltx_path()))?;

    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_raw_selected_profile() {
        let ini = "[General]\nselected_profile=GAMMA\ngamma=true\n";
        assert_eq!(parse_selected_profile(ini).as_deref(), Some("GAMMA"));
    }

    #[test]
    fn parses_bytearray_selected_profile() {
        let ini = "[General]\r\nselected_profile=@ByteArray(GAMMA)\r\n";
        assert_eq!(parse_selected_profile(ini).as_deref(), Some("GAMMA"));
    }

    #[test]
    fn parses_quoted_selected_profile() {
        let ini = "[General]\nselected_profile=\"My Profile\"\n";
        assert_eq!(parse_selected_profile(ini).as_deref(), Some("My Profile"));
    }

    #[test]
    fn ignores_selected_profile_outside_general() {
        let ini = "[Settings]\nselected_profile=WrongSection\n[General]\nselected_profile=Correct\n";
        assert_eq!(parse_selected_profile(ini).as_deref(), Some("Correct"));
    }

    #[test]
    fn returns_none_when_missing() {
        let ini = "[General]\nother_key=value\n";
        assert_eq!(parse_selected_profile(ini), None);
    }

    #[test]
    fn resolves_and_syncs_active_profile() {
        let dir = std::env::temp_dir().join(format!("mo2-test-{}", std::process::id()));
        let profiles_dir = dir.join("profiles").join("GAMMA");
        fs::create_dir_all(&profiles_dir).unwrap();
        fs::write(
            dir.join("ModOrganizer.ini"),
            "[General]\nselected_profile=@ByteArray(GAMMA)\n",
        )
        .unwrap();
        fs::write(profiles_dir.join("user.ltx"), "cl_name old\r\n").unwrap();

        let profile = sync_identity_into_active_profile(&dir, "Marked One", "Marked One").unwrap();
        assert_eq!(profile.name, "GAMMA");

        let saved = fs::read_to_string(profiles_dir.join("user.ltx")).unwrap();
        assert!(saved.contains("cl_name Marked One"));
        assert!(saved.contains("net_player_name Marked One"));

        fs::remove_dir_all(&dir).ok();
    }
}
