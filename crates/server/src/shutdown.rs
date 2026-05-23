use crate::config::ServerConfig;
use crate::session_registry::SessionRegistrySnapshot;
use crate::status::ServerRuntimeStatus;

/// Reason the server was requested to shut down.
/// Local-only, in-memory, never persisted or sent over a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    AdminQuit,
    InternalError,
    TestRequested,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShutdownReason::AdminQuit => write!(f, "admin_quit"),
            ShutdownReason::InternalError => write!(f, "internal_error"),
            ShutdownReason::TestRequested => write!(f, "test_requested"),
        }
    }
}

/// In-memory shutdown request state.
/// Local-only, no persistence, no remote trigger.
#[derive(Debug, Clone)]
pub struct ShutdownState {
    requested: bool,
    reason: Option<ShutdownReason>,
}

impl ShutdownState {
    pub fn new() -> Self {
        Self {
            requested: false,
            reason: None,
        }
    }

    /// Request shutdown with a reason. First call sets the reason; subsequent
    /// calls are no-ops (deterministic first-wins behaviour).
    pub fn request(&mut self, reason: ShutdownReason) {
        if !self.requested {
            self.requested = true;
            self.reason = Some(reason);
        }
    }

    pub fn is_requested(&self) -> bool {
        self.requested
    }

    /// The reason for shutdown, or `None` if shutdown has not been requested.
    pub fn reason(&self) -> Option<ShutdownReason> {
        self.reason
    }
}

/// Deterministic shutdown summary printed after the accept loop ends.
/// Contains reason, runtime status text, and registry diagnostics text.
/// No IP addresses, personal data, or timestamps.
#[derive(Debug, Clone)]
pub struct ShutdownSummary {
    pub reason: ShutdownReason,
    pub status_dump: String,
    pub registry_dump: String,
}

/// Build a shutdown summary from config, registry snapshot, and reason.
/// Uses existing `ServerRuntimeStatus::from_config` and
/// `SessionRegistrySnapshot::to_diagnostics_text` internally.
pub fn build_shutdown_summary(
    config: &ServerConfig,
    registry_snapshot: &SessionRegistrySnapshot,
    reason: ShutdownReason,
) -> ShutdownSummary {
    let status = ServerRuntimeStatus::from_config(config).with_session_counts(
        registry_snapshot.connected_sessions,
        registry_snapshot.ready_dry_run_sessions,
        registry_snapshot.failed_sessions,
    );
    ShutdownSummary {
        reason,
        status_dump: status.to_text(),
        registry_dump: registry_snapshot.to_diagnostics_text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;
    use crate::session_registry::SessionRegistry;

    #[test]
    fn new_state_not_requested() {
        let s = ShutdownState::new();
        assert!(!s.is_requested());
        assert!(s.reason().is_none());
    }

    #[test]
    fn request_sets_reason() {
        let mut s = ShutdownState::new();
        s.request(ShutdownReason::AdminQuit);
        assert!(s.is_requested());
        assert_eq!(s.reason(), Some(ShutdownReason::AdminQuit));
    }

    #[test]
    fn repeated_request_keeps_first_reason() {
        let mut s = ShutdownState::new();
        s.request(ShutdownReason::AdminQuit);
        s.request(ShutdownReason::InternalError);
        assert_eq!(s.reason(), Some(ShutdownReason::AdminQuit));
    }

    #[test]
    fn is_requested_false_before_request() {
        let s = ShutdownState::new();
        assert!(!s.is_requested());
    }

    #[test]
    fn reason_none_before_request() {
        let s = ShutdownState::new();
        assert!(s.reason().is_none());
    }

    #[test]
    fn all_reasons_distinct() {
        assert_ne!(
            ShutdownReason::AdminQuit,
            ShutdownReason::InternalError
        );
        assert_ne!(
            ShutdownReason::InternalError,
            ShutdownReason::TestRequested
        );
        assert_ne!(
            ShutdownReason::TestRequested,
            ShutdownReason::AdminQuit
        );
    }

    #[test]
    fn display_reason_admin_quit() {
        assert_eq!(ShutdownReason::AdminQuit.to_string(), "admin_quit");
    }

    #[test]
    fn display_reason_internal_error() {
        assert_eq!(ShutdownReason::InternalError.to_string(), "internal_error");
    }

    #[test]
    fn display_reason_test_requested() {
        assert_eq!(
            ShutdownReason::TestRequested.to_string(),
            "test_requested"
        );
    }

    #[test]
    fn build_summary_includes_reason() {
        let config = ServerConfig::default();
        let reg = SessionRegistry::new();
        let snap = reg.snapshot();
        let summary = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        assert_eq!(summary.reason, ShutdownReason::AdminQuit);
    }

    #[test]
    fn build_summary_includes_status_text() {
        let config = ServerConfig::default();
        let reg = SessionRegistry::new();
        let snap = reg.snapshot();
        let summary = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        assert!(summary.status_dump.contains("server_name:"));
        assert!(summary.status_dump.contains("protocol_version:"));
        assert!(summary.status_dump.contains("exact_version_required:"));
    }

    #[test]
    fn build_summary_includes_registry_text() {
        let config = ServerConfig::default();
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let summary = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        assert!(summary.registry_dump.contains("sessions:"));
        assert!(summary.registry_dump.contains("session-1"));
    }

    #[test]
    fn build_summary_no_ip_personal_data() {
        let config = ServerConfig::default();
        let reg = SessionRegistry::new();
        let snap = reg.snapshot();
        let summary = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        assert!(!summary.status_dump.contains("client_ip"));
        assert!(!summary.status_dump.contains("peer_addr"));
        assert!(!summary.status_dump.contains("remote_addr"));
        assert!(!summary.registry_dump.contains("ip"));
        assert!(!summary.registry_dump.contains("addr"));
        assert!(!summary.registry_dump.contains("peer"));
        assert!(!summary.registry_dump.contains("name"));
    }

    #[test]
    fn build_summary_deterministic() {
        let config = ServerConfig::default();
        let mut reg = SessionRegistry::new();
        reg.create_session();
        reg.create_session();
        let snap = reg.snapshot();
        let s1 = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        let s2 = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        assert_eq!(s1.status_dump, s2.status_dump);
        assert_eq!(s1.registry_dump, s2.registry_dump);
        assert_eq!(s1.reason, s2.reason);
    }

    #[test]
    fn build_summary_reason_distinct_per_call() {
        let config = ServerConfig::default();
        let reg = SessionRegistry::new();
        let snap = reg.snapshot();
        let admin = build_shutdown_summary(&config, &snap, ShutdownReason::AdminQuit);
        let test_req = build_shutdown_summary(&config, &snap, ShutdownReason::TestRequested);
        assert_eq!(admin.reason, ShutdownReason::AdminQuit);
        assert_eq!(test_req.reason, ShutdownReason::TestRequested);
        assert_eq!(admin.status_dump, test_req.status_dump);
        assert_eq!(admin.registry_dump, test_req.registry_dump);
    }
}
