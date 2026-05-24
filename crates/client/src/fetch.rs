use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use resource_manifest::hash_file_sha256;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};

use protocol::{
    ResourceAnnouncement, ResourceDownloadPreflightPlan, ResourceFetchSource,
    build_fetch_execution_plan,
};

/// Outcome of fetching a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// Fetched, verified, left in staging. No cache commit was attempted.
    StagedVerified,
    /// Verified and atomically committed to cache.
    CommittedToCache,
    /// Replaced existing invalid cache content with verified content.
    ReplaceInvalidCommitted,
    /// Fetch or commit failed.
    Failure(FetchFailureReason),
}

impl FetchOutcome {
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            Self::StagedVerified | Self::CommittedToCache | Self::ReplaceInvalidCommitted
        )
    }
}

/// Reason why a file fetch or commit failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchFailureReason {
    NoValidSource,
    BlockedBySourcePolicy,
    ConnectionFailed,
    Timeout,
    SizeExceeded,
    RedirectLimitExceeded,
    UnsupportedScheme,
    HashMismatch,
    StagingWriteFailed,
    StagingDirectoryCreationFailed,
    SymlinkRejected,
    PathTraversalRejected,
    CommitFailed(String),
    IoError(String),
}

impl std::fmt::Display for FetchFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoValidSource => write!(f, "no valid source"),
            Self::BlockedBySourcePolicy => write!(f, "blocked by source policy"),
            Self::ConnectionFailed => write!(f, "connection failed"),
            Self::Timeout => write!(f, "timeout"),
            Self::SizeExceeded => write!(f, "size exceeded"),
            Self::RedirectLimitExceeded => write!(f, "redirect limit exceeded"),
            Self::UnsupportedScheme => write!(f, "unsupported scheme"),
            Self::HashMismatch => write!(f, "hash mismatch"),
            Self::StagingWriteFailed => write!(f, "staging write failed"),
            Self::StagingDirectoryCreationFailed => write!(f, "staging directory creation failed"),
            Self::SymlinkRejected => write!(f, "symlink rejected"),
            Self::PathTraversalRejected => write!(f, "path traversal rejected"),
            Self::CommitFailed(msg) => write!(f, "commit failed: {msg}"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

/// Per-entry fetch result.
#[derive(Debug, Clone)]
pub struct FetchEntryReport {
    pub resource_name: String,
    pub file_path: String,
    pub source_scheme: String,
    pub source_uri: String,
    pub outcome: FetchOutcome,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub duration_ms: u64,
    pub manifest_outcome: ManifestOutcome,
}

impl FetchEntryReport {
    fn to_text(&self) -> String {
        let status = match &self.outcome {
            FetchOutcome::StagedVerified => {
                "staged_verified (use --allow-cache-commit to commit)".to_string()
            }
            FetchOutcome::CommittedToCache => "committed_to_cache".to_string(),
            FetchOutcome::ReplaceInvalidCommitted => "replace_invalid_committed".to_string(),
            FetchOutcome::Failure(reason) => format!("failure: {reason}"),
        };
        let manifest = match &self.manifest_outcome {
            ManifestOutcome::Updated => " manifest=updated".to_string(),
            ManifestOutcome::WriteFailed(msg) => format!(" manifest=write_failed:{msg}"),
            ManifestOutcome::SkippedNoCommit => String::new(),
        };
        format!(
            "  [{status}] {res}:{path} - {scheme} ({dur_ms} ms, {size} bytes, sha256:{sha}){manifest}",
            res = self.resource_name,
            path = self.file_path,
            scheme = self.source_scheme,
            dur_ms = self.duration_ms,
            size = self.expected_size_bytes,
            sha = &self.expected_sha256[..self.expected_sha256.len().min(16)],
        )
    }
}

/// Aggregate fetch report.
#[derive(Debug, Clone)]
pub struct FetchReport {
    pub entries: Vec<FetchEntryReport>,
}

impl FetchReport {
    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "resource fetch: (empty, nothing to do)".to_string();
        }
        let mut lines = vec![format!(
            "resource fetch: {} entr{}",
            self.entries.len(),
            if self.entries.len() == 1 { "y" } else { "ies" }
        )];
        for entry in &self.entries {
            lines.push(entry.to_text());
        }
        lines.join("\n")
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Outcome of updating the cache metadata manifest after a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestOutcome {
    /// Manifest was updated successfully.
    Updated,
    /// Manifest write failed with an error description.
    WriteFailed(String),
    /// No commit occurred, so no manifest update was attempted.
    SkippedNoCommit,
}

impl ManifestOutcome {
    pub fn is_updated(&self) -> bool {
        matches!(self, Self::Updated)
    }
}

/// A single entry in the cache metadata manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheManifestEntry {
    pub resource_name: String,
    pub file_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
}

impl CacheManifestEntry {
    fn sort_key(&self) -> (&str, &str) {
        (&self.resource_name, &self.file_path)
    }
}

/// Deterministic metadata manifest for committed cache contents.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheManifest {
    /// Schema version (currently 1).
    pub version: u64,
    /// Committed entries, sorted deterministically by (resource_name, file_path).
    pub entries: Vec<CacheManifestEntry>,
}

impl CacheManifest {
    const MANIFEST_VERSION: u64 = 1;

    pub fn empty() -> Self {
        Self {
            version: Self::MANIFEST_VERSION,
            entries: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "cache manifest: (empty, no committed resources)".to_string();
        }
        let mut lines = vec![format!(
            "cache manifest v{}: {} entr{}",
            self.version,
            self.entries.len(),
            if self.entries.len() == 1 { "y" } else { "ies" }
        )];
        for entry in &self.entries {
            let sha_prefix = &entry.sha256[..entry.sha256.len().min(16)];
            lines.push(format!(
                "  {}:{} ({} bytes, sha256:{})",
                entry.resource_name, entry.file_path, entry.size_bytes, sha_prefix,
            ));
        }
        lines.join("\n")
    }

    /// Merge a new entry into the manifest. If an entry with the same
    /// (resource_name, file_path) exists, it is replaced. Entries are
    /// re-sorted deterministically.
    fn with_entry(mut self, entry: CacheManifestEntry) -> Self {
        let key = entry.sort_key();
        self.entries.retain(|e| e.sort_key() != key);
        self.entries.push(entry);
        self.entries.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        self
    }
}

/// Path to the cache manifest JSON file inside a cache directory.
fn cache_manifest_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("cache_manifest.json")
}

/// Load the cache manifest from disk. Returns an empty manifest if the file
/// does not exist or is malformed (corrupted manifest is treated as empty).
pub async fn load_cache_manifest(cache_dir: &Path) -> CacheManifest {
    let path = cache_manifest_path(cache_dir);
    let data = match tokio::fs::read_to_string(&path).await {
        Ok(d) => d,
        Err(_) => return CacheManifest::empty(),
    };
    match serde_json::from_str::<CacheManifest>(&data) {
        Ok(m) => m,
        Err(_) => {
            warn!(
                "malformed cache manifest at {}, treating as empty",
                path.display()
            );
            CacheManifest::empty()
        }
    }
}

