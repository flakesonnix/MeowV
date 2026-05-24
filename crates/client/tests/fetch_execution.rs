use client::fetch::{
    FetchConfig, FetchFailureReason, FetchOutcome, ManifestOutcome, execute_fetch_plan,
};
use protocol::{
    AnnouncedResource, AnnouncedResourceFile, PROTOCOL_VERSION, ResourceAnnouncement,
    ResourceDownloadPreflightAction, ResourceDownloadPreflightEntry, ResourceDownloadPreflightPlan,
    ResourceFetchSource, ResourceFetchSourcePolicyDecision, ResourceFetchSourcePolicyReport,
    ResourceRequirementLevel,
};

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

fn make_preflight_entry(
    resource_name: &str,
    file_path: &str,
    action: ResourceDownloadPreflightAction,
    source: ResourceFetchSource,
) -> ResourceDownloadPreflightEntry {
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
            protocol_version: PROTOCOL_VERSION,
            requirement_level: ResourceRequirementLevel::Required,
        }],
        signature: None,
    }
}

/// Integration test: fetch a missing file via file:// source.
/// Verifies the full pipeline: preflight entry → fetch → verify → report.
#[test]
fn fetch_missing_file_via_file_source_reports_success() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"hello from integration test";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "test_resource",
        "test.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement = make_single_resource_announcement(
        "test_resource",
        "test.dat",
        content.len() as u64,
        &expected_sha,
    );

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
    assert_eq!(report.entries[0].resource_name, "test_resource");
    assert_eq!(report.entries[0].file_path, "test.dat");

    // Verify the staged file exists
    let staged = cache_dir.join(".staging");
    assert!(staged.exists(), "staging directory should exist");
    let entries: Vec<_> = std::fs::read_dir(&staged).unwrap().collect();
    assert_eq!(entries.len(), 1, "one file should be in staging");
}

/// Integration test: fetch is skipped when --allow-fetch is false.
#[test]
fn fetch_skipped_when_not_allowed() {
    let preflight = ResourceDownloadPreflightPlan { entries: vec![] };
    let announcement = ResourceAnnouncement {
        resources: vec![],
        signature: None,
    };
    let config = FetchConfig {
        allow_fetch: false,
        allow_cache_commit: false,
        cache_dir: Some("/tmp/nonexistent".to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();
    assert!(report.entries.is_empty());
}

/// Integration test: fetch is skipped when no cache dir is configured.
#[test]
fn fetch_skipped_when_no_cache_dir() {
    let preflight = ResourceDownloadPreflightPlan { entries: vec![] };
    let announcement = ResourceAnnouncement {
        resources: vec![],
        signature: None,
    };
    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: false,
        cache_dir: None,
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();
    assert!(report.entries.is_empty());
}

/// Integration test: fetch with hash mismatch cleans up staging.
#[test]
fn fetch_hash_mismatch_cleans_staging() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"data for hash mismatch test";
    std::fs::write(&source_file, content).unwrap();

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "test_r",
        "bad_hash.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    // Announcement has WRONG sha256
    let wrong_sha = "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let announcement = make_single_resource_announcement(
        "test_r",
        "bad_hash.dat",
        content.len() as u64,
        &wrong_sha,
    );

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
    assert_eq!(
        report.entries[0].outcome,
        FetchOutcome::Failure(FetchFailureReason::HashMismatch)
    );

    // Staging should be empty after cleanup
    let staged = cache_dir.join(".staging");
    if staged.exists() {
        let entries: Vec<_> = std::fs::read_dir(&staged).unwrap().collect();
        assert_eq!(
            entries.len(),
            0,
            "staging should be cleaned up after failure"
        );
    }
}

/// Integration test: fetch report text output.
#[test]
fn fetch_report_to_text_includes_all_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"report integration test";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "report_r",
        "r.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement =
        make_single_resource_announcement("report_r", "r.dat", content.len() as u64, &expected_sha);

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

    let text = report.to_text();
    assert!(text.contains("resource fetch: 1 entry"));
    assert!(text.contains("staged_verified"));
    assert!(text.contains("file"));
}

/// Integration test: fetch report JSON output.
#[test]
fn fetch_report_json_is_valid() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"json integration test";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "json_r",
        "j.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement =
        make_single_resource_announcement("json_r", "j.dat", content.len() as u64, &expected_sha);

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

    let json = report.to_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let entries = parsed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["outcome"], "staged_verified");
    assert_eq!(entries[0]["resource_name"], "json_r");
}

