/// Local-only admin debug command set.
/// No network exposure. No authentication required (no remote access).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminCommand {
    Help,
    Status,
    Sessions,
    Resources,
    Diagnostics,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminCommandParseError {
    Empty,
    UnknownCommand(String),
}

impl std::fmt::Display for AdminCommandParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminCommandParseError::Empty => write!(f, "empty command"),
            AdminCommandParseError::UnknownCommand(cmd) => {
                write!(f, "unknown command: {cmd}")
            }
        }
    }
}

impl std::error::Error for AdminCommandParseError {}

#[derive(Debug, Clone)]
pub struct AdminCommandResult {
    pub command: AdminCommand,
    pub message: String,
    pub should_quit: bool,
}

/// Parse a local admin command from a line of stdin input.
/// Case-insensitive. Trims leading/trailing whitespace.
/// Returns `Err(Empty)` for blank input.
/// Returns `Err(UnknownCommand)` for unrecognised input.
pub fn parse_admin_command(input: &str) -> Result<AdminCommand, AdminCommandParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AdminCommandParseError::Empty);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "help" => Ok(AdminCommand::Help),
        "status" => Ok(AdminCommand::Status),
        "sessions" => Ok(AdminCommand::Sessions),
        "resources" => Ok(AdminCommand::Resources),
        "diagnostics" => Ok(AdminCommand::Diagnostics),
        "quit" => Ok(AdminCommand::Quit),
        other => Err(AdminCommandParseError::UnknownCommand(other.to_string())),
    }
}

/// Handle a parsed admin command. Returns placeholder messages; `should_quit` only for `Quit`.
pub fn handle_admin_command(command: AdminCommand) -> AdminCommandResult {
    handle_admin_command_with_context(command, None, None)
}

/// Handle a parsed admin command with an optional runtime status snapshot.
pub fn handle_admin_command_with_status(
    command: AdminCommand,
    status: Option<&crate::status::ServerRuntimeStatus>,
) -> AdminCommandResult {
    handle_admin_command_with_context(command, status, None)
}

