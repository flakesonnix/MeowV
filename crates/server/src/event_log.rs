use crate::session::SessionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEventKind {
    Connected,
    HelloReceived,
    VersionChecked,
    ProtocolNegotiationDryRun,
    CapabilityGateChecked,
    /// Heartbeat ping was received from the client
    PingReceived,
    /// Heartbeat pong was sent to the client
    PongSent,
    /// Server-initiated heartbeat ping sent to the client
    ServerPingSent,
    /// Client replied to a server-initiated heartbeat ping
    ServerPongReceived,
    ResourceAnnouncementSent,
    AvailabilityReportReceived,
    ResourcePolicyEvaluated,
    JoinGateDryRunSent,
    ReadyDryRun,
    Failed,
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub sequence: u64,
    pub kind: SessionEventKind,
    pub state: SessionState,
    pub message: String,
}

pub struct SessionEventLog {
    events: Vec<SessionEvent>,
    next_sequence: u64,
}

impl SessionEventLog {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_sequence: 1,
        }
    }

    pub fn record(
        &mut self,
        kind: SessionEventKind,
        state: SessionState,
        message: impl Into<String>,
    ) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.events.push(SessionEvent {
            sequence,
            kind,
            state,
            message: message.into(),
        });
    }

    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    pub fn last(&self) -> Option<&SessionEvent> {
        self.events.last()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn count_kind(&self, kind: SessionEventKind) -> usize {
        self.events.iter().filter(|event| event.kind == kind).count()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_event_log() {
        let log = SessionEventLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.last().is_none());
        assert!(log.events().is_empty());
    }

    #[test]
    fn first_sequence_is_one() {
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::Connected,
            SessionState::Connected,
            "connected",
        );
        assert_eq!(log.events()[0].sequence, 1);
    }

    #[test]
    fn sequence_increments() {
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::Connected,
            SessionState::Connected,
            "connected",
        );
        log.record(
            SessionEventKind::HelloReceived,
            SessionState::HelloReceived,
            "hello",
        );
        assert_eq!(log.events()[0].sequence, 1);
        assert_eq!(log.events()[1].sequence, 2);
    }

    #[test]
    fn last_event_works() {
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::Connected,
            SessionState::Connected,
            "connected",
        );
        log.record(
            SessionEventKind::HelloReceived,
            SessionState::HelloReceived,
            "hello",
        );
        let last = log.last().unwrap();
        assert_eq!(last.sequence, 2);
        assert_eq!(last.kind, SessionEventKind::HelloReceived);
    }

    #[test]
    fn event_stores_state_and_message() {
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::VersionChecked,
            SessionState::VersionChecked,
            "version 1 matched",
        );
        let ev = &log.events()[0];
        assert_eq!(ev.kind, SessionEventKind::VersionChecked);
        assert_eq!(ev.state, SessionState::VersionChecked);
        assert_eq!(ev.message, "version 1 matched");
    }

    #[test]
    fn failed_event_can_be_recorded() {
        let mut log = SessionEventLog::new();
        log.record(
            SessionEventKind::Failed,
            SessionState::Failed,
            "protocol mismatch: client=99 server=1",
        );
        let ev = log.last().unwrap();
        assert_eq!(ev.kind, SessionEventKind::Failed);
        assert_eq!(ev.state, SessionState::Failed);
    }

    #[test]
    fn len_matches_record_count() {
        let mut log = SessionEventLog::new();
        assert_eq!(log.len(), 0);
        log.record(SessionEventKind::Connected, SessionState::Connected, "a");
        assert_eq!(log.len(), 1);
        log.record(
            SessionEventKind::HelloReceived,
            SessionState::HelloReceived,
            "b",
        );
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn count_kind_counts_matching_events() {
        let mut log = SessionEventLog::new();
        log.record(SessionEventKind::PingReceived, SessionState::Connected, "ping-1");
        log.record(SessionEventKind::PongSent, SessionState::Connected, "pong-1");
        log.record(SessionEventKind::PingReceived, SessionState::Connected, "ping-2");
        assert_eq!(log.count_kind(SessionEventKind::PingReceived), 2);
        assert_eq!(log.count_kind(SessionEventKind::PongSent), 1);
    }

    #[test]
    fn full_session_records_in_order() {
        let mut log = SessionEventLog::new();
        let entries: &[(SessionEventKind, SessionState, &str)] = &[
            (
                SessionEventKind::Connected,
                SessionState::Connected,
                "connected",
            ),
            (
                SessionEventKind::HelloReceived,
                SessionState::HelloReceived,
                "hello",
            ),
            (
                SessionEventKind::VersionChecked,
                SessionState::VersionChecked,
                "v1",
            ),
            (
                SessionEventKind::ProtocolNegotiationDryRun,
                SessionState::NegotiationDryRunLogged,
                "negotiated",
            ),
            (
                SessionEventKind::ResourceAnnouncementSent,
                SessionState::ResourceAnnouncementSent,
                "sent",
            ),
            (
                SessionEventKind::AvailabilityReportReceived,
                SessionState::AvailabilityReportReceived,
                "report",
            ),
            (
                SessionEventKind::ResourcePolicyEvaluated,
                SessionState::ResourcePolicyEvaluated,
                "policy",
            ),
            (
                SessionEventKind::JoinGateDryRunSent,
                SessionState::JoinGateDryRunSent,
                "gate",
            ),
            (
                SessionEventKind::ReadyDryRun,
                SessionState::ReadyDryRun,
                "ready",
            ),
        ];
        for (kind, state, msg) in entries.iter().cloned() {
            log.record(kind, state, msg);
        }
        assert_eq!(log.len(), 9);
        for (i, ev) in log.events().iter().enumerate() {
            assert_eq!(ev.sequence, (i + 1) as u64);
        }
        assert_eq!(log.last().unwrap().kind, SessionEventKind::ReadyDryRun);
    }
}