// --- Cache commit integration tests ---

/// Integration test: fetch + cache commit with allow_cache_commit.
#[test]
fn fetch_commits_to_cache_when_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"data for cache commit integration test";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "commit_r",
        "committed.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement = make_single_resource_announcement(
        "commit_r",
        "committed.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].outcome, FetchOutcome::CommittedToCache);

    // File should exist in cache root
    let cached = cache_dir.join("committed.dat");
    assert!(cached.exists(), "committed file should exist in cache");

    // Staging should be empty
    let staging = cache_dir.join(".staging");
    if staging.exists() {
        let entries: Vec<_> = std::fs::read_dir(&staging).unwrap().collect();
        assert_eq!(entries.len(), 0, "staging should be empty after commit");
    }
}

/// Integration test: fetch + cache commit with ReplaceInvalid action.
#[test]
fn fetch_replace_invalid_commits_to_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"fresh replacement content";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    // Create stale cache entry
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("stale.dat"), b"old stale content").unwrap();

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "replace_r",
        "stale.dat",
        ResourceDownloadPreflightAction::ReplaceInvalid,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement = make_single_resource_announcement(
        "replace_r",
        "stale.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(
        report.entries[0].outcome,
        FetchOutcome::ReplaceInvalidCommitted
    );

    // Cache should now have new content
    let cached = cache_dir.join("stale.dat");
    assert!(cached.exists());
    let actual_hash = sha256_hex(&std::fs::read(&cached).unwrap());
    assert_eq!(actual_hash, expected_sha);
}

/// Integration test: hash mismatch does NOT commit even when allow_cache_commit is true.
#[test]
fn fetch_hash_mismatch_does_not_commit() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"data that will mismatch";
    std::fs::write(&source_file, content).unwrap();

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "bad_hash_r",
        "bad.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    // Wrong sha256 so it will mismatch
    let wrong_sha = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    let announcement = make_single_resource_announcement(
        "bad_hash_r",
        "bad.dat",
        content.len() as u64,
        &wrong_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true, // should still not commit because hash doesn't match
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(
        report.entries[0].outcome,
        FetchOutcome::Failure(FetchFailureReason::HashMismatch)
    );

    // Cache should NOT have the file
    let cached = cache_dir.join("bad.dat");
    assert!(
        !cached.exists(),
        "file should not exist in cache after hash mismatch"
    );
}

/// Integration test: committed cache file has correct content and sha256.
#[test]
fn fetch_committed_file_content_is_correct() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"verify committed file content";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "verify_r",
        "subdir/verified.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement = make_single_resource_announcement(
        "verify_r",
        "subdir/verified.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].outcome, FetchOutcome::CommittedToCache);

    // Verify file content
    let cached = cache_dir.join("subdir/verified.dat");
    assert!(cached.exists());
    let actual = std::fs::read(&cached).unwrap();
    assert_eq!(actual, content);
    let actual_hash = sha256_hex(&actual);
    assert_eq!(actual_hash, expected_sha);
}

/// Integration test: committed outcome appears correctly in text report.
#[test]
fn fetch_commit_report_to_text() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"commit report text check";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "text_r",
        "t.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement =
        make_single_resource_announcement("text_r", "t.dat", content.len() as u64, &expected_sha);

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    let text = report.to_text();
    assert!(
        text.contains("committed_to_cache"),
        "report should mention committed_to_cache"
    );
    assert!(text.contains("resource fetch: 1 entry"));
}

/// Integration test: committed outcome appears correctly in JSON report.
#[test]
fn fetch_commit_report_json() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"commit report json check";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "json_r2",
        "j2.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement =
        make_single_resource_announcement("json_r2", "j2.dat", content.len() as u64, &expected_sha);

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    let json = report.to_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let entries = parsed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["outcome"], "committed_to_cache");
    assert_eq!(entries[0]["resource_name"], "json_r2");
}

// --- Cache manifest integration tests ---

/// Integration test: manifest is created after first commit.
#[test]
fn fetch_commit_creates_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let src_file = dir.path().join("source.dat");
    let content = b"manifest creation test";
    std::fs::write(&src_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &src_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "manifest_r",
        "m.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );
    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };
    let announcement = make_single_resource_announcement(
        "manifest_r",
        "m.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].outcome, FetchOutcome::CommittedToCache);
    assert_eq!(report.entries[0].manifest_outcome, ManifestOutcome::Updated);

    // Verify manifest file exists
    let manifest_path = cache_dir.join("cache_manifest.json");
    assert!(manifest_path.exists(), "manifest should exist after commit");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["entries"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["entries"][0]["resource_name"], "manifest_r");
}

