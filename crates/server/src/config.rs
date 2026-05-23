use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context;
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

// --- Top-level config ---

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub server: ServerSection,
    pub protocol: ProtocolSection,
    pub resources: ResourcesSection,
    pub join_gate: JoinGateSection,
    pub diagnostics: DiagnosticsSection,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSection::default(),
            protocol: ProtocolSection::default(),
            resources: ResourcesSection::default(),
            join_gate: JoinGateSection::default(),
            diagnostics: DiagnosticsSection::default(),
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
}