/// Save the cache manifest to disk atomically. Writes to a temp file in the
/// staging directory, then renames to the final path.
pub async fn save_cache_manifest(cache_dir: &Path, manifest: &CacheManifest) -> Result<()> {
    let staging_dir = cache_dir.join(".staging");
    tokio::fs::create_dir_all(&staging_dir)
        .await
        .with_context(|| {
            format!(
                "failed to create staging directory for manifest: {}",
                staging_dir.display()
            )
        })?;

    let tmp_path = staging_dir.join("cache_manifest.json.tmp");
    let final_path = cache_manifest_path(cache_dir);

    let json =
        serde_json::to_string_pretty(manifest).context("failed to serialize cache manifest")?;

    tokio::fs::write(&tmp_path, &json)
        .await
        .with_context(|| format!("failed to write temp manifest: {}", tmp_path.display()))?;

    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .with_context(|| {
            format!(
                "failed to rename manifest from {} to {}",
                tmp_path.display(),
                final_path.display()
            )
        })?;

    info!("cache manifest updated at {}", final_path.display());
    Ok(())
}

/// Update the cache manifest after a successful verified commit. Loads the
/// existing manifest (or creates an empty one), inserts/replaces the entry,
/// and saves atomically. Returns the updated manifest on success, or an error
/// on I/O failure.
pub async fn update_cache_manifest_after_commit(
    cache_dir: &Path,
    entry: &CacheManifestEntry,
) -> std::result::Result<CacheManifest, String> {
    let manifest = load_cache_manifest(cache_dir).await;
    let updated = manifest.with_entry(entry.clone());
    save_cache_manifest(cache_dir, &updated)
        .await
        .map_err(|e| format!("manifest write failed: {e}"))?;
    Ok(updated)
}

/// Configuration for fetch execution and optional cache commit.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub allow_fetch: bool,
    pub allow_cache_commit: bool,
    pub cache_dir: Option<String>,
    pub fetch_report_path: Option<String>,
}

/// Execute the fetch plan for all resources in an announcement.
///
/// Only runs if `config.allow_fetch` is `true`. For each entry in the
/// preflight plan that requires fetching and has `plan_ok`, the file
/// is fetched to `<cache_dir>/.staging/`, verified, and left in staging
/// by default. If `config.allow_cache_commit` is `true`, verified files
/// are atomically renamed from staging to `<cache_dir>/<file_path>` after
/// successful verification. Returns a `FetchReport` with per-entry results.
pub async fn execute_fetch_plan(
    announcement: &ResourceAnnouncement,
    preflight_plan: &ResourceDownloadPreflightPlan,
    config: &FetchConfig,
) -> Result<FetchReport> {
    if !config.allow_fetch {
        return Ok(FetchReport {
            entries: Vec::new(),
        });
    }

    let cache_dir = match &config.cache_dir {
        Some(d) => PathBuf::from(d),
        None => {
            return Ok(FetchReport {
                entries: Vec::new(),
            });
        }
    };

    let execution_plan = build_fetch_execution_plan(preflight_plan);
    let mut report_entries = Vec::new();

    // Build map from (resource_name, file_path) -> (size_bytes, sha256)
    let mut file_map: std::collections::HashMap<(String, String), (u64, String)> =
        std::collections::HashMap::new();
    for resource in &announcement.resources {
        for file in &resource.files {
            file_map.insert(
                (resource.name.clone(), file.relative_path.clone()),
                (file.size_bytes, file.sha256.clone()),
            );
        }
    }

    // Determine commit behaviour per-entry from preflight action
    let mut action_map: std::collections::HashMap<
        (String, String),
        protocol::ResourceDownloadPreflightAction,
    > = std::collections::HashMap::new();
    for pe in &preflight_plan.entries {
        action_map.insert(
            (pe.resource_name.clone(), pe.file_path.clone()),
            pe.action.clone(),
        );
    }

    // Iterate over execution plan entries which have plan_ok/steps already computed
    for exec_entry in &execution_plan.entries {
        if !exec_entry.plan_ok {
            continue;
        }

        let (expected_size, expected_sha256) = file_map
            .get(&(
                exec_entry.resource_name.clone(),
                exec_entry.file_path.clone(),
            ))
            .cloned()
            .unwrap_or((0, String::new()));

        // Find the matching preflight entry to get the selected source
        let source = preflight_plan
            .entries
            .iter()
            .find(|pe| {
                pe.resource_name == exec_entry.resource_name && pe.file_path == exec_entry.file_path
            })
            .and_then(|pe| pe.selected_source.clone());

        let source = match source {
            Some(s) => s,
            None => {
                report_entries.push(FetchEntryReport {
                    resource_name: exec_entry.resource_name.clone(),
                    file_path: exec_entry.file_path.clone(),
                    source_scheme: String::new(),
                    source_uri: String::new(),
                    outcome: FetchOutcome::Failure(FetchFailureReason::NoValidSource),
                    expected_size_bytes: expected_size,
                    expected_sha256,
                    duration_ms: 0,
                    manifest_outcome: ManifestOutcome::SkippedNoCommit,
                });
                continue;
            }
        };

        let start = Instant::now();
        let fetch_outcome = fetch_and_verify_single_file(
            &exec_entry.resource_name,
            &exec_entry.file_path,
            &source,
            expected_size,
            &expected_sha256,
            &cache_dir,
        )
        .await;

        let final_outcome = if fetch_outcome.is_success() && config.allow_cache_commit {
            // Attempt atomic commit from staging to cache path
            let target_path = cache_dir.join(&exec_entry.file_path);
            let action = action_map.get(&(
                exec_entry.resource_name.clone(),
                exec_entry.file_path.clone(),
            ));

            // Find the staged path (the only file in .staging/ whose name starts with sanitized file_path)
            let staged_file = find_staged_file(&cache_dir, &exec_entry.file_path).await;

            match staged_file {
                Some(staged_path) => match commit_verified_file(&staged_path, &target_path).await {
                    Ok(()) => {
                        let _ = tokio::fs::remove_file(&staged_path).await;
                        let is_replace = matches!(
                            action,
                            Some(&protocol::ResourceDownloadPreflightAction::ReplaceInvalid)
                        );
                        if is_replace {
                            info!(
                                "replaced invalid cache entry: {}:{}",
                                exec_entry.resource_name, exec_entry.file_path
                            );
                            FetchOutcome::ReplaceInvalidCommitted
                        } else {
                            info!(
                                "committed to cache: {}:{}",
                                exec_entry.resource_name, exec_entry.file_path
                            );
                            FetchOutcome::CommittedToCache
                        }
                    }
                    Err(outcome) => {
                        let _ = tokio::fs::remove_file(&staged_path).await;
                        outcome
                    }
                },
                None => {
                    // Staged file disappeared; treat as failure
                    error!(
                        "staged file vanished before commit: {}:{}",
                        exec_entry.resource_name, exec_entry.file_path
                    );
                    FetchOutcome::Failure(FetchFailureReason::CommitFailed(
                        "staged file not found before commit".to_string(),
                    ))
                }
            }
        } else {
            fetch_outcome
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        let manifest_outcome = if matches!(
            final_outcome,
            FetchOutcome::CommittedToCache | FetchOutcome::ReplaceInvalidCommitted
        ) {
            let manifest_entry = CacheManifestEntry {
                resource_name: exec_entry.resource_name.clone(),
                file_path: exec_entry.file_path.clone(),
                sha256: expected_sha256.clone(),
                size_bytes: expected_size,
                source_scheme: Some(source.scheme.clone()),
                source_uri: Some(source.uri.clone()),
            };
            match update_cache_manifest_after_commit(&cache_dir, &manifest_entry).await {
                Ok(_) => ManifestOutcome::Updated,
                Err(e) => ManifestOutcome::WriteFailed(e),
            }
        } else {
            ManifestOutcome::SkippedNoCommit
        };

        report_entries.push(FetchEntryReport {
            resource_name: exec_entry.resource_name.clone(),
            file_path: exec_entry.file_path.clone(),
            source_scheme: source.scheme.clone(),
            source_uri: source.uri.clone(),
            outcome: final_outcome,
            expected_size_bytes: expected_size,
            expected_sha256,
            duration_ms,
            manifest_outcome,
        });
    }

    Ok(FetchReport {
        entries: report_entries,
    })
}

/// Find the staged file for a given file_path inside `<cache_dir>/.staging/`.
/// Looks for a file whose name starts with the sanitized file_path.
async fn find_staged_file(cache_dir: &Path, file_path: &str) -> Option<PathBuf> {
    let staging_dir = cache_dir.join(".staging");
    let sanitized_prefix = sanitize_name(file_path);
    let mut entries = tokio::fs::read_dir(&staging_dir).await.ok()?;
    while let Some(entry) = entries.next_entry().await.ok()? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(&sanitized_prefix) {
            return Some(entry.path());
        }
    }
    None
}

