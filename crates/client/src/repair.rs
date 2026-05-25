use anyhow::Result;
use protocol::{
    AnnouncedResourceFile, ResourceAnnouncement, ResourceDownloadPreflightAction,
    ResourceDownloadPreflightEntry, ResourceDownloadPreflightPlan,
};

use crate::reconciliation::{
    CacheReconciliationAction, CacheReconciliationPlan, build_cache_reconciliation_plan,
    scan_cache_directory,
};
use crate::trust::{Announcement, Trusted};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheRepairAction {
    RepairManifestEntry,
    RefetchMissingFile,
    ReplaceMismatchedFile,
    RebuildManifest,
}

impl CacheRepairAction {
    pub fn label(&self) -> &str {
        match self {
            Self::RepairManifestEntry => "repair_manifest_entry",
            Self::RefetchMissingFile => "refetch_missing_file",
            Self::ReplaceMismatchedFile => "replace_mismatched_file",
            Self::RebuildManifest => "rebuild_manifest",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheRepairOutcome {
    Repaired,
    Skipped,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheRepairFailureReason {
    DryRun,
    MissingCacheDir,
    NoMatchingAnnouncementFile,
    RepairNotPermitted,
    ManifestWriteFailed(String),
    RefetchFailed(String),
}

impl CacheRepairFailureReason {
    pub fn label(&self) -> &str {
        match self {
            Self::DryRun => "dry_run",
            Self::MissingCacheDir => "missing_cache_dir",
            Self::NoMatchingAnnouncementFile => "no_matching_announcement_file",
            Self::RepairNotPermitted => "repair_not_permitted",
            Self::ManifestWriteFailed(_) => "manifest_write_failed",
            Self::RefetchFailed(_) => "refetch_failed",
        }
    }
}

impl CacheRepairOutcome {
    pub fn label(&self) -> &str {
        match self {
            Self::Repaired => "repaired",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRepairEntry {
    pub resource_name: String,
    pub file_path: String,
    pub action: CacheRepairAction,
    pub reason: String,
    pub cache_sha256: Option<String>,
}

impl CacheRepairEntry {
    fn sort_key(&self) -> (&str, &str, &str) {
        (&self.resource_name, &self.file_path, self.action.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRepairPlan {
    pub entries: Vec<CacheRepairEntry>,
    pub manifest_corrupted_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRepairExecutionEntry {
    pub resource_name: String,
    pub file_path: String,
    pub planned_action: CacheRepairAction,
    pub outcome: CacheRepairOutcome,
    pub reason: String,
    pub failure_reason: Option<CacheRepairFailureReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRepairExecutionReport {
    pub entries: Vec<CacheRepairExecutionEntry>,
    pub dry_run: bool,
    pub manifest_corrupted_input: bool,
}

#[derive(Debug, Clone)]
pub struct CacheRepairConfig {
    pub dry_run: bool,
    pub allow_manifest_repair: bool,
    pub allow_refetch_repair: bool,
    pub cache_dir: Option<String>,
}

impl CacheRepairPlan {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            manifest_corrupted_input: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "cache repair: (empty, nothing to repair)".to_string();
        }

        let mut lines = vec![format!(
            "cache repair: {} entr{}",
            self.entries.len(),
            if self.entries.len() == 1 { "y" } else { "ies" }
        )];

        if self.manifest_corrupted_input {
            lines.push("  input: manifest_corrupted=true".to_string());
        }

        for entry in &self.entries {
            lines.push(format!(
                "  [{}] {}:{} - {}",
                entry.action.label(),
                entry.resource_name,
                entry.file_path,
                entry.reason,
            ));
        }

        lines.join("\n")
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

impl CacheRepairExecutionReport {
    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "cache repair execution: (empty, nothing executed)".to_string();
        }

        let mut lines = vec![format!(
            "cache repair execution: {} entr{}{}",
            self.entries.len(),
            if self.entries.len() == 1 { "y" } else { "ies" },
            if self.dry_run { " [dry-run]" } else { "" }
        )];

        for entry in &self.entries {
            let failure = match &entry.failure_reason {
                Some(reason) => format!(" failure={}", reason.label()),
                None => String::new(),
            };
            lines.push(format!(
                "  [{}] {} {}:{} - {}{}",
                entry.outcome.label(),
                entry.planned_action.label(),
                entry.resource_name,
                entry.file_path,
                entry.reason,
                failure,
            ));
        }

        lines.join("\n")
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

impl serde::Serialize for CacheRepairEntry {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CacheRepairEntry", 4)?;
        s.serialize_field("resource_name", &self.resource_name)?;
        s.serialize_field("file_path", &self.file_path)?;
        s.serialize_field("action", self.action.label())?;
        s.serialize_field("reason", &self.reason)?;
        s.end()
    }
}

impl serde::Serialize for CacheRepairPlan {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CacheRepairPlan", 2)?;
        s.serialize_field("entries", &self.entries)?;
        s.serialize_field("manifest_corrupted_input", &self.manifest_corrupted_input)?;
        s.end()
    }
}

impl serde::Serialize for CacheRepairExecutionEntry {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CacheRepairExecutionEntry", 6)?;
        s.serialize_field("resource_name", &self.resource_name)?;
        s.serialize_field("file_path", &self.file_path)?;
        s.serialize_field("planned_action", self.planned_action.label())?;
        s.serialize_field("outcome", self.outcome.label())?;
        s.serialize_field("reason", &self.reason)?;
        if let Some(ref failure) = self.failure_reason {
            s.serialize_field("failure_reason", failure.label())?;
        }
        s.end()
    }
}

impl serde::Serialize for CacheRepairExecutionReport {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CacheRepairExecutionReport", 3)?;
        s.serialize_field("entries", &self.entries)?;
        s.serialize_field("dry_run", &self.dry_run)?;
        s.serialize_field("manifest_corrupted_input", &self.manifest_corrupted_input)?;
        s.end()
    }
}

type KeyMap<T> = std::collections::BTreeMap<(String, String), T>;

fn build_ann_map(announcement: &ResourceAnnouncement) -> KeyMap<&AnnouncedResourceFile> {
    let mut map = KeyMap::new();
    for resource in &announcement.resources {
        for file in &resource.files {
            map.insert((resource.name.clone(), file.relative_path.clone()), file);
        }
    }
    map
}

pub fn build_cache_repair_plan(
    reconciliation: &CacheReconciliationPlan,
    announcement: &ResourceAnnouncement,
) -> CacheRepairPlan {
    let ann_map = build_ann_map(announcement);
    let mut entries = Vec::new();

    for entry in &reconciliation.entries {
        let key = (entry.resource_name.clone(), entry.file_path.clone());
        let ann_file = ann_map.get(&key);

        let repair = match &entry.action {
            CacheReconciliationAction::MissingManifestEntry => {
                if entry.cache_sha256.is_some() && ann_file.is_some() {
                    Some(CacheRepairEntry {
                        resource_name: entry.resource_name.clone(),
                        file_path: entry.file_path.clone(),
                        action: CacheRepairAction::RepairManifestEntry,
                        reason: "cache file exists and announcement still expects it".to_string(),
                        cache_sha256: entry.cache_sha256.clone(),
                    })
                } else {
                    None
                }
            }
            CacheReconciliationAction::MissingCacheFile => Some(CacheRepairEntry {
                resource_name: entry.resource_name.clone(),
                file_path: entry.file_path.clone(),
                action: CacheRepairAction::RefetchMissingFile,
                reason: "manifest and announcement expect file but cache is missing".to_string(),
                cache_sha256: None,
            }),
            CacheReconciliationAction::HashMismatch { .. } => Some(CacheRepairEntry {
                resource_name: entry.resource_name.clone(),
                file_path: entry.file_path.clone(),
                action: CacheRepairAction::ReplaceMismatchedFile,
                reason: "cached file hash differs from manifest expectation".to_string(),
                cache_sha256: None,
            }),
            CacheReconciliationAction::ManifestCorrupted => Some(CacheRepairEntry {
                resource_name: entry.resource_name.clone(),
                file_path: entry.file_path.clone(),
                action: CacheRepairAction::RebuildManifest,
                reason: "manifest is corrupted; rebuild authoritative entry from cache+announcement"
                    .to_string(),
                cache_sha256: None,
            }),
            _ => None,
        };

        if let Some(repair) = repair {
            entries.push(repair);
        }
    }

    entries.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    CacheRepairPlan {
        entries,
        manifest_corrupted_input: reconciliation.manifest_corrupted,
    }
}

/// Plan a cache repair. Requires `Announcement<Trusted>` — repair strategy is shaped by
/// announcement content; untrusted announcements must not influence mutation intent.
pub async fn plan_cache_repair(
    cache_dir: &std::path::Path,
    announcement: &Announcement<Trusted>,
) -> Result<(CacheReconciliationPlan, CacheRepairPlan)> {
    let announcement = announcement.as_announcement();
    let manifest = crate::fetch::load_cache_manifest(cache_dir).await;
    let manifest_path = cache_dir.join("cache_manifest.json");
    let manifest_corrupted = if manifest_path.exists() && manifest.is_empty() {
        match tokio::fs::read_to_string(&manifest_path).await {
            Ok(data) => !data.trim().is_empty(),
            Err(e) => {
                // I/O error (permissions, transient) ≠ structural corruption.
                // Do not trigger rebuild; let the caller handle inaccessible manifests.
                tracing::warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "failed to read manifest for corruption check; treating as non-corrupted"
                );
                false
            }
        }
    } else {
        false
    };
    let cache_files = scan_cache_directory(cache_dir).await.unwrap_or_default();
    let reconciliation = build_cache_reconciliation_plan(
        &manifest,
        &cache_files,
        announcement,
        manifest_corrupted,
    );
    let repair = build_cache_repair_plan(&reconciliation, announcement);
    Ok((reconciliation, repair))
}

/// Execute a cache repair plan. Requires `Announcement<Trusted>` — callers that have
/// not completed the trust state machine (`Unverified → Parsed → PolicyChecked → Trusted`)
/// will not compile. No repair mutation may begin from an unverified announcement.
pub async fn execute_cache_repair(
    announcement: &Announcement<Trusted>,
    repair_plan: &CacheRepairPlan,
    config: &CacheRepairConfig,
) -> Result<CacheRepairExecutionReport> {
    let announcement = announcement.as_announcement();
    let cache_dir = match &config.cache_dir {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            return Ok(CacheRepairExecutionReport {
                entries: vec![CacheRepairExecutionEntry {
                    resource_name: String::new(),
                    file_path: String::new(),
                    planned_action: CacheRepairAction::RepairManifestEntry,
                    outcome: CacheRepairOutcome::Blocked,
                    reason: "cache repair requires --resource-cache <path>".to_string(),
                    failure_reason: Some(CacheRepairFailureReason::MissingCacheDir),
                }],
                dry_run: config.dry_run,
                manifest_corrupted_input: repair_plan.manifest_corrupted_input,
            });
        }
    };

    let mut report_entries = Vec::new();

    for entry in &repair_plan.entries {
        let (outcome, reason, failure_reason) = if config.dry_run {
            (
                CacheRepairOutcome::Skipped,
                "dry-run: no mutation performed".to_string(),
                Some(CacheRepairFailureReason::DryRun),
            )
        } else {
            match entry.action {
                CacheRepairAction::RepairManifestEntry | CacheRepairAction::RebuildManifest => {
                    if !config.allow_manifest_repair {
                        (
                            CacheRepairOutcome::Blocked,
                            "manifest repair blocked (use --allow-manifest-repair)".to_string(),
                            Some(CacheRepairFailureReason::RepairNotPermitted),
                        )
                    } else {
                        match repair_manifest_entry(&cache_dir, announcement, entry).await {
                            Ok(()) => (
                                CacheRepairOutcome::Repaired,
                                "manifest updated atomically".to_string(),
                                None,
                            ),
                            Err(err) => (
                                CacheRepairOutcome::Failed,
                                err.clone(),
                                Some(CacheRepairFailureReason::ManifestWriteFailed(err)),
                            ),
                        }
                    }
                }
                CacheRepairAction::RefetchMissingFile | CacheRepairAction::ReplaceMismatchedFile => {
                    if !config.allow_refetch_repair {
                        (
                            CacheRepairOutcome::Blocked,
                            "refetch repair blocked (use --allow-refetch-repair)".to_string(),
                            Some(CacheRepairFailureReason::RepairNotPermitted),
                        )
                    } else {
                        match execute_refetch_repair(cache_dir.as_path(), announcement, entry).await {
                            Ok(fetch_report) => {
                                let repaired = fetch_report.entries.iter().any(|fetch_entry| {
                                    fetch_entry.resource_name == entry.resource_name
                                        && fetch_entry.file_path == entry.file_path
                                        && fetch_entry.outcome.is_success()
                                });
                                if repaired {
                                    (
                                        CacheRepairOutcome::Repaired,
                                        "refetch repair completed via staged fetch pipeline"
                                            .to_string(),
                                        None,
                                    )
                                } else {
                                    (
                                        CacheRepairOutcome::Failed,
                                        "refetch repair did not produce a successful fetch outcome"
                                            .to_string(),
                                        Some(CacheRepairFailureReason::RefetchFailed(
                                            fetch_report.to_text(),
                                        )),
                                    )
                                }
                            }
                            Err(err) => (
                                CacheRepairOutcome::Failed,
                                err.to_string(),
                                Some(CacheRepairFailureReason::RefetchFailed(err.to_string())),
                            ),
                        }
                    }
                }
            }
        };

        report_entries.push(CacheRepairExecutionEntry {
            resource_name: entry.resource_name.clone(),
            file_path: entry.file_path.clone(),
            planned_action: entry.action.clone(),
            outcome,
            reason,
            failure_reason,
        });
    }

    Ok(CacheRepairExecutionReport {
        entries: report_entries,
        dry_run: config.dry_run,
        manifest_corrupted_input: repair_plan.manifest_corrupted_input,
    })
}

async fn repair_manifest_entry(
    cache_dir: &std::path::Path,
    announcement: &ResourceAnnouncement,
    entry: &CacheRepairEntry,
) -> std::result::Result<(), String> {
    let ann_file = announcement
        .resources
        .iter()
        .find(|resource| resource.name == entry.resource_name)
        .and_then(|resource| {
            resource
                .files
                .iter()
                .find(|file| file.relative_path == entry.file_path)
        })
        .ok_or_else(|| "no matching announcement file for manifest repair".to_string())?;

    let cache_sha = entry
        .cache_sha256
        .as_deref()
        .ok_or_else(|| "manifest repair requires a verified cache sha256".to_string())?;

    if cache_sha != ann_file.sha256 {
        return Err(format!(
            "integrity mismatch: cache sha256={} announcement sha256={}; manifest repair aborted",
            cache_sha, ann_file.sha256
        ));
    }

    let manifest_entry = crate::fetch::CacheManifestEntry {
        resource_name: entry.resource_name.clone(),
        file_path: entry.file_path.clone(),
        sha256: cache_sha.to_string(),
        size_bytes: ann_file.size_bytes,
        source_scheme: None,
        source_uri: None,
    };

    crate::fetch::write_manifest_entry(cache_dir, &manifest_entry)
        .await
        .map(|_| ())
}

async fn execute_refetch_repair(
    cache_dir: &std::path::Path,
    announcement: &ResourceAnnouncement,
    entry: &CacheRepairEntry,
) -> Result<crate::fetch::FetchReport> {
    let source = announcement
        .resources
        .iter()
        .find(|resource| resource.name == entry.resource_name)
        .and_then(|resource| {
            resource
                .files
                .iter()
                .find(|file| file.relative_path == entry.file_path)
        })
        .and_then(|file| file.sources.as_ref())
        .and_then(|sources| sources.first())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no matching announcement source for refetch repair"))?;

    let preflight_action = match entry.action {
        CacheRepairAction::RefetchMissingFile => ResourceDownloadPreflightAction::FetchMissing,
        CacheRepairAction::ReplaceMismatchedFile => ResourceDownloadPreflightAction::ReplaceInvalid,
        _ => return Err(anyhow::anyhow!("unsupported refetch repair action")),
    };

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![ResourceDownloadPreflightEntry {
            resource_name: entry.resource_name.clone(),
            file_path: entry.file_path.clone(),
            action: preflight_action,
            reason: entry.reason.clone(),
            source_errors: Vec::new(),
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: Vec::new(),
            source_policy: None,
        }],
    };

    crate::fetch::execute_fetch_plan(
        announcement,
        &preflight,
        &crate::fetch::FetchConfig {
            allow_fetch: true,
            allow_cache_commit: true,
            cache_dir: Some(cache_dir.display().to_string()),
            fetch_report_path: None,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{CacheManifest, CacheManifestEntry};
    use crate::reconciliation::{CacheFileEntry, build_cache_reconciliation_plan};
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
    fn test_empty_repair_plan() {
        let repair = build_cache_repair_plan(&CacheReconciliationPlan::empty(), &make_announcement(vec![]));
        assert!(repair.is_empty());
        assert_eq!(repair.entry_count(), 0);
    }

    #[test]
    fn test_missing_manifest_maps_to_manifest_repair() {
        let sha = "a".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let reconciliation =
            build_cache_reconciliation_plan(&CacheManifest::empty(), &cache, &ann, false);
        let repair = build_cache_repair_plan(&reconciliation, &ann);
        assert_eq!(repair.entry_count(), 1);
        assert_eq!(repair.entries[0].action, CacheRepairAction::RepairManifestEntry);
    }

    #[test]
    fn test_missing_cache_maps_to_refetch() {
        let sha = "b".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let reconciliation = build_cache_reconciliation_plan(&manifest, &[], &ann, false);
        let repair = build_cache_repair_plan(&reconciliation, &ann);
        assert_eq!(repair.entry_count(), 1);
        assert_eq!(repair.entries[0].action, CacheRepairAction::RefetchMissingFile);
    }

    #[test]
    fn test_hash_mismatch_maps_to_replace() {
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
        let reconciliation = build_cache_reconciliation_plan(&manifest, &cache, &ann, false);
        let repair = build_cache_repair_plan(&reconciliation, &ann);
        assert_eq!(repair.entry_count(), 1);
        assert_eq!(repair.entries[0].action, CacheRepairAction::ReplaceMismatchedFile);
    }

    #[test]
    fn test_manifest_corrupted_maps_to_rebuild() {
        let sha = "e".repeat(64);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let reconciliation =
            build_cache_reconciliation_plan(&CacheManifest::empty(), &cache, &ann, true);
        let repair = build_cache_repair_plan(&reconciliation, &ann);
        assert_eq!(repair.entry_count(), 1);
        assert_eq!(repair.entries[0].action, CacheRepairAction::RebuildManifest);
        assert!(repair.manifest_corrupted_input);
    }

    #[test]
    fn test_orphan_not_included_in_m6_15_plan() {
        let cache = vec![make_cache_entry("orphan/test.dat", &"f".repeat(64), 100)];
        let reconciliation = build_cache_reconciliation_plan(
            &CacheManifest::empty(),
            &cache,
            &make_announcement(vec![]),
            false,
        );
        let repair = build_cache_repair_plan(&reconciliation, &make_announcement(vec![]));
        assert!(repair.is_empty());
    }

    #[test]
    fn test_announcement_missing_not_included_in_m6_15_plan() {
        let sha = "g".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let cache = vec![make_cache_entry("chat/main.lua", &sha, 100)];
        let reconciliation = build_cache_reconciliation_plan(
            &manifest,
            &cache,
            &make_announcement(vec![]),
            false,
        );
        let repair = build_cache_repair_plan(&reconciliation, &make_announcement(vec![]));
        assert!(repair.is_empty());
    }

    #[test]
    fn test_repair_plan_ordering_is_deterministic() {
        let sha = "h".repeat(64);
        let manifest = make_manifest(vec![
            make_manifest_entry("z_res", "a.txt", &sha, 10),
            make_manifest_entry("a_res", "z.txt", &sha, 20),
        ]);
        let ann = make_announcement(vec![
            make_ann_resource("z_res", vec![make_ann_file("a.txt", 10, &sha)]),
            make_ann_resource("a_res", vec![make_ann_file("z.txt", 20, &sha)]),
        ]);
        let reconciliation = build_cache_reconciliation_plan(&manifest, &[], &ann, false);
        let repair = build_cache_repair_plan(&reconciliation, &ann);
        assert_eq!(repair.entry_count(), 2);
        assert_eq!(repair.entries[0].resource_name, "a_res");
        assert_eq!(repair.entries[1].resource_name, "z_res");
    }

    #[test]
    fn test_repair_plan_text_and_json() {
        let sha = "i".repeat(64);
        let manifest = make_manifest(vec![make_manifest_entry("chat", "main.lua", &sha, 100)]);
        let ann = make_announcement(vec![make_ann_resource(
            "chat",
            vec![make_ann_file("main.lua", 100, &sha)],
        )]);
        let reconciliation = build_cache_reconciliation_plan(&manifest, &[], &ann, false);
        let repair = build_cache_repair_plan(&reconciliation, &ann);
        let text = repair.to_text();
        let json = repair.to_json().unwrap();
        assert!(text.contains("refetch_missing_file"));
        assert!(json.contains("refetch_missing_file"));
    }
}
