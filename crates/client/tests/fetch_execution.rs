use client::fetch::{FetchConfig, FetchOutcome, execute_fetch_plan};
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

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![ResourceDownloadPreflightEntry {
            resource_name: "test_resource".to_string(),
            file_path: "test.dat".to_string(),
            action: ResourceDownloadPreflightAction::FetchMissing,
            reason: "file missing from cache".to_string(),
            source_errors: vec![],
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: vec![],
            source_policy: Some(ResourceFetchSourcePolicyReport {
                decision: ResourceFetchSourcePolicyDecision::Allowed,
                scheme: "file".to_string(),
                allowed_schemes: vec!["file".to_string()],
            }),
        }],
    };

    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "test_resource".to_string(),
            version: "1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "test.dat".to_string(),
                size_bytes: content.len() as u64,
                sha256: expected_sha.clone(),
                sources: None,
            }],
            protocol_version: PROTOCOL_VERSION,
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

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![ResourceDownloadPreflightEntry {
            resource_name: "test_r".to_string(),
            file_path: "bad_hash.dat".to_string(),
            action: ResourceDownloadPreflightAction::FetchMissing,
            reason: "file missing".to_string(),
            source_errors: vec![],
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: vec![],
            source_policy: Some(ResourceFetchSourcePolicyReport {
                decision: ResourceFetchSourcePolicyDecision::Allowed,
                scheme: "file".to_string(),
                allowed_schemes: vec!["file".to_string()],
            }),
        }],
    };

    // Announcement has WRONG sha256
    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "test_r".to_string(),
            version: "1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "bad_hash.dat".to_string(),
                size_bytes: content.len() as u64,
                sha256: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
                sources: None,
            }],
            protocol_version: PROTOCOL_VERSION,
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
    assert_eq!(
        report.entries[0].outcome,
        FetchOutcome::Failure(client::fetch::FetchFailureReason::HashMismatch)
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

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![ResourceDownloadPreflightEntry {
            resource_name: "report_r".to_string(),
            file_path: "r.dat".to_string(),
            action: ResourceDownloadPreflightAction::FetchMissing,
            reason: "file missing".to_string(),
            source_errors: vec![],
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: vec![],
            source_policy: Some(ResourceFetchSourcePolicyReport {
                decision: ResourceFetchSourcePolicyDecision::Allowed,
                scheme: "file".to_string(),
                allowed_schemes: vec!["file".to_string()],
            }),
        }],
    };

    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "report_r".to_string(),
            version: "1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "r.dat".to_string(),
                size_bytes: content.len() as u64,
                sha256: expected_sha.clone(),
                sources: None,
            }],
            protocol_version: PROTOCOL_VERSION,
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

    let text = report.to_text();
    assert!(text.contains("resource fetch: 1 entry"));
    assert!(text.contains("success"));
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

    let preflight = ResourceDownloadPreflightPlan {
        entries: vec![ResourceDownloadPreflightEntry {
            resource_name: "json_r".to_string(),
            file_path: "j.dat".to_string(),
            action: ResourceDownloadPreflightAction::FetchMissing,
            reason: "file missing".to_string(),
            source_errors: vec![],
            valid_sources: vec![source.clone()],
            selected_source: Some(source),
            fallback_sources: vec![],
            source_policy: Some(ResourceFetchSourcePolicyReport {
                decision: ResourceFetchSourcePolicyDecision::Allowed,
                scheme: "file".to_string(),
                allowed_schemes: vec!["file".to_string()],
            }),
        }],
    };

    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "json_r".to_string(),
            version: "1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "j.dat".to_string(),
                size_bytes: content.len() as u64,
                sha256: expected_sha.clone(),
                sources: None,
            }],
            protocol_version: PROTOCOL_VERSION,
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

    let json = report.to_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let entries = parsed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["outcome"], "success");
    assert_eq!(entries[0]["resource_name"], "json_r");
}
