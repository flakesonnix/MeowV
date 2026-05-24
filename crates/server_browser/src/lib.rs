use std::{fs, path::Path};

use anyhow::{Context, Result};
use protocol::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionCompatibility {
    Legacy,
    Enhanced,
    Any,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub current_players: u32,
    pub max_players: u32,
    pub protocol_version: u32,
    pub tags: Vec<String>,
    pub edition_compatibility: EditionCompatibility,
}

pub trait ServerListSource {
    fn load(&self) -> Result<Vec<ServerEntry>>;
}

#[derive(Debug, Clone)]
pub struct LocalJsonServerListSource {
    path: std::path::PathBuf,
}

impl LocalJsonServerListSource {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl ServerListSource for LocalJsonServerListSource {
    fn load(&self) -> Result<Vec<ServerEntry>> {
        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read server list: {}", self.path.display()))?;
        let entries: Vec<ServerEntry> =
            serde_json::from_str(&raw).context("failed to parse server list JSON")?;
        validate_entries(&entries)?;
        Ok(entries)
    }
}

pub fn filter_by_protocol_version(entries: &[ServerEntry], version: u32) -> Vec<ServerEntry> {
    entries
        .iter()
        .filter(|entry| entry.protocol_version == version)
        .cloned()
        .collect()
}

pub fn filter_by_edition(
    entries: &[ServerEntry],
    edition: EditionCompatibility,
) -> Vec<ServerEntry> {
    entries
        .iter()
        .filter(|entry| match edition {
            EditionCompatibility::Legacy => matches!(
                entry.edition_compatibility,
                EditionCompatibility::Legacy | EditionCompatibility::Any
            ),
            EditionCompatibility::Enhanced => matches!(
                entry.edition_compatibility,
                EditionCompatibility::Enhanced | EditionCompatibility::Any
            ),
            EditionCompatibility::Any => true,
            EditionCompatibility::Unknown => matches!(
                entry.edition_compatibility,
                EditionCompatibility::Unknown | EditionCompatibility::Any
            ),
        })
        .cloned()
        .collect()
}

pub fn filter_current_protocol(entries: &[ServerEntry]) -> Vec<ServerEntry> {
    filter_by_protocol_version(entries, PROTOCOL_VERSION)
}

fn validate_entries(entries: &[ServerEntry]) -> Result<()> {
    for entry in entries {
        anyhow::ensure!(
            !entry.name.trim().is_empty(),
            "server entry name cannot be empty"
        );
        anyhow::ensure!(
            !entry.address.trim().is_empty(),
            "server entry address cannot be empty"
        );
        anyhow::ensure!(
            entry.current_players <= entry.max_players,
            "server entry current_players exceeds max_players"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn parse_valid_server_list() {
        let path = write_temp_file(
            "valid-server-list.json",
            r#"[
  {
    "name": "Local Test",
    "address": "127.0.0.1",
    "port": 7000,
    "current_players": 4,
    "max_players": 32,
    "protocol_version": 1,
    "tags": ["dev", "local"],
    "edition_compatibility": "any"
  }
]"#,
        );

        let entries = LocalJsonServerListSource::new(&path).load().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Local Test");
    }

    #[test]
    fn reject_invalid_server_list() {
        let path = write_temp_file(
            "invalid-server-list.json",
            r#"[
  {
    "name": "Broken",
    "address": "127.0.0.1",
    "port": 7000,
    "current_players": 40,
    "max_players": 32,
    "protocol_version": 1,
    "tags": [],
    "edition_compatibility": "legacy"
  }
]"#,
        );

        let err = LocalJsonServerListSource::new(&path).load().unwrap_err();
        assert!(
            err.to_string()
                .contains("current_players exceeds max_players")
        );
    }

    #[test]
    fn filter_by_protocol_version_returns_matching_entries() {
        let entries = sample_entries();
        let filtered = filter_by_protocol_version(&entries, 1);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|entry| entry.protocol_version == 1));
    }

    #[test]
    fn filter_by_edition_returns_compatible_entries() {
        let entries = sample_entries();
        let filtered = filter_by_edition(&entries, EditionCompatibility::Enhanced);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|entry| entry.name == "Enhanced Test"));
        assert!(filtered.iter().any(|entry| entry.name == "Any Test"));
    }

    fn sample_entries() -> Vec<ServerEntry> {
        vec![
            ServerEntry {
                name: "Legacy Test".to_string(),
                address: "127.0.0.1".to_string(),
                port: 7000,
                current_players: 1,
                max_players: 32,
                protocol_version: 1,
                tags: vec!["legacy".to_string()],
                edition_compatibility: EditionCompatibility::Legacy,
            },
            ServerEntry {
                name: "Enhanced Test".to_string(),
                address: "127.0.0.1".to_string(),
                port: 7001,
                current_players: 2,
                max_players: 32,
                protocol_version: 2,
                tags: vec!["enhanced".to_string()],
                edition_compatibility: EditionCompatibility::Enhanced,
            },
            ServerEntry {
                name: "Any Test".to_string(),
                address: "127.0.0.1".to_string(),
                port: 7002,
                current_players: 3,
                max_players: 32,
                protocol_version: 1,
                tags: vec!["any".to_string()],
                edition_compatibility: EditionCompatibility::Any,
            },
        ]
    }

    fn write_temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("meowv-{unique}-{name}"));
        fs::write(&path, contents).unwrap();
        path
    }
}
