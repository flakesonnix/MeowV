use anyhow::Result;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use protocol::{ResourceAnnouncement, AnnouncedResource, AnnouncedResourceFile, ResourceRequirementLevel};
use client::get_resource_download_preflight_plan_text;

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
