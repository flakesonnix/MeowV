use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use protocol::{AnnouncedResourceFile, ResourceAnnouncement};
use resource_manifest::hash_file_sha256;

use crate::fetch::{CacheManifest, CacheManifestEntry, load_cache_manifest};

/// Action that the cache reconciliation planner recommends for a cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheReconciliationAction {
    /// Manifest, cache file, and announcement all agree.
    AlreadyConsistent,
    /// File exists on disk but is not tracked in the manifest.
    MissingManifestEntry,
    /// Manifest has an entry but the cache file is missing.
    MissingCacheFile,
    /// Cache file hash differs from manifest expectation.
    HashMismatch {
        expected_sha256: String,
        actual_sha256: String,
    },
    /// File exists in cache but is not referenced by announcement or manifest.
    OrphanedCacheFile,
    /// Manifest file is missing or corrupted (treated as empty).
    ManifestCorrupted,
    /// Manifest references a resource/file not present in the announcement.
    AnnouncementMissing,
    /// Would add a missing entry to the manifest.
    WouldRepairManifest,
    /// Would remove an orphaned file from cache.
    WouldRemoveOrphan,
    /// Would refetch a missing or mismatched file.
    WouldRefetch,
}

impl CacheReconciliationAction {
    pub fn label(&self) -> &str {
        match self {
            Self::AlreadyConsistent => "already_consistent",
            Self::MissingManifestEntry => "missing_manifest_entry",
            Self::MissingCacheFile => "missing_cache_file",
            Self::HashMismatch { .. } => "hash_mismatch",
            Self::OrphanedCacheFile => "orphaned_cache_file",
            Self::ManifestCorrupted => "manifest_corrupted",
            Self::AnnouncementMissing => "announcement_missing",
            Self::WouldRepairManifest => "would_repair_manifest",
            Self::WouldRemoveOrphan => "would_remove_orphan",
            Self::WouldRefetch => "would_refetch",
        }
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::AlreadyConsistent)
    }
}

/// Per-entry reconciliation result.
#[derive(Debug, Clone)]
pub struct CacheReconciliationEntry {
    pub resource_name: String,
    pub file_path: String,
    pub action: CacheReconciliationAction,
    pub manifest_sha256: Option<String>,
    pub cache_sha256: Option<String>,
    pub manifest_size_bytes: Option<u64>,
    pub cache_size_bytes: Option<u64>,
}

impl CacheReconciliationEntry {
    fn sort_key(&self) -> (&str, &str) {
        (&self.resource_name, &self.file_path)
    }
}

/// Aggregate reconciliation plan.
#[derive(Debug, Clone)]
pub struct CacheReconciliationPlan {
    pub entries: Vec<CacheReconciliationEntry>,
    pub manifest_corrupted: bool,
}

impl CacheReconciliationPlan {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            manifest_corrupted: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && !self.manifest_corrupted
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn unhealthy_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| !e.action.is_healthy())
            .count()
    }

    pub fn to_text(&self) -> String {
        if self.is_empty() {
            return "cache reconciliation: (empty, no entries to reconcile)".to_string();
        }
        let mut lines = Vec::new();
        let total = self.entries.len();
        let unhealthy = self.unhealthy_count();
        let healthy = total - unhealthy;
        lines.push(format!(
            "cache reconciliation: {total} entr{}, {healthy} consistent, {unhealthy} issue{}",
            if total == 1 { "y" } else { "ies" },
            if unhealthy == 1 { "" } else { "s" },
        ));
        if self.manifest_corrupted {
            lines.push(
                "  manifest: corrupted (treating as empty, all committed files unaccounted)"
                    .to_string(),
            );
        }
        for entry in &self.entries {
            let action_label = entry.action.label();
            let details = match &entry.action {
                CacheReconciliationAction::HashMismatch {
                    expected_sha256,
                    actual_sha256,
                } => {
                    format!(
                        " expected_sha256={} actual_sha256={}",
                        &expected_sha256[..expected_sha256.len().min(16)],
                        &actual_sha256[..actual_sha256.len().min(16)],
                    )
                }
                _ => String::new(),
            };
            lines.push(format!(
                "  [{action_label}] {res}:{path}{details}",
                res = entry.resource_name,
                path = entry.file_path,
            ));
        }
        lines.join("\n")
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

impl serde::Serialize for CacheReconciliationEntry {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CacheReconciliationEntry", 6)?;
        s.serialize_field("resource_name", &self.resource_name)?;
        s.serialize_field("file_path", &self.file_path)?;
        s.serialize_field("action", self.action.label())?;
        if let CacheReconciliationAction::HashMismatch {
            expected_sha256,
            actual_sha256,
        } = &self.action
        {
            s.serialize_field("expected_sha256", expected_sha256)?;
            s.serialize_field("actual_sha256", actual_sha256)?;
            s.serialize_field("manifest_sha256", expected_sha256)?;
            s.serialize_field("cache_sha256", actual_sha256)?;
        }
        s.end()
    }
}

