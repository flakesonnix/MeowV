use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEdition {
    Legacy,
    Enhanced,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePlatform {
    Windows,
    Linux,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBuildInfo {
    pub edition: GameEdition,
    pub executable_path: Option<PathBuf>,
    pub version_string: Option<String>,
    pub platform: GamePlatform,
}

pub fn detect_from_path(path: impl AsRef<Path>) -> GameBuildInfo {
    let path = path.as_ref();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase());

    let edition = match file_name.as_deref() {
        Some(name) if is_enhanced_like_name(name) => GameEdition::Enhanced,
        Some(name) if is_legacy_like_name(name) => GameEdition::Legacy,
        _ => GameEdition::Unknown,
    };

    GameBuildInfo {
        edition,
        executable_path: Some(path.to_path_buf()),
        version_string: None,
        platform: detect_platform(),
    }
}

pub fn detect_installed_game() -> GameBuildInfo {
    // TODO: add legal, filesystem-only install discovery methods after review.
    // Keep this conservative until per-platform discovery rules are documented.
    GameBuildInfo {
        edition: GameEdition::Unknown,
        executable_path: None,
        version_string: None,
        platform: detect_platform(),
    }
}

fn is_enhanced_like_name(name: &str) -> bool {
    matches!(name, "gta5_enhanced.exe" | "gtav_enhanced.exe")
}

fn is_legacy_like_name(name: &str) -> bool {
    matches!(name, "gta5.exe" | "gtav.exe")
}

fn detect_platform() -> GamePlatform {
    if cfg!(target_os = "windows") {
        GamePlatform::Windows
    } else if cfg!(target_os = "linux") {
        GamePlatform::Linux
    } else {
        GamePlatform::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_path_returns_unknown_edition() {
        let info = detect_from_path("/tmp/not-gta.bin");

        assert_eq!(info.edition, GameEdition::Unknown);
        assert_eq!(info.version_string, None);
        assert_eq!(
            info.executable_path,
            Some(PathBuf::from("/tmp/not-gta.bin"))
        );
    }

    #[test]
    fn enhanced_like_name_returns_enhanced() {
        let info = detect_from_path("C:/Games/GTAV_Enhanced.exe");

        assert_eq!(info.edition, GameEdition::Enhanced);
    }

    #[test]
    fn legacy_like_name_returns_legacy() {
        let info = detect_from_path("C:/Games/GTA5.exe");

        assert_eq!(info.edition, GameEdition::Legacy);
    }

    #[test]
    fn platform_detection_matches_target() {
        let info = detect_installed_game();

        if cfg!(target_os = "windows") {
            assert_eq!(info.platform, GamePlatform::Windows);
        } else if cfg!(target_os = "linux") {
            assert_eq!(info.platform, GamePlatform::Linux);
        } else {
            assert_eq!(info.platform, GamePlatform::Unknown);
        }
    }
}
