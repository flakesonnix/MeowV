use std::collections::BTreeMap;

use crate::heartbeat_planner::{
    HeartbeatPlannerInput, HeartbeatPolicy, ServerHeartbeatPlannerInput, evaluate_heartbeat,
    evaluate_server_heartbeat,
};
use crate::session::SessionState;
use protocol::LoginCapabilities;

/// Opaque session identifier. Monotonic u64; never based on IP or personal data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

impl SessionId {
    pub fn value(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

/// Snapshot of a single registered session. No IP addresses or personal data.
#[derive(Debug, Clone)]
pub struct SessionRegistryEntry {
    pub id: SessionId,
    pub state: SessionState,
    pub event_count: usize,
    pub ready_dry_run: bool,
    pub failed: bool,
    pub protocol_version: Option<u32>,
    pub login_capabilities: Option<LoginCapabilities>,
    pub ping_received_count: usize,
    pub pong_sent_count: usize,
    pub server_ping_sent_count: usize,
    pub server_pong_received_count: usize,
}

/// Point-in-time aggregate snapshot of all registered sessions.
#[derive(Debug, Clone)]
pub struct SessionRegistrySnapshot {
    pub connected_sessions: usize,
    pub ready_dry_run_sessions: usize,
    pub failed_sessions: usize,
    pub sessions: Vec<SessionRegistryEntry>,
    pub heartbeat_policy: HeartbeatPolicy,
}

impl SessionRegistrySnapshot {
    /// Deterministic diagnostics text from snapshot. No timestamps, IPs, or personal data.
    pub fn to_diagnostics_text(&self) -> String {
        if self.sessions.is_empty() {
            return format!(
                "sessions: {}\n(no active sessions)",
                self.connected_sessions
            );
        }
        let mut lines = vec![format!(
            "sessions: {}  ready_dry_run: {}  failed: {}",
            self.connected_sessions, self.ready_dry_run_sessions, self.failed_sessions,
        )];
        for entry in &self.sessions {
            let proto = match entry.protocol_version {
                Some(v) => format!("protocol=v{}", v),
                None => "protocol=unknown".to_string(),
            };
            let login_caps = match &entry.login_capabilities {
                Some(caps) => format!(
                    "login_caps=req:{} opt:{} flags:{}",
                    caps.required.len(),
                    caps.optional.len(),
                    caps.feature_flags.as_ref().map(|flags| flags.len()).unwrap_or(0)
                ),
                None => "login_caps=unknown".to_string(),
            };
            let hb_input = HeartbeatPlannerInput {
                ping_sent: entry.ping_received_count as u64,
                pong_received: entry.pong_sent_count as u64,
                timeout_or_error: 0,
            };
            let hb_label = evaluate_heartbeat(&hb_input, &self.heartbeat_policy)
                .to_short_label();
            let srv_hb_input = ServerHeartbeatPlannerInput {
                pings_sent: entry.server_ping_sent_count as u64,
                pongs_received: entry.server_pong_received_count as u64,
            };
            let srv_hb_label = evaluate_server_heartbeat(&srv_hb_input, &self.heartbeat_policy)
                .to_short_label();
            lines.push(format!(
                "  {}: state={:?}  events={}  ready_dry_run={}  failed={}  {}  {}  ping_rx={}  pong_tx={}  srv_ping_tx={}  srv_pong_rx={}  heartbeat={}  srv_heartbeat={}",
                entry.id, entry.state, entry.event_count, entry.ready_dry_run, entry.failed, proto,
                login_caps,
                entry.ping_received_count, entry.pong_sent_count,
                entry.server_ping_sent_count, entry.server_pong_received_count,
                hb_label, srv_hb_label,
            ));
        }
        lines.join("\n")
    }
}

/// In-memory live session registry.
/// Keyed by monotonic `SessionId`. BTreeMap ensures deterministic snapshot ordering.
/// Local only; never persisted or exposed over a network.
pub struct SessionRegistry {
    next_id: u64,
    entries: BTreeMap<SessionId, SessionRegistryEntry>,
    heartbeat_policy: HeartbeatPolicy,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: BTreeMap::new(),
            heartbeat_policy: HeartbeatPolicy::ReportOnly,
        }
    }

    /// Update the heartbeat policy used when generating diagnostics text.
    /// Call this once after config is loaded; defaults to `ReportOnly`.
    pub fn set_heartbeat_policy(&mut self, policy: HeartbeatPolicy) {
        self.heartbeat_policy = policy;
    }

    /// Register a new session, starting in `Connected` state. Returns its ID.
    pub fn create_session(&mut self) -> SessionId {
        let id = SessionId(self.next_id);
        self.next_id += 1;
        self.entries.insert(
            id,
            SessionRegistryEntry {
                id,
                state: SessionState::Connected,
                event_count: 0,
                ready_dry_run: false,
                failed: false,
                protocol_version: None,
                login_capabilities: None,
                ping_received_count: 0,
                pong_sent_count: 0,
                server_ping_sent_count: 0,
                server_pong_received_count: 0,
            },
        );
        id
    }

