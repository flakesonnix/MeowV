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
    Success,
    Failure(FetchFailureReason),
}

impl FetchOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

/// Reason why a file fetch failed.
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
}

impl FetchEntryReport {
    fn to_text(&self) -> String {
        let status = match &self.outcome {
            FetchOutcome::Success => "success".to_string(),
            FetchOutcome::Failure(reason) => format!("failure: {reason}"),
        };
        format!(
            "  [{}] {}:{} - {} ({} ms, {} bytes, sha256:{})",
            status,
            self.resource_name,
            self.file_path,
            self.source_scheme,
            self.duration_ms,
            self.expected_size_bytes,
            &self.expected_sha256[..self.expected_sha256.len().min(16)],
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

/// Configuration for fetch execution.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub allow_fetch: bool,
    pub cache_dir: Option<String>,
    pub fetch_report_path: Option<String>,
}

/// Execute the fetch plan for all resources in an announcement.
///
/// Only runs if `config.allow_fetch` is `true`. For each entry in the
/// preflight plan that requires fetching and has `plan_ok`, the file
/// is fetched to `<cache_dir>/.staging/`, verified, and left in staging
/// (no cache commit). Returns a `FetchReport` with per-entry results.
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
                });
                continue;
            }
        };

        let start = Instant::now();
        let outcome = fetch_and_verify_single_file(
            &exec_entry.resource_name,
            &exec_entry.file_path,
            &source,
            expected_size,
            &expected_sha256,
            &cache_dir,
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;
        report_entries.push(FetchEntryReport {
            resource_name: exec_entry.resource_name.clone(),
            file_path: exec_entry.file_path.clone(),
            source_scheme: source.scheme.clone(),
            source_uri: source.uri.clone(),
            outcome,
            expected_size_bytes: expected_size,
            expected_sha256,
            duration_ms,
        });
    }

    Ok(FetchReport {
        entries: report_entries,
    })
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
            FetchOutcome::Success
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
        let mut s = serializer.serialize_struct("FetchEntryReport", 9)?;
        s.serialize_field("resource_name", &self.resource_name)?;
        s.serialize_field("file_path", &self.file_path)?;
        s.serialize_field("source_scheme", &self.source_scheme)?;
        s.serialize_field("source_uri", &self.source_uri)?;
        match &self.outcome {
            FetchOutcome::Success => {
                s.serialize_field("outcome", "success")?;
            }
            FetchOutcome::Failure(reason) => {
                s.serialize_field("outcome", "failure")?;
                s.serialize_field("failure_reason", &reason.to_string())?;
            }
        }
        s.serialize_field("expected_size_bytes", &self.expected_size_bytes)?;
        s.serialize_field("expected_sha256", &self.expected_sha256)?;
        s.serialize_field("duration_ms", &self.duration_ms)?;
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

        assert_eq!(outcome, FetchOutcome::Success);
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
                    outcome: FetchOutcome::Success,
                    expected_size_bytes: 100,
                    expected_sha256: "abcdef".to_string(),
                    duration_ms: 5,
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
                },
            ],
        };
        let text = report.to_text();
        assert!(text.contains("resource fetch: 2 entries"));
        assert!(text.contains("success"));
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
                outcome: FetchOutcome::Success,
                expected_size_bytes: 100,
                expected_sha256: "abcdef".to_string(),
                duration_ms: 5,
            }],
        };
        let json = report.to_json().unwrap();
        assert!(json.contains("resource_name"));
        assert!(json.contains("outcome"));
        assert!(json.contains("success"));
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
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            fetch_report_path: None,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let report = rt
            .block_on(execute_fetch_plan(&announcement, &preflight, &config))
            .unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, FetchOutcome::Success);
        assert_eq!(report.entries[0].resource_name, "test_r");
        assert_eq!(report.entries[0].file_path, "f.dat");
    }
}
