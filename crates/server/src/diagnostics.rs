use crate::event_log::{SessionEvent, SessionEventLog};
use crate::session::{SessionState, SessionStateMachine};

pub struct SessionDiagnostics {
    pub current_state: SessionState,
    pub state_history: Vec<SessionState>,
    pub event_count: usize,
    pub events: Vec<SessionEvent>,
    pub last_event_message: Option<String>,
    pub ready_dry_run: bool,
    pub failure_reason: Option<String>,
}

impl SessionDiagnostics {
    pub fn from_parts(machine: &SessionStateMachine, log: &SessionEventLog) -> Self {
        let current_state = machine.state().clone();
        let ready_dry_run = current_state == SessionState::ReadyDryRun;
        Self {
            ready_dry_run,
            failure_reason: machine.failure_reason().map(str::to_owned),
            state_history: machine.history().to_vec(),
            event_count: log.len(),
            last_event_message: log.last().map(|e| e.message.clone()),
            events: log.events().to_vec(),
            current_state,
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("state: {:?}\n", self.current_state));
        out.push_str(&format!("ready_dry_run: {}\n", self.ready_dry_run));
        if let Some(reason) = &self.failure_reason {
            out.push_str(&format!("failure_reason: {reason}\n"));
        }
        out.push_str("state_history:");
        if self.state_history.is_empty() {
            out.push_str(" (none)");
        } else {
            for s in &self.state_history {
                out.push_str(&format!(" {s:?}"));
            }
        }
        out.push('\n');
        out.push_str(&format!("event_count: {}\n", self.event_count));
        for ev in &self.events {
            out.push_str(&format!(
                "  [{}] {:?} @ {:?}: {}\n",
                ev.sequence, ev.kind, ev.state, ev.message
            ));
        }
        if let Some(msg) = &self.last_event_message {
            out.push_str(&format!("last_event: {msg}\n"));
        }
        out
    }

    pub fn to_json_stub(&self) -> String {
        let history: Vec<String> = self
            .state_history
            .iter()
            .map(|s| format!("\"{s:?}\""))
            .collect();
        let events: Vec<String> = self
            .events
            .iter()
            .map(|e| {
                format!(
                    "{{\"seq\":{},\"kind\":\"{:?}\",\"state\":\"{:?}\",\"message\":{}}}",
                    e.sequence,
                    e.kind,
                    e.state,
                    json_string(&e.message)
                )
            })
            .collect();
        format!(
            "{{\"current_state\":\"{:?}\",\"ready_dry_run\":{},\"failure_reason\":{},\
\"state_history\":[{}],\"event_count\":{},\"events\":[{}],\"last_event_message\":{}}}",
            self.current_state,
            self.ready_dry_run,
            optional_json_string(self.failure_reason.as_deref()),
            history.join(","),
            self.event_count,
            events.join(","),
            optional_json_string(self.last_event_message.as_deref()),
        )
    }
}