    /// Set the protocol version for a session (call after version check succeeds).
    pub fn set_protocol_version(&mut self, id: &SessionId, version: u32) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.protocol_version = Some(version);
        }
    }

    pub fn set_login_capabilities(&mut self, id: &SessionId, capabilities: LoginCapabilities) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.login_capabilities = Some(capabilities);
        }
    }

    /// Update the state of an existing session.
    pub fn update_session_state(&mut self, id: &SessionId, state: SessionState) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.ready_dry_run = state == SessionState::ReadyDryRun;
            entry.failed = state == SessionState::Failed;
            entry.state = state;
        }
    }

    /// Update the event count of an existing session.
    pub fn update_session_event_count(&mut self, id: &SessionId, event_count: usize) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.event_count = event_count;
        }
    }

    /// Update heartbeat counts derived from the session event log.
    pub fn update_session_heartbeat_counts(
        &mut self,
        id: &SessionId,
        ping_received: usize,
        pong_sent: usize,
    ) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.ping_received_count = ping_received;
            entry.pong_sent_count = pong_sent;
        }
    }

    /// Update server-initiated heartbeat counts derived from the session event log.
    pub fn update_server_heartbeat_counts(
        &mut self,
        id: &SessionId,
        ping_sent: usize,
        pong_received: usize,
    ) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.server_ping_sent_count = ping_sent;
            entry.server_pong_received_count = pong_received;
        }
    }

    /// Update state and event count in one call (avoids double lock from callers).
    pub fn update_session(&mut self, id: &SessionId, state: SessionState, event_count: usize) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.ready_dry_run = state == SessionState::ReadyDryRun;
            entry.failed = state == SessionState::Failed;
            entry.state = state;
            entry.event_count = event_count;
        }
    }

    /// Remove a session from the registry (call on disconnect/handler exit).
    pub fn remove_session(&mut self, id: &SessionId) {
        self.entries.remove(id);
    }

    /// Return a deterministic point-in-time snapshot. Sessions ordered by ID.
    pub fn snapshot(&self) -> SessionRegistrySnapshot {
        let sessions: Vec<SessionRegistryEntry> = self.entries.values().cloned().collect();
        let connected_sessions = sessions.len();
        let ready_dry_run_sessions = sessions.iter().filter(|e| e.ready_dry_run).count();
        let failed_sessions = sessions.iter().filter(|e| e.failed).count();
        SessionRegistrySnapshot {
            connected_sessions,
            ready_dry_run_sessions,
            failed_sessions,
            sessions,
            heartbeat_policy: self.heartbeat_policy.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionState;

    #[test]
    fn registry_starts_empty() {
        let reg = SessionRegistry::new();
        let snap = reg.snapshot();
        assert_eq!(snap.connected_sessions, 0);
        assert_eq!(snap.ready_dry_run_sessions, 0);
        assert_eq!(snap.failed_sessions, 0);
        assert!(snap.sessions.is_empty());
    }

    #[test]
    fn create_session_increments_connected_count() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        assert_eq!(reg.snapshot().connected_sessions, 1);
    }

    #[test]
    fn session_ids_are_deterministic() {
        let mut reg = SessionRegistry::new();
        let id1 = reg.create_session();
        let id2 = reg.create_session();
        let id3 = reg.create_session();
        assert_eq!(id1, SessionId(1));
        assert_eq!(id2, SessionId(2));
        assert_eq!(id3, SessionId(3));
    }

    #[test]
    fn update_state_reflects_in_snapshot() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::VersionChecked);
        let snap = reg.snapshot();
        assert_eq!(snap.sessions[0].state, SessionState::VersionChecked);
        assert!(!snap.sessions[0].ready_dry_run);
        assert!(!snap.sessions[0].failed);
    }

    #[test]
    fn ready_dry_run_count_works() {
        let mut reg = SessionRegistry::new();
        let id1 = reg.create_session();
        let id2 = reg.create_session();
        reg.update_session_state(&id1, SessionState::ReadyDryRun);
        let snap = reg.snapshot();
        assert_eq!(snap.ready_dry_run_sessions, 1);
        assert_eq!(snap.connected_sessions, 2);
        assert!(
            snap.sessions
                .iter()
                .find(|e| e.id == id1)
                .unwrap()
                .ready_dry_run
        );
        assert!(
            !snap
                .sessions
                .iter()
                .find(|e| e.id == id2)
                .unwrap()
                .ready_dry_run
        );
    }

    #[test]
    fn failed_count_works() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::Failed);
        let snap = reg.snapshot();
        assert_eq!(snap.failed_sessions, 1);
        assert_eq!(snap.connected_sessions, 1);
        assert!(snap.sessions[0].failed);
        assert!(!snap.sessions[0].ready_dry_run);
    }

    #[test]
    fn removing_session_decrements_connected_count() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.remove_session(&id);
        assert_eq!(reg.snapshot().connected_sessions, 0);
    }

    #[test]
    fn snapshot_ordering_is_deterministic() {
        let mut reg = SessionRegistry::new();
        let id1 = reg.create_session();
        let id2 = reg.create_session();
        let id3 = reg.create_session();
        let snap = reg.snapshot();
        assert_eq!(snap.sessions[0].id, id1);
        assert_eq!(snap.sessions[1].id, id2);
        assert_eq!(snap.sessions[2].id, id3);
    }

    #[test]
    fn event_count_update_works() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_event_count(&id, 7);
        assert_eq!(reg.snapshot().sessions[0].event_count, 7);
    }

    #[test]
    fn failed_session_counts_connected_until_removed() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::Failed);
        assert_eq!(reg.snapshot().connected_sessions, 1);
        assert_eq!(reg.snapshot().failed_sessions, 1);
        reg.remove_session(&id);
        assert_eq!(reg.snapshot().connected_sessions, 0);
        assert_eq!(reg.snapshot().failed_sessions, 0);
    }

    #[test]
    fn update_session_sets_state_and_count_atomically() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session(&id, SessionState::ReadyDryRun, 11);
        let snap = reg.snapshot();
        assert_eq!(snap.sessions[0].state, SessionState::ReadyDryRun);
        assert_eq!(snap.sessions[0].event_count, 11);
        assert!(snap.sessions[0].ready_dry_run);
        assert_eq!(snap.ready_dry_run_sessions, 1);
    }

    #[test]
    fn remove_nonexistent_session_is_noop() {
        let mut reg = SessionRegistry::new();
        let id = SessionId(999);
        reg.remove_session(&id);
        assert_eq!(reg.snapshot().connected_sessions, 0);
    }

    #[test]
    fn multiple_sessions_tracked_independently() {
        let mut reg = SessionRegistry::new();
        let id1 = reg.create_session();
        let id2 = reg.create_session();
        reg.update_session_state(&id1, SessionState::ReadyDryRun);
        reg.update_session_state(&id2, SessionState::Failed);
        let snap = reg.snapshot();
        assert_eq!(snap.connected_sessions, 2);
        assert_eq!(snap.ready_dry_run_sessions, 1);
        assert_eq!(snap.failed_sessions, 1);
        reg.remove_session(&id1);
        let snap2 = reg.snapshot();
        assert_eq!(snap2.connected_sessions, 1);
        assert_eq!(snap2.ready_dry_run_sessions, 0);
        assert_eq!(snap2.failed_sessions, 1);
    }

    #[test]
    fn to_diagnostics_text_empty_registry() {
        let reg = SessionRegistry::new();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("sessions: 0"));
        assert!(text.contains("no active sessions"));
    }

    #[test]
    fn to_diagnostics_text_is_deterministic() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        assert_eq!(snap.to_diagnostics_text(), snap.to_diagnostics_text());
    }

    #[test]
    fn to_diagnostics_text_includes_session_ids() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains(&id.to_string()));
    }

    #[test]
    fn to_diagnostics_text_shows_ready_dry_run() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::ReadyDryRun);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("ready_dry_run=true"));
        assert!(text.contains("ReadyDryRun"));
    }

    #[test]
    fn to_diagnostics_text_shows_failed() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::Failed);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("failed=true"));
        assert!(text.contains("Failed"));
    }

    #[test]
    fn to_diagnostics_text_omits_ip_and_personal_data() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(!text.contains("ip"));
        assert!(!text.contains("addr"));
        assert!(!text.contains("peer"));
        assert!(!text.contains("name"));
    }

    #[test]
    fn new_session_has_zero_heartbeat_counts() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        assert_eq!(snap.sessions[0].ping_received_count, 0);
        assert_eq!(snap.sessions[0].pong_sent_count, 0);
    }

    #[test]
    fn update_heartbeat_counts_reflected_in_snapshot() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 3, 3);
        let snap = reg.snapshot();
        assert_eq!(snap.sessions[0].ping_received_count, 3);
        assert_eq!(snap.sessions[0].pong_sent_count, 3);
    }

    #[test]
    fn to_diagnostics_text_shows_zero_heartbeat_counts() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("ping_rx=0"));
        assert!(text.contains("pong_tx=0"));
    }

    #[test]
    fn to_diagnostics_text_shows_nonzero_heartbeat_counts() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 1, 1);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("ping_rx=1"));
        assert!(text.contains("pong_tx=1"));
    }

    #[test]
    fn heartbeat_counts_independent_per_session() {
        let mut reg = SessionRegistry::new();
        let id1 = reg.create_session();
        let id2 = reg.create_session();
        reg.update_session_heartbeat_counts(&id1, 2, 2);
        let snap = reg.snapshot();
        let e1 = snap.sessions.iter().find(|e| e.id == id1).unwrap();
        let e2 = snap.sessions.iter().find(|e| e.id == id2).unwrap();
        assert_eq!(e1.ping_received_count, 2);
        assert_eq!(e2.ping_received_count, 0);
    }

    #[test]
    fn to_diagnostics_text_does_not_mutate_snapshot() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let count_before = snap.connected_sessions;
        let _ = snap.to_diagnostics_text();
        assert_eq!(snap.connected_sessions, count_before);
    }

    #[test]
    fn to_diagnostics_text_shows_no_activity_heartbeat_for_new_session() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=no_activity"));
    }

    #[test]
    fn to_diagnostics_text_shows_healthy_heartbeat_after_ping_pong() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 1, 1);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=healthy"));
    }

    #[test]
    fn to_diagnostics_text_shows_no_pong_yet_when_ping_sent_no_pong() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 1, 0);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=no_pong_yet"));
    }

    #[test]
    fn to_diagnostics_text_shows_unhealthy_when_pong_gap() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 5, 3);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=unhealthy"));
    }

    #[test]
    fn to_diagnostics_text_heartbeat_label_is_deterministic() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        assert_eq!(snap.to_diagnostics_text(), snap.to_diagnostics_text());
    }

    #[test]
    fn to_diagnostics_text_shows_no_activity_srv_heartbeat_for_new_session() {
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("srv_heartbeat=no_activity"), "text: {text}");
    }

    #[test]
    fn to_diagnostics_text_shows_healthy_srv_heartbeat_after_ping_pong() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_server_heartbeat_counts(&id, 3, 3);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("srv_heartbeat=healthy"), "text: {text}");
    }

    #[test]
    fn to_diagnostics_text_shows_awaiting_pong_when_no_reply() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_server_heartbeat_counts(&id, 2, 0);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("srv_heartbeat=awaiting_pong"), "text: {text}");
    }

    #[test]
    fn to_diagnostics_text_shows_missed_pong_when_gap() {
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_server_heartbeat_counts(&id, 5, 3);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("srv_heartbeat=missed_pong"), "text: {text}");
    }

    #[test]
    fn to_diagnostics_text_shows_would_disconnect_under_strict_at_threshold() {
        let mut reg = SessionRegistry::new();
        reg.set_heartbeat_policy(HeartbeatPolicy::Strict);
        let id = reg.create_session();
        reg.update_server_heartbeat_counts(&id, 3, 0);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("srv_heartbeat=would_disconnect"), "text: {text}");
    }

    #[test]
    fn to_diagnostics_text_report_only_never_shows_would_disconnect_for_srv_heartbeat() {
        let mut reg = SessionRegistry::new();
        reg.set_heartbeat_policy(HeartbeatPolicy::ReportOnly);
        let id = reg.create_session();
        reg.update_server_heartbeat_counts(&id, 10, 0);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(!text.contains("srv_heartbeat=would_disconnect"), "text: {text}");
    }

    #[test]
    fn default_heartbeat_policy_is_report_only() {
        let reg = SessionRegistry::new();
        let snap = reg.snapshot();
        assert_eq!(snap.heartbeat_policy, HeartbeatPolicy::ReportOnly);
    }

    #[test]
    fn set_heartbeat_policy_updates_snapshot() {
        let mut reg = SessionRegistry::new();
        reg.set_heartbeat_policy(HeartbeatPolicy::Strict);
        let snap = reg.snapshot();
        assert_eq!(snap.heartbeat_policy, HeartbeatPolicy::Strict);
    }

    #[test]
    fn strict_policy_shows_no_activity_for_new_session() {
        let mut reg = SessionRegistry::new();
        reg.set_heartbeat_policy(HeartbeatPolicy::Strict);
        reg.create_session();
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=no_activity"));
    }

    #[test]
    fn strict_policy_shows_healthy_after_ping_pong() {
        let mut reg = SessionRegistry::new();
        reg.set_heartbeat_policy(HeartbeatPolicy::Strict);
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 3, 3);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=healthy"));
    }

    #[test]
    fn strict_policy_shows_unhealthy_for_pong_gap() {
        let mut reg = SessionRegistry::new();
        reg.set_heartbeat_policy(HeartbeatPolicy::Strict);
        let id = reg.create_session();
        reg.update_session_heartbeat_counts(&id, 5, 3);
        let text = reg.snapshot().to_diagnostics_text();
        assert!(text.contains("heartbeat=unhealthy"));
    }
}
