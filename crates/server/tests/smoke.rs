//! Server lifecycle smoke tests.
//!
//! Verify the dev server can initialize with safe config and shutdown-related
//! components without networking, downloads, execution, or remote admin.
//!
//! Hard boundaries tested:
//! - No remote admin fields or defaults
//! - Admin stdin disabled by default
//! - No IP addresses or personal data in lifecycle output
//! - All dry-run policies enforced at config level

use std::path::Path;

fn workspace_root() -> &'static Path {
    // CARGO_MANIFEST_DIR = /path/to/workspace/crates/server
    // workspace root = ../..
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

#[test]
fn example_config_file_validates() {
    let path = workspace_root().join("example.server.toml");
    let cfg = server::ServerConfig::load_from_path(&path).unwrap();
    assert!(cfg.protocol.exact_version_required);
    assert!(cfg.protocol.negotiation_dry_run);
    assert!(!cfg.join_gate.enforce_required_resources);
}

#[test]
fn default_config_initializes_lifecycle() {
    let config = server::ServerConfig::default();
    config.validate().unwrap();
    let summary = config.to_lifecycle_summary_text();
    assert!(summary.contains("server_name:"));
    assert!(summary.contains("bind_addr:"));
    assert!(summary.contains("protocol_version:"));
}

#[test]
fn runtime_status_from_config_and_empty_registry() {
    let config = server::ServerConfig::default();
    let reg = server::SessionRegistry::new();
    let snap = reg.snapshot();
    let status = server::ServerRuntimeStatus::from_config(&config)
        .with_session_counts(snap.connected_sessions, snap.ready_dry_run_sessions, snap.failed_sessions);
    assert_eq!(status.connected_sessions, 0);
    assert_eq!(status.ready_dry_run_sessions, 0);
    assert_eq!(status.failed_sessions, 0);
    let text = status.to_text();
    assert!(text.contains("connected_sessions: 0"));
    assert!(text.contains("ready_dry_run_sessions: 0"));
    assert!(text.contains("failed_sessions: 0"));
}

#[test]
fn shutdown_summary_from_default_lifecycle() {
    let config = server::ServerConfig::default();
    let reg = server::SessionRegistry::new();
    let snap = reg.snapshot();
    let summary = server::build_shutdown_summary(
        &config,
        &snap,
        server::ShutdownReason::AdminQuit,
    );
    assert_eq!(summary.reason, server::ShutdownReason::AdminQuit);
    assert!(summary.status_dump.contains("server_name:"));
    assert!(summary.status_dump.contains("exact_version_required: true"));
    assert!(summary.registry_dump.contains("no active sessions"));
}

#[test]
fn admin_stdin_disabled_by_default() {
    let config = server::ServerConfig::default();
    assert!(!config.admin.local_stdin_enabled);
    let summary = config.to_lifecycle_summary_text();
    assert!(summary.contains("admin_stdin: disabled"));
}

#[test]
fn no_remote_admin_fields() {
    let config = server::ServerConfig::default();
    let summary = config.to_lifecycle_summary_text();
    assert!(!summary.contains("remote"));
    assert!(!summary.contains("api"));
    assert!(!summary.contains("web"));
    assert!(!summary.contains("http"));
}

#[test]
fn lifecycle_summary_no_ip_personal_data() {
    let config = server::ServerConfig::default();
    let summary = config.to_lifecycle_summary_text();
    assert!(!summary.contains("client_ip"));
    assert!(!summary.contains("peer_addr"));
    assert!(!summary.contains("remote_addr"));
}

#[test]
fn dry_run_policies_reflected_in_summary() {
    let config = server::ServerConfig::default();
    let summary = config.to_lifecycle_summary_text();
    assert!(summary.contains("exact_version_required: true"));
    assert!(summary.contains("negotiation_dry_run: true"));
    assert!(summary.contains("capability_gates_report_only: true"));
    assert!(summary.contains("join_gate_mode: dry_run"));
    assert!(summary.contains("join_gate_enforcement: disabled"));
}
