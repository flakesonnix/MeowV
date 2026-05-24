use std::net::SocketAddr;
use std::path::Path;

use crate::enforcement::SessionEnforcementPolicy;
use crate::heartbeat_planner::HeartbeatPolicy;
use anyhow::Context;
use protocol::signature_engine::SignaturePolicy;
use protocol::PROTOCOL_VERSION;
use serde::Deserialize;

/// Error from [`ServerConfig::validate`].
#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "config validation error: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

// --- Section: [server] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerSection {
    pub bind_addr: String,
    pub name: String,
    pub tick_rate: u64,
    pub motd: String,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:7000".to_string(),
            name: "MeowV Local Dev Server".to_string(),
            tick_rate: 10,
            motd: "welcome to meowv milestone 0".to_string(),
        }
    }
}

// --- Section: [protocol] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProtocolSection {
    pub exact_version_required: bool,
    pub negotiation_dry_run: bool,
    pub capability_gates_report_only: bool,
}

impl Default for ProtocolSection {
    fn default() -> Self {
        Self {
            exact_version_required: true,
            negotiation_dry_run: true,
            capability_gates_report_only: true,
        }
    }
}

// --- Section: [resources] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ResourcesSection {
    pub announcement_resource_dir: String,
    pub cache_dir: String,
}

impl Default for ResourcesSection {
    fn default() -> Self {
        Self {
            announcement_resource_dir: "examples/resources/chat".to_string(),
            cache_dir: "examples/cache/chat-valid".to_string(),
        }
    }
}

// --- Section: [join_gate] ---

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinGateConfigMode {
    DryRun,
}

impl Default for JoinGateConfigMode {
    fn default() -> Self {
        JoinGateConfigMode::DryRun
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct JoinGateSection {
    pub mode: JoinGateConfigMode,
    pub enforce_required_resources: bool,
}

impl Default for JoinGateSection {
    fn default() -> Self {
        Self {
            mode: JoinGateConfigMode::DryRun,
            enforce_required_resources: false,
        }
    }
}

// --- Section: [diagnostics] ---

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsFormat {
    Text,
    JsonStub,
}

impl Default for DiagnosticsFormat {
    fn default() -> Self {
        DiagnosticsFormat::Text
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DiagnosticsSection {
    pub print_session_diagnostics: bool,
    pub print_event_log: bool,
    pub format: DiagnosticsFormat,
}

impl Default for DiagnosticsSection {
    fn default() -> Self {
        Self {
            print_session_diagnostics: true,
            print_event_log: false,
            format: DiagnosticsFormat::Text,
        }
    }
}

// --- Section: [logging] ---

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    Text,
    Json,
}

impl Default for LogFormat {
    fn default() -> Self {
        LogFormat::Text
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LoggingSection {
    pub level: LogLevel,
    pub format: LogFormat,
    pub show_targets: bool,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Text,
            show_targets: false,
        }
    }
}

// --- Section: [admin] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AdminSection {
    pub local_stdin_enabled: bool,
}

impl Default for AdminSection {
    fn default() -> Self {
        Self {
            local_stdin_enabled: false,
        }
    }
}

// --- Section: [enforcement] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EnforcementSection {
    pub mode: SessionEnforcementPolicy,
}

impl Default for EnforcementSection {
    fn default() -> Self {
        Self {
            mode: SessionEnforcementPolicy::ReportOnly,
        }
    }
}

// --- Section: [signature] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SignatureSection {
    pub policy: SignaturePolicy,
}

impl Default for SignatureSection {
    fn default() -> Self {
        Self {
            policy: SignaturePolicy::ReportOnly,
        }
    }
}

// --- Section: [heartbeat] ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HeartbeatSection {
    pub policy: HeartbeatPolicy,
    /// Interval in milliseconds between server-initiated ServerPing messages.
    /// Set to 0 to disable the server-initiated heartbeat scheduler entirely.
    /// Default: 5000 (5 seconds).
    pub server_ping_interval_ms: u64,
}

impl Default for HeartbeatSection {
    fn default() -> Self {
        Self {
            policy: HeartbeatPolicy::ReportOnly,
            server_ping_interval_ms: 5000,
        }
    }
}

// --- Top-level config ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub server: ServerSection,
    pub protocol: ProtocolSection,
    pub resources: ResourcesSection,
    pub join_gate: JoinGateSection,
    pub diagnostics: DiagnosticsSection,
    pub logging: LoggingSection,
    pub admin: AdminSection,
    pub enforcement: EnforcementSection,
    pub signature: SignatureSection,
    pub heartbeat: HeartbeatSection,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSection::default(),
            protocol: ProtocolSection::default(),
            resources: ResourcesSection::default(),
            join_gate: JoinGateSection::default(),
            diagnostics: DiagnosticsSection::default(),
            logging: LoggingSection::default(),
            admin: AdminSection::default(),
            enforcement: EnforcementSection::default(),
            signature: SignatureSection::default(),
            heartbeat: HeartbeatSection::default(),
        }
    }
}

