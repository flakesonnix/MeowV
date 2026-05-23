use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use protocol::PROTOCOL_VERSION;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;

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
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    pub entrypoints: ResourceEntrypoints,
    #[serde(default)]
    pub dependencies: Vec<ResourceDependency>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub protocol_version: u32,
    pub edition_compatibility: EditionCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFileEntry {
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePackIndex {
    pub manifest: ResourceManifest,
    pub files: Vec<ResourceFileEntry>,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheFileStatus {
    Valid,
    Missing,
    SizeMismatch,
    HashMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheVerificationEntry {
    pub relative_path: PathBuf,
    pub expected_size_bytes: u64,
    pub actual_size_bytes: Option<u64>,
    pub expected_sha256: String,
    pub actual_sha256: Option<String>,
    pub status: CacheFileStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheVerificationReport {
    pub entries: Vec<CacheVerificationEntry>,
    pub valid_count: usize,
    pub missing_count: usize,
    pub size_mismatch_count: usize,
    pub hash_mismatch_count: usize,
    pub is_fully_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredResource {
    pub name: String,
    pub root_dir: PathBuf,
    pub manifest: ResourceManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRegistry {
    pub resources: BTreeMap<String, RegisteredResource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyResolutionError {
    MissingDependency {
        resource: String,
        dependency: String,
    },
    DependencyCycle {
        resources: Vec<String>,
    },
    DuplicateResource {
        name: String,
    },
    InvalidResource {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for DependencyResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDependency {
                resource,
                dependency,
            } => write!(
                f,
                "MissingDependency: resource '{}' depends on missing '{}'",
                resource, dependency
            ),
            Self::DependencyCycle { resources } => {
                write!(f, "DependencyCycle: {}", resources.join(", "))
            }
            Self::DuplicateResource { name } => {
                write!(f, "DuplicateResource: '{}'", name)
            }
            Self::InvalidResource { path, reason } => {
                write!(f, "InvalidResource: {} ({})", path.display(), reason)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOrder {
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRuntimePhase {
    Planned,
    Validated,
    Ready,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceEntrypointKind {
    Server,
    Client,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEntrypoint {
    pub kind: ResourceEntrypointKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedResource {
    pub name: String,
    pub root_dir: PathBuf,
    pub phase: ResourceRuntimePhase,
    pub entrypoints: Vec<PlannedEntrypoint>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLoadPlan {
    pub resources: Vec<PlannedResource>,
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

pub fn build_pack_index(resource_dir: impl AsRef<Path>) -> Result<ResourcePackIndex> {
    let resource_dir = resource_dir.as_ref();
    let manifest_path = resource_dir.join("resource.toml");
    anyhow::ensure!(
        manifest_path.is_file(),
        "resource.toml not found in resource directory"
    );

    let manifest = load_manifest_from_path(&manifest_path)?;
    let mut files = Vec::new();

    collect_files(resource_dir, resource_dir, &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let total_size_bytes = files.iter().map(|file| file.size_bytes).sum();

    Ok(ResourcePackIndex {
        manifest,
        files,
        total_size_bytes,
    })
}

pub fn hash_file_sha256(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open file for hashing: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read file for hashing: {}", path.display()))?;
        if read == 0 {
            break;
        }

        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn verify_cache_against_index(
    index: &ResourcePackIndex,
    cache_dir: impl AsRef<Path>,
) -> Result<CacheVerificationReport> {
    let cache_dir = cache_dir.as_ref();
    let mut entries = Vec::with_capacity(index.files.len());
    let mut valid_count = 0;
    let mut missing_count = 0;
    let mut size_mismatch_count = 0;
    let mut hash_mismatch_count = 0;

    for file in &index.files {
        validate_resource_file_path(&file.relative_path)?;

        let cache_path = cache_dir.join(&file.relative_path);
        let status_entry = match fs::symlink_metadata(&cache_path) {
            Ok(metadata) => {
                anyhow::ensure!(
                    !metadata.file_type().is_symlink(),
                    "symlinks are not supported in cache verification"
                );

                if !metadata.is_file() {
                    missing_count += 1;
                    CacheVerificationEntry {
                        relative_path: file.relative_path.clone(),
                        expected_size_bytes: file.size_bytes,
                        actual_size_bytes: None,
                        expected_sha256: file.sha256.clone(),
                        actual_sha256: None,
                        status: CacheFileStatus::Missing,
                    }
                } else {
                    let actual_size = metadata.len();
                    if actual_size != file.size_bytes {
                        size_mismatch_count += 1;
                        CacheVerificationEntry {
                            relative_path: file.relative_path.clone(),
                            expected_size_bytes: file.size_bytes,
                            actual_size_bytes: Some(actual_size),
                            expected_sha256: file.sha256.clone(),
                            actual_sha256: None,
                            status: CacheFileStatus::SizeMismatch,
                        }
                    } else {
                        let actual_sha256 = hash_file_sha256(&cache_path)?;
                        if actual_sha256 != file.sha256 {
                            hash_mismatch_count += 1;
                            CacheVerificationEntry {
                                relative_path: file.relative_path.clone(),
                                expected_size_bytes: file.size_bytes,
                                actual_size_bytes: Some(actual_size),
                                expected_sha256: file.sha256.clone(),
                                actual_sha256: Some(actual_sha256),
                                status: CacheFileStatus::HashMismatch,
                            }
                        } else {
                            valid_count += 1;
                            CacheVerificationEntry {
                                relative_path: file.relative_path.clone(),
                                expected_size_bytes: file.size_bytes,
                                actual_size_bytes: Some(actual_size),
                                expected_sha256: file.sha256.clone(),
                                actual_sha256: Some(actual_sha256),
                                status: CacheFileStatus::Valid,
                            }
                        }
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                missing_count += 1;
                CacheVerificationEntry {
                    relative_path: file.relative_path.clone(),
                    expected_size_bytes: file.size_bytes,
                    actual_size_bytes: None,
                    expected_sha256: file.sha256.clone(),
                    actual_sha256: None,
                    status: CacheFileStatus::Missing,
                }
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to stat cache file: {}", cache_path.display())
                });
            }
        };

        entries.push(status_entry);
    }

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(CacheVerificationReport {
        entries,
        valid_count,
        missing_count,
        size_mismatch_count,
        hash_mismatch_count,
        is_fully_valid: missing_count == 0 && size_mismatch_count == 0 && hash_mismatch_count == 0,
    })
}

pub fn verify_cache_for_resource(
    resource_dir: impl AsRef<Path>,
    cache_dir: impl AsRef<Path>,
) -> Result<CacheVerificationReport> {
    let index = build_pack_index(resource_dir)?;
    verify_cache_against_index(&index, cache_dir)
}

pub fn discover_resources(root_dir: impl AsRef<Path>) -> Result<ResourceRegistry> {
    let root_dir = root_dir.as_ref();
    let mut resources = BTreeMap::new();

    for entry in fs::read_dir(root_dir)
        .with_context(|| format!("failed to read resources root: {}", root_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).with_context(|| {
            format!(
                "failed to read resource candidate metadata: {}",
                path.display()
            )
        })?;

        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "symlinks are not supported in resource discovery"
        );

        if !metadata.is_dir() {
            continue;
        }

        let manifest_path = path.join("resource.toml");
        if !manifest_path.is_file() {
            continue;
        }

        let manifest = load_manifest_from_path(&manifest_path).map_err(|err| {
            anyhow::anyhow!(DependencyResolutionError::InvalidResource {
                path: path.clone(),
                reason: err.to_string(),
            })
        })?;

        let folder_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                anyhow::anyhow!(DependencyResolutionError::InvalidResource {
                    path: path.clone(),
                    reason: "resource folder name is not valid UTF-8".to_string(),
                })
            })?;

        if manifest.name != folder_name {
            return Err(anyhow::anyhow!(
                DependencyResolutionError::InvalidResource {
                    path: path.clone(),
                    reason: format!(
                        "resource folder name '{}' does not match manifest name '{}'",
                        folder_name, manifest.name
                    ),
                }
            ));
        }

        insert_registered_resource(&mut resources, path, manifest)?;
    }

    Ok(ResourceRegistry { resources })
}

pub fn resolve_load_order(registry: &ResourceRegistry) -> Result<LoadOrder> {
    validate_registry(registry)?;

    let mut indegree: BTreeMap<String, usize> = registry
        .resources
        .keys()
        .map(|name| (name.clone(), 0_usize))
        .collect();
    let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (name, resource) in &registry.resources {
        for dependency in &resource.manifest.dependencies {
            *indegree.get_mut(name).expect("resource must exist") += 1;
            dependents
                .entry(dependency.name.clone())
                .or_default()
                .insert(name.clone());
        }
    }

    let mut ready: VecDeque<String> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(name, _)| name.clone())
        .collect();
    let mut ordered = Vec::with_capacity(registry.resources.len());

    while let Some(name) = ready.pop_front() {
        ordered.push(name.clone());

        if let Some(children) = dependents.get(&name) {
            for child in children {
                let degree = indegree.get_mut(child).expect("dependent must exist");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(child.clone());
                }
            }
        }
    }

    if ordered.len() != registry.resources.len() {
        let cycle_resources = indegree
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .map(|(name, _)| name)
            .collect();
        return Err(anyhow::anyhow!(
            DependencyResolutionError::DependencyCycle {
                resources: cycle_resources,
            }
        ));
    }

    Ok(LoadOrder { resources: ordered })
}

pub fn validate_registry(registry: &ResourceRegistry) -> Result<()> {
    for (name, resource) in &registry.resources {
        for dependency in &resource.manifest.dependencies {
            if !registry.resources.contains_key(&dependency.name) {
                return Err(anyhow::anyhow!(
                    DependencyResolutionError::MissingDependency {
                        resource: name.clone(),
                        dependency: dependency.name.clone(),
                    }
                ));
            }
        }
    }

    Ok(())
}

pub fn build_load_plan(
    registry: &ResourceRegistry,
    load_order: &LoadOrder,
) -> Result<ResourceLoadPlan> {
    let mut resources = Vec::with_capacity(load_order.resources.len());

    for name in &load_order.resources {
        let resource = registry
            .resources
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("load order references missing resource: {name}"))?;
        let mut entrypoints = Vec::new();

        if let Some(server) = &resource.manifest.entrypoints.server {
            entrypoints.push(PlannedEntrypoint {
                kind: ResourceEntrypointKind::Server,
                path: PathBuf::from(server),
            });
        }

        if let Some(client) = &resource.manifest.entrypoints.client {
            entrypoints.push(PlannedEntrypoint {
                kind: ResourceEntrypointKind::Client,
                path: PathBuf::from(client),
            });
        }

        resources.push(PlannedResource {
            name: resource.name.clone(),
            root_dir: resource.root_dir.clone(),
            phase: ResourceRuntimePhase::Planned,
            entrypoints,
            dependencies: resource
                .manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.name.clone())
                .collect(),
        });
    }

    Ok(ResourceLoadPlan { resources })
}

pub fn build_load_plan_from_root(root_dir: impl AsRef<Path>) -> Result<ResourceLoadPlan> {
    let registry = discover_resources(root_dir)?;
    let load_order = resolve_load_order(&registry)?;
    build_load_plan(&registry, &load_order)
}

fn insert_registered_resource(
    resources: &mut BTreeMap<String, RegisteredResource>,
    root_dir: PathBuf,
    manifest: ResourceManifest,
) -> Result<()> {
    if resources.contains_key(&manifest.name) {
        return Err(anyhow::anyhow!(
            DependencyResolutionError::DuplicateResource {
                name: manifest.name.clone(),
            }
        ));
    }

    resources.insert(
        manifest.name.clone(),
        RegisteredResource {
            name: manifest.name.clone(),
            root_dir,
            manifest,
        },
    );

    Ok(())
}

pub fn validate_resource_file_path(path: &Path) -> Result<()> {
    anyhow::ensure!(!path.is_absolute(), "resource file path must be relative");
    anyhow::ensure!(
        !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir)),
        "resource file path cannot contain parent-directory traversal"
    );
    Ok(())
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
            .any(|component| matches!(component, std::path::Component::ParentDir)),
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

fn collect_files(
    resource_root: &Path,
    current_dir: &Path,
    files: &mut Vec<ResourceFileEntry>,
) -> Result<()> {
    for entry in fs::read_dir(current_dir).with_context(|| {
        format!(
            "failed to read resource directory: {}",
            current_dir.display()
        )
    })? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to read metadata: {}", path.display()))?;

        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "symlinks are not supported in resource packs"
        );

        if metadata.is_dir() {
            collect_files(resource_root, &path, files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(resource_root)
            .with_context(|| format!("failed to build relative path: {}", path.display()))?
            .to_path_buf();
        validate_resource_file_path(&relative_path)?;

        files.push(ResourceFileEntry {
            relative_path,
            size_bytes: metadata.len(),
            sha256: hash_file_sha256(&path)?,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

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

    #[test]
    fn index_valid_resource_directory() {
        let dir = create_resource_dir(
            valid_manifest(),
            &[(&PathBuf::from("server/main.lua"), "print('server')")],
        );

        let index = build_pack_index(&dir).unwrap();
        assert_eq!(index.manifest.name, "chat");
        assert_eq!(index.files.len(), 2);
    }

    #[test]
    fn hash_known_file_content() {
        let path = write_temp_file("hash.txt", "abc");
        let hash = hash_file_sha256(&path).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn missing_resource_toml_returns_error() {
        let dir = unique_temp_dir("missing-manifest");
        fs::create_dir_all(&dir).unwrap();

        let err = build_pack_index(&dir).unwrap_err();
        assert!(err.to_string().contains("resource.toml not found"));
    }

    #[test]
    fn invalid_manifest_returns_error() {
        let dir = create_resource_dir(
            valid_manifest()
                .replace("name = \"chat\"", "name = \"Chat!\"")
                .as_str(),
            &[(&PathBuf::from("server/main.lua"), "print('server')")],
        );

        let err = build_pack_index(&dir).unwrap_err();
        assert!(err
            .to_string()
            .contains("resource name contains invalid characters"));
    }

    #[test]
    fn total_size_calculation_is_correct() {
        let dir = create_resource_dir(
            valid_manifest(),
            &[
                (&PathBuf::from("server/main.lua"), "abc"),
                (&PathBuf::from("client/main.lua"), "hello"),
            ],
        );

        let index = build_pack_index(&dir).unwrap();
        let expected: u64 = index.files.iter().map(|file| file.size_bytes).sum();
        assert_eq!(index.total_size_bytes, expected);
    }

    #[test]
    fn deterministic_sorting_orders_by_relative_path() {
        let dir = create_resource_dir(
            valid_manifest(),
            &[
                (&PathBuf::from("zeta.lua"), "z"),
                (&PathBuf::from("alpha.lua"), "a"),
            ],
        );

        let index = build_pack_index(&dir).unwrap();
        let paths: Vec<_> = index
            .files
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("alpha.lua"),
                PathBuf::from("resource.toml"),
                PathBuf::from("zeta.lua")
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn reject_symlink_if_platform_supports_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = create_resource_dir(valid_manifest(), &[]);
        let target = dir.join("resource.toml");
        let link = dir.join("linked.toml");
        symlink(&target, &link).unwrap();

        let err = build_pack_index(&dir).unwrap_err();
        assert!(err.to_string().contains("symlinks are not supported"));
    }

    #[test]
    fn reject_absolute_resource_file_path() {
        let err = validate_resource_file_path(Path::new("/tmp/file.txt")).unwrap_err();
        assert!(err
            .to_string()
            .contains("resource file path must be relative"));
    }

    #[test]
    fn reject_parent_traversal_resource_file_path() {
        let err = validate_resource_file_path(Path::new("../file.txt")).unwrap_err();
        assert!(err
            .to_string()
            .contains("resource file path cannot contain parent-directory traversal"));
    }

    #[test]
    fn accept_normal_relative_resource_file_path() {
        validate_resource_file_path(Path::new("server/main.lua")).unwrap();
    }

    #[test]
    fn fully_valid_cache() {
        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[
                (&PathBuf::from("server/main.lua"), "print('server')"),
                (&PathBuf::from("client/main.lua"), "print('client')"),
            ],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-valid");

        let report = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap();
        assert!(report.is_fully_valid);
        assert_eq!(report.valid_count, report.entries.len());
        assert_eq!(report.missing_count, 0);
        assert_eq!(report.size_mismatch_count, 0);
        assert_eq!(report.hash_mismatch_count, 0);
    }

    #[test]
    fn missing_file_is_reported() {
        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[
                (&PathBuf::from("server/main.lua"), "print('server')"),
                (&PathBuf::from("client/main.lua"), "print('client')"),
            ],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-missing");
        fs::remove_file(cache_dir.join("client/main.lua")).unwrap();

        let report = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap();
        assert_eq!(report.missing_count, 1);
        assert!(!report.is_fully_valid);
    }

    #[test]
    fn size_mismatch_is_reported() {
        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[(&PathBuf::from("server/main.lua"), "print('server')")],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-size-mismatch");
        fs::write(cache_dir.join("server/main.lua"), "xx").unwrap();

        let report = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap();
        assert_eq!(report.size_mismatch_count, 1);
        assert!(!report.is_fully_valid);
    }

    #[test]
    fn hash_mismatch_is_reported() {
        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[(&PathBuf::from("server/main.lua"), "abc")],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-hash-mismatch");
        fs::write(cache_dir.join("server/main.lua"), "xyz").unwrap();

        let report = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap();
        assert_eq!(report.hash_mismatch_count, 1);
        assert!(!report.is_fully_valid);
    }

    #[cfg(unix)]
    #[test]
    fn reject_symlink_cache_entry_if_platform_supports_symlinks() {
        use std::os::unix::fs::symlink;

        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[(&PathBuf::from("server/main.lua"), "print('server')")],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-symlink");
        let target = cache_dir.join("server/main.lua");
        let link = cache_dir.join("resource.toml");
        fs::remove_file(&link).unwrap();
        symlink(&target, &link).unwrap();

        let err = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap_err();
        assert!(err
            .to_string()
            .contains("symlinks are not supported in cache verification"));
    }

    #[test]
    fn deterministic_report_ordering() {
        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[
                (&PathBuf::from("zeta.lua"), "z"),
                (&PathBuf::from("alpha.lua"), "a"),
            ],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-ordering");

        let report = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap();
        let paths: Vec<_> = report
            .entries
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("alpha.lua"),
                PathBuf::from("resource.toml"),
                PathBuf::from("zeta.lua")
            ]
        );
    }

    #[test]
    fn counts_are_correct() {
        let resource_dir = create_resource_dir(
            valid_manifest(),
            &[
                (&PathBuf::from("server/main.lua"), "abc"),
                (&PathBuf::from("client/main.lua"), "print('client')"),
            ],
        );
        let cache_dir = clone_dir_contents(&resource_dir, "cache-counts");
        fs::remove_file(cache_dir.join("client/main.lua")).unwrap();
        fs::write(cache_dir.join("server/main.lua"), "zz").unwrap();

        let report = verify_cache_for_resource(&resource_dir, &cache_dir).unwrap();
        assert_eq!(report.valid_count, 1);
        assert_eq!(report.missing_count, 1);
        assert_eq!(report.size_mismatch_count, 1);
        assert_eq!(report.hash_mismatch_count, 0);
        assert!(!report.is_fully_valid);
    }

    #[test]
    fn discover_multiple_valid_resources() {
        let root = create_registry_root(&[
            ("chat", registry_manifest("chat", &[])),
            ("scoreboard", registry_manifest("scoreboard", &["chat"])),
            ("admin", registry_manifest("admin", &["chat", "scoreboard"])),
        ]);

        let registry = discover_resources(&root).unwrap();
        assert_eq!(registry.resources.len(), 3);
        assert!(registry.resources.contains_key("chat"));
        assert!(registry.resources.contains_key("scoreboard"));
        assert!(registry.resources.contains_key("admin"));
    }

    #[test]
    fn deterministic_load_order() {
        let root = create_registry_root(&[
            ("chat", registry_manifest("chat", &[])),
            ("scoreboard", registry_manifest("scoreboard", &["chat"])),
            ("admin", registry_manifest("admin", &["chat", "scoreboard"])),
        ]);

        let registry = discover_resources(&root).unwrap();
        let order = resolve_load_order(&registry).unwrap();
        assert_eq!(order.resources, vec!["chat", "scoreboard", "admin"]);
    }

    #[test]
    fn missing_dependency_returns_error() {
        let root = create_registry_root(&[("chat", registry_manifest("chat", &["missing_dep"]))]);

        let registry = discover_resources(&root).unwrap();
        let err = resolve_load_order(&registry).unwrap_err();
        assert!(err.to_string().contains("MissingDependency"));
    }

    #[test]
    fn dependency_cycle_returns_error() {
        let root = create_registry_root(&[
            ("chat", registry_manifest("chat", &["scoreboard"])),
            ("scoreboard", registry_manifest("scoreboard", &["chat"])),
        ]);

        let registry = discover_resources(&root).unwrap();
        let err = resolve_load_order(&registry).unwrap_err();
        assert!(err.to_string().contains("DependencyCycle"));
    }

    #[test]
    fn duplicate_resource_name_returns_error_if_possible() {
        let mut resources = BTreeMap::new();
        let manifest = parse_manifest_toml(&registry_manifest("chat", &[])).unwrap();

        insert_registered_resource(&mut resources, PathBuf::from("/tmp/chat"), manifest.clone())
            .unwrap();
        let err =
            insert_registered_resource(&mut resources, PathBuf::from("/tmp/chat-dup"), manifest)
                .unwrap_err();
        assert!(err.to_string().contains("DuplicateResource"));
    }

    #[test]
    fn resource_directory_without_manifest_is_ignored() {
        let root = unique_temp_dir("ignore-missing-manifest");
        fs::create_dir_all(root.join("chat")).unwrap();
        fs::write(
            root.join("chat/resource.toml"),
            registry_manifest("chat", &[]),
        )
        .unwrap();
        fs::create_dir_all(root.join("empty_folder")).unwrap();

        let registry = discover_resources(&root).unwrap();
        assert_eq!(registry.resources.len(), 1);
        assert!(registry.resources.contains_key("chat"));
    }

    #[test]
    fn resource_folder_name_and_manifest_name_mismatch_returns_error() {
        let root = create_registry_root(&[("chat_folder", registry_manifest("chat", &[]))]);

        let err = discover_resources(&root).unwrap_err();
        assert!(err.to_string().contains("does not match manifest name"));
    }

    #[test]
    fn independent_resources_sorted_lexically() {
        let root = create_registry_root(&[
            ("zeta", registry_manifest("zeta", &[])),
            ("alpha", registry_manifest("alpha", &[])),
        ]);

        let registry = discover_resources(&root).unwrap();
        let order = resolve_load_order(&registry).unwrap();
        assert_eq!(order.resources, vec!["alpha", "zeta"]);
    }

    #[test]
    fn build_load_plan_from_valid_registry() {
        let root = create_registry_root(&[
            ("chat", registry_manifest("chat", &[])),
            ("scoreboard", registry_manifest("scoreboard", &["chat"])),
        ]);

        let registry = discover_resources(&root).unwrap();
        let order = resolve_load_order(&registry).unwrap();
        let plan = build_load_plan(&registry, &order).unwrap();
        assert_eq!(plan.resources.len(), 2);
    }

    #[test]
    fn load_plan_order_matches_dependency_order() {
        let root = create_registry_root(&[
            ("chat", registry_manifest("chat", &[])),
            ("scoreboard", registry_manifest("scoreboard", &["chat"])),
            ("admin", registry_manifest("admin", &["chat", "scoreboard"])),
        ]);

        let plan = build_load_plan_from_root(&root).unwrap();
        let names: Vec<_> = plan
            .resources
            .iter()
            .map(|resource| resource.name.as_str())
            .collect();
        assert_eq!(names, vec!["chat", "scoreboard", "admin"]);
    }

    #[test]
    fn includes_server_entrypoints() {
        let root = create_registry_root(&[("chat", registry_manifest("chat", &[]))]);

        let plan = build_load_plan_from_root(&root).unwrap();
        assert!(plan.resources[0]
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint.kind == ResourceEntrypointKind::Server));
    }

    #[test]
    fn includes_client_entrypoints() {
        let root = create_registry_root(&[("chat", registry_manifest("chat", &[]))]);

        let plan = build_load_plan_from_root(&root).unwrap();
        assert!(plan.resources[0]
            .entrypoints
            .iter()
            .any(|entrypoint| entrypoint.kind == ResourceEntrypointKind::Client));
    }

    #[test]
    fn resource_phase_defaults_to_planned() {
        let root = create_registry_root(&[("chat", registry_manifest("chat", &[]))]);

        let plan = build_load_plan_from_root(&root).unwrap();
        assert_eq!(plan.resources[0].phase, ResourceRuntimePhase::Planned);
    }

    #[test]
    fn missing_resource_in_load_order_returns_error() {
        let root = create_registry_root(&[("chat", registry_manifest("chat", &[]))]);
        let registry = discover_resources(&root).unwrap();
        let order = LoadOrder {
            resources: vec!["missing".to_string()],
        };

        let err = build_load_plan(&registry, &order).unwrap_err();
        assert!(err
            .to_string()
            .contains("load order references missing resource"));
    }

    #[test]
    fn deterministic_output_order() {
        let root = create_registry_root(&[
            ("zeta", registry_manifest("zeta", &[])),
            ("alpha", registry_manifest("alpha", &[])),
        ]);

        let plan = build_load_plan_from_root(&root).unwrap();
        let names: Vec<_> = plan
            .resources
            .iter()
            .map(|resource| resource.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn no_file_contents_are_read_or_executed_by_design() {
        let root = create_registry_root(&[("chat", registry_manifest("chat", &[]))]);

        let plan = build_load_plan_from_root(&root).unwrap();
        assert_eq!(plan.resources[0].entrypoints.len(), 2);
        assert_eq!(
            plan.resources[0].entrypoints[0].path,
            PathBuf::from("server/main.js")
        );
        assert_eq!(
            plan.resources[0].entrypoints[1].path,
            PathBuf::from("client/main.js")
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

    fn create_resource_dir(manifest: &str, files: &[(&PathBuf, &str)]) -> PathBuf {
        let dir = unique_temp_dir("resource-pack");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("resource.toml"), manifest).unwrap();

        for (relative_path, contents) in files {
            let full_path = dir.join(relative_path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full_path, contents).unwrap();
        }

        dir
    }

    fn clone_dir_contents(source_dir: &Path, label: &str) -> PathBuf {
        let dest_dir = unique_temp_dir(label);
        copy_dir_recursive(source_dir, &dest_dir);
        dest_dir
    }

    fn copy_dir_recursive(source_dir: &Path, dest_dir: &Path) {
        fs::create_dir_all(dest_dir).unwrap();

        for entry in fs::read_dir(source_dir).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            let dest_path = dest_dir.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).unwrap();

            if metadata.is_dir() {
                copy_dir_recursive(&source_path, &dest_path);
            } else if metadata.is_file() {
                fs::copy(&source_path, &dest_path).unwrap();
            }
        }
    }

    fn write_temp_file(name: &str, contents: &str) -> PathBuf {
        let path = unique_temp_dir(name).join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("meowv-{label}-{unique}"))
    }

    fn create_registry_root(resources: &[(&str, String)]) -> PathBuf {
        let root = unique_temp_dir("resource-registry");
        fs::create_dir_all(&root).unwrap();

        for (dir_name, manifest) in resources {
            let resource_dir = root.join(dir_name);
            fs::create_dir_all(&resource_dir).unwrap();
            fs::write(resource_dir.join("resource.toml"), manifest).unwrap();
        }

        root
    }

    fn registry_manifest(name: &str, dependencies: &[&str]) -> String {
        let dependency_lines = dependencies
            .iter()
            .map(|dependency| format!("\n[[dependencies]]\nname = \"{dependency}\""))
            .collect::<String>();

        format!(
            "name = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"Registry test\"\nauthors = [\"MeowV Team\"]\nlicense = \"MIT\"\ntags = [\"test\"]\nprotocol_version = 1\nedition_compatibility = \"any\"\n\n[entrypoints]\nserver = \"server/main.js\"\nclient = \"client/main.js\"{dependency_lines}\n"
        )
    }
}
