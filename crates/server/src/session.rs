use protocol::{PROTOCOL_VERSION, ResourceJoinDecision};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    HelloReceived,
    VersionChecked,
    NegotiationDryRunLogged,
    ResourceAnnouncementSent,
    AvailabilityReportReceived,
    ResourcePolicyEvaluated,
    JoinGateDryRunSent,
    ReadyDryRun,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStateError {
    InvalidTransition {
        from: SessionState,
        to: SessionState,
    },
    ProtocolMismatch {
        client: u32,
        server: u32,
    },
    MissingAnnouncement,
    MissingAvailabilityReport,
    PolicyBlockedDryRun,
    InternalError(String),
}

impl std::fmt::Display for SessionStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStateError::InvalidTransition { from, to } => {
                write!(f, "invalid state transition: {from:?} -> {to:?}")
            }
            SessionStateError::ProtocolMismatch { client, server } => {
                write!(f, "protocol mismatch: client={client} server={server}")
            }
            SessionStateError::MissingAnnouncement => {
                write!(f, "resource announcement not available")
            }
            SessionStateError::MissingAvailabilityReport => {
                write!(f, "resource availability report not received")
            }
            SessionStateError::PolicyBlockedDryRun => {
                write!(
                    f,
                    "resource policy would block (dry-run only, not enforced)"
                )
            }
            SessionStateError::InternalError(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for SessionStateError {}

pub struct SessionStateMachine {
    state: SessionState,
    failure_reason: Option<String>,
    history: Vec<SessionState>,
}

impl SessionStateMachine {
    pub fn new() -> Self {
        Self {
            state: SessionState::Connected,
            failure_reason: None,
            history: Vec::new(),
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn failure_reason(&self) -> Option<&str> {
        self.failure_reason.as_deref()
    }

    pub fn history(&self) -> &[SessionState] {
        &self.history
    }

    pub fn transition_to(&mut self, next: SessionState) -> Result<(), SessionStateError> {
        if self.state == SessionState::Failed {
            return Err(SessionStateError::InvalidTransition {
                from: SessionState::Failed,
                to: next,
            });
        }

        if !is_valid_transition(&self.state, &next) {
            return Err(SessionStateError::InvalidTransition {
                from: self.state.clone(),
                to: next,
            });
        }

        let prev = std::mem::replace(&mut self.state, next);
        self.history.push(prev);
        Ok(())
    }

    pub fn fail(&mut self, reason: impl Into<String>) {
        let prev = std::mem::replace(&mut self.state, SessionState::Failed);
        self.history.push(prev);
        self.failure_reason = Some(reason.into());
    }

    pub fn on_hello_received(&mut self) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::HelloReceived)
    }

    pub fn on_version_checked(&mut self, client_version: u32) -> Result<(), SessionStateError> {
        if client_version != PROTOCOL_VERSION {
            let err = SessionStateError::ProtocolMismatch {
                client: client_version,
                server: PROTOCOL_VERSION,
            };
            self.fail(err.to_string());
            return Err(err);
        }
        self.transition_to(SessionState::VersionChecked)
    }

    pub fn on_negotiation_logged(&mut self) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::NegotiationDryRunLogged)
    }

    pub fn on_resource_announcement_sent(&mut self) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::ResourceAnnouncementSent)
    }

    pub fn on_availability_report_received(&mut self) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::AvailabilityReportReceived)
    }

    /// Transitions to `ResourcePolicyEvaluated`.
    /// Returns `Err(PolicyBlockedDryRun)` when the policy would block — the state
    /// transition still occurs; the error is a signal for the caller to log.
    /// No disconnect or enforcement in dry-run mode.
    pub fn on_policy_evaluated(
        &mut self,
        decision: &ResourceJoinDecision,
    ) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::ResourcePolicyEvaluated)?;
        if *decision == ResourceJoinDecision::Blocked {
            return Err(SessionStateError::PolicyBlockedDryRun);
        }
        Ok(())
    }

    pub fn on_join_gate_sent(&mut self) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::JoinGateDryRunSent)
    }

    pub fn mark_ready_dry_run(&mut self) -> Result<(), SessionStateError> {
        self.transition_to(SessionState::ReadyDryRun)
    }
}