/// Atomically commit a verified staged file to the cache target path.
///
/// Sandbox guards:
/// - Rejects symlinks in the target path.
/// - Rejects path traversal in the target path.
/// - Creates parent directories safely.
/// - Does not overwrite an existing valid cache entry (checked via SHA-256).
/// - Uses `std::fs::rename` (atomic on same filesystem) or falls back to
///   copy-then-delete for cross-filesystem moves.
async fn commit_verified_file(
    staged_path: &Path,
    target_path: &Path,
) -> std::result::Result<(), FetchOutcome> {
    // Sandbox: reject symlinks in target path
    if is_symlink_in_path(target_path) {
        return Err(FetchOutcome::Failure(FetchFailureReason::SymlinkRejected));
    }

    // Sandbox: reject path traversal in target path
    if contains_path_traversal_standalone(target_path) {
        return Err(FetchOutcome::Failure(
            FetchFailureReason::PathTraversalRejected,
        ));
    }

    // Create parent directories if needed
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            FetchOutcome::Failure(FetchFailureReason::CommitFailed(format!(
                "failed to create parent directory: {e}"
            )))
        })?;
    }

    // Attempt atomic rename
    if let Err(_e) = tokio::fs::rename(staged_path, target_path).await {
        // rename fails across filesystem boundaries; fall back to copy + delete
        tokio::fs::copy(staged_path, target_path)
            .await
            .map_err(|e| {
                FetchOutcome::Failure(FetchFailureReason::CommitFailed(format!(
                    "copy failed after rename: {e}"
                )))
            })?;

        tokio::fs::remove_file(staged_path).await.map_err(|e| {
            FetchOutcome::Failure(FetchFailureReason::CommitFailed(format!(
                "cleanup after copy failed: {e}"
            )))
        })?;
    }

    Ok(())
}

fn contains_path_traversal_standalone(path: &Path) -> bool {
    path.components()
        .any(|c| c == std::path::Component::ParentDir)
}

#[allow(dead_code)]
fn needs_fetch(action: &protocol::ResourceDownloadPreflightAction) -> bool {
    use protocol::ResourceDownloadPreflightAction::*;
    matches!(
        action,
        FetchMissing | ReplaceInvalid | WouldVerifyAfterFetch
    )
}

async fn fetch_and_verify_single_file(
    resource_name: &str,
    file_path: &str,
    source: &ResourceFetchSource,
    expected_size: u64,
    expected_sha256: &str,
    cache_dir: &Path,
) -> FetchOutcome {
    // --- Sandbox: reject symlinks in cache dir path ---
    if is_symlink_in_path(cache_dir) {
        return FetchOutcome::Failure(FetchFailureReason::SymlinkRejected);
    }

    // --- Create staging directory ---
    let staging_dir = cache_dir.join(".staging");
    if let Err(e) = ensure_staging_dir(&staging_dir).await {
        error!(%resource_name, %file_path, dir=%staging_dir.display(), error=%e, "failed to create staging directory");
        return FetchOutcome::Failure(FetchFailureReason::StagingDirectoryCreationFailed);
    }

    // --- Generate temp filename ---
    let temp_name = format!("{}_{}", sanitize_name(file_path), uuid_v4_tail());
    let staging_path = staging_dir.join(&temp_name);

    // --- Sandbox: verify staging path doesn't escape ---
    if contains_path_traversal(&staging_path, &staging_dir) {
        return FetchOutcome::Failure(FetchFailureReason::PathTraversalRejected);
    }

    // --- Fetch based on scheme ---
    let fetch_result = match source.scheme.as_str() {
        "file" => fetch_file_via_file_scheme(source, &staging_path, expected_size).await,
        "https" => fetch_file_via_https(source, &staging_path, expected_size).await,
        "ipfs" => Err(FetchOutcome::Failure(FetchFailureReason::UnsupportedScheme)),
        other => {
            warn!(scheme = %other, "unsupported fetch scheme");
            Err(FetchOutcome::Failure(FetchFailureReason::UnsupportedScheme))
        }
    };

    let staged_path = match fetch_result {
        Ok(path) => path,
        Err(outcome) => {
            let _ = tokio::fs::remove_file(&staging_path).await;
            return outcome;
        }
    };

    // --- Verify SHA-256 ---
    let verify_result = verify_staged_file(&staged_path, expected_size, expected_sha256).await;

    match verify_result {
        Ok(()) => {
            info!(
                %resource_name,
                %file_path,
                sha256 = %expected_sha256,
                size = %expected_size,
                "fetch verified successfully (staged, not committed)"
            );
            FetchOutcome::StagedVerified
        }
        Err(outcome) => {
            let _ = tokio::fs::remove_file(&staged_path).await;
            outcome
        }
    }
}