impl serde::Serialize for CacheReconciliationPlan {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CacheReconciliationPlan", 2)?;
        s.serialize_field("entries", &self.entries)?;
        s.serialize_field("manifest_corrupted", &self.manifest_corrupted)?;
        s.end()
    }
}

/// A file found in the cache directory during reconciliation scanning.
#[derive(Debug, Clone)]
pub struct CacheFileEntry {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// Scan a cache directory and return all regular files found,
/// excluding the manifest file and staging directory.
pub async fn scan_cache_directory(cache_dir: &Path) -> Result<Vec<CacheFileEntry>> {
    let mut entries = Vec::new();
    scan_dir_recursive(cache_dir, cache_dir, &mut entries).await?;
    Ok(entries)
}

fn is_cache_special(name: &str) -> bool {
    name == "cache_manifest.json" || name == ".staging"
}

async fn scan_dir_recursive(
    root: &Path,
    current: &Path,
    out: &mut Vec<CacheFileEntry>,
) -> Result<()> {
    let mut read_dir = tokio::fs::read_dir(current).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if path.parent() == Some(root) && is_cache_special(&relative) {
            continue;
        }

        if file_type.is_dir() {
            Box::pin(scan_dir_recursive(root, &path, out)).await?;
        } else if file_type.is_file() {
            let sha256 = hash_file_sha256(&path)?;
            let metadata = tokio::fs::metadata(&path).await?;
            out.push(CacheFileEntry {
                relative_path: relative,
                sha256,
                size_bytes: metadata.len(),
            });
        }
    }
    Ok(())
}

/// Build lookup maps for fast key-based access.
type KeyMap<T> = std::collections::BTreeMap<(String, String), T>;

fn build_manifest_map(manifest: &CacheManifest) -> KeyMap<&CacheManifestEntry> {
    manifest
        .entries
        .iter()
        .map(|e| ((e.resource_name.clone(), e.file_path.clone()), e))
        .collect()
}

fn build_cache_map(cache_files: &[CacheFileEntry]) -> KeyMap<&CacheFileEntry> {
    cache_files
        .iter()
        .map(|e| {
            let (res, path) = derive_key_from_cache_path(&e.relative_path);
            ((res, path), e)
        })
        .collect()
}

fn build_ann_map(announcement: &ResourceAnnouncement) -> KeyMap<&AnnouncedResourceFile> {
    let mut map = KeyMap::new();
    for resource in &announcement.resources {
        for file in &resource.files {
            map.insert((resource.name.clone(), file.relative_path.clone()), file);
        }
    }
    map
}

