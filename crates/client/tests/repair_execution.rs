use client::fetch::{CacheManifestEntry, load_cache_manifest};
use client::repair::{
    CacheRepairConfig, CacheRepairOutcome, build_cache_repair_plan, execute_cache_repair,
};
use client::reconciliation::{CacheFileEntry, build_cache_reconciliation_plan};
use protocol::{
    AnnouncedResource, AnnouncedResourceFile, PROTOCOL_VERSION, ResourceAnnouncement,
    ResourceFetchSource, ResourceRequirementLevel,
};

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn make_source(uri: &str, size_bytes: u64, sha256: &str) -> ResourceFetchSource {
    ResourceFetchSource {
        id: None,
        scheme: "file".to_string(),
        uri: uri.to_string(),
        size_bytes: Some(size_bytes),
        sha256: Some(sha256.to_string()),
        compression: None,
        media_type: None,
        priority: None,
        mirrors: None,
    }
}

fn make_announcement_with_source(
    resource_name: &str,
    file_path: &str,
    size_bytes: u64,
    sha256: &str,
    source: ResourceFetchSource,
) -> ResourceAnnouncement {
    ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: resource_name.to_string(),
            version: "1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: file_path.to_string(),
                size_bytes,
                sha256: sha256.to_string(),
                sources: Some(vec![source]),
            }],
            protocol_version: PROTOCOL_VERSION,
            requirement_level: ResourceRequirementLevel::Required,
        }],
        signature: None,
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

#[test]
fn repair_dry_run_does_not_mutate_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let content = b"repair dry run file";
    let expected_sha = sha256_hex(content);
    let source_file = dir.path().join("source.dat");
    std::fs::write(&source_file, content).unwrap();

    let announcement = make_announcement_with_source(
        "chat",
        "main.lua",
        content.len() as u64,
        &expected_sha,
        make_source(&source_file.to_string_lossy(), content.len() as u64, &expected_sha),
    );

    let reconciliation = build_cache_reconciliation_plan(
        &client::fetch::CacheManifest {
            version: 1,
            entries: vec![make_manifest_entry(
                "chat",
                "main.lua",
                &expected_sha,
                content.len() as u64,
            )],
        },
        &[],
        &announcement,
        false,
    );
    let repair_plan = build_cache_repair_plan(&reconciliation, &announcement);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_cache_repair(
            &announcement,
            &repair_plan,
            &CacheRepairConfig {
                dry_run: true,
                allow_manifest_repair: true,
                allow_refetch_repair: true,
                cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            },
        ))
        .unwrap();

    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].outcome, CacheRepairOutcome::Skipped);
    assert!(!cache_dir.join("main.lua").exists());
    let manifest = rt.block_on(load_cache_manifest(&cache_dir));
    assert!(manifest.entries.is_empty());
}

#[test]
fn repair_manifest_entry_updates_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(cache_dir.join("chat")).unwrap();

    let content = b"existing cache file";
    let expected_sha = sha256_hex(content);
    std::fs::write(cache_dir.join("chat/main.lua"), content).unwrap();
    let source_file = dir.path().join("source.dat");
    std::fs::write(&source_file, content).unwrap();

    let announcement = make_announcement_with_source(
        "chat",
        "main.lua",
        content.len() as u64,
        &expected_sha,
        make_source(&source_file.to_string_lossy(), content.len() as u64, &expected_sha),
    );

    let reconciliation = build_cache_reconciliation_plan(
        &client::fetch::CacheManifest::empty(),
        &[make_cache_entry("chat/main.lua", &expected_sha, content.len() as u64)],
        &announcement,
        false,
    );
    let repair_plan = build_cache_repair_plan(&reconciliation, &announcement);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_cache_repair(
            &announcement,
            &repair_plan,
            &CacheRepairConfig {
                dry_run: false,
                allow_manifest_repair: true,
                allow_refetch_repair: false,
                cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            },
        ))
        .unwrap();

    assert_eq!(report.entries[0].outcome, CacheRepairOutcome::Repaired);
    let manifest = rt.block_on(load_cache_manifest(&cache_dir));
    assert_eq!(manifest.entries.len(), 1);
    assert_eq!(manifest.entries[0].resource_name, "chat");
    assert_eq!(manifest.entries[0].file_path, "main.lua");
}