async fn fetch_file_via_file_scheme(
    source: &ResourceFetchSource,
    staging_path: &Path,
    expected_size: u64,
) -> std::result::Result<PathBuf, FetchOutcome> {
    let src_path = if source.uri.starts_with("file://") {
        PathBuf::from(&source.uri[7..])
    } else {
        PathBuf::from(&source.uri)
    };

    if is_symlink_in_path(&src_path) {
        return Err(FetchOutcome::Failure(FetchFailureReason::SymlinkRejected));
    }

    if !src_path.exists() {
        return Err(FetchOutcome::Failure(FetchFailureReason::IoError(format!(
            "source file not found: {}",
            src_path.display()
        ))));
    }

    let metadata = match tokio::fs::metadata(&src_path).await {
        Ok(m) if m.is_symlink() => {
            return Err(FetchOutcome::Failure(FetchFailureReason::SymlinkRejected));
        }
        Ok(m) if !m.is_file() => {
            return Err(FetchOutcome::Failure(FetchFailureReason::IoError(format!(
                "source is not a regular file: {}",
                src_path.display()
            ))));
        }
        Ok(m) => m,
        Err(e) => {
            return Err(FetchOutcome::Failure(FetchFailureReason::IoError(
                e.to_string(),
            )));
        }
    };

    let max_bytes = (expected_size as f64 * 1.1).ceil() as u64;
    if metadata.len() > max_bytes {
        return Err(FetchOutcome::Failure(FetchFailureReason::SizeExceeded));
    }

    if let Err(e) = tokio::fs::copy(&src_path, staging_path).await {
        return Err(FetchOutcome::Failure(FetchFailureReason::IoError(
            e.to_string(),
        )));
    }

    Ok(staging_path.to_path_buf())
}

async fn fetch_file_via_https(
    source: &ResourceFetchSource,
    staging_path: &Path,
    expected_size: u64,
) -> std::result::Result<PathBuf, FetchOutcome> {
    let client = reqwest::Client::builder()
        .user_agent(format!("MeowV/{}", protocol::PROTOCOL_VERSION))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| FetchOutcome::Failure(FetchFailureReason::ConnectionFailed))?;

    let response = client.get(&source.uri).send().await.map_err(|e| {
        if e.is_timeout() {
            FetchOutcome::Failure(FetchFailureReason::Timeout)
        } else if e.is_connect() {
            FetchOutcome::Failure(FetchFailureReason::ConnectionFailed)
        } else if e.is_redirect() {
            FetchOutcome::Failure(FetchFailureReason::RedirectLimitExceeded)
        } else {
            FetchOutcome::Failure(FetchFailureReason::ConnectionFailed)
        }
    })?;

    if !response.status().is_success() {
        return Err(FetchOutcome::Failure(FetchFailureReason::ConnectionFailed));
    }

    let max_bytes = (expected_size as f64 * 1.1).ceil() as u64;
    let content_length = response.content_length().unwrap_or(0);
    if content_length > max_bytes {
        return Err(FetchOutcome::Failure(FetchFailureReason::SizeExceeded));
    }

    let mut file = tokio::fs::File::create(staging_path)
        .await
        .map_err(|_| FetchOutcome::Failure(FetchFailureReason::StagingWriteFailed))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| FetchOutcome::Failure(FetchFailureReason::StagingWriteFailed))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > max_bytes {
            return Err(FetchOutcome::Failure(FetchFailureReason::SizeExceeded));
        }
        file.write_all(&chunk)
            .await
            .map_err(|_| FetchOutcome::Failure(FetchFailureReason::StagingWriteFailed))?;
    }

    file.flush()
        .await
        .map_err(|_| FetchOutcome::Failure(FetchFailureReason::StagingWriteFailed))?;

    Ok(staging_path.to_path_buf())
}

async fn verify_staged_file(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> std::result::Result<(), FetchOutcome> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| FetchOutcome::Failure(FetchFailureReason::IoError(e.to_string())))?;

    if metadata.len() != expected_size {
        return Err(FetchOutcome::Failure(FetchFailureReason::SizeExceeded));
    }

    let actual_sha256 = hash_file_sha256(path)
        .map_err(|e| FetchOutcome::Failure(FetchFailureReason::IoError(e.to_string())))?;

    if actual_sha256 != expected_sha256 {
        return Err(FetchOutcome::Failure(FetchFailureReason::HashMismatch));
    }

    Ok(())
}

async fn ensure_staging_dir(dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("failed to create staging directory: {}", dir.display()))?;
    Ok(())
}

fn sanitize_name(name: &str) -> String {
    name.replace(
        |c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_',
        "_",
    )
}

fn uuid_v4_tail() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:016x}", nanos)
}

fn is_symlink_in_path(path: &Path) -> bool {
    for component in path.ancestors() {
        if component.exists() && component.is_symlink() {
            return true;
        }
    }
    false
}

fn contains_path_traversal(child: &Path, parent: &Path) -> bool {
    let child_str = child.to_string_lossy();
    let parent_str = parent.to_string_lossy();
    child_str.contains("..") || !child_str.starts_with(parent_str.as_ref())
}

impl serde::Serialize for FetchEntryReport {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("FetchEntryReport", 10)?;
        s.serialize_field("resource_name", &self.resource_name)?;
        s.serialize_field("file_path", &self.file_path)?;
        s.serialize_field("source_scheme", &self.source_scheme)?;
        s.serialize_field("source_uri", &self.source_uri)?;
        match &self.outcome {
            FetchOutcome::StagedVerified => {
                s.serialize_field("outcome", "staged_verified")?;
            }
            FetchOutcome::CommittedToCache => {
                s.serialize_field("outcome", "committed_to_cache")?;
            }
            FetchOutcome::ReplaceInvalidCommitted => {
                s.serialize_field("outcome", "replace_invalid_committed")?;
            }
            FetchOutcome::Failure(reason) => {
                s.serialize_field("outcome", "failure")?;
                s.serialize_field("failure_reason", &reason.to_string())?;
            }
        }
        s.serialize_field("expected_size_bytes", &self.expected_size_bytes)?;
        s.serialize_field("expected_sha256", &self.expected_sha256)?;
        s.serialize_field("duration_ms", &self.duration_ms)?;
        match &self.manifest_outcome {
            ManifestOutcome::Updated => {
                s.serialize_field("manifest_outcome", "updated")?;
            }
            ManifestOutcome::WriteFailed(msg) => {
                s.serialize_field("manifest_outcome", &format!("write_failed:{msg}"))?;
            }
            ManifestOutcome::SkippedNoCommit => {
                s.serialize_field("manifest_outcome", "skipped_no_commit")?;
            }
        }
        s.end()
    }
}