fn is_valid_transition(from: &SessionState, to: &SessionState) -> bool {
    matches!(
        (from, to),
        (SessionState::Connected, SessionState::HelloReceived)
            | (SessionState::HelloReceived, SessionState::VersionChecked)
            | (
                SessionState::VersionChecked,
                SessionState::NegotiationDryRunLogged
            )
            | (
                SessionState::NegotiationDryRunLogged,
                SessionState::ResourceAnnouncementSent
            )
            | (
                SessionState::ResourceAnnouncementSent,
                SessionState::AvailabilityReportReceived
            )
            | (
                SessionState::AvailabilityReportReceived,
                SessionState::ResourcePolicyEvaluated
            )
            | (
                SessionState::ResourcePolicyEvaluated,
                SessionState::JoinGateDryRunSent
            )
            | (SessionState::JoinGateDryRunSent, SessionState::ReadyDryRun)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_connected() {
        let sm = SessionStateMachine::new();
        assert_eq!(*sm.state(), SessionState::Connected);
    }

    #[test]
    fn valid_full_transition_path_reaches_ready_dry_run() {
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
        assert_eq!(*sm.state(), SessionState::ReadyDryRun);
    }

    #[test]
    fn skipping_states_is_rejected() {
        let mut sm = SessionStateMachine::new();
        let result = sm.on_resource_announcement_sent();
        assert!(matches!(
            result,
            Err(SessionStateError::InvalidTransition { .. })
        ));
        assert_eq!(*sm.state(), SessionState::Connected);
    }

    #[test]
    fn backwards_transition_is_rejected() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(PROTOCOL_VERSION).unwrap();
        let result = sm.transition_to(SessionState::Connected);
        assert!(matches!(
            result,
            Err(SessionStateError::InvalidTransition { .. })
        ));
        assert_eq!(*sm.state(), SessionState::VersionChecked);
    }

    #[test]
    fn failed_is_terminal() {
        let mut sm = SessionStateMachine::new();
        sm.fail("test failure");
        assert_eq!(*sm.state(), SessionState::Failed);
        let result = sm.on_hello_received();
        assert!(matches!(
            result,
            Err(SessionStateError::InvalidTransition {
                from: SessionState::Failed,
                ..
            })
        ));
    }

    #[test]
    fn invalid_transition_returns_invalid_transition_error() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        let result = sm.transition_to(SessionState::ResourceAnnouncementSent);
        assert!(matches!(
            result,
            Err(SessionStateError::InvalidTransition {
                from: SessionState::HelloReceived,
                to: SessionState::ResourceAnnouncementSent,
            })
        ));
    }

    #[test]
    fn version_mismatch_marks_failed() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        let result = sm.on_version_checked(99);
        assert!(matches!(
            result,
            Err(SessionStateError::ProtocolMismatch { client: 99, .. })
        ));
        assert_eq!(*sm.state(), SessionState::Failed);
        assert!(sm.failure_reason().is_some());
    }

    #[test]
    fn policy_blocked_dry_run_session_still_continues() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(PROTOCOL_VERSION).unwrap();
        sm.on_negotiation_logged().unwrap();
        sm.on_resource_announcement_sent().unwrap();
        sm.on_availability_report_received().unwrap();
        let result = sm.on_policy_evaluated(&ResourceJoinDecision::Blocked);
        assert!(matches!(
            result,
            Err(SessionStateError::PolicyBlockedDryRun)
        ));
        assert_eq!(*sm.state(), SessionState::ResourcePolicyEvaluated);
        sm.on_join_gate_sent().unwrap();
        sm.mark_ready_dry_run().unwrap();
        assert_eq!(*sm.state(), SessionState::ReadyDryRun);
    }

    #[test]
    fn fail_records_reason() {
        let mut sm = SessionStateMachine::new();
        sm.fail("connection reset by peer");
        assert_eq!(*sm.state(), SessionState::Failed);
        assert_eq!(sm.failure_reason(), Some("connection reset by peer"));
    }

    #[test]
    fn history_tracks_transitions() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(PROTOCOL_VERSION).unwrap();
        assert_eq!(
            sm.history(),
            &[SessionState::Connected, SessionState::HelloReceived]
        );
    }
}
