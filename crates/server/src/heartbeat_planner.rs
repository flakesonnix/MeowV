/// Heartbeat timeout policy mode. Report-only by default; no enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatPolicy {
    /// Never escalate to disconnect. Log issues only.
    ReportOnly,
    /// Evaluate what would happen under strict enforcement (no actual disconnect).
    Strict,
}

/// Cumulative heartbeat counts used as planner input.
///
/// Under a server-only view, `timeout_or_error` is always 0 — the server does not
/// receive client-side timeout reports. When evaluated from client-side `HeartbeatMetrics`,
/// all three fields may be non-zero.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeartbeatPlannerInput {
    /// Total pings initiated (client-sent or server-received).
    pub ping_sent: u64,
    /// Total matching pongs received.
    pub pong_received: u64,
    /// Total ping timeouts or send/receive errors (client-side only; 0 in server view).
    pub timeout_or_error: u64,
}

/// Under `Strict` policy, `timeout_or_error >= MISSED_HEARTBEAT_DISCONNECT_THRESHOLD`
/// yields `WouldDisconnectMissedHeartbeat`. No actual disconnect is performed.
pub const MISSED_HEARTBEAT_DISCONNECT_THRESHOLD: u64 = 3;

/// Deterministic outcome of a heartbeat health evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatDecision {
    /// No heartbeat activity — client never sent a Ping.
    NoHeartbeatObserved,
    /// All pings received matching pongs; no errors or timeouts.
    Healthy,
    /// Pings sent but no pong received yet; no timeout recorded.
    WouldWarnNoPongYet,
    /// One or more heartbeat timeouts or errors occurred.
    WouldWarnTimeout,
    /// Pong gap (pong_received < ping_sent) with no recorded timeout.
    WouldMarkUnhealthy,
    /// Under `Strict`: repeated missed heartbeats would trigger disconnect.
    WouldDisconnectMissedHeartbeat,
}

impl HeartbeatDecision {
    pub fn to_text(&self) -> String {
        match self {
            HeartbeatDecision::NoHeartbeatObserved => {
                "heartbeat_decision: no_heartbeat_observed".to_string()
            }
            HeartbeatDecision::Healthy => "heartbeat_decision: healthy".to_string(),
            HeartbeatDecision::WouldWarnNoPongYet => {
                "heartbeat_decision: would_warn no_pong_yet".to_string()
            }
            HeartbeatDecision::WouldWarnTimeout => {
                "heartbeat_decision: would_warn timeout".to_string()
            }
            HeartbeatDecision::WouldMarkUnhealthy => {
                "heartbeat_decision: would_mark_unhealthy".to_string()
            }
            HeartbeatDecision::WouldDisconnectMissedHeartbeat => {
                "heartbeat_decision: would_disconnect missed_heartbeat".to_string()
            }
        }
    }

    /// Concise single-word label for compact admin/sessions output.
    pub fn to_short_label(&self) -> &'static str {
        match self {
            HeartbeatDecision::NoHeartbeatObserved => "no_activity",
            HeartbeatDecision::Healthy => "healthy",
            HeartbeatDecision::WouldWarnNoPongYet => "no_pong_yet",
            HeartbeatDecision::WouldWarnTimeout => "warn_timeout",
            HeartbeatDecision::WouldMarkUnhealthy => "unhealthy",
            HeartbeatDecision::WouldDisconnectMissedHeartbeat => "would_disconnect",
        }
    }
}

