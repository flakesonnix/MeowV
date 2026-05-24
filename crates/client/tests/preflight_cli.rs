use anyhow::Result;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use protocol::{ResourceAnnouncement, AnnouncedResource, AnnouncedResourceFile, ResourceRequirementLevel};
use client::get_resource_download_preflight_plan_text;
use protocol::signature_engine::SignaturePolicy;
use std::fs;
use std::path::PathBuf;

#[test]
fn preflight_plan_generates_text() -> Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("announcement.json");
    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "chat".to_string(),
            version: "0.1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "resource.toml".to_string(),
                size_bytes: 123,
                sha256: "abc".to_string(),
            }],
            protocol_version: protocol::PROTOCOL_VERSION,
            requirement_level: ResourceRequirementLevel::Required,
        }],
        signature: None,
    };

    let mut f = File::create(&file_path)?;
    write!(f, "{}", serde_json::to_string(&announcement)?)?;

    let args: Vec<String> = vec![];
    let text = get_resource_download_preflight_plan_text(file_path.to_str().unwrap(), &args, &protocol::signature_engine::SignaturePolicy::ReportOnly)?;
    assert!(text.contains("resource download preflight"));
    // must mention announced resource and file
    assert!(text.contains("chat:resource.toml") || text.contains(&"chat:resource.toml".to_string()));
    Ok(())
}

#[test]
fn preflight_plan_missing_file_errors() {
    let args: Vec<String> = vec![];
    let res = get_resource_download_preflight_plan_text("nonexistent.json", &args, &protocol::signature_engine::SignaturePolicy::ReportOnly);
    assert!(res.is_err());
}

#[test]
fn strict_policy_without_trusted_keys_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("announcement.json");
    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "chat".to_string(),
            version: "0.1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "resource.toml".to_string(),
                size_bytes: 123,
                sha256: "abc".to_string(),
            }],
            protocol_version: protocol::PROTOCOL_VERSION,
            requirement_level: ResourceRequirementLevel::Required,
        }],
        signature: None,
    };

    let mut f = File::create(&file_path).unwrap();
    write!(f, "{}", serde_json::to_string(&announcement).unwrap()).unwrap();

    let args: Vec<String> = vec![];
    let res = get_resource_download_preflight_plan_text(file_path.to_str().unwrap(), &args, &SignaturePolicy::Strict);
    assert!(res.is_err());
}

#[test]
fn preflight_reports_already_available_with_local_cache() -> Result<()> {
    // create a temp cache dir and populate with a valid file matching examples/resources/chat/resource.toml
    let cache_dir = tempdir()?;
    let cache_file_dir = cache_dir.path().join("resource.toml");
    // ensure parent exists
    if let Some(parent) = cache_file_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    // copy the example resource file to cache (resolve workspace-relative path)
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap();
    let src = workspace_root.join("examples/resources/chat/resource.toml");
    fs::copy(&src, &cache_file_dir)?;

    let dir = tempdir()?;
    let file_path = dir.path().join("announcement.json");
    // compute sha256 and size from the copied file using resource_manifest helpers
    let metadata = fs::metadata(&cache_file_dir)?;
    let size = metadata.len() as u64;
    let sha = resource_manifest::hash_file_sha256(&cache_file_dir)?;

    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "chat".to_string(),
            version: "0.1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "resource.toml".to_string(),
                size_bytes: size,
                sha256: sha.clone(),
            }],
            protocol_version: protocol::PROTOCOL_VERSION,
            requirement_level: ResourceRequirementLevel::Required,
        }],
        signature: None,
    };

    let mut f = File::create(&file_path)?;
    write!(f, "{}", serde_json::to_string(&announcement)?)?;

    let args: Vec<String> = vec!["--resource-cache".to_string(), cache_dir.path().to_str().unwrap().to_string()];
    let text = get_resource_download_preflight_plan_text(file_path.to_str().unwrap(), &args, &SignaturePolicy::ReportOnly)?;
    assert!(text.contains("already_available") || text.contains("already_available"));
    Ok(())
}

#[test]
fn preflight_reports_replace_invalid_for_bad_cached_file() -> Result<()> {
    // create a temp cache dir and populate with an invalid file (wrong content / hash)
    let cache_dir = tempdir()?;
    let cache_file_dir = cache_dir.path().join("resource.toml");
    if let Some(parent) = cache_file_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    // write invalid content
    fs::write(&cache_file_dir, "invalid content")?;

    let dir = tempdir()?;
    let file_path = dir.path().join("announcement.json");
    // announcement expects a different sha/size
    let announcement = ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: "chat".to_string(),
            version: "0.1.0".to_string(),
            files: vec![AnnouncedResourceFile {
                relative_path: "resource.toml".to_string(),
                size_bytes: 99999,
                sha256: "deadbeef".to_string(),
            }],
            protocol_version: protocol::PROTOCOL_VERSION,
            // mark optional so invalid cached file maps to ReplaceInvalid instead of blocking join
            requirement_level: ResourceRequirementLevel::Optional,
        }],
        signature: None,
    };

    let mut f = File::create(&file_path)?;
    write!(f, "{}", serde_json::to_string(&announcement)?)?;

    let args: Vec<String> = vec!["--resource-cache".to_string(), cache_dir.path().to_str().unwrap().to_string()];
    let text = get_resource_download_preflight_plan_text(file_path.to_str().unwrap(), &args, &SignaturePolicy::ReportOnly)?;
    println!("preflight plan text:\n{}", text);
    assert!(text.contains("replace_invalid") || text.contains("replace_invalid"));
    Ok(())
}