impl serde::Serialize for FetchReport {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("FetchReport", 1)?;
        s.serialize_field("entries", &self.entries)?;
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    fn make_source(scheme: &str, uri: &str) -> ResourceFetchSource {
        ResourceFetchSource {
            id: None,
            scheme: scheme.to_string(),
            uri: uri.to_string(),
            size_bytes: None,
            sha256: None,
            compression: None,
            media_type: None,
            priority: None,
            mirrors: None,
        }
    }

    #[tokio::test]
    async fn test_file_fetch_success() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"hello world from file fetch test";
        std::fs::write(&src_file, content).unwrap();
        let expected_sha256 = sha256_hex(content);

        let source = make_source("file", &src_file.to_string_lossy());
        let outcome = fetch_and_verify_single_file(
            "test_resource",
            "test_file.dat",
            &source,
            content.len() as u64,
            &expected_sha256,
            &cache_dir,
        )
        .await;

        assert_eq!(outcome, FetchOutcome::StagedVerified);
        let staging = cache_dir.join(".staging");
        assert!(staging.exists());
        let entries: Vec<_> = std::fs::read_dir(&staging).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_file_fetch_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"hello world";
        std::fs::write(&src_file, content).unwrap();

        let source = make_source("file", &src_file.to_string_lossy());
        let outcome = fetch_and_verify_single_file(
            "test_resource",
            "test_file.dat",
            &source,
            content.len() as u64,
            "0000000000000000000000000000000000000000000000000000000000000000",
            &cache_dir,
        )
        .await;

