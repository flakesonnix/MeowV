use crate::enforcement::SessionEnforcementPolicy;
use protocol::signature_engine::SignaturePolicy;
use protocol::PROTOCOL_VERSION;

use crate::config::{JoinGateConfigMode, ServerConfig};

/// Compact in-memory snapshot of server runtime state.
/// Derived from config and optional live session counts.
/// Local-only: never serialized to disk or sent over a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRuntimeStatus {
    pub server_name: String,
    pub bind_addr: String,
    pub protocol_version: u32,
    pub exact_version_required: bool,
    pub negotiation_dry_run: bool,
    pub capability_gates_report_only: bool,
    pub join_gate_mode: String,
    pub connected_sessions: usize,
    pub ready_dry_run_sessions: usize,
    pub failed_sessions: usize,
    pub resource_announcement_dir: String,
    pub diagnostics_enabled: bool,
    pub admin_stdin_enabled: bool,
    pub session_enforcement: String,
    pub signature_policy: String,
}

impl ServerRuntimeStatus {
    /// Build a status snapshot from server config.
    /// Session counts default to zero; use `with_session_counts` to override.
    pub fn from_config(config: &ServerConfig) -> Self {
        Self {
            server_name: config.server.name.clone(),
            bind_addr: config.server.bind_addr.clone(),
            protocol_version: PROTOCOL_VERSION,
            exact_version_required: config.protocol.exact_version_required,
            negotiation_dry_run: config.protocol.negotiation_dry_run,
            capability_gates_report_only: config.protocol.capability_gates_report_only,
            join_gate_mode: match config.join_gate.mode {
                JoinGateConfigMode::DryRun => "dry_run".to_string(),
            },
            connected_sessions: 0,
            ready_dry_run_sessions: 0,
            failed_sessions: 0,
            resource_announcement_dir: config.resources.announcement_resource_dir.clone(),
            diagnostics_enabled: config.diagnostics.print_session_diagnostics,
            admin_stdin_enabled: config.admin.local_stdin_enabled,
            session_enforcement: match config.enforcement.mode {
                SessionEnforcementPolicy::ReportOnly => "report_only".to_string(),
                SessionEnforcementPolicy::Strict => "strict".to_string(),
            },
            signature_policy: match config.signature.policy {
                SignaturePolicy::ReportOnly => "report_only".to_string(),
                SignaturePolicy::Strict => "strict".to_string(),
            },
        }
    }

    /// Return a new snapshot with updated session counts.
    pub fn with_session_counts(
        mut self,
        connected: usize,
        ready_dry_run: usize,
        failed: usize,
    ) -> Self {
        self.connected_sessions = connected;
        self.ready_dry_run_sessions = ready_dry_run;
        self.failed_sessions = failed;
        self
    }

    /// Deterministic human-readable text dump. No timestamps. No client IPs.
    pub fn to_text(&self) -> String {
        format!(
            "server_name: {}\n\
             bind_addr: {}\n\
             protocol_version: {}\n\
             exact_version_required: {}\n\
             negotiation_dry_run: {}\n\
             capability_gates_report_only: {}\n\
             join_gate_mode: {}\n\
             connected_sessions: {}\n\
             ready_dry_run_sessions: {}\n\
             failed_sessions: {}\n\
             resource_announcement_dir: {}\n\
             diagnostics_enabled: {}\n\
             admin_stdin_enabled: {}\n\
             session_enforcement: {}\n\
             signature_policy: {}",
            self.server_name,
            self.bind_addr,
            self.protocol_version,
            self.exact_version_required,
            self.negotiation_dry_run,
            self.capability_gates_report_only,
            self.join_gate_mode,
            self.connected_sessions,
            self.ready_dry_run_sessions,
            self.failed_sessions,
            self.resource_announcement_dir,
            self.diagnostics_enabled,
            self.admin_stdin_enabled,
            self.session_enforcement,
            self.signature_policy,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;

    #[test]
    fn status_from_default_config() {
        let config = ServerConfig::default();
        let status = ServerRuntimeStatus::from_config(&config);
        assert_eq!(status.server_name, config.server.name);
        assert_eq!(status.bind_addr, config.server.bind_addr);
        assert_eq!(status.protocol_version, PROTOCOL_VERSION);
        assert!(status.exact_version_required);
        assert!(status.negotiation_dry_run);
        assert!(status.capability_gates_report_only);
        assert_eq!(status.join_gate_mode, "dry_run");
        assert_eq!(status.connected_sessions, 0);
        assert_eq!(status.ready_dry_run_sessions, 0);
        assert_eq!(status.failed_sessions, 0);
        assert!(!status.resource_announcement_dir.is_empty());
        assert!(status.diagnostics_enabled);
        assert!(!status.admin_stdin_enabled);
        assert_eq!(status.session_enforcement, "report_only");
        assert_eq!(status.signature_policy, "report_only");
    }

    #[test]
    fn to_text_is_deterministic() {
        let config = ServerConfig::default();
        let status = ServerRuntimeStatus::from_config(&config);
        assert_eq!(status.to_text(), status.to_text());
    }

    #[test]
    fn to_text_includes_policy_flags() {
        let config = ServerConfig::default();
        let text = ServerRuntimeStatus::from_config(&config).to_text();
        assert!(text.contains("exact_version_required: true"));
        assert!(text.contains("negotiation_dry_run: true"));
        assert!(text.contains("capability_gates_report_only: true"));
        assert!(text.contains("join_gate_mode: dry_run"));
        assert!(text.contains("session_enforcement: report_only"));
        assert!(text.contains("signature_policy: report_only"));
    }

    #[test]
    fn to_text_includes_admin_and_diagnostics_flags() {
        let config = ServerConfig::default();
        let text = ServerRuntimeStatus::from_config(&config).to_text();
        assert!(text.contains("diagnostics_enabled:"));
        assert!(text.contains("admin_stdin_enabled:"));
    }

    #[test]
    fn to_text_does_not_contain_client_ip_fields() {
        let config = ServerConfig::default();
        let text = ServerRuntimeStatus::from_config(&config).to_text();
        assert!(!text.contains("client_ip"));
        assert!(!text.contains("peer_addr"));
        assert!(!text.contains("remote_addr"));
    }

    #[test]
    fn to_text_includes_resource_announcement_dir() {
        let config = ServerConfig::default();
        let text = ServerRuntimeStatus::from_config(&config).to_text();
        assert!(text.contains("resource_announcement_dir:"));
    }

    #[test]
    fn with_session_counts_updates_fields() {
        let config = ServerConfig::default();
        let status = ServerRuntimeStatus::from_config(&config).with_session_counts(3, 1, 2);
        assert_eq!(status.connected_sessions, 3);
        assert_eq!(status.ready_dry_run_sessions, 1);
        assert_eq!(status.failed_sessions, 2);
    }

    #[test]
    fn with_session_counts_text_reflects_counts() {
        let config = ServerConfig::default();
        let text = ServerRuntimeStatus::from_config(&config)
            .with_session_counts(5, 2, 1)
            .to_text();
        assert!(text.contains("connected_sessions: 5"));
        assert!(text.contains("ready_dry_run_sessions: 2"));
        assert!(text.contains("failed_sessions: 1"));
    }
}