/// Build a pure reconciliation plan by comparing the manifest, cache files,
/// and announcement expectations. Deterministic ordering by (resource_name, file_path).
pub fn build_cache_reconciliation_plan(
    manifest: &CacheManifest,
    cache_files: &[CacheFileEntry],
    announcement: &ResourceAnnouncement,
    manifest_corrupted: bool,
) -> CacheReconciliationPlan {
    let manifest_map = build_manifest_map(manifest);
    let cache_map = build_cache_map(cache_files);
    let ann_map = build_ann_map(announcement);

    let mut plan_entries: Vec<CacheReconciliationEntry> = Vec::new();
    let mut seen = BTreeSet::new();

    // From manifest
    for entry in &manifest.entries {
        let key = (entry.resource_name.clone(), entry.file_path.clone());
        if seen.insert(key.clone()) {
            process_key(
                &key,
                &manifest_map,
                &cache_map,
                &ann_map,
                manifest_corrupted,
                &mut plan_entries,
            );
        }
    }

    // From cache files
    for entry in cache_files {
        let key = derive_key_from_cache_path(&entry.relative_path);
        if seen.insert(key.clone()) {
            process_key(
                &key,
                &manifest_map,
                &cache_map,
                &ann_map,
                manifest_corrupted,
                &mut plan_entries,
            );
        }
    }

    // From announcement
    for resource in &announcement.resources {
        for file in &resource.files {
            let key = (resource.name.clone(), file.relative_path.clone());
            if seen.insert(key.clone()) {
                process_key(
                    &key,
                    &manifest_map,
                    &cache_map,
                    &ann_map,
                    manifest_corrupted,
                    &mut plan_entries,
                );
            }
        }
    }

    plan_entries.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    CacheReconciliationPlan {
        entries: plan_entries,
        manifest_corrupted,
    }
}

/// Derive a (resource_name, file_path) key from a cache relative path.
/// If the path contains a '/', the first component is treated as resource_name.
fn derive_key_from_cache_path(path: &str) -> (String, String) {
    if let Some(slash_pos) = path.find('/') {
        let resource_name = path[..slash_pos].to_string();
        let file_path = path[slash_pos + 1..].to_string();
        (resource_name, file_path)
    } else {
        // No directory separator, treat entire path as file_path with empty resource
        (String::new(), path.to_string())
    }
}

/// Build a cache path from a (resource_name, file_path) key.
/// Returns file_path if resource_name is empty, otherwise "resource_name/file_path".
fn build_cache_path(key: &(String, String)) -> String {
    if key.0.is_empty() {
        key.1.clone()
    } else {
        format!("{}/{}", key.0, key.1)
    }
}

fn process_key(
    key: &(String, String),
    manifest_map: &KeyMap<&CacheManifestEntry>,
    cache_map: &KeyMap<&CacheFileEntry>,
    ann_map: &KeyMap<&AnnouncedResourceFile>,
    manifest_corrupted: bool,
    out: &mut Vec<CacheReconciliationEntry>,
) {
    let cache_key = derive_key_from_cache_path(&build_cache_path(key));
    let in_manifest = manifest_map.contains_key(key);
    let in_cache = cache_map.contains_key(&cache_key);
    let in_announcement = ann_map.contains_key(key);

    let effective_in_manifest = if manifest_corrupted {
        false
    } else {
        in_manifest
    };

    let action = if effective_in_manifest && in_cache && in_announcement {
        let manifest_entry = manifest_map.get(key);
        let cache_entry = cache_map.get(&cache_key);

        let manifest_sha = manifest_entry.map(|e| e.sha256.as_str());
        let cache_sha = cache_entry.map(|e| e.sha256.as_str());

        match (manifest_sha, cache_sha) {
            (Some(m_sha), Some(c_sha)) if m_sha == c_sha => {
                CacheReconciliationAction::AlreadyConsistent
            }
            (Some(m_sha), Some(c_sha)) => CacheReconciliationAction::HashMismatch {
                expected_sha256: m_sha.to_string(),
                actual_sha256: c_sha.to_string(),
            },
            _ => CacheReconciliationAction::HashMismatch {
                expected_sha256: String::new(),
                actual_sha256: String::new(),
            },
        }
    } else if !effective_in_manifest && in_cache && in_announcement {
        if manifest_corrupted {
            CacheReconciliationAction::ManifestCorrupted
        } else {
            CacheReconciliationAction::MissingManifestEntry
        }
    } else if effective_in_manifest && !in_cache && in_announcement {
        CacheReconciliationAction::MissingCacheFile
    } else if effective_in_manifest && in_cache && !in_announcement {
        CacheReconciliationAction::AnnouncementMissing
    } else if !effective_in_manifest && in_cache && !in_announcement {
        CacheReconciliationAction::OrphanedCacheFile
    } else if effective_in_manifest && !in_cache && !in_announcement {
        CacheReconciliationAction::AnnouncementMissing
    } else if !effective_in_manifest && !in_cache && in_announcement {
        // Announcement says we need it but we don't have it - normal fetch case
        // Skip this entry as it's handled by the fetch planner
        return;
    } else {
        // Nothing references this key, skip
        return;
    };

    let manifest_entry = manifest_map.get(key);
    let cache_entry = cache_map.get(&cache_key);

    out.push(CacheReconciliationEntry {
        resource_name: key.0.clone(),
        file_path: key.1.clone(),
        action,
        manifest_sha256: manifest_entry.map(|e| e.sha256.clone()),
        cache_sha256: cache_entry.map(|e| e.sha256.clone()),
        manifest_size_bytes: manifest_entry.map(|e| e.size_bytes),
        cache_size_bytes: cache_entry.map(|e| e.size_bytes),
    });
}