/// Integration test: manifest not created when fetch only stages (no commit).
#[test]
fn fetch_stage_only_does_not_create_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let src_file = dir.path().join("source.dat");
    let content = b"stage only test";
    std::fs::write(&src_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &src_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "stage_r",
        "s.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );
    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };
    let announcement =
        make_single_resource_announcement("stage_r", "s.dat", content.len() as u64, &expected_sha);

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
    assert_eq!(
        report.entries[0].manifest_outcome,
        ManifestOutcome::SkippedNoCommit
    );

    // Manifest should NOT exist
    let manifest_path = cache_dir.join("cache_manifest.json");
    assert!(
        !manifest_path.exists(),
        "manifest should NOT exist after stage only"
    );
}

/// Integration test: manifest report shows in JSON output.
#[test]
fn fetch_commit_manifest_in_json_report() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let src_file = dir.path().join("source.dat");
    let content = b"manifest json report";
    std::fs::write(&src_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    let source = make_source("file", &src_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "json_manifest_r",
        "jm.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );
    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };
    let announcement = make_single_resource_announcement(
        "json_manifest_r",
        "jm.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    let json = report.to_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let entry = &parsed["entries"][0];
    assert_eq!(entry["outcome"], "committed_to_cache");
    assert_eq!(entry["manifest_outcome"], "updated");
}

/// Integration test: replace invalid also updates manifest.
#[test]
fn fetch_replace_invalid_updates_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let src_file = dir.path().join("source.dat");
    let content = b"replace invalid manifest test";
    std::fs::write(&src_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    // Create stale cache entry
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("old.dat"), b"stale").unwrap();

    let source = make_source("file", &src_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "rep_manifest_r",
        "old.dat",
        ResourceDownloadPreflightAction::ReplaceInvalid,
        source,
    );
    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };
    let announcement = make_single_resource_announcement(
        "rep_manifest_r",
        "old.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(
        report.entries[0].outcome,
        FetchOutcome::ReplaceInvalidCommitted
    );
    assert_eq!(report.entries[0].manifest_outcome, ManifestOutcome::Updated);

    // Manifest should exist with correct entry
    let manifest_path = cache_dir.join("cache_manifest.json");
    assert!(manifest_path.exists());
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["entries"][0]["resource_name"], "rep_manifest_r");
}

/// Integration test: hash mismatch does not create manifest.
#[test]
fn fetch_hash_mismatch_does_not_create_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let src_file = dir.path().join("source.dat");
    let content = b"hash mismatch no manifest";
    std::fs::write(&src_file, content).unwrap();

    let source = make_source("file", &src_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "bad_r",
        "bad.dat",
        ResourceDownloadPreflightAction::FetchMissing,
        source,
    );
    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };
    let wrong_sha = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    let announcement =
        make_single_resource_announcement("bad_r", "bad.dat", content.len() as u64, &wrong_sha);

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert!(matches!(
        report.entries[0].outcome,
        FetchOutcome::Failure(_)
    ));
    assert_eq!(
        report.entries[0].manifest_outcome,
        ManifestOutcome::SkippedNoCommit
    );

    let manifest_path = cache_dir.join("cache_manifest.json");
    assert!(
        !manifest_path.exists(),
        "manifest should not exist after hash mismatch"
    );
}

/// Integration test: replace_invalid_committed appears correctly in text report.
#[test]
fn fetch_replace_invalid_report_to_text() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let source_file = dir.path().join("source.dat");
    let content = b"replace invalid text report";
    std::fs::write(&source_file, content).unwrap();
    let expected_sha = sha256_hex(content);

    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("old.dat"), b"stale").unwrap();

    let source = make_source("file", &source_file.to_string_lossy());
    let preflight_entry = make_preflight_entry(
        "rep_text_r",
        "old.dat",
        ResourceDownloadPreflightAction::ReplaceInvalid,
        source,
    );

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![preflight_entry],
    };

    let announcement = make_single_resource_announcement(
        "rep_text_r",
        "old.dat",
        content.len() as u64,
        &expected_sha,
    );

    let config = FetchConfig {
        allow_fetch: true,
        allow_cache_commit: true,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
        fetch_report_path: None,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_fetch_plan(&announcement, &preflight, &config))
        .unwrap();

    let text = report.to_text();
    assert!(
        text.contains("replace_invalid_committed"),
        "report should mention replace_invalid_committed"
    );
}