#[test]
fn repair_missing_file_refetches_into_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let content = b"missing file repair";
    let expected_sha = sha256_hex(content);
    let source_file = dir.path().join("source.dat");
    std::fs::write(&source_file, content).unwrap();

    let announcement = make_announcement_with_source(
        "chat",
        "main.lua",
        content.len() as u64,
        &expected_sha,
        make_source(&source_file.to_string_lossy(), content.len() as u64, &expected_sha),
    );

    let reconciliation = build_cache_reconciliation_plan(
        &client::fetch::CacheManifest {
            version: 1,
            entries: vec![make_manifest_entry(
                "chat",
                "main.lua",
                &expected_sha,
                content.len() as u64,
            )],
        },
        &[],
        &announcement,
        false,
    );
    let repair_plan = build_cache_repair_plan(&reconciliation, &announcement);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_cache_repair(
            &announcement,
            &repair_plan,
            &CacheRepairConfig {
                dry_run: false,
                allow_manifest_repair: false,
                allow_refetch_repair: true,
                cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            },
        ))
        .unwrap();

    assert_eq!(report.entries[0].outcome, CacheRepairOutcome::Repaired);
    let cached = std::fs::read(cache_dir.join("main.lua")).unwrap();
    assert_eq!(cached, content);
    let manifest = rt.block_on(load_cache_manifest(&cache_dir));
    assert_eq!(manifest.entries.len(), 1);
}

#[test]
fn repair_hash_mismatch_replaces_file() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let bad_content = b"bad cache content";
    let good_content = b"good cache content";
    let bad_sha = sha256_hex(bad_content);
    let good_sha = sha256_hex(good_content);
    std::fs::write(cache_dir.join("main.lua"), bad_content).unwrap();
    let source_file = dir.path().join("source.dat");
    std::fs::write(&source_file, good_content).unwrap();

    let announcement = make_announcement_with_source(
        "chat",
        "main.lua",
        good_content.len() as u64,
        &good_sha,
        make_source(&source_file.to_string_lossy(), good_content.len() as u64, &good_sha),
    );

    let reconciliation = build_cache_reconciliation_plan(
        &client::fetch::CacheManifest {
            version: 1,
            entries: vec![make_manifest_entry(
                "chat",
                "main.lua",
                &good_sha,
                good_content.len() as u64,
            )],
        },
        &[make_cache_entry("main.lua", &bad_sha, bad_content.len() as u64)],
        &announcement,
        false,
    );
    let repair_plan = build_cache_repair_plan(&reconciliation, &announcement);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt
        .block_on(execute_cache_repair(
            &announcement,
            &repair_plan,
            &CacheRepairConfig {
                dry_run: false,
                allow_manifest_repair: false,
                allow_refetch_repair: true,
                cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            },
        ))
        .unwrap();

    assert_eq!(report.entries[0].outcome, CacheRepairOutcome::Repaired);
    let cached = std::fs::read(cache_dir.join("main.lua")).unwrap();
    assert_eq!(cached, good_content);
}

#[test]
fn repair_plan_ignores_orphan_and_announcement_missing() {
    let announcement = ResourceAnnouncement {
        resources: vec![],
        signature: None,
    };
    let reconciliation = build_cache_reconciliation_plan(
        &client::fetch::CacheManifest {
            version: 1,
            entries: vec![make_manifest_entry("chat", "main.lua", &"a".repeat(64), 10)],
        },
        &[make_cache_entry("orphan/test.dat", &"b".repeat(64), 20)],
        &announcement,
        false,
    );
    let repair_plan = build_cache_repair_plan(&reconciliation, &announcement);
    assert!(repair_plan.entries.is_empty());
}
