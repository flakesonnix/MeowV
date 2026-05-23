use crate::session::SessionState;
use serde::Deserialize;

/// Session enforcement policy mode.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEnforcementPolicy {
    /// Report only — never enforce, always returns Allow.
    ReportOnly,
    /// Strict — evaluates and returns the enforcement decision.
    Strict,
}

/// Deterministic outcome of a session enforcement evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnforcementDecision {
    /// Session handshake successful, no enforcement action.
    Allow,
    /// Client sent a non-Login message as the first packet.
    WouldDisconnectInvalidFirstMessage,
    /// Protocol version mismatch detected.
    WouldDisconnectVersionMismatch { client: u32, server: u32 },
    /// Capability gate check would block the session.
    WouldDisconnectCapabilityGateFailure { capability: String },
    /// Session state machine received an invalid transition.
    WouldDisconnectInvalidStateTransition { from: String, to: String },
    /// Session marked as failed for other reasons.
    WouldMarkSessionFailed { reason: String },
}

impl SessionEnforcementDecision {
    pub fn to_text(&self) -> String {
        match self {
            SessionEnforcementDecision::Allow => "decision: allow".to_string(),
            SessionEnforcementDecision::WouldDisconnectInvalidFirstMessage => {
                "decision: would_disconnect invalid_first_message".to_string()
            }
            SessionEnforcementDecision::WouldDisconnectVersionMismatch { client, server } => {
                format!(
                    "decision: would_disconnect version_mismatch client={client} server={server}"
                )
            }
            SessionEnforcementDecision::WouldDisconnectCapabilityGateFailure { capability } => {
                format!(
                    "decision: would_disconnect capability_gate_failure capability={capability}"
                )
            }
            SessionEnforcementDecision::WouldDisconnectInvalidStateTransition { from, to } => {
                format!("decision: would_disconnect invalid_state_transition from={from} to={to}")
            }
            SessionEnforcementDecision::WouldMarkSessionFailed { reason } => {
                format!("decision: would_mark_session_failed reason={reason}")
            }
        }
    }
}

/// Evaluate the enforcement decision for a session given its final state,
/// optional failure reason, and the active policy.
///
/// Pure function — no I/O, no side effects. Deterministic.
///
/// Under `ReportOnly` the decision is always `Allow`.
/// Under `Strict` the decision reflects what the server would enforce:
///   - `ReadyDryRun` → Allow
///   - `Connected` without progress → WouldDisconnectInvalidFirstMessage
///   - `Failed` → parsed from the failure reason string
///   - Other intermediate states → WouldMarkSessionFailed
pub fn evaluate_enforcement(
    current_state: &SessionState,
    failure_reason: Option<&str>,
    policy: &SessionEnforcementPolicy,
) -> SessionEnforcementDecision {
    match policy {
        SessionEnforcementPolicy::ReportOnly => SessionEnforcementDecision::Allow,
        SessionEnforcementPolicy::Strict => match current_state {
            SessionState::ReadyDryRun => SessionEnforcementDecision::Allow,
            SessionState::Connected => {
                SessionEnforcementDecision::WouldDisconnectInvalidFirstMessage
            }
            SessionState::Failed => match failure_reason {
                Some(reason) if reason.contains("protocol mismatch") => {
                    let (client, server) = extract_versions(reason);
                    SessionEnforcementDecision::WouldDisconnectVersionMismatch { client, server }
                }
                Some(reason) if reason.contains("invalid state transition") => {
                    let (from, to) = extract_transition(reason);
                    SessionEnforcementDecision::WouldDisconnectInvalidStateTransition { from, to }
                }
                Some(reason) => SessionEnforcementDecision::WouldMarkSessionFailed {
                    reason: reason.to_string(),
                },
                None => SessionEnforcementDecision::WouldMarkSessionFailed {
                    reason: "unknown failure".to_string(),
                },
            },
            other => SessionEnforcementDecision::WouldMarkSessionFailed {
                reason: format!("handshake incomplete at {other:?}"),
            },
        },
    }
}

