use std::{fs, path::Path};

use anyhow::{Context, Result};
use protocol::PROTOCOL_VERSION;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionCompatibility {
    Legacy,
    Enhanced,
    Any,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResourceEntrypoints {
    pub server: Option<String>,
    pub client: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResourceDependency {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResourceManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub entrypoints: ResourceEntrypoints,
    pub dependencies: Vec<ResourceDependency>,
    pub tags: Vec<String>,
    pub protocol_version: u32,
    pub edition_compatibility: EditionCompatibility,
}

pub fn load_manifest_from_path(path: impl AsRef<Path>) -> Result<ResourceManifest> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read resource manifest: {}", path.display()))?;
    parse_manifest_toml(&raw)
}

pub fn parse_manifest_toml(input: &str) -> Result<ResourceManifest> {
    let manifest: ResourceManifest =
        toml::from_str(input).context("failed to parse manifest TOML")?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &ResourceManifest) -> Result<()> {
    anyhow::ensure!(
        !manifest.name.trim().is_empty(),
        "resource name cannot be empty"
    );
    anyhow::ensure!(
        !manifest.version.trim().is_empty(),
        "resource version cannot be empty"
    );
    anyhow::ensure!(
        is_valid_resource_name(&manifest.name),
        "resource name contains invalid characters"
    );
    anyhow::ensure!(
        manifest.protocol_version == PROTOCOL_VERSION,
        "resource protocol_version must match current protocol version"
    );

    if let Some(server) = &manifest.entrypoints.server {
        validate_entrypoint_path(server)?;
    }

    if let Some(client) = &manifest.entrypoints.client {
        validate_entrypoint_path(client)?;
    }

    for dependency in &manifest.dependencies {
        anyhow::ensure!(
            is_valid_resource_name(&dependency.name),
            "dependency name contains invalid characters"
        );
    }

    Ok(())
}

fn validate_entrypoint_path(path: &str) -> Result<()> {
    let candidate = Path::new(path);
    anyhow::ensure!(!candidate.is_absolute(), "entrypoint path must be relative");
    anyhow::ensure!(
        !candidate
            .components()
            .any(|component| { matches!(component, std::path::Component::ParentDir) }),
        "entrypoint path cannot contain parent-directory traversal"
    );
    Ok(())
}

fn is_valid_resource_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_manifest() {
        let manifest = parse_manifest_toml(valid_manifest()).unwrap();

        assert_eq!(manifest.name, "chat");
        assert_eq!(manifest.edition_compatibility, EditionCompatibility::Any);
    }

    #[test]
    fn reject_empty_name() {
        let err = parse_manifest_toml(&valid_manifest().replace("name = \"chat\"", "name = \"\""))
            .unwrap_err();
        assert!(err.to_string().contains("resource name cannot be empty"));
    }

    #[test]
    fn reject_invalid_name() {
        let err =
            parse_manifest_toml(&valid_manifest().replace("name = \"chat\"", "name = \"Chat!\""))
                .unwrap_err();
        assert!(err
            .to_string()
            .contains("resource name contains invalid characters"));
    }

    #[test]
    fn reject_absolute_entrypoint_path() {
        let err = parse_manifest_toml(&valid_manifest().replace(
            "server = \"server/main.js\"",
            "server = \"/tmp/server/main.js\"",
        ))
        .unwrap_err();
        assert!(err.to_string().contains("entrypoint path must be relative"));
    }

    #[test]
    fn reject_parent_directory_traversal_path() {
        let err = parse_manifest_toml(&valid_manifest().replace(
            "client = \"client/main.js\"",
            "client = \"../client/main.js\"",
        ))
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("entrypoint path cannot contain parent-directory traversal"));
    }

    #[test]
    fn reject_invalid_dependency_name() {
        let err = parse_manifest_toml(
            &valid_manifest().replace("name = \"core_ui\"", "name = \"Core UI\""),
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("dependency name contains invalid characters"));
    }

    #[test]
    fn validate_edition_compatibility_values() {
        let manifest = parse_manifest_toml(&valid_manifest().replace(
            "edition_compatibility = \"any\"",
            "edition_compatibility = \"enhanced\"",
        ))
        .unwrap();
        assert_eq!(
            manifest.edition_compatibility,
            EditionCompatibility::Enhanced
        );
    }

    fn valid_manifest() -> &'static str {
        r#"name = "chat"
version = "0.1.0"
description = "Local chat resource"
authors = ["MeowV Team"]
license = "MIT"
tags = ["chat", "example"]
protocol_version = 1
edition_compatibility = "any"

[entrypoints]
server = "server/main.js"
client = "client/main.js"

[[dependencies]]
name = "core_ui"
"#
    }
}