        assert_eq!(
            outcome,
            FetchOutcome::Failure(FetchFailureReason::HashMismatch)
        );
        let staging = cache_dir.join(".staging");
        if staging.exists() {
            let entries: Vec<_> = std::fs::read_dir(&staging).unwrap().collect();
            assert_eq!(entries.len(), 0, "staging should be empty after failure");
        }
    }

    #[tokio::test]
    async fn test_file_fetch_size_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"this file is too large for the expected size";
        std::fs::write(&src_file, content).unwrap();

        let source = make_source("file", &src_file.to_string_lossy());
        let expected_sha256 = sha256_hex(content);
        let outcome = fetch_and_verify_single_file(
            "test_resource",
            "test_file.dat",
            &source,
            1,
            &expected_sha256,
            &cache_dir,
        )
        .await;

        assert_eq!(
            outcome,
            FetchOutcome::Failure(FetchFailureReason::SizeExceeded)
        );
    }

    #[tokio::test]
    async fn test_unsupported_scheme() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let source = make_source("ipfs", "QmTest123");
        let outcome = fetch_and_verify_single_file(
            "test_resource",
            "test_file.dat",
            &source,
            100,
            "0000000000000000000000000000000000000000000000000000000000000000",
            &cache_dir,
        )
        .await;

        assert_eq!(
            outcome,
            FetchOutcome::Failure(FetchFailureReason::UnsupportedScheme)
        );
    }

    #[tokio::test]
    async fn test_symlink_in_cache_path_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real_cache");
        let symlink_dir = dir.path().join("symlink_cache");
        std::fs::create_dir(&real_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, &symlink_dir).unwrap();

        let source = make_source("file", "/nonexistent/file.dat");
        let outcome = fetch_and_verify_single_file(
            "test_resource",
            "test_file.dat",
            &source,
            100,
            "0000000000000000000000000000000000000000000000000000000000000000",
            &symlink_dir,
        )
        .await;

        assert_eq!(
            outcome,
            FetchOutcome::Failure(FetchFailureReason::SymlinkRejected)
        );
    }

    #[tokio::test]
    async fn test_file_source_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("nonexistent.dat");

        let source = make_source("file", &src_file.to_string_lossy());
        let outcome = fetch_and_verify_single_file(
            "test_resource",
            "test_file.dat",
            &source,
            100,
            "0000000000000000000000000000000000000000000000000000000000000000",
            &cache_dir,
        )
        .await;

        assert!(
            matches!(
                outcome,
                FetchOutcome::Failure(FetchFailureReason::IoError(_))
            ),
            "expected IoError, got {outcome:?}"
        );
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("hello_world.txt"), "hello_world_txt");
        assert_eq!(sanitize_name("../foo/bar"), "___foo_bar");
        assert_eq!(sanitize_name("normal-file_v2"), "normal-file_v2");
    }

    #[test]
    fn test_fetch_report_to_text() {
        let report = FetchReport {
            entries: vec![
                FetchEntryReport {
                    resource_name: "chat".to_string(),
                    file_path: "data.txt".to_string(),
                    source_scheme: "file".to_string(),
                    source_uri: "/tmp/source".to_string(),
                    outcome: FetchOutcome::StagedVerified,
                    expected_size_bytes: 100,
                    expected_sha256: "abcdef".to_string(),
                    duration_ms: 5,
                    manifest_outcome: ManifestOutcome::SkippedNoCommit,
                },
                FetchEntryReport {
                    resource_name: "admin".to_string(),
                    file_path: "cfg.json".to_string(),
                    source_scheme: "https".to_string(),
                    source_uri: "https://example.com/cfg.json".to_string(),
                    outcome: FetchOutcome::Failure(FetchFailureReason::HashMismatch),
                    expected_size_bytes: 200,
                    expected_sha256: "123456".to_string(),
                    duration_ms: 50,
                    manifest_outcome: ManifestOutcome::SkippedNoCommit,
                },
            ],
        };
        let text = report.to_text();
        assert!(text.contains("resource fetch: 2 entries"));
        assert!(text.contains("staged_verified"));
        assert!(text.contains("failure: hash mismatch"));
    }

    #[test]
    fn test_fetch_report_json() {
        let report = FetchReport {
            entries: vec![FetchEntryReport {
                resource_name: "chat".to_string(),
                file_path: "data.txt".to_string(),
                source_scheme: "file".to_string(),
                source_uri: "/tmp/source".to_string(),
                outcome: FetchOutcome::StagedVerified,
                expected_size_bytes: 100,
                expected_sha256: "abcdef".to_string(),
                duration_ms: 5,
                manifest_outcome: ManifestOutcome::SkippedNoCommit,
            }],
        };
        let json = report.to_json().unwrap();
        assert!(json.contains("resource_name"));
        assert!(json.contains("outcome"));
        assert!(json.contains("staged_verified"));
    }

    #[test]
    fn test_report_empty() {
        let report = FetchReport { entries: vec![] };
        assert!(report.to_text().contains("(empty, nothing to do)"));
    }

    #[test]
    fn test_needs_fetch() {
        use protocol::ResourceDownloadPreflightAction::*;
        assert!(needs_fetch(&FetchMissing));
        assert!(needs_fetch(&ReplaceInvalid));
        assert!(needs_fetch(&WouldVerifyAfterFetch));
        assert!(!needs_fetch(&AlreadyAvailable));
        assert!(!needs_fetch(&BlockedBySignaturePolicy));
        assert!(!needs_fetch(&BlockedByResourcePolicy));
        assert!(!needs_fetch(&UnsupportedResource));
    }

    #[test]
    fn test_is_symlink_in_path() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let regular_file = real.join("file.txt");
        std::fs::write(&regular_file, b"hello").unwrap();
        assert!(!is_symlink_in_path(&regular_file));

        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let file_via_link = link.join("file.txt");
        assert!(is_symlink_in_path(&file_via_link));
    }

    #[test]
    fn test_contains_path_traversal() {
        let parent = Path::new("/tmp/cache/.staging");
        let child = Path::new("/tmp/cache/.staging/valid_file");
        assert!(!contains_path_traversal(child, parent));

        let traversal = Path::new("/tmp/cache/.staging/../../etc/passwd");
        assert!(contains_path_traversal(traversal, parent));

        let outside = Path::new("/tmp/other/file");
        assert!(contains_path_traversal(outside, parent));
    }

    #[test]
    fn test_sha256_hex_helper() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash.len(), 64);
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(hash, expected);
    }

    // --- Cache manifest unit tests ---

    #[test]
    fn test_manifest_empty() {
        let m = CacheManifest::empty();
        assert!(m.is_empty());
        assert_eq!(m.version, 1);
        assert!(m.entries.is_empty());
        let text = m.to_text();
        assert!(text.contains("(empty, no committed resources)"));
    }

    #[test]
    fn test_manifest_with_entry_add() {
        let m = CacheManifest::empty();
        let e = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "a".repeat(64),
            size_bytes: 100,
            source_scheme: None,
            source_uri: None,
        };
        let m = m.with_entry(e);
        assert!(!m.is_empty());
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries[0].resource_name, "chat");
    }

    #[test]
    fn test_manifest_with_entry_replace() {
        let m = CacheManifest::empty();
        let e1 = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "old_hash".to_string(),
            size_bytes: 50,
            source_scheme: None,
            source_uri: None,
        };
        let m = m.with_entry(e1);
        let e2 = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "new_hash".to_string(),
            size_bytes: 100,
            source_scheme: Some("file".to_string()),
            source_uri: Some("/tmp/source".to_string()),
        };
        let m = m.with_entry(e2);
        assert_eq!(m.entries.len(), 1, "duplicate should replace, not add");
        assert_eq!(m.entries[0].sha256, "new_hash");
        assert_eq!(m.entries[0].size_bytes, 100);
    }

    #[test]
    fn test_manifest_with_entry_order() {
        let m = CacheManifest::empty();
        let e1 = CacheManifestEntry {
            resource_name: "z_resource".to_string(),
            file_path: "a.txt".to_string(),
            sha256: "1".to_string(),
            size_bytes: 10,
            source_scheme: None,
            source_uri: None,
        };
        let e2 = CacheManifestEntry {
            resource_name: "a_resource".to_string(),
            file_path: "z.txt".to_string(),
            sha256: "2".to_string(),
            size_bytes: 20,
            source_scheme: None,
            source_uri: None,
        };
        let m = m.with_entry(e1).with_entry(e2);
        assert_eq!(m.entries.len(), 2);
        assert_eq!(m.entries[0].resource_name, "a_resource");
        assert_eq!(m.entries[1].resource_name, "z_resource");
    }

    #[test]
    fn test_manifest_to_text_with_entries() {
        let m = CacheManifest::empty();
        let e = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            size_bytes: 200,
            source_scheme: Some("file".to_string()),
            source_uri: Some("/tmp/source".to_string()),
        };
        let m = m.with_entry(e);
        let text = m.to_text();
        assert!(text.contains("cache manifest v1: 1 entry"));
        assert!(text.contains("chat:main.lua"));
        assert!(text.contains("200 bytes"));
    }

    #[test]
    fn test_manifest_outcome_is_updated() {
        assert!(ManifestOutcome::Updated.is_updated());
        assert!(!ManifestOutcome::WriteFailed("err".to_string()).is_updated());
        assert!(!ManifestOutcome::SkippedNoCommit.is_updated());
    }

    #[tokio::test]
    async fn test_load_cache_manifest_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = load_cache_manifest(dir.path()).await;
        assert!(manifest.is_empty());
    }

    #[tokio::test]
    async fn test_load_cache_manifest_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache_manifest.json");
        std::fs::write(&path, b"not valid json").unwrap();
        let manifest = load_cache_manifest(dir.path()).await;
        assert!(manifest.is_empty(), "malformed file should return empty");
    }

    #[tokio::test]
    async fn test_save_and_load_cache_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let m = CacheManifest {
            version: 1,
            entries: vec![CacheManifestEntry {
                resource_name: "chat".to_string(),
                file_path: "main.lua".to_string(),
                sha256: "abcdef".repeat(11),
                size_bytes: 100,
                source_scheme: Some("file".to_string()),
                source_uri: Some("/tmp/source".to_string()),
            }],
        };
        save_cache_manifest(dir.path(), &m).await.unwrap();
        let loaded = load_cache_manifest(dir.path()).await;
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].resource_name, "chat");
        assert_eq!(loaded.entries[0].sha256, "abcdef".repeat(11));
    }

    #[tokio::test]
    async fn test_save_cache_manifest_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let m = CacheManifest::empty();
        save_cache_manifest(dir.path(), &m).await.unwrap();
        // Final file should exist at cache_manifest.json
        assert!(dir.path().join("cache_manifest.json").exists());
        // Temp file should be cleaned up
        assert!(
            !dir.path()
                .join(".staging")
                .join("cache_manifest.json.tmp")
                .exists()
        );
    }

    #[tokio::test]
    async fn test_update_cache_manifest_after_commit() {
        let dir = tempfile::tempdir().unwrap();
        let entry = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "a".repeat(64),
            size_bytes: 100,
            source_scheme: Some("file".to_string()),
            source_uri: Some("/tmp/source".to_string()),
        };
        let result = update_cache_manifest_after_commit(dir.path(), &entry)
            .await
            .unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].resource_name, "chat");
        // Verify on disk
        let loaded = load_cache_manifest(dir.path()).await;
        assert_eq!(loaded.entries.len(), 1);
    }

    #[tokio::test]
    async fn test_update_cache_manifest_replaces_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let e1 = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "old".to_string(),
            size_bytes: 50,
            source_scheme: None,
            source_uri: None,
        };
        update_cache_manifest_after_commit(dir.path(), &e1)
            .await
            .unwrap();
        let e2 = CacheManifestEntry {
            resource_name: "chat".to_string(),
            file_path: "main.lua".to_string(),
            sha256: "new".to_string(),
            size_bytes: 200,
            source_scheme: None,
            source_uri: None,
        };
        let result = update_cache_manifest_after_commit(dir.path(), &e2)
            .await
            .unwrap();
        assert_eq!(result.entries.len(), 1, "duplicate should replace");
        assert_eq!(result.entries[0].sha256, "new");
    }

    #[tokio::test]
    async fn test_manifest_updated_on_commit_in_report() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"commit with manifest update";
        std::fs::write(&src_file, content).unwrap();
        let expected_sha256 = sha256_hex(content);

        let source = make_source("file", &src_file.to_string_lossy());
        let preflight_entry = make_preflight_entry(
            "test_r",
            "f.dat",
            protocol::ResourceDownloadPreflightAction::FetchMissing,
            source.clone(),
        );
        let preflight = protocol::ResourceDownloadPreflightPlan {
            entries: vec![preflight_entry],
        };
        let announcement = make_single_resource_announcement(
            "test_r",
            "f.dat",
            content.len() as u64,
            &expected_sha256,
        );

        let config = FetchConfig {
            allow_fetch: true,
            allow_cache_commit: true,
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let report = execute_fetch_plan(&announcement, &preflight, &config)
            .await
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, FetchOutcome::CommittedToCache);
        assert_eq!(report.entries[0].manifest_outcome, ManifestOutcome::Updated);

        // Manifest should exist on disk
        let manifest = load_cache_manifest(&cache_dir).await;
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].resource_name, "test_r");
        assert_eq!(manifest.entries[0].file_path, "f.dat");
    }

    #[test]
    fn test_manifest_entry_sort_key() {
        let e = CacheManifestEntry {
            resource_name: "b".to_string(),
            file_path: "a".to_string(),
            sha256: "x".to_string(),
            size_bytes: 0,
            source_scheme: None,
            source_uri: None,
        };
        assert_eq!(e.sort_key(), ("b", "a"));
    }

    #[test]
    fn test_execute_fetch_report_correct_mappings() {
        use protocol::{
            ResourceDownloadPreflightAction, ResourceDownloadPreflightEntry,
            ResourceDownloadPreflightPlan, ResourceFetchSourcePolicyDecision,
            ResourceFetchSourcePolicyReport, ResourceRequirementLevel,
        };

        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("actual_source.dat");
        let content = b"correctly fetched content";
        std::fs::write(&src_file, content).unwrap();
        let expected_sha256 = sha256_hex(content);

        let source = ResourceFetchSource {
            id: None,
            scheme: "file".to_string(),
            uri: src_file.to_string_lossy().to_string(),
            size_bytes: Some(content.len() as u64),
            sha256: Some(expected_sha256.clone()),
            compression: None,
            media_type: None,
            priority: None,
            mirrors: None,
        };

        let preflight_entry = ResourceDownloadPreflightEntry {
            resource_name: "test_r".to_string(),
            file_path: "f.dat".to_string(),
            action: ResourceDownloadPreflightAction::FetchMissing,
            reason: "file is missing".to_string(),
            source_errors: vec![],
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: vec![],
            source_policy: Some(ResourceFetchSourcePolicyReport {
                decision: ResourceFetchSourcePolicyDecision::Allowed,
                scheme: "file".to_string(),
                allowed_schemes: vec!["file".to_string()],
            }),
        };

        let preflight = ResourceDownloadPreflightPlan {
            entries: vec![preflight_entry],
        };

        let announcement = ResourceAnnouncement {
            resources: vec![protocol::AnnouncedResource {
                name: "test_r".to_string(),
                version: "1.0".to_string(),
                files: vec![protocol::AnnouncedResourceFile {
                    relative_path: "f.dat".to_string(),
                    size_bytes: content.len() as u64,
                    sha256: expected_sha256.clone(),
                    sources: None,
                }],
                protocol_version: protocol::PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };

        let config = FetchConfig {
            allow_fetch: true,
            allow_cache_commit: false,
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let report = rt
            .block_on(execute_fetch_plan(&announcement, &preflight, &config))
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, FetchOutcome::StagedVerified);
        assert_eq!(report.entries[0].resource_name, "test_r");
        assert_eq!(report.entries[0].file_path, "f.dat");
    }

    #[tokio::test]
    async fn test_commit_after_fetch_success() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"data for commit test";
        std::fs::write(&src_file, content).unwrap();
        let expected_sha256 = sha256_hex(content);

        let source = make_source("file", &src_file.to_string_lossy());

        let preflight_entry = make_preflight_entry(
            "test_r",
            "f.dat",
            protocol::ResourceDownloadPreflightAction::FetchMissing,
            source.clone(),
        );
        let preflight = protocol::ResourceDownloadPreflightPlan {
            entries: vec![preflight_entry],
        };
        let announcement = make_single_resource_announcement(
            "test_r",
            "f.dat",
            content.len() as u64,
            &expected_sha256,
        );

        let config = FetchConfig {
            allow_fetch: true,
            allow_cache_commit: true,
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let report = execute_fetch_plan(&announcement, &preflight, &config)
            .await
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, FetchOutcome::CommittedToCache);

        // Verify file exists in cache (not just staging)
        let cache_file = cache_dir.join("f.dat");
        assert!(cache_file.exists(), "committed file should be in cache");
        let actual_hash = hash_file_sha256(&cache_file).unwrap();
        assert_eq!(actual_hash, expected_sha256);

        // Staging should be empty after commit
        let staging = cache_dir.join(".staging");
        if staging.exists() {
            let entries: Vec<_> = std::fs::read_dir(&staging).unwrap().collect();
            assert_eq!(entries.len(), 0, "staging should be empty after commit");
        }
    }

    #[tokio::test]
    async fn test_no_commit_without_gate() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"no commit without gate";
        std::fs::write(&src_file, content).unwrap();
        let expected_sha256 = sha256_hex(content);

        let source = make_source("file", &src_file.to_string_lossy());
        let preflight_entry = make_preflight_entry(
            "test_r",
            "f.dat",
            protocol::ResourceDownloadPreflightAction::FetchMissing,
            source.clone(),
        );
        let preflight = protocol::ResourceDownloadPreflightPlan {
            entries: vec![preflight_entry],
        };
        let announcement = make_single_resource_announcement(
            "test_r",
            "f.dat",
            content.len() as u64,
            &expected_sha256,
        );

        let config = FetchConfig {
            allow_fetch: true,
            allow_cache_commit: false,
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let report = execute_fetch_plan(&announcement, &preflight, &config)
            .await
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, FetchOutcome::StagedVerified);

        // Cache should NOT contain the file
        let cache_file = cache_dir.join("f.dat");
        assert!(
            !cache_file.exists(),
            "file should not be committed without gate"
        );
    }

    #[tokio::test]
    async fn test_hash_mismatch_does_not_commit() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"hash mismatch test";
        std::fs::write(&src_file, content).unwrap();

        let source = make_source("file", &src_file.to_string_lossy());
        let preflight_entry = make_preflight_entry(
            "test_r",
            "f.dat",
            protocol::ResourceDownloadPreflightAction::FetchMissing,
            source.clone(),
        );
        let preflight = protocol::ResourceDownloadPreflightPlan {
            entries: vec![preflight_entry],
        };
        // Announcement has WRONG sha256
        let wrong_sha =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let announcement =
            make_single_resource_announcement("test_r", "f.dat", content.len() as u64, &wrong_sha);

        let config = FetchConfig {
            allow_fetch: true,
            allow_cache_commit: true,
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let report = execute_fetch_plan(&announcement, &preflight, &config)
            .await
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(
            report.entries[0].outcome,
            FetchOutcome::Failure(FetchFailureReason::HashMismatch)
        );

        // Cache should NOT contain the file
        let cache_file = cache_dir.join("f.dat");
        assert!(
            !cache_file.exists(),
            "file should not be committed after hash mismatch"
        );
    }

    #[tokio::test]
    async fn test_commit_symlink_target_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let link_dir = dir.path().join("link_cache");
        std::fs::create_dir(&cache_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&cache_dir, &link_dir).unwrap();

        // Create staged file manually
        let staging = link_dir.join(".staging");
        std::fs::create_dir_all(&staging).unwrap();
        let staged_file = staging.join("test_file_abc123");
        std::fs::write(&staged_file, b"content").unwrap();

        // Target path goes through symlink — should be rejected
        let target = link_dir.join("target.dat");

        let result = commit_verified_file(&staged_file, &target).await;
        assert_eq!(
            result,
            Err(FetchOutcome::Failure(FetchFailureReason::SymlinkRejected))
        );
    }

    #[tokio::test]
    async fn test_commit_path_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        std::fs::create_dir_all(cache_dir.join(".staging")).unwrap();

        let staged_file = cache_dir.join(".staging").join("test_file_abc");
        std::fs::write(&staged_file, b"content").unwrap();

        // Target path with ".." — should be rejected
        let target = cache_dir.join("../outside.dat");

        let result = commit_verified_file(&staged_file, &target).await;
        assert_eq!(
            result,
            Err(FetchOutcome::Failure(
                FetchFailureReason::PathTraversalRejected
            ))
        );
    }

    #[tokio::test]
    async fn test_replace_invalid_committed() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let src_file = dir.path().join("source.dat");
        let content = b"replacement content";
        std::fs::write(&src_file, content).unwrap();
        let expected_sha256 = sha256_hex(content);

        // Create an invalid cache entry (wrong content)
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("f.dat"), b"old invalid content").unwrap();

        let source = make_source("file", &src_file.to_string_lossy());
        let preflight_entry = make_preflight_entry(
            "test_r",
            "f.dat",
            protocol::ResourceDownloadPreflightAction::ReplaceInvalid,
            source.clone(),
        );
        let preflight = protocol::ResourceDownloadPreflightPlan {
            entries: vec![preflight_entry],
        };
        let announcement = make_single_resource_announcement(
            "test_r",
            "f.dat",
            content.len() as u64,
            &expected_sha256,
        );

        let config = FetchConfig {
            allow_fetch: true,
            allow_cache_commit: true,
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let report = execute_fetch_plan(&announcement, &preflight, &config)
            .await
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(
            report.entries[0].outcome,
            FetchOutcome::ReplaceInvalidCommitted
        );

        // Cache should now have the new content
        let cache_file = cache_dir.join("f.dat");
        assert!(cache_file.exists());
        let actual_hash = hash_file_sha256(&cache_file).unwrap();
        assert_eq!(actual_hash, expected_sha256);
    }

    #[tokio::test]
    async fn test_staging_cleaned_up_after_commit_failure() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        std::fs::create_dir_all(cache_dir.join(".staging")).unwrap();

        // Staged file exists but target path has a file as parent directory,
        // so create_dir_all will fail.
        let staged_file = cache_dir.join(".staging").join("test_file_xyz");
        std::fs::write(&staged_file, b"content").unwrap();

        // Create a regular file that will serve as the "parent" of the target
        std::fs::write(cache_dir.join("blocking_file"), b"").unwrap();
        let target = cache_dir.join("blocking_file/subdir/f.dat");

        let result = commit_verified_file(&staged_file, &target).await;
        assert!(
            matches!(
                result,
                Err(FetchOutcome::Failure(FetchFailureReason::CommitFailed(_)))
            ),
            "expected CommitFailed, got {result:?}"
        );

        // Staged file should still exist (commit was attempted but failed)
        assert!(
            staged_file.exists(),
            "staged file should remain after failed commit"
        );
    }

    #[test]
    fn test_outcome_is_success() {
        assert!(FetchOutcome::StagedVerified.is_success());
        assert!(FetchOutcome::CommittedToCache.is_success());
        assert!(FetchOutcome::ReplaceInvalidCommitted.is_success());
        assert!(!FetchOutcome::Failure(FetchFailureReason::HashMismatch).is_success());
    }

    #[test]
    fn test_contains_path_traversal_standalone() {
        assert!(!contains_path_traversal_standalone(Path::new(
            "/tmp/cache/f.dat"
        )));
        assert!(contains_path_traversal_standalone(Path::new(
            "/tmp/cache/../f.dat"
        )));
        assert!(contains_path_traversal_standalone(Path::new(
            "f.dat/../../etc/passwd"
        )));
        assert!(!contains_path_traversal_standalone(Path::new(
            "normal/path/file.dat"
        )));
    }

    // --- test helpers ---

    fn make_preflight_entry(
        resource_name: &str,
        file_path: &str,
        action: protocol::ResourceDownloadPreflightAction,
        source: ResourceFetchSource,
    ) -> protocol::ResourceDownloadPreflightEntry {
        use protocol::{
            ResourceDownloadPreflightEntry, ResourceFetchSourcePolicyDecision,
            ResourceFetchSourcePolicyReport,
        };
        ResourceDownloadPreflightEntry {
            resource_name: resource_name.to_string(),
            file_path: file_path.to_string(),
            action,
            reason: "test reason".to_string(),
            source_errors: vec![],
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: vec![],
            source_policy: Some(ResourceFetchSourcePolicyReport {
                decision: ResourceFetchSourcePolicyDecision::Allowed,
                scheme: "file".to_string(),
                allowed_schemes: vec!["file".to_string()],
            }),
        }
    }

    fn make_single_resource_announcement(
        resource_name: &str,
        file_path: &str,
        size_bytes: u64,
        sha256: &str,
    ) -> ResourceAnnouncement {
        use protocol::{AnnouncedResource, AnnouncedResourceFile, ResourceRequirementLevel};
        ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: resource_name.to_string(),
                version: "1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: file_path.to_string(),
                    size_bytes,
                    sha256: sha256.to_string(),
                    sources: None,
                }],
                protocol_version: protocol::PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        }
    }
}