fn extract_versions(reason: &str) -> (u32, u32) {
    let client = reason
        .split_whitespace()
        .find(|part| part.starts_with("client="))
        .and_then(|part| part.split('=').nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let server = reason
        .split_whitespace()
        .find(|part| part.starts_with("server="))
        .and_then(|part| part.split('=').nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (client, server)
}

fn extract_transition(reason: &str) -> (String, String) {
    let parts: Vec<&str> = reason.split_whitespace().collect();
    let from = parts
        .iter()
        .position(|p| p == &"transition:")
        .and_then(|i| parts.get(i + 1))
        .map(|s| s.to_string())
        .unwrap_or_default();
    let to = parts
        .iter()
        .position(|p| p == &"->")
        .and_then(|i| parts.get(i + 1))
        .map(|s| s.to_string())
        .unwrap_or_default();
    (from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SessionState, SessionStateMachine};
    use protocol::{PROTOCOL_VERSION, ResourceJoinDecision};

    #[test]
    fn report_only_always_allows_ready_dry_run() {
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

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::ReportOnly,
        );
        assert_eq!(decision, SessionEnforcementDecision::Allow);
    }

    #[test]
    fn report_only_always_allows_failed() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(99).unwrap_err();

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::ReportOnly,
        );
        assert_eq!(decision, SessionEnforcementDecision::Allow);
    }

    #[test]
    fn strict_allows_ready_dry_run() {
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

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        assert_eq!(decision, SessionEnforcementDecision::Allow);
    }

    #[test]
    fn strict_disconnects_connected_no_progress() {
        let sm = SessionStateMachine::new();
        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        assert_eq!(
            decision,
            SessionEnforcementDecision::WouldDisconnectInvalidFirstMessage
        );
    }

    #[test]
    fn strict_disconnects_version_mismatch() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(99).unwrap_err();

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        assert_eq!(
            decision,
            SessionEnforcementDecision::WouldDisconnectVersionMismatch {
                client: 99,
                server: PROTOCOL_VERSION,
            }
        );
    }

    #[test]
    fn strict_disconnects_invalid_state_transition() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        // transition_to returns Err but does NOT call fail() — state stays unchanged
        let _err = sm.transition_to(SessionState::ResourceAnnouncementSent);
        assert_eq!(*sm.state(), SessionState::HelloReceived);

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        // No failure reason recorded; decision based on intermediate state
        assert_eq!(
            decision,
            SessionEnforcementDecision::WouldMarkSessionFailed {
                reason: format!("handshake incomplete at {:?}", SessionState::HelloReceived),
            }
        );
    }

    #[test]
    fn strict_marks_failed_for_generic_failure() {
        let mut sm = SessionStateMachine::new();
        sm.fail("connection reset by peer");

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        assert_eq!(
            decision,
            SessionEnforcementDecision::WouldMarkSessionFailed {
                reason: "connection reset by peer".to_string(),
            }
        );
    }

    #[test]
    fn strict_marks_failed_for_intermediate_state() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        sm.on_version_checked(PROTOCOL_VERSION).unwrap();

        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        assert_eq!(
            decision,
            SessionEnforcementDecision::WouldMarkSessionFailed {
                reason: format!("handshake incomplete at {:?}", SessionState::VersionChecked),
            }
        );
    }

    #[test]
    fn decision_to_text_allow() {
        let text = SessionEnforcementDecision::Allow.to_text();
        assert_eq!(text, "decision: allow");
    }

    #[test]
    fn decision_to_text_invalid_first_message() {
        let text = SessionEnforcementDecision::WouldDisconnectInvalidFirstMessage.to_text();
        assert!(text.contains("would_disconnect"));
        assert!(text.contains("invalid_first_message"));
    }

    #[test]
    fn decision_to_text_version_mismatch() {
        let text = SessionEnforcementDecision::WouldDisconnectVersionMismatch {
            client: 2,
            server: 1,
        }
        .to_text();
        assert!(text.contains("client=2"));
        assert!(text.contains("server=1"));
    }

    #[test]
    fn decision_to_text_capability_gate_failure() {
        let text = SessionEnforcementDecision::WouldDisconnectCapabilityGateFailure {
            capability: "ResourceAnnouncement".to_string(),
        }
        .to_text();
        assert!(text.contains("capability=ResourceAnnouncement"));
    }

    #[test]
    fn decision_to_text_invalid_state_transition() {
        let text = SessionEnforcementDecision::WouldDisconnectInvalidStateTransition {
            from: "HelloReceived".to_string(),
            to: "ReadyDryRun".to_string(),
        }
        .to_text();
        assert!(text.contains("from=HelloReceived"));
        assert!(text.contains("to=ReadyDryRun"));
    }

    #[test]
    fn decision_to_text_mark_failed() {
        let text = SessionEnforcementDecision::WouldMarkSessionFailed {
            reason: "test failure".to_string(),
        }
        .to_text();
        assert!(text.contains("reason=test failure"));
    }

    #[test]
    fn to_text_is_deterministic() {
        let d1 = SessionEnforcementDecision::Allow;
        let d2 = SessionEnforcementDecision::Allow;
        assert_eq!(d1.to_text(), d2.to_text());

        let d1 = SessionEnforcementDecision::WouldDisconnectVersionMismatch {
            client: 1,
            server: 1,
        };
        let d2 = SessionEnforcementDecision::WouldDisconnectVersionMismatch {
            client: 1,
            server: 1,
        };
        assert_eq!(d1.to_text(), d2.to_text());
    }

    #[test]
    fn extract_versions_from_standard_reason() {
        let reason = "protocol mismatch: client=99 server=1";
        let (client, server) = super::extract_versions(reason);
        assert_eq!(client, 99);
        assert_eq!(server, 1);
    }

    #[test]
    fn extract_versions_missing_values_defaults_zero() {
        let reason = "protocol mismatch: client= server=";
        let (client, server) = super::extract_versions(reason);
        assert_eq!(client, 0);
        assert_eq!(server, 0);
    }

    #[test]
    fn extract_versions_no_match_defaults_zero() {
        let reason = "some other error";
        let (client, server) = super::extract_versions(reason);
        assert_eq!(client, 0);
        assert_eq!(server, 0);
    }

    #[test]
    fn policy_equality() {
        assert_eq!(
            SessionEnforcementPolicy::ReportOnly,
            SessionEnforcementPolicy::ReportOnly
        );
        assert_eq!(
            SessionEnforcementPolicy::Strict,
            SessionEnforcementPolicy::Strict
        );
        assert_ne!(
            SessionEnforcementPolicy::ReportOnly,
            SessionEnforcementPolicy::Strict
        );
    }

    #[test]
    fn failed_without_reason_marks_failed() {
        let decision = evaluate_enforcement(
            &SessionState::Failed,
            None,
            &SessionEnforcementPolicy::Strict,
        );
        assert_eq!(
            decision,
            SessionEnforcementDecision::WouldMarkSessionFailed {
                reason: "unknown failure".to_string()
            }
        );
    }

    #[test]
    fn session_stuck_at_hello_received_marks_failed() {
        let mut sm = SessionStateMachine::new();
        sm.on_hello_received().unwrap();
        // transition_to doesn't call fail() — state stays HelloReceived
        let _ = sm.transition_to(SessionState::ResourceAnnouncementSent);
        let decision = evaluate_enforcement(
            sm.state(),
            sm.failure_reason(),
            &SessionEnforcementPolicy::Strict,
        );
        assert!(matches!(
            decision,
            SessionEnforcementDecision::WouldMarkSessionFailed { .. }
        ));
    }
}