/// Full context handler — status for policy/resource fields, registry for diagnostics.
/// When `registry` is `Some`, the `Diagnostics` command emits live session registry text.
pub fn handle_admin_command_with_context(
    command: AdminCommand,
    status: Option<&crate::status::ServerRuntimeStatus>,
    registry: Option<&crate::session_registry::SessionRegistrySnapshot>,
) -> AdminCommandResult {
    let (message, should_quit) = match &command {
        AdminCommand::Help => (
            "commands: help, status, sessions, resources, diagnostics, quit".to_string(),
            false,
        ),
        AdminCommand::Status => (
            status.map(|s| s.to_text()).unwrap_or_else(|| {
                "server is running (dry-run mode, all policies report-only)".to_string()
            }),
            false,
        ),
        AdminCommand::Sessions => {
            let msg = match registry {
                Some(r) => r.to_diagnostics_text(),
                None => status
                    .map(|s| {
                        format!(
                            "connected={} ready_dry_run={} failed={}",
                            s.connected_sessions, s.ready_dry_run_sessions, s.failed_sessions,
                        )
                    })
                    .unwrap_or_else(|| {
                        "live session data not yet available (placeholder)".to_string()
                    }),
            };
            (msg, false)
        }
        AdminCommand::Resources => (
            status
                .map(|s| format!("announcement_dir={}", s.resource_announcement_dir))
                .unwrap_or_else(|| {
                    "live resource data not yet available (placeholder)".to_string()
                }),
            false,
        ),
        AdminCommand::Diagnostics => (
            registry
                .map(|r| r.to_diagnostics_text())
                .unwrap_or_else(|| "live diagnostics not yet available (placeholder)".to_string()),
            false,
        ),
        AdminCommand::Quit => (
            "server shutdown requested via admin command".to_string(),
            true,
        ),
    };
    AdminCommandResult {
        command,
        message,
        should_quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help() {
        assert_eq!(parse_admin_command("help").unwrap(), AdminCommand::Help);
    }

    #[test]
    fn parse_status() {
        assert_eq!(parse_admin_command("status").unwrap(), AdminCommand::Status);
    }

    #[test]
    fn parse_sessions() {
        assert_eq!(
            parse_admin_command("sessions").unwrap(),
            AdminCommand::Sessions
        );
    }

    #[test]
    fn parse_resources() {
        assert_eq!(
            parse_admin_command("resources").unwrap(),
            AdminCommand::Resources
        );
    }

    #[test]
    fn parse_diagnostics() {
        assert_eq!(
            parse_admin_command("diagnostics").unwrap(),
            AdminCommand::Diagnostics
        );
    }

    #[test]
    fn parse_quit() {
        assert_eq!(parse_admin_command("quit").unwrap(), AdminCommand::Quit);
    }

    #[test]
    fn case_insensitive_parsing() {
        assert_eq!(parse_admin_command("HELP").unwrap(), AdminCommand::Help);
        assert_eq!(parse_admin_command("Status").unwrap(), AdminCommand::Status);
        assert_eq!(parse_admin_command("QUIT").unwrap(), AdminCommand::Quit);
        assert_eq!(
            parse_admin_command("SESSIONS").unwrap(),
            AdminCommand::Sessions
        );
        assert_eq!(
            parse_admin_command("RESOURCES").unwrap(),
            AdminCommand::Resources
        );
        assert_eq!(
            parse_admin_command("DIAGNOSTICS").unwrap(),
            AdminCommand::Diagnostics
        );
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(parse_admin_command("  help  ").unwrap(), AdminCommand::Help);
        assert_eq!(parse_admin_command("\tquit\n").unwrap(), AdminCommand::Quit);
        assert_eq!(
            parse_admin_command("  status").unwrap(),
            AdminCommand::Status
        );
    }

    #[test]
    fn empty_input_returns_empty_error() {
        assert_eq!(
            parse_admin_command("").unwrap_err(),
            AdminCommandParseError::Empty
        );
        assert_eq!(
            parse_admin_command("   ").unwrap_err(),
            AdminCommandParseError::Empty
        );
        assert_eq!(
            parse_admin_command("\t\n").unwrap_err(),
            AdminCommandParseError::Empty
        );
    }

    #[test]
    fn unknown_command_error() {
        let err = parse_admin_command("reboot").unwrap_err();
        assert_eq!(
            err,
            AdminCommandParseError::UnknownCommand("reboot".to_string())
        );
    }

    #[test]
    fn quit_result_sets_should_quit() {
        let result = handle_admin_command(AdminCommand::Quit);
        assert!(result.should_quit);
        assert_eq!(result.command, AdminCommand::Quit);
    }

    #[test]
    fn non_quit_commands_do_not_quit() {
        for cmd in [
            AdminCommand::Help,
            AdminCommand::Status,
            AdminCommand::Sessions,
            AdminCommand::Resources,
            AdminCommand::Diagnostics,
        ] {
            let result = handle_admin_command(cmd);
            assert!(
                !result.should_quit,
                "expected should_quit=false for {:?}",
                result.command
            );
        }
    }

    #[test]
    fn handle_produces_nonempty_message_for_all_commands() {
        for cmd in [
            AdminCommand::Help,
            AdminCommand::Status,
            AdminCommand::Sessions,
            AdminCommand::Resources,
            AdminCommand::Diagnostics,
            AdminCommand::Quit,
        ] {
            let result = handle_admin_command(cmd);
            assert!(!result.message.is_empty());
        }
    }

    #[test]
    fn unknown_command_error_includes_input() {
        let err = parse_admin_command("invalidcmd").unwrap_err();
        match err {
            AdminCommandParseError::UnknownCommand(s) => assert_eq!(s, "invalidcmd"),
            other => panic!("expected UnknownCommand, got {other:?}"),
        }
    }

    #[test]
    fn handle_with_status_returns_status_text_for_status_command() {
        use crate::config::ServerConfig;
        use crate::status::ServerRuntimeStatus;
        let status = ServerRuntimeStatus::from_config(&ServerConfig::default());
        let result = handle_admin_command_with_status(AdminCommand::Status, Some(&status));
        assert_eq!(result.message, status.to_text());
        assert!(!result.should_quit);
    }

    #[test]
    fn handle_with_status_none_falls_back_to_placeholder_for_status() {
        let result = handle_admin_command_with_status(AdminCommand::Status, None);
        assert!(result.message.contains("server is running"));
    }

    #[test]
    fn handle_with_status_sessions_shows_counts() {
        use crate::config::ServerConfig;
        use crate::status::ServerRuntimeStatus;
        let status =
            ServerRuntimeStatus::from_config(&ServerConfig::default()).with_session_counts(4, 2, 1);
        let result = handle_admin_command_with_status(AdminCommand::Sessions, Some(&status));
        assert!(result.message.contains("connected=4"));
        assert!(result.message.contains("ready_dry_run=2"));
        assert!(result.message.contains("failed=1"));
    }

    #[test]
    fn handle_with_status_resources_shows_dir() {
        use crate::config::ServerConfig;
        use crate::status::ServerRuntimeStatus;
        let status = ServerRuntimeStatus::from_config(&ServerConfig::default());
        let result = handle_admin_command_with_status(AdminCommand::Resources, Some(&status));
        assert!(result.message.contains("announcement_dir="));
        assert!(!result.message.contains("placeholder"));
    }

    #[test]
    fn diagnostics_with_empty_registry_snapshot() {
        use crate::session_registry::SessionRegistry;
        let snap = SessionRegistry::new().snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert!(result.message.contains("no active sessions"));
        assert!(!result.should_quit);
    }

    #[test]
    fn diagnostics_with_connected_session() {
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert!(result.message.contains("sessions: 1"));
        assert!(result.message.contains("session-1"));
    }

    #[test]
    fn diagnostics_with_ready_dry_run_session() {
        use crate::session::SessionState;
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::ReadyDryRun);
        let snap = reg.snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert!(result.message.contains("ready_dry_run=true"));
        assert!(result.message.contains("ready_dry_run: 1"));
    }

    #[test]
    fn diagnostics_with_failed_session() {
        use crate::session::SessionState;
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.update_session_state(&id, SessionState::Failed);
        let snap = reg.snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert!(result.message.contains("failed=true"));
        assert!(result.message.contains("failed: 1"));
    }

    #[test]
    fn diagnostics_output_is_deterministic() {
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let r1 = handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        let r2 = handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert_eq!(r1.message, r2.message);
    }

    #[test]
    fn diagnostics_output_omits_ip_personal_data() {
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert!(!result.message.contains("ip"));
        assert!(!result.message.contains("addr"));
        assert!(!result.message.contains("peer"));
        assert!(!result.message.contains("name"));
    }

    #[test]
    fn diagnostics_does_not_mutate_snapshot() {
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let count_before = snap.connected_sessions;
        let _ = handle_admin_command_with_context(AdminCommand::Diagnostics, None, Some(&snap));
        assert_eq!(snap.connected_sessions, count_before);
    }

    #[test]
    fn diagnostics_falls_back_to_placeholder_without_registry() {
        let result = handle_admin_command_with_context(AdminCommand::Diagnostics, None, None);
        assert!(result.message.contains("placeholder"));
    }

    // --- Sessions command tests ---

    #[test]
    fn sessions_with_empty_registry() {
        use crate::session_registry::SessionRegistry;
        let snap = SessionRegistry::new().snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Sessions, None, Some(&snap));
        assert!(result.message.contains("no active sessions"));
        assert!(!result.should_quit);
    }

    #[test]
    fn sessions_with_connected_session() {
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Sessions, None, Some(&snap));
        assert!(result.message.contains("sessions: 1"));
        assert!(result.message.contains("session-1"));
        assert!(result.message.contains("protocol=unknown"));
    }

    #[test]
    fn sessions_with_ready_dry_run_and_protocol_version() {
        use crate::session::SessionState;
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        let id = reg.create_session();
        reg.set_protocol_version(&id, 1);
        reg.update_session_state(&id, SessionState::ReadyDryRun);
        let snap = reg.snapshot();
        let result =
            handle_admin_command_with_context(AdminCommand::Sessions, None, Some(&snap));
        assert!(result.message.contains("ready_dry_run=true"));
        assert!(result.message.contains("ready_dry_run: 1"));
        assert!(result.message.contains("protocol=v1"));
    }

    #[test]
    fn sessions_falls_back_to_counts_without_registry() {
        use crate::config::ServerConfig;
        use crate::status::ServerRuntimeStatus;
        let status =
            ServerRuntimeStatus::from_config(&ServerConfig::default()).with_session_counts(2, 1, 1);
        let result = handle_admin_command_with_context(AdminCommand::Sessions, Some(&status), None);
        assert!(result.message.contains("connected=2"));
        assert!(result.message.contains("ready_dry_run=1"));
        assert!(result.message.contains("failed=1"));
        assert!(!result.message.contains("session-"));
    }

    #[test]
    fn sessions_with_registry_preferred_over_status() {
        use crate::session_registry::SessionRegistry;
        let mut reg = SessionRegistry::new();
        reg.create_session();
        let snap = reg.snapshot();
        let result = handle_admin_command_with_context(AdminCommand::Sessions, None, Some(&snap));
        // Registry output shows session-1, not fallback count format
        assert!(result.message.contains("session-1"));
    }
}