impl ServerConfig {
    /// Load config from a TOML file and validate it.
    pub fn load_from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        let cfg: ServerConfig = toml::from_str(&raw).context("failed to parse config TOML")?;
        cfg.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(cfg)
    }

    /// Load config from an explicit path or `MEOWV_CONFIG` env var (fallback to
    /// defaults if neither is set), then apply `MEOWV_SERVER_BIND` and
    /// `MEOWV_TICK_RATE` env overrides.
    pub fn load_with_env(explicit_path: Option<&str>) -> anyhow::Result<Self> {
        let file_path = explicit_path
            .map(str::to_owned)
            .or_else(|| std::env::var("MEOWV_CONFIG").ok());

        let mut cfg = match file_path {
            Some(path) => Self::load_from_path(&path)?,
            None => Self::default(),
        };

        if let Ok(bind) = std::env::var("MEOWV_SERVER_BIND") {
            cfg.server.bind_addr = bind;
        }
        if let Ok(tick_rate) = std::env::var("MEOWV_TICK_RATE") {
            cfg.server.tick_rate = tick_rate.parse().context("invalid MEOWV_TICK_RATE")?;
        }

        Ok(cfg)
    }

    /// Validate the config, enforcing dry-run restrictions and path safety.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.protocol.exact_version_required {
            return Err(ConfigError(
                "protocol.exact_version_required must be true; \
relaxing version matching is not supported in this milestone"
                    .to_string(),
            ));
        }
        if !self.protocol.negotiation_dry_run {
            return Err(ConfigError(
                "protocol.negotiation_dry_run must be true; \
active negotiation enforcement is not yet supported"
                    .to_string(),
            ));
        }
        if self.join_gate.enforce_required_resources {
            return Err(ConfigError(
                "join_gate.enforce_required_resources cannot be true; \
join gate enforcement is not yet supported"
                    .to_string(),
            ));
        }
        if self.resources.announcement_resource_dir.contains("..") {
            return Err(ConfigError(
                "resources.announcement_resource_dir must not contain '..'".to_string(),
            ));
        }
        if self.resources.cache_dir.contains("..") {
            return Err(ConfigError(
                "resources.cache_dir must not contain '..'".to_string(),
            ));
        }
        self.server.bind_addr.parse::<SocketAddr>().map_err(|_| {
            ConfigError(format!(
                "server.bind_addr '{}' is not a valid socket address",
                self.server.bind_addr
            ))
        })?;
        Ok(())
    }

    /// Deterministic multi-line summary of the server's lifecycle configuration.
    /// Logged at startup. Contains no IP addresses, personal data, or timestamps.
    pub fn to_lifecycle_summary_text(&self) -> String {
        let enforce_mode = match &self.enforcement.mode {
            SessionEnforcementPolicy::ReportOnly => "report_only",
            SessionEnforcementPolicy::Strict => "strict",
        };
        let sig_policy = match &self.signature.policy {
            SignaturePolicy::ReportOnly => "report_only",
            SignaturePolicy::Strict => "strict",
        };
        let hb_policy = match &self.heartbeat.policy {
            HeartbeatPolicy::ReportOnly => "report_only",
            HeartbeatPolicy::Strict => "strict",
        };
        format!(
            "server_name: {}\n\
             bind_addr: {}\n\
             protocol_version: {}\n\
             exact_version_required: {}\n\
             negotiation_dry_run: {}\n\
             capability_gates_report_only: {}\n\
             resource_announcement_dir: {}\n\
             join_gate_mode: dry_run\n\
             join_gate_enforcement: disabled\n\
             diagnostics_print: {}\n\
             diagnostics_format: {}\n\
             admin_stdin: {}\n\
             log_level: {}\n\
             log_format: {}\n\
             session_enforcement: {enforce_mode}\n\
             signature_policy: {sig_policy}\n\
             heartbeat_policy: {hb_policy}",
            self.server.name,
            self.server.bind_addr,
            PROTOCOL_VERSION,
            self.protocol.exact_version_required,
            self.protocol.negotiation_dry_run,
            self.protocol.capability_gates_report_only,
            self.resources.announcement_resource_dir,
            self.diagnostics.print_session_diagnostics,
            match self.diagnostics.format {
                DiagnosticsFormat::Text => "text",
                DiagnosticsFormat::JsonStub => "json_stub",
            },
            if self.admin.local_stdin_enabled {
                "enabled"
            } else {
                "disabled"
            },
            self.logging.level.as_str(),
            match self.logging.format {
                LogFormat::Text => "text",
                LogFormat::Json => "json",
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        ServerConfig::default().validate().unwrap();
    }

    #[test]
    fn parse_valid_toml_config() {
        let toml = r#"
[server]
bind_addr = "127.0.0.1:9000"
name = "Test Server"
tick_rate = 20
motd = "hello"

[protocol]
exact_version_required = true
negotiation_dry_run = true
capability_gates_report_only = true

[resources]
announcement_resource_dir = "examples/resources/chat"
cache_dir = "examples/cache/chat-valid"

[join_gate]
mode = "dry_run"
enforce_required_resources = false

[diagnostics]
print_session_diagnostics = true
print_event_log = false
format = "text"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.server.bind_addr, "127.0.0.1:9000");
        assert_eq!(cfg.server.tick_rate, 20);
        assert!(cfg.protocol.exact_version_required);
        assert!(cfg.diagnostics.print_session_diagnostics);
        assert_eq!(cfg.diagnostics.format, DiagnosticsFormat::Text);
    }

    #[test]
    fn reject_exact_version_required_false() {
        let toml = r#"
[protocol]
exact_version_required = false
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.0.contains("exact_version_required"));
    }

    #[test]
    fn reject_negotiation_dry_run_false() {
        let toml = r#"
[protocol]
negotiation_dry_run = false
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.0.contains("negotiation_dry_run"));
    }

    #[test]
    fn reject_enforce_required_resources_true() {
        let toml = r#"
[join_gate]
enforce_required_resources = true
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.0.contains("enforce_required_resources"));
    }

    #[test]
    fn reject_path_traversal_in_announcement_resource_dir() {
        let toml = r#"
[resources]
announcement_resource_dir = "../../../etc/passwd"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.0.contains("announcement_resource_dir"));
    }

    #[test]
    fn reject_path_traversal_in_cache_dir() {
        let toml = r#"
[resources]
cache_dir = "../secret"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.0.contains("cache_dir"));
    }

    #[test]
    fn default_bind_addr_parses_as_socket_addr() {
        let cfg = ServerConfig::default();
        let parsed: Result<SocketAddr, _> = cfg.server.bind_addr.parse();
        assert!(parsed.is_ok());
    }

    #[test]
    fn reject_invalid_bind_addr() {
        let toml = r#"
[server]
bind_addr = "not-an-address"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.0.contains("bind_addr"));
    }

    #[test]
    fn unknown_diagnostics_format_rejected_at_parse() {
        let toml = r#"
[diagnostics]
format = "binary_dump"
"#;
        let result: Result<ServerConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn json_stub_diagnostics_format_parses() {
        let toml = r#"
[diagnostics]
format = "json_stub"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.diagnostics.format, DiagnosticsFormat::JsonStub);
        cfg.validate().unwrap();
    }

    #[test]
    fn partial_config_fills_missing_fields_with_defaults() {
        let toml = r#"
[server]
tick_rate = 30
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.server.tick_rate, 30);
        assert_eq!(cfg.server.bind_addr, ServerSection::default().bind_addr);
        assert!(cfg.protocol.exact_version_required);
    }

    #[test]
    fn default_logging_config_validates() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.logging.level, LogLevel::Info);
        assert_eq!(cfg.logging.format, LogFormat::Text);
        assert!(!cfg.logging.show_targets);
        cfg.validate().unwrap();
    }

    #[test]
    fn parse_valid_logging_section() {
        let toml = r#"
[logging]
level = "debug"
format = "json"
show_targets = true
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.logging.level, LogLevel::Debug);
        assert_eq!(cfg.logging.format, LogFormat::Json);
        assert!(cfg.logging.show_targets);
        cfg.validate().unwrap();
    }

    #[test]
    fn all_log_levels_parse() {
        for level in &["trace", "debug", "info", "warn", "error"] {
            let toml = format!("[logging]\nlevel = \"{level}\"");
            let cfg: ServerConfig = toml::from_str(&toml).unwrap();
            cfg.validate().unwrap();
        }
    }

    #[test]
    fn invalid_log_level_rejected_at_parse() {
        let toml = r#"
[logging]
level = "verbose"
"#;
        let result: Result<ServerConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_log_format_rejected_at_parse() {
        let toml = r#"
[logging]
format = "binary"
"#;
        let result: Result<ServerConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn log_level_as_str_matches_serde_name() {
        assert_eq!(LogLevel::Trace.as_str(), "trace");
        assert_eq!(LogLevel::Debug.as_str(), "debug");
        assert_eq!(LogLevel::Info.as_str(), "info");
        assert_eq!(LogLevel::Warn.as_str(), "warn");
        assert_eq!(LogLevel::Error.as_str(), "error");
    }

    #[test]
    fn default_admin_section_has_stdin_disabled() {
        let cfg = ServerConfig::default();
        assert!(!cfg.admin.local_stdin_enabled);
    }

    #[test]
    fn admin_section_parses_enabled() {
        let toml = r#"
[admin]
local_stdin_enabled = true
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.admin.local_stdin_enabled);
        cfg.validate().unwrap();
    }

    #[test]
    fn admin_section_omitted_defaults_to_disabled() {
        let toml = r#"
[server]
tick_rate = 20
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert!(!cfg.admin.local_stdin_enabled);
    }

    #[test]
    fn lifecycle_summary_deterministic() {
        let cfg = ServerConfig::default();
        assert_eq!(
            cfg.to_lifecycle_summary_text(),
            cfg.to_lifecycle_summary_text()
        );
    }

    #[test]
    fn lifecycle_summary_includes_dry_run_policies() {
        let text = ServerConfig::default().to_lifecycle_summary_text();
        assert!(text.contains("exact_version_required: true"));
        assert!(text.contains("negotiation_dry_run: true"));
        assert!(text.contains("capability_gates_report_only: true"));
        assert!(text.contains("join_gate_mode: dry_run"));
        assert!(text.contains("join_gate_enforcement: disabled"));
    }

    #[test]
    fn lifecycle_summary_includes_enforcement_and_signature() {
        let text = ServerConfig::default().to_lifecycle_summary_text();
        assert!(text.contains("session_enforcement: report_only"));
        assert!(text.contains("signature_policy: report_only"));
    }

    #[test]
    fn strict_enforcement_in_lifecycle_summary() {
        let toml = r#"
[enforcement]
mode = "strict"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let text = cfg.to_lifecycle_summary_text();
        assert!(text.contains("session_enforcement: strict"));
    }

    #[test]
    fn strict_signature_in_lifecycle_summary() {
        let toml = r#"
[signature]
policy = "strict"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let text = cfg.to_lifecycle_summary_text();
        assert!(text.contains("signature_policy: strict"));
    }

    #[test]
    fn lifecycle_summary_includes_admin_logging_diagnostics() {
        let text = ServerConfig::default().to_lifecycle_summary_text();
        assert!(text.contains("admin_stdin: disabled"));
        assert!(text.contains("log_level: info"));
        assert!(text.contains("log_format: text"));
        assert!(text.contains("diagnostics_print: true"));
        assert!(text.contains("diagnostics_format: text"));
    }

    #[test]
    fn lifecycle_summary_includes_server_identity() {
        let text = ServerConfig::default().to_lifecycle_summary_text();
        assert!(text.contains("server_name:"));
        assert!(text.contains("bind_addr:"));
        assert!(text.contains("protocol_version:"));
    }

    #[test]
    fn lifecycle_summary_reflects_admin_enabled() {
        let toml = r#"
[admin]
local_stdin_enabled = true
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let text = cfg.to_lifecycle_summary_text();
        assert!(text.contains("admin_stdin: enabled"));
    }

    #[test]
    fn lifecycle_summary_no_ip_personal_data() {
        let text = ServerConfig::default().to_lifecycle_summary_text();
        assert!(!text.contains("client_ip"));
        assert!(!text.contains("peer_addr"));
        assert!(!text.contains("remote_addr"));
    }

    #[test]
    fn default_heartbeat_policy_is_report_only() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.heartbeat.policy, HeartbeatPolicy::ReportOnly);
    }

    #[test]
    fn heartbeat_section_omitted_defaults_to_report_only() {
        let toml = r#"
[server]
tick_rate = 20
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.heartbeat.policy, HeartbeatPolicy::ReportOnly);
    }

    #[test]
    fn heartbeat_policy_strict_parses() {
        let toml = r#"
[heartbeat]
policy = "strict"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.heartbeat.policy, HeartbeatPolicy::Strict);
    }

    #[test]
    fn heartbeat_policy_report_only_parses_explicitly() {
        let toml = r#"
[heartbeat]
policy = "report_only"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.heartbeat.policy, HeartbeatPolicy::ReportOnly);
    }

    #[test]
    fn invalid_heartbeat_policy_is_rejected() {
        let toml = r#"
[heartbeat]
policy = "aggressive"
"#;
        let result: Result<ServerConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn lifecycle_summary_includes_heartbeat_policy_report_only() {
        let text = ServerConfig::default().to_lifecycle_summary_text();
        assert!(text.contains("heartbeat_policy: report_only"));
    }

    #[test]
    fn lifecycle_summary_includes_heartbeat_policy_strict() {
        let toml = r#"
[heartbeat]
policy = "strict"
"#;
        let cfg: ServerConfig = toml::from_str(toml).unwrap();
        let text = cfg.to_lifecycle_summary_text();
        assert!(text.contains("heartbeat_policy: strict"));
    }
}