fn json_string(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn optional_json_string(s: Option<&str>) -> String {
    match s {
        Some(v) => json_string(v),
        None => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{SessionEventKind, SessionEventLog};
    use crate::session::{SessionState, SessionStateMachine};
    use protocol::{PROTOCOL_VERSION, ResourceJoinDecision};

    fn make_ready_machine() -> SessionStateMachine {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(PROTOCOL_VERSION).unwrap();
        sm.on_negotiation_logged().unwrap();
        sm.on_resource_announcement_sent().unwrap();
        sm.on_availability_report_received().unwrap();
        sm.on_policy_evaluated(&ResourceJoinDecision::Allowed)
            .unwrap();
        sm.on_join_gate_sent().unwrap();
        sm.mark_ready_dry_run().unwrap();
        sm
    }

    fn make_ready_log() -> SessionEventLog {
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::Connected,
            SessionState::Connected,
            "client connected",
        );
        log.record(
            SessionEventKind::HelloReceived,
            SessionState::HelloReceived,
            "login from test",
        );
        log.record(
            SessionEventKind::VersionChecked,
            SessionState::VersionChecked,
            "version matched",
        );
        log.record(
            SessionEventKind::ProtocolNegotiationDryRun,
            SessionState::NegotiationDryRunLogged,
            "negotiation status: ExactMatch",
        );
        log.record(
            SessionEventKind::CapabilityGateChecked,
            SessionState::NegotiationDryRunLogged,
            "ResourceAnnouncement gate: supported=false",
        );
        log.record(
            SessionEventKind::ResourceAnnouncementSent,
            SessionState::ResourceAnnouncementSent,
            "resource announcement sent to client",
        );
        log.record(
            SessionEventKind::AvailabilityReportReceived,
            SessionState::AvailabilityReportReceived,
            "resource availability report received from client",
        );
        log.record(
            SessionEventKind::ResourcePolicyEvaluated,
            SessionState::ResourcePolicyEvaluated,
            "policy decision: Allowed",
        );
        log.record(
            SessionEventKind::CapabilityGateChecked,
            SessionState::ResourcePolicyEvaluated,
            "JoinGateDryRun gate: supported=false",
        );
        log.record(
            SessionEventKind::JoinGateDryRunSent,
            SessionState::JoinGateDryRunSent,
            "join gate dry-run decision sent to client",
        );
        log.record(
            SessionEventKind::ReadyDryRun,
            SessionState::ReadyDryRun,
            "handshake pipeline complete (dry-run)",
        );
        log
    }

    #[test]
    fn diagnostics_from_new_session() {
        let sm = SessionStateMachine::new();
        let log = SessionEventLog::new();
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(diag.current_state, SessionState::Connected);
        assert!(!diag.ready_dry_run);
        assert_eq!(diag.event_count, 0);
        assert!(diag.events.is_empty());
        assert!(diag.state_history.is_empty());
        assert!(diag.last_event_message.is_none());
        assert!(diag.failure_reason.is_none());
    }

    #[test]
    fn diagnostics_from_ready_dry_run_session() {
        let sm = make_ready_machine();
        let log = make_ready_log();
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(diag.current_state, SessionState::ReadyDryRun);
        assert!(diag.ready_dry_run);
        assert_eq!(diag.event_count, 11);
        assert!(diag.failure_reason.is_none());
    }

    #[test]
    fn diagnostics_includes_event_count() {
        let sm = SessionStateMachine::new();
        let mut log = SessionEventLog::new();
        log.record(SessionEventKind::Connected, SessionState::Connected, "a");
        log.record(
            SessionEventKind::HelloReceived,
            SessionState::HelloReceived,
            "b",
        );
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(diag.event_count, 2);
        assert_eq!(diag.events.len(), 2);
    }

    #[test]
    fn diagnostics_includes_last_event_message() {
        let sm = SessionStateMachine::new();
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::Connected,
            SessionState::Connected,
            "first",
        );
        log.record(
            SessionEventKind::HelloReceived,
            SessionState::HelloReceived,
            "last message",
        );
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(diag.last_event_message.as_deref(), Some("last message"));
    }

    #[test]
    fn diagnostics_text_output_is_deterministic() {
        let sm = make_ready_machine();
        let log = make_ready_log();
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(diag.to_text(), diag.to_text());
    }

    #[test]
    fn diagnostics_failure_reason_included() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.fail("protocol mismatch: client=99 server=1");
        let log = SessionEventLog::new();
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(diag.current_state, SessionState::Failed);
        assert!(!diag.ready_dry_run);
        assert_eq!(
            diag.failure_reason.as_deref(),
            Some("protocol mismatch: client=99 server=1")
        );
    }

    #[test]
    fn diagnostics_does_not_mutate_state_or_log() {
        let sm = make_ready_machine();
        let log = make_ready_log();
        let state_before = sm.state().clone();
        let count_before = log.len();
        let _ = SessionDiagnostics::from_parts(&sm, &log);
        assert_eq!(*sm.state(), state_before);
        assert_eq!(log.len(), count_before);
    }

    #[test]
    fn diagnostics_text_contains_state_and_count() {
        let sm = make_ready_machine();
        let log = make_ready_log();
        let diag = SessionDiagnostics::from_parts(&sm, &log);
        let text = diag.to_text();
        assert!(text.contains("state: ReadyDryRun"));
        assert!(text.contains("ready_dry_run: true"));
        assert!(text.contains("event_count: 11"));
    }
}