/// Load the manifest and scan the cache directory, then build
/// a reconciliation plan. High-level convenience wrapper.
pub async fn reconcile_cache(
    cache_dir: &Path,
    announcement: &ResourceAnnouncement,
) -> CacheReconciliationPlan {
    let manifest = load_cache_manifest(cache_dir).await;
    let manifest_corrupted = {
        // If the manifest file exists but is empty after load, it was corrupted.
        // We detect this by checking if the file exists but wasn't parsed.
        let manifest_path = cache_dir.join("cache_manifest.json");
        if manifest_path.exists() && manifest.is_empty() {
            // Check if it was truly empty or corrupt
            let content = tokio::fs::read_to_string(&manifest_path).await;
            match content {
                Ok(ref data) if data.trim().is_empty() => false,
                Ok(_) => true, // file exists and has content but parsed as empty = malformed
                Err(_) => true, // can't read = treat as corrupted
            }
        } else {
            false
        }
    };

    let cache_files = scan_cache_directory(cache_dir).await.unwrap_or_default();

    build_cache_reconciliation_plan(&manifest, &cache_files, announcement, manifest_corrupted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{
        AnnouncedResource, AnnouncedResourceFile, PROTOCOL_VERSION, ResourceRequirementLevel,
    };

    fn make_announcement(resources: Vec<AnnouncedResource>) -> ResourceAnnouncement {
        ResourceAnnouncement {
            resources,
            signature: None,
        }
    }

    fn make_ann_resource(name: &str, files: Vec<AnnouncedResourceFile>) -> AnnouncedResource {
        AnnouncedResource {
            name: name.to_string(),
            version: "1.0".to_string(),
            files,
            protocol_version: PROTOCOL_VERSION,
            requirement_level: ResourceRequirementLevel::Required,
        }
    }

    fn make_ann_file(path: &str, size: u64, sha: &str) -> AnnouncedResourceFile {
        AnnouncedResourceFile {
            relative_path: path.to_string(),
            size_bytes: size,
            sha256: sha.to_string(),
            sources: None,
        }
    }

    fn make_manifest_entry(
        resource_name: &str,
        file_path: &str,
        sha256: &str,
        size_bytes: u64,
    ) -> CacheManifestEntry {
        CacheManifestEntry {
            resource_name: resource_name.to_string(),
            file_path: file_path.to_string(),
            sha256: sha256.to_string(),
            size_bytes,
            source_scheme: None,
            source_uri: None,
        }
    }

    fn make_cache_entry(path: &str, sha: &str, size: u64) -> CacheFileEntry {
        CacheFileEntry {
            relative_path: path.to_string(),
            sha256: sha.to_string(),
            size_bytes: size,
        }
    }

    fn make_manifest(entries: Vec<CacheManifestEntry>) -> CacheManifest {
        let mut m = CacheManifest::empty();
        for e in entries {
            m = m.with_entry(e);
        }
        m
    }

    #[test]
    fn test_empty_everything() {
        let plan = build_cache_reconciliation_plan(
            &CacheManifest::empty(),
            &[],
            &make_announcement(vec![]),
            false,
        );
        assert!(plan.is_empty());
        assert_eq!(plan.entry_count(), 0);
    }

    #[test]
    fn test_already_consistent() {
        let sha = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", sha, 100)]);
        let cache = vec![make_cache_entry("chat/main.lua", sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::AlreadyConsistent
        );
        assert_eq!(plan.unhealthy_count(), 0);
    }

    #[test]
    fn test_missing_manifest_entry() {
        let sha = "a".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&CacheManifest::empty(), &cache, &ann, false);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::MissingManifestEntry
        );
        assert_eq!(plan.unhealthy_count(), 1);
    }

    #[test]
    fn test_missing_cache_file() {
        let sha = "b".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&manifest, &[], &ann, false);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::MissingCacheFile
        );
        assert_eq!(plan.unhealthy_count(), 1);
    }

    #[test]
    fn test_hash_mismatch() {
        let manifest_sha = "c".repeat(64);
        let cache_sha = "d".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry(
            "chat",
            "main.lua",
            &manifest_sha,
            100,
        )]);
        let cache = vec![make_cache_entry("chat/main.lua", &cache_sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &manifest_sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::HashMismatch {
                expected_sha256: manifest_sha.clone(),
                actual_sha256: cache_sha.clone(),
            }
        );
        if let CacheReconciliationAction::HashMismatch {
            expected_sha256,
            actual_sha256,
        } = &plan.entries[0].action
        {
            assert_eq!(*expected_sha256, manifest_sha);
            assert_eq!(*actual_sha256, cache_sha);
        } else {
            panic!("expected HashMismatch");
        }
    }

    #[test]
    fn test_orphaned_cache_file() {
        let cache = vec![make_cache_entry(
            "unknown/orphan.dat",
            "e".repeat(64).as_str(),
            50,
        )];
        let plan = build_cache_reconciliation_plan(
            &CacheManifest::empty(),
            &cache,
            &make_announcement(vec![]),
            false,
        );
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::OrphanedCacheFile
        );
    }

    #[test]
    fn test_announcement_missing() {
        let sha = "f".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let plan =
            build_cache_reconciliation_plan(&manifest, &cache, &make_announcement(vec![]), false);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::AnnouncementMissing
        );
    }

    #[test]
    fn test_manifest_corrupted_flag() {
        let sha = "g".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&CacheManifest::empty(), &cache, &ann, true);
        assert!(plan.manifest_corrupted);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::ManifestCorrupted
        );
    }

    #[test]
    fn test_deterministic_ordering() {
        let sha = "h".repeat(64);
        let manifest = make_manifest(vec![
            make_manifest_entry("z_resource", "a.txt", &sha, 10),
            make_manifest_entry("a_resource", "z.txt", &sha, 20),
        ]);
        let cache = vec![
            make_cache_entry("z_resource/a.txt", &sha, 10),
            make_cache_entry("a_resource/z.txt", &sha, 20),
        ];
        let ann = make_announcement(vec![
            make_ann_resource("z_resource", vec![make_ann_file("a.txt", 10, &sha)]),
            make_ann_resource("a_resource", vec![make_ann_file("z.txt", 20, &sha)]),
        ]);
        let plan = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        assert_eq!(plan.entry_count(), 2);
        assert_eq!(plan.entries[0].resource_name, "a_resource");
        assert_eq!(plan.entries[1].resource_name, "z_resource");
    }

    #[test]
    fn test_action_label() {
        assert_eq!(
            CacheReconciliationAction::AlreadyConsistent.label(),
            "already_consistent"
        );
        assert_eq!(
            CacheReconciliationAction::MissingManifestEntry.label(),
            "missing_manifest_entry"
        );
        assert_eq!(
            CacheReconciliationAction::MissingCacheFile.label(),
            "missing_cache_file"
        );
        assert_eq!(
            CacheReconciliationAction::HashMismatch {
                expected_sha256: String::new(),
                actual_sha256: String::new(),
            }
            .label(),
            "hash_mismatch"
        );
        assert_eq!(
            CacheReconciliationAction::OrphanedCacheFile.label(),
            "orphaned_cache_file"
        );
        assert_eq!(
            CacheReconciliationAction::ManifestCorrupted.label(),
            "manifest_corrupted"
        );
        assert_eq!(
            CacheReconciliationAction::AnnouncementMissing.label(),
            "announcement_missing"
        );
        assert_eq!(
            CacheReconciliationAction::WouldRepairManifest.label(),
            "would_repair_manifest"
        );
        assert_eq!(
            CacheReconciliationAction::WouldRemoveOrphan.label(),
            "would_remove_orphan"
        );
        assert_eq!(
            CacheReconciliationAction::WouldRefetch.label(),
            "would_refetch"
        );
    }

    #[test]
    fn test_is_healthy() {
        assert!(CacheReconciliationAction::AlreadyConsistent.is_healthy());
        assert!(!CacheReconciliationAction::MissingManifestEntry.is_healthy());
        assert!(!CacheReconciliationAction::OrphanedCacheFile.is_healthy());
    }

    #[test]
    fn test_plan_to_text_empty() {
        let plan = CacheReconciliationPlan::empty();
        let text = plan.to_text();
        assert!(text.contains("(empty, no entries to reconcile)"));
    }

    #[test]
    fn test_plan_to_text_with_entries() {
        let sha = "i".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        let text = plan.to_text();
        assert!(text.contains("already_consistent"));
        assert!(text.contains("chat:main.lua"));
    }

    #[test]
    fn test_derive_key_from_cache_path() {
        let (res, path) = derive_key_from_cache_path("chat/main.lua");
        assert_eq!(res, "chat");
        assert_eq!(path, "main.lua");

        let (res, path) = derive_key_from_cache_path("orphan.dat");
        assert_eq!(res, "");
        assert_eq!(path, "orphan.dat");
    }

    #[test]
    fn test_build_cache_path() {
        let path = build_cache_path(&("chat".to_string(), "main.lua".to_string()));
        assert_eq!(path, "chat/main.lua");

        let path = build_cache_path(&("".to_string(), "orphan.dat".to_string()));
        assert_eq!(path, "orphan.dat");
    }

    #[test]
    fn test_missing_manifest_with_corrupted_flag_not_reapplied() {
        // When manifest is corrupted, files in cache + announcement
        // should get ManifestCorrupted, not MissingManifestEntry
        let sha = "j".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&CacheManifest::empty(), &cache, &ann, true);
        assert_eq!(plan.entry_count(), 1);
        assert_eq!(
            plan.entries[0].action,
            CacheReconciliationAction::ManifestCorrupted
        );
    }

    #[test]
    fn test_multiple_issues_combined() {
        let sha_ok = "k".repeat(64);
        let sha_bad_manifest = "l".repeat(64);
        let sha_bad_cache = "m".repeat(64);

        let manifest = make_manifest(vec![
            make_manifest_entry("chat", "main.lua", &sha_ok, 100),
            make_manifest_entry("chat", "bad_hash.lua", &sha_bad_manifest, 200),
        ]);

        let cache = vec![
            make_cache_entry("chat/main.lua", &sha_ok, 100),
            make_cache_entry("chat/bad_hash.lua", &sha_bad_cache, 200),
            make_cache_entry("orphan/unknown.dat", "n".repeat(64).as_str(), 50),
        ];

        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![
                make_ann_file("main.lua", 100, &sha_ok),
                make_ann_file("bad_hash.lua", 200, &sha_bad_manifest),
            ],
        )]);

        let plan = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        assert_eq!(plan.entry_count(), 3);

        // Check actions are correctly assigned
        let mut consistent = false;
        let mut hash_mismatch = false;
        let mut orphan = false;
        for entry in &plan.entries {
            match &entry.action {
                CacheReconciliationAction::AlreadyConsistent => {
                    assert_eq!(entry.resource_name, "chat");
                    assert_eq!(entry.file_path, "main.lua");
                    consistent = true;
                }
                CacheReconciliationAction::HashMismatch { .. } => {
                    assert_eq!(entry.resource_name, "chat");
                    assert_eq!(entry.file_path, "bad_hash.lua");
                    hash_mismatch = true;
                }
                CacheReconciliationAction::OrphanedCacheFile => {
                    assert_eq!(entry.resource_name, "orphan");
                    assert_eq!(entry.file_path, "unknown.dat");
                    orphan = true;
                }
                _ => {}
            }
        }
        assert!(consistent, "expected AlreadyConsistent entry");
        assert!(hash_mismatch, "expected HashMismatch entry");
        assert!(orphan, "expected OrphanedCacheFile entry");
        assert_eq!(plan.unhealthy_count(), 2);
    }

    #[test]
    fn test_plan_json_serialization() {
        let sha = "o".repeat(64);
        let cache = vec![make_cache_entry("orphan/test.dat", &sha, 100)];
        let plan = build_cache_reconciliation_plan(
            &CacheManifest::empty(),
            &cache,
            &make_announcement(vec![]),
            false,
        );
        let json = plan.to_json().unwrap();
        assert!(json.contains("orphaned_cache_file"));
        assert!(json.contains("entries"));
        assert!(json.contains("manifest_corrupted"));
    }

    #[test]
    fn test_plan_to_text_with_manifest_corrupted_flag() {
        let sha = "p".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan = build_cache_reconciliation_plan(&CacheManifest::empty(), &cache, &ann, true);
        let text = plan.to_text();
        assert!(text.contains("manifest_corrupted"));
        assert!(text.contains("treating as empty"));
    }

    #[test]
    fn test_multiple_identical_keys_not_duplicated() {
        let sha = "q".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);

        // Add duplicate manifest entry (same key different sha - last one wins in BTreeMap)
        let manifest_with_dup = make_manifest(vec![
            make_manifest_entry("chat", "main.lua", &sha, 100),
            make_manifest_entry("chat", "main.lua", &sha, 100),
        ]);

        let plan = build_cache_reconciliation_plan(&manifest_with_dup, &cache, &ann, false);
        assert_eq!(
            plan.entry_count(),
            1,
            "duplicate keys should not produce duplicate entries"
        );
    }

    #[test]
    fn test_reconcile_multiple_resources() {
        let sha = "r".repeat(64);
        let manifest = make_manifest(vec![
            make_manifest_entry("res_a", "f1.dat", &sha, 10),
            make_manifest_entry("res_b", "f2.dat", &sha, 20),
        ]);
        let cache = vec![
            make_cache_entry("res_a/f1.dat", &sha, 10),
            make_cache_entry("res_b/f2.dat", &sha, 20),
        ];
        let ann = make_announcement(vec![
            make_ann_resource("res_a", vec![make_ann_file("f1.dat", 10, &sha)]),
            make_ann_resource("res_b", vec![make_ann_file("f2.dat", 20, &sha)]),
        ]);
        let plan = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        assert_eq!(plan.entry_count(), 2);
        assert!(plan.entries.iter().all(|e| e.action.is_healthy()));
        assert_eq!(plan.unhealthy_count(), 0);
    }

    #[test]
    fn test_reconciliation_report_stability() {
        let sha = "s".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let plan1 = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        let plan2 = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        assert_eq!(plan1.entry_count(), plan2.entry_count());
        for (a, b) in plan1.entries.iter().zip(plan2.entries.iter()) {
            assert_eq!(a.action, b.action);
            assert_eq!(a.resource_name, b.resource_name);
            assert_eq!(a.file_path, b.file_path);
        }
    }
}