/// Evaluate the heartbeat health decision from cumulative counts and an active policy.
///
/// Pure function — no I/O, no side effects. Deterministic.
///
/// Under `ReportOnly` the decision never escalates to `WouldDisconnectMissedHeartbeat`.
/// Under `Strict`, `timeout_or_error >= MISSED_HEARTBEAT_DISCONNECT_THRESHOLD` returns
/// `WouldDisconnectMissedHeartbeat`. No actual disconnect is performed by this function.
pub fn evaluate_heartbeat(
    input: &HeartbeatPlannerInput,
    policy: &HeartbeatPolicy,
) -> HeartbeatDecision {
    if input.ping_sent == 0 {
        return HeartbeatDecision::NoHeartbeatObserved;
    }

    if input.pong_received >= input.ping_sent && input.timeout_or_error == 0 {
        return HeartbeatDecision::Healthy;
    }

    if input.pong_received == 0 && input.timeout_or_error == 0 {
        return HeartbeatDecision::WouldWarnNoPongYet;
    }

    if input.timeout_or_error > 0 {
        return match policy {
            HeartbeatPolicy::ReportOnly => HeartbeatDecision::WouldWarnTimeout,
            HeartbeatPolicy::Strict => {
                if input.timeout_or_error >= MISSED_HEARTBEAT_DISCONNECT_THRESHOLD {
                    HeartbeatDecision::WouldDisconnectMissedHeartbeat
                } else {
                    HeartbeatDecision::WouldWarnTimeout
                }
            }
        };
    }

    HeartbeatDecision::WouldMarkUnhealthy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(ping_sent: u64, pong_received: u64, timeout_or_error: u64) -> HeartbeatPlannerInput {
        HeartbeatPlannerInput {
            ping_sent,
            pong_received,
            timeout_or_error,
        }
    }

    #[test]
    fn no_heartbeat_activity_returns_no_heartbeat_observed() {
        let i = input(0, 0, 0);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::NoHeartbeatObserved
        );
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::NoHeartbeatObserved
        );
    }

    #[test]
    fn all_pings_answered_returns_healthy() {
        let i = input(5, 5, 0);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::Healthy
        );
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::Healthy
        );
    }

    #[test]
    fn single_ping_single_pong_is_healthy() {
        let i = input(1, 1, 0);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::Healthy
        );
    }

    #[test]
    fn ping_sent_no_pong_no_error_returns_would_warn_no_pong_yet() {
        let i = input(1, 0, 0);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::WouldWarnNoPongYet
        );
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::WouldWarnNoPongYet
        );
    }

    #[test]
    fn one_timeout_under_report_only_returns_would_warn_timeout() {
        let i = input(3, 2, 1);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::WouldWarnTimeout
        );
    }

    #[test]
    fn one_timeout_under_strict_below_threshold_returns_would_warn_timeout() {
        let i = input(3, 2, 1);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::WouldWarnTimeout
        );
    }

    #[test]
    fn two_timeouts_under_strict_below_threshold_returns_would_warn_timeout() {
        let i = input(5, 3, 2);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::WouldWarnTimeout
        );
    }

    #[test]
    fn at_threshold_timeouts_under_strict_returns_would_disconnect() {
        let i = input(5, 2, MISSED_HEARTBEAT_DISCONNECT_THRESHOLD);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::WouldDisconnectMissedHeartbeat
        );
    }

    #[test]
    fn above_threshold_timeouts_under_strict_returns_would_disconnect() {
        let i = input(10, 4, MISSED_HEARTBEAT_DISCONNECT_THRESHOLD + 2);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::WouldDisconnectMissedHeartbeat
        );
    }

    #[test]
    fn above_threshold_timeouts_under_report_only_never_disconnects() {
        let i = input(10, 4, MISSED_HEARTBEAT_DISCONNECT_THRESHOLD + 5);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::WouldWarnTimeout
        );
    }

    #[test]
    fn pong_gap_no_timeout_returns_would_mark_unhealthy() {
        let i = input(5, 3, 0);
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::WouldMarkUnhealthy
        );
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::Strict),
            HeartbeatDecision::WouldMarkUnhealthy
        );
    }

    #[test]
    fn to_text_no_heartbeat_observed() {
        let text = HeartbeatDecision::NoHeartbeatObserved.to_text();
        assert!(text.contains("no_heartbeat_observed"));
    }

    #[test]
    fn to_text_healthy() {
        let text = HeartbeatDecision::Healthy.to_text();
        assert!(text.contains("healthy"));
    }

    #[test]
    fn to_text_would_warn_no_pong_yet() {
        let text = HeartbeatDecision::WouldWarnNoPongYet.to_text();
        assert!(text.contains("no_pong_yet"));
    }

    #[test]
    fn to_text_would_warn_timeout() {
        let text = HeartbeatDecision::WouldWarnTimeout.to_text();
        assert!(text.contains("timeout"));
    }

    #[test]
    fn to_text_would_mark_unhealthy() {
        let text = HeartbeatDecision::WouldMarkUnhealthy.to_text();
        assert!(text.contains("unhealthy"));
    }

    #[test]
    fn to_text_would_disconnect_missed_heartbeat() {
        let text = HeartbeatDecision::WouldDisconnectMissedHeartbeat.to_text();
        assert!(text.contains("missed_heartbeat"));
    }

    #[test]
    fn to_text_is_deterministic() {
        let d1 = evaluate_heartbeat(&input(3, 3, 0), &HeartbeatPolicy::ReportOnly);
        let d2 = evaluate_heartbeat(&input(3, 3, 0), &HeartbeatPolicy::ReportOnly);
        assert_eq!(d1.to_text(), d2.to_text());
    }

    #[test]
    fn policy_equality() {
        assert_eq!(HeartbeatPolicy::ReportOnly, HeartbeatPolicy::ReportOnly);
        assert_eq!(HeartbeatPolicy::Strict, HeartbeatPolicy::Strict);
        assert_ne!(HeartbeatPolicy::ReportOnly, HeartbeatPolicy::Strict);
    }

    #[test]
    fn default_input_has_no_heartbeat_observed() {
        let i = HeartbeatPlannerInput::default();
        assert_eq!(
            evaluate_heartbeat(&i, &HeartbeatPolicy::ReportOnly),
            HeartbeatDecision::NoHeartbeatObserved
        );
    }

    #[test]
    fn to_short_label_no_heartbeat_observed() {
        assert_eq!(HeartbeatDecision::NoHeartbeatObserved.to_short_label(), "no_activity");
    }

    #[test]
    fn to_short_label_healthy() {
        assert_eq!(HeartbeatDecision::Healthy.to_short_label(), "healthy");
    }

    #[test]
    fn to_short_label_no_pong_yet() {
        assert_eq!(HeartbeatDecision::WouldWarnNoPongYet.to_short_label(), "no_pong_yet");
    }

    #[test]
    fn to_short_label_warn_timeout() {
        assert_eq!(HeartbeatDecision::WouldWarnTimeout.to_short_label(), "warn_timeout");
    }

    #[test]
    fn to_short_label_unhealthy() {
        assert_eq!(HeartbeatDecision::WouldMarkUnhealthy.to_short_label(), "unhealthy");
    }

    #[test]
    fn to_short_label_would_disconnect() {
        assert_eq!(
            HeartbeatDecision::WouldDisconnectMissedHeartbeat.to_short_label(),
            "would_disconnect"
        );
    }
}
