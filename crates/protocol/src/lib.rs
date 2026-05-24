use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: u32,
    pub owner_id: Uuid,
    pub position: Position,
    pub tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAnnouncement {
    pub resources: Vec<AnnouncedResource>,
    pub signature: Option<ResourceAnnouncementSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAnnouncementSignature {
    pub algorithm: String,
    pub key_id: String,
    pub signature: String,
}

// ---------------------------------------------------------------------------
// Signature data model (M3.6 — structural validation only, no crypto)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    Ed25519,
}

impl SignatureAlgorithm {
    pub fn known_names() -> &'static [&'static str] {
        &["ed25519"]
    }

    pub fn is_known(name: &str) -> bool {
        Self::known_names().contains(&name)
    }
}

impl fmt::Display for SignatureAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ed25519 => write!(f, "ed25519"),
        }
    }
}

impl FromStr for SignatureAlgorithm {
    type Err = SignatureMetadataError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ed25519" => Ok(Self::Ed25519),
            other => Err(SignatureMetadataError::UnsupportedAlgorithm(
                other.to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureMetadataError {
    UnsupportedAlgorithm(String),
    EmptyAlgorithm,
    EmptyKeyId,
    EmptySignature,
}

impl fmt::Display for SignatureMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAlgorithm(alg) => {
                write!(f, "unsupported signature algorithm: '{alg}'")
            }
            Self::EmptyAlgorithm => write!(f, "signature algorithm is empty"),
            Self::EmptyKeyId => write!(f, "signature key_id is empty"),
            Self::EmptySignature => write!(f, "signature value is empty"),
        }
    }
}

pub fn validate_signature_metadata(
    signature: &ResourceAnnouncementSignature,
) -> Result<(), SignatureMetadataError> {
    if signature.algorithm.is_empty() {
        return Err(SignatureMetadataError::EmptyAlgorithm);
    }
    if signature.key_id.is_empty() {
        return Err(SignatureMetadataError::EmptyKeyId);
    }
    if signature.signature.is_empty() {
        return Err(SignatureMetadataError::EmptySignature);
    }
    let _ = SignatureAlgorithm::from_str(&signature.algorithm)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Canonical announcement payload — defines what would be signed
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalAnnouncementPayload {
    pub protocol_version: u32,
    pub algorithm: String,
    pub key_id: String,
    pub resources: Vec<CanonicalResourcePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalResourcePayload {
    pub name: String,
    pub version: String,
    pub requirement_level: ResourceRequirementLevel,
    pub files: Vec<CanonicalFilePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalFilePayload {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub fn build_canonical_payload(
    announcement: &ResourceAnnouncement,
) -> Option<CanonicalAnnouncementPayload> {
    let sig = announcement.signature.as_ref()?;
    if sig.algorithm.is_empty() || sig.key_id.is_empty() {
        return None;
    }

    let mut resources: Vec<CanonicalResourcePayload> = announcement
        .resources
        .iter()
        .map(|r| {
            let mut files: Vec<CanonicalFilePayload> = r
                .files
                .iter()
                .map(|f| CanonicalFilePayload {
                    relative_path: f.relative_path.clone(),
                    size_bytes: f.size_bytes,
                    sha256: f.sha256.clone(),
                })
                .collect();
            files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

            CanonicalResourcePayload {
                name: r.name.clone(),
                version: r.version.clone(),
                requirement_level: r.requirement_level.clone(),
                files,
            }
        })
        .collect();
    resources.sort_by(|a, b| a.name.cmp(&b.name));

    Some(CanonicalAnnouncementPayload {
        protocol_version: announcement
            .resources
            .first()
            .map(|r| r.protocol_version)
            .unwrap_or(0),
        algorithm: sig.algorithm.clone(),
        key_id: sig.key_id.clone(),
        resources,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRequirementLevel {
    Required,
    Optional,
    Recommended,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnouncedResource {
    pub name: String,
    pub version: String,
    pub files: Vec<AnnouncedResourceFile>,
    pub protocol_version: u32,
    pub requirement_level: ResourceRequirementLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnouncedResourceFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<ResourceFetchSource>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceFetchSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub scheme: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirrors: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceFetchMetadataError {
    UnsupportedScheme(String),
    DigestMismatch { expected: String, found: String },
    SizeMismatch { expected: u64, found: u64 },
    DuplicateSource { scheme: String, uri: String },
    PathTraversalInFileUri(String),
}

impl std::fmt::Display for ResourceFetchMetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceFetchMetadataError::UnsupportedScheme(s) => {
                write!(f, "unsupported scheme: {s}")
            }

            ResourceFetchMetadataError::DigestMismatch { expected, found } => {
                write!(f, "digest mismatch: expected {expected}, found {found}")
            }
            ResourceFetchMetadataError::SizeMismatch { expected, found } => {
                write!(f, "size mismatch: expected {expected}, found {found}")
            }
            ResourceFetchMetadataError::DuplicateSource { scheme, uri } => {
                write!(f, "duplicate source: {}:{}", scheme, uri)
            }
            ResourceFetchMetadataError::PathTraversalInFileUri(s) => write!(
                f,
                "file:// uri contains path traversal or suspicious components: {s}"
            ),
        }
    }
}

/// Validate and produce a deterministically-ordered list of sources for a file.
///
/// Rules implemented here are intentionally conservative and deterministic:
/// - allowed schemes: https, file, ipfs
/// - if a source includes sha256/size they must match the announced file-level values
/// - duplicate (scheme+uri) entries are rejected
/// - file:// URIs containing ".." are rejected as path-traversal
/// - returned list is sorted by (priority asc, id asc, uri asc)
pub fn validate_and_order_sources(
    file: &AnnouncedResourceFile,
) -> Result<Vec<ResourceFetchSource>, ResourceFetchMetadataError> {
    let sources = match &file.sources {
        None => return Ok(Vec::new()),
        Some(s) => s.clone(),
    };

    let mut seen = std::collections::HashSet::new();
    for s in &sources {
        // scheme validation
        let scheme = s.scheme.as_str();
        match scheme {
            "https" | "file" | "ipfs" => {}
            other => {
                return Err(ResourceFetchMetadataError::UnsupportedScheme(
                    other.to_string(),
                ));
            }
        }

        // duplicate check
        let key = (s.scheme.clone(), s.uri.clone());
        if seen.contains(&key) {
            return Err(ResourceFetchMetadataError::DuplicateSource {
                scheme: s.scheme.clone(),
                uri: s.uri.clone(),
            });
        }
        seen.insert(key);

        // file:// path traversal check (simple heuristic)
        if s.scheme == "file" && s.uri.contains("..") {
            return Err(ResourceFetchMetadataError::PathTraversalInFileUri(
                s.uri.clone(),
            ));
        }

        // If source provides sha256/size they must match file-level values
        if let Some(ref sha) = s.sha256
            && sha != &file.sha256
        {
            return Err(ResourceFetchMetadataError::DigestMismatch {
                expected: file.sha256.clone(),
                found: sha.clone(),
            });
        }
        if let Some(size) = s.size_bytes
            && size != file.size_bytes
        {
            return Err(ResourceFetchMetadataError::SizeMismatch {
                expected: file.size_bytes,
                found: size,
            });
        }
    }

    // deterministic ordering by (priority asc, id asc, uri asc)
    let mut normalized = sources;
    normalized.sort_by(|a, b| {
        let pa = a.priority.unwrap_or(100);
        let pb = b.priority.unwrap_or(100);
        pa.cmp(&pb)
            .then(
                a.id.clone()
                    .unwrap_or_default()
                    .cmp(&b.id.clone().unwrap_or_default()),
            )
            .then(a.uri.cmp(&b.uri))
    });

    Ok(normalized)
}

/// Select the best source from a list of validated, deterministically-sorted sources.
///
/// The first source (lowest priority, then id, then uri) is selected as primary;
/// remaining valid sources become fallbacks. Returns `(None, [])` when the list
/// is empty.
///
/// This is a pure, deterministic, report-only function. No network access,
/// no cache writes, no execution.
pub fn select_fetch_source(
    valid_sources: &[ResourceFetchSource],
) -> (Option<ResourceFetchSource>, Vec<ResourceFetchSource>) {
    if valid_sources.is_empty() {
        return (None, Vec::new());
    }
    let selected = valid_sources[0].clone();
    let fallbacks: Vec<ResourceFetchSource> = valid_sources[1..].to_vec();
    (Some(selected), fallbacks)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAvailabilityStatus {
    Available,
    Missing,
    SizeMismatch,
    HashMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAvailabilityReport {
    pub resources: Vec<ResourceAvailabilityEntry>,
    pub is_fully_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAvailabilityEntry {
    pub resource_name: String,
    pub file_path: String,
    pub status: ResourceAvailabilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceJoinDecision {
    Allowed,
    Blocked,
    WarningOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourcePolicyEvaluation {
    pub decision: ResourceJoinDecision,
    pub missing_required: Vec<String>,
    pub invalid_required: Vec<String>,
    pub missing_optional: Vec<String>,
    pub invalid_optional: Vec<String>,
    pub missing_recommended: Vec<String>,
    pub invalid_recommended: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JoinGateMode {
    DryRun,
    Enforced,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JoinGateOutcome {
    WouldAllow,
    WouldWarn,
    WouldBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JoinGateDecision {
    pub mode: JoinGateMode,
    pub outcome: JoinGateOutcome,
    pub reason: String,
    pub policy_evaluation: ResourcePolicyEvaluation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignatureVerificationStatus {
    NotProvided,
    UnsupportedAlgorithm,
    Invalid,
    Valid,
    NotChecked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignatureVerificationReport {
    pub status: SignatureVerificationStatus,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceDownloadPreflightAction {
    AlreadyAvailable,
    FetchMissing,
    ReplaceInvalid,
    BlockedBySignaturePolicy,
    BlockedByResourcePolicy,
    UnsupportedResource,
    WouldVerifyAfterFetch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceDownloadPreflightEntry {
    pub resource_name: String,
    pub file_path: String,
    pub action: ResourceDownloadPreflightAction,
    pub reason: String,
    /// Validation errors from source metadata, if any.
    /// Empty when sources are absent, valid, or not checked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_errors: Vec<String>,
    /// Deterministically-ordered list of valid fetch sources.
    /// Empty when no sources are declared or validation fails.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_sources: Vec<ResourceFetchSource>,
    /// The best candidate source for fetching, selected deterministically
    /// from valid_sources by lowest priority (then id, then uri).
    /// `None` when no valid sources exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_source: Option<ResourceFetchSource>,
    /// Remaining valid sources after the primary selection, in deterministic order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_sources: Vec<ResourceFetchSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceDownloadPreflightPlan {
    pub entries: Vec<ResourceDownloadPreflightEntry>,
}

impl ResourceDownloadPreflightPlan {
    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "resource download preflight: (empty)".to_string();
        }
        let mut lines = vec![format!(
            "resource download preflight: {} entr{}",
            self.entries.len(),
            if self.entries.len() == 1 { "y" } else { "ies" }
        )];
        for entry in &self.entries {
            lines.push(format!(
                "  [{}] {}:{} - {}",
                match entry.action {
                    ResourceDownloadPreflightAction::AlreadyAvailable => "already_available",
                    ResourceDownloadPreflightAction::FetchMissing => "fetch_missing",
                    ResourceDownloadPreflightAction::ReplaceInvalid => "replace_invalid",
                    ResourceDownloadPreflightAction::BlockedBySignaturePolicy =>
                        "blocked_by_signature_policy",
                    ResourceDownloadPreflightAction::BlockedByResourcePolicy =>
                        "blocked_by_resource_policy",
                    ResourceDownloadPreflightAction::UnsupportedResource => "unsupported_resource",
                    ResourceDownloadPreflightAction::WouldVerifyAfterFetch =>
                        "would_verify_after_fetch",
                },
                entry.resource_name,
                entry.file_path,
                entry.reason,
            ));
            if !entry.source_errors.is_empty() {
                for err in &entry.source_errors {
                    lines.push(format!("    source error: {}", err));
                }
            }
            if !entry.valid_sources.is_empty() {
                lines.push(format!(
                    "    sources: {} validated",
                    entry.valid_sources.len()
                ));
            }
            if let Some(ref src) = entry.selected_source {
                lines.push(format!("    selected source: {} {}", src.scheme, src.uri));
            }
            if !entry.fallback_sources.is_empty() {
                lines.push(format!(
                    "    fallback sources: {}",
                    entry.fallback_sources.len()
                ));
            }
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Login {
        name: String,
        protocol_version: u32,
        capabilities: LoginCapabilities,
    },
    /// Heartbeat ping from client to server. Server should reply with Pong(sequence).
    Ping {
        sequence: u64,
    },
    /// Reply to a server-initiated ServerPing. Client echoes the sequence back.
    /// This is the authoritative liveness path: the server owns the timer and
    /// can detect missed replies independently of the client.
    ServerPong {
        sequence: u64,
    },
    Chat {
        message: String,
    },
    ResourceAvailabilityReport(ResourceAvailabilityReport),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginCapabilities {
    pub required: Vec<ProtocolCapability>,
    pub optional: Vec<ProtocolCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<Vec<String>>,
}

impl LoginCapabilities {
    pub fn normalize(&mut self) {
        self.required.sort();
        self.required.dedup();
        self.optional.sort();
        self.optional.dedup();

        if let Some(flags) = self.feature_flags.as_mut() {
            flags.sort();
            flags.dedup();
            if flags.is_empty() {
                self.feature_flags = None;
            }
        }
    }
}

pub fn current_login_capabilities() -> LoginCapabilities {
    LoginCapabilities {
        required: vec![
            ProtocolCapability::ResourceAnnouncement,
            ProtocolCapability::ResourceAvailabilityReport,
        ],
        optional: vec![
            ProtocolCapability::JoinGateDryRun,
            ProtocolCapability::ResourceCompatibilityReport,
            ProtocolCapability::SignatureMetadata,
        ],
        feature_flags: None,
    }
}

pub fn all_login_capabilities(capabilities: &LoginCapabilities) -> Vec<ProtocolCapability> {
    let mut merged = capabilities.required.clone();
    merged.extend(capabilities.optional.iter().cloned());
    merged.sort();
    merged.dedup();
    merged
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityNegotiationPolicy {
    pub required: Vec<ProtocolCapability>,
    pub optional: Vec<ProtocolCapability>,
}

impl CapabilityNegotiationPolicy {
    pub fn normalize(&mut self) {
        self.required.sort();
        self.required.dedup();
        self.optional.sort();
        self.optional.dedup();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityNegotiationDecision {
    Accepted,
    AcceptedWithWarnings,
    WouldReject,
}

impl CapabilityNegotiationDecision {
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::AcceptedWithWarnings => "accepted_with_warnings",
            Self::WouldReject => "would_reject",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityNegotiationWarning {
    MissingOptionalCapability { capability: ProtocolCapability },
    UnknownFeatureFlag { feature_flag: String },
    UnsupportedOptionalCapability { capability: ProtocolCapability },
}

impl CapabilityNegotiationWarning {
    pub fn to_text(&self) -> String {
        match self {
            Self::MissingOptionalCapability { capability } => {
                format!("missing optional capability: {}", capability.to_name())
            }
            Self::UnsupportedOptionalCapability { capability } => {
                format!(
                    "client advertised unsupported optional capability: {}",
                    capability.to_name()
                )
            }
            Self::UnknownFeatureFlag { feature_flag } => {
                format!("unknown feature flag: {feature_flag}")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityNegotiationViolation {
    MissingRequiredCapability { capability: ProtocolCapability },
}

impl CapabilityNegotiationViolation {
    pub fn to_text(&self) -> String {
        match self {
            Self::MissingRequiredCapability { capability } => {
                format!("missing required capability: {}", capability.to_name())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityNegotiationReport {
    pub decision: CapabilityNegotiationDecision,
    pub required_supported: Vec<ProtocolCapability>,
    pub required_missing: Vec<ProtocolCapability>,
    pub optional_supported: Vec<ProtocolCapability>,
    pub optional_missing: Vec<ProtocolCapability>,
    pub unsupported_client_optional: Vec<ProtocolCapability>,
    pub unknown_feature_flags: Vec<String>,
    pub warnings: Vec<CapabilityNegotiationWarning>,
    pub violations: Vec<CapabilityNegotiationViolation>,
}

impl CapabilityNegotiationReport {
    pub fn to_text(&self) -> String {
        let fmt_caps = |caps: &[ProtocolCapability]| {
            if caps.is_empty() {
                "<none>".to_string()
            } else {
                caps.iter()
                    .map(ProtocolCapability::to_name)
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        let fmt_flags = |flags: &[String]| {
            if flags.is_empty() {
                "<none>".to_string()
            } else {
                flags.join(",")
            }
        };
        let fmt_warn = |warnings: &[CapabilityNegotiationWarning]| {
            if warnings.is_empty() {
                "<none>".to_string()
            } else {
                warnings
                    .iter()
                    .map(CapabilityNegotiationWarning::to_text)
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
        };
        let fmt_viol = |violations: &[CapabilityNegotiationViolation]| {
            if violations.is_empty() {
                "<none>".to_string()
            } else {
                violations
                    .iter()
                    .map(CapabilityNegotiationViolation::to_text)
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
        };

        format!(
            "capability_negotiation_decision: {}\nrequired_supported: {}\nrequired_missing: {}\noptional_supported: {}\noptional_missing: {}\nunsupported_client_optional: {}\nunknown_feature_flags: {}\nwarnings: {}\nviolations: {}",
            self.decision.to_text(),
            fmt_caps(&self.required_supported),
            fmt_caps(&self.required_missing),
            fmt_caps(&self.optional_supported),
            fmt_caps(&self.optional_missing),
            fmt_caps(&self.unsupported_client_optional),
            fmt_flags(&self.unknown_feature_flags),
            fmt_warn(&self.warnings),
            fmt_viol(&self.violations),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectReason {
    ProtocolMismatch,
    InvalidHandshake,
    ClientClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        client_id: Uuid,
        motd: String,
        protocol_version: u32,
    },
    /// Heartbeat pong from server to client, echoing the sequence from Ping.
    /// This is the client-diagnostic direction: client initiates, server echoes.
    Pong {
        sequence: u64,
    },
    /// Server-initiated heartbeat ping. Client must reply with ServerPong(sequence).
    /// This is the authoritative liveness direction: the server owns the timer and
    /// can measure missed replies without relying on client-reported data.
    /// Inert stub — no live timer or enforcement in current milestone.
    ServerPing {
        sequence: u64,
    },
    ChatBroadcast {
        from: String,
        message: String,
    },
    EntitySnapshot {
        entities: Vec<EntityState>,
    },
    ResourceAnnouncement(ResourceAnnouncement),
    JoinGateDecision(JoinGateDecision),
    Disconnect {
        reason: DisconnectReason,
        message: String,
    },
    Error {
        message: String,
    },
}

pub fn encode_line<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

pub fn decode_client_line(line: &str) -> Result<ClientMessage, serde_json::Error> {
    let mut message: ClientMessage = serde_json::from_str(line)?;
    if let ClientMessage::Login { capabilities, .. } = &mut message {
        capabilities.normalize();
    }
    Ok(message)
}

pub fn decode_server_line(line: &str) -> Result<ServerMessage, serde_json::Error> {
    serde_json::from_str(line)
}

pub fn evaluate_resource_policy(
    announcement: &ResourceAnnouncement,
    report: &ResourceAvailabilityReport,
) -> ResourcePolicyEvaluation {
    let report_map = report
        .resources
        .iter()
        .map(|entry| {
            (
                (entry.resource_name.as_str(), entry.file_path.as_str()),
                &entry.status,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut evaluation = ResourcePolicyEvaluation {
        decision: ResourceJoinDecision::Allowed,
        missing_required: Vec::new(),
        invalid_required: Vec::new(),
        missing_optional: Vec::new(),
        invalid_optional: Vec::new(),
        missing_recommended: Vec::new(),
        invalid_recommended: Vec::new(),
    };

    for resource in &announcement.resources {
        for file in &resource.files {
            let file_id = format!("{}:{}", resource.name, file.relative_path);
            let status = report_map
                .get(&(resource.name.as_str(), file.relative_path.as_str()))
                .copied()
                .unwrap_or(&ResourceAvailabilityStatus::Missing);

            match (resource.requirement_level.clone(), status) {
                (ResourceRequirementLevel::Required, ResourceAvailabilityStatus::Available) => {}
                (ResourceRequirementLevel::Required, ResourceAvailabilityStatus::Missing) => {
                    evaluation.missing_required.push(file_id)
                }
                (ResourceRequirementLevel::Required, _) => {
                    evaluation.invalid_required.push(file_id)
                }
                (ResourceRequirementLevel::Optional, ResourceAvailabilityStatus::Available) => {}
                (ResourceRequirementLevel::Optional, ResourceAvailabilityStatus::Missing) => {
                    evaluation.missing_optional.push(file_id)
                }
                (ResourceRequirementLevel::Optional, _) => {
                    evaluation.invalid_optional.push(file_id)
                }
                (ResourceRequirementLevel::Recommended, ResourceAvailabilityStatus::Available) => {}
                (ResourceRequirementLevel::Recommended, ResourceAvailabilityStatus::Missing) => {
                    evaluation.missing_recommended.push(file_id)
                }
                (ResourceRequirementLevel::Recommended, _) => {
                    evaluation.invalid_recommended.push(file_id)
                }
            }
        }
    }

    evaluation.decision =
        if !evaluation.missing_required.is_empty() || !evaluation.invalid_required.is_empty() {
            ResourceJoinDecision::Blocked
        } else if !evaluation.missing_optional.is_empty()
            || !evaluation.invalid_optional.is_empty()
            || !evaluation.missing_recommended.is_empty()
            || !evaluation.invalid_recommended.is_empty()
        {
            ResourceJoinDecision::WarningOnly
        } else {
            ResourceJoinDecision::Allowed
        };

    evaluation
}

pub fn build_join_gate_decision(policy_evaluation: ResourcePolicyEvaluation) -> JoinGateDecision {
    let (outcome, reason) = match policy_evaluation.decision {
        ResourceJoinDecision::Allowed => (
            JoinGateOutcome::WouldAllow,
            "all required resources available".to_string(),
        ),
        ResourceJoinDecision::WarningOnly => (
            JoinGateOutcome::WouldWarn,
            "optional or recommended resources are missing or invalid".to_string(),
        ),
        ResourceJoinDecision::Blocked => (
            JoinGateOutcome::WouldBlock,
            "required resources are missing or invalid".to_string(),
        ),
    };

    JoinGateDecision {
        mode: JoinGateMode::DryRun,
        outcome,
        reason,
        policy_evaluation,
    }
}

pub fn check_announcement_signature_stub(
    announcement: &ResourceAnnouncement,
) -> SignatureVerificationReport {
    match &announcement.signature {
        None => SignatureVerificationReport {
            status: SignatureVerificationStatus::NotProvided,
            reason: "resource announcement signature not provided".to_string(),
        },
        Some(signature) => match validate_signature_metadata(signature) {
            Err(err) => {
                let status = match &err {
                    SignatureMetadataError::UnsupportedAlgorithm(_) => {
                        SignatureVerificationStatus::UnsupportedAlgorithm
                    }
                    _ => SignatureVerificationStatus::Invalid,
                };
                SignatureVerificationReport {
                    status,
                    reason: err.to_string(),
                }
            }
            Ok(()) => SignatureVerificationReport {
                status: SignatureVerificationStatus::NotChecked,
                reason: format!(
                    "signature metadata is valid: algorithm '{}', key '{}'; cryptographic verification not enforced in this milestone",
                    signature.algorithm, signature.key_id
                ),
            },
        },
    }
}

pub fn build_resource_download_preflight_plan(
    announcement: &ResourceAnnouncement,
    availability: &ResourceAvailabilityReport,
    signature: &SignatureVerificationReport,
    signature_policy: &signature_engine::SignaturePolicy,
    policy_evaluation: Option<&ResourcePolicyEvaluation>,
) -> ResourceDownloadPreflightPlan {
    let mut entries = Vec::new();

    let blocked_by_signature =
        matches!(signature_policy, signature_engine::SignaturePolicy::Strict)
            && !matches!(signature.status, SignatureVerificationStatus::Valid);
    let blocked_by_resource_policy = policy_evaluation
        .map(|evaluation| evaluation.decision == ResourceJoinDecision::Blocked)
        .unwrap_or(false);

    let availability_map = availability
        .resources
        .iter()
        .map(|entry| {
            (
                (entry.resource_name.as_str(), entry.file_path.as_str()),
                entry.status.clone(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    for resource in &announcement.resources {
        let unsupported = resource.protocol_version != PROTOCOL_VERSION;
        for file in &resource.files {
            let key = (resource.name.as_str(), file.relative_path.as_str());
            let status = availability_map.get(&key);

            // Evaluate source metadata (report-only, never fetches)
            let (source_errors, valid_sources) = match validate_and_order_sources(file) {
                Ok(sources) => (Vec::new(), sources),
                Err(e) => (vec![e.to_string()], Vec::new()),
            };
            let (selected_source, fallback_sources) = select_fetch_source(&valid_sources);

            let (action, reason) = if unsupported {
                (
                    ResourceDownloadPreflightAction::UnsupportedResource,
                    format!(
                        "resource protocol_version {} does not match client {}",
                        resource.protocol_version, PROTOCOL_VERSION
                    ),
                )
            } else if blocked_by_signature {
                (
                    ResourceDownloadPreflightAction::BlockedBySignaturePolicy,
                    signature.reason.clone(),
                )
            } else if blocked_by_resource_policy {
                (
                    ResourceDownloadPreflightAction::BlockedByResourcePolicy,
                    policy_evaluation
                        .map(|evaluation| {
                            format!("resource policy decision: {:?}", evaluation.decision)
                        })
                        .unwrap_or_else(|| "resource policy blocked".to_string()),
                )
            } else {
                match status {
                    Some(ResourceAvailabilityStatus::Available) => (
                        ResourceDownloadPreflightAction::AlreadyAvailable,
                        "resource file already available locally".to_string(),
                    ),
                    Some(ResourceAvailabilityStatus::Missing) | None => (
                        ResourceDownloadPreflightAction::FetchMissing,
                        "resource file missing from local cache".to_string(),
                    ),
                    Some(ResourceAvailabilityStatus::SizeMismatch)
                    | Some(ResourceAvailabilityStatus::HashMismatch) => (
                        ResourceDownloadPreflightAction::ReplaceInvalid,
                        "resource file present but invalid locally".to_string(),
                    ),
                }
            };

            entries.push(ResourceDownloadPreflightEntry {
                resource_name: resource.name.clone(),
                file_path: file.relative_path.clone(),
                action,
                reason,
                source_errors: source_errors.clone(),
                valid_sources: valid_sources.clone(),
                selected_source: selected_source.clone(),
                fallback_sources: fallback_sources.clone(),
            });

            if !blocked_by_signature
                && !blocked_by_resource_policy
                && !unsupported
                && matches!(status, Some(ResourceAvailabilityStatus::Missing))
            {
                entries.push(ResourceDownloadPreflightEntry {
                    resource_name: resource.name.clone(),
                    file_path: file.relative_path.clone(),
                    action: ResourceDownloadPreflightAction::WouldVerifyAfterFetch,
                    reason: "downloaded file would require post-fetch verification".to_string(),
                    source_errors: source_errors.clone(),
                    valid_sources: valid_sources.clone(),
                    selected_source: selected_source.clone(),
                    fallback_sources: fallback_sources.clone(),
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        a.resource_name
            .cmp(&b.resource_name)
            .then(a.file_path.cmp(&b.file_path))
            .then(format!("{:?}", a.action).cmp(&format!("{:?}", b.action)))
    });

    ResourceDownloadPreflightPlan { entries }
}

// ---------------------------------------------------------------------------
// Protocol Compatibility Negotiation (dry-run only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolVersionRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolCapability {
    ResourceAnnouncement,
    ResourceAvailabilityReport,
    JoinGateDryRun,
    ResourceCompatibilityReport,
    SignatureMetadata,
}

impl ProtocolCapability {
    pub fn to_name(&self) -> &'static str {
        match self {
            Self::ResourceAnnouncement => "resource_announcement",
            Self::ResourceAvailabilityReport => "resource_availability_report",
            Self::JoinGateDryRun => "join_gate_dry_run",
            Self::ResourceCompatibilityReport => "resource_compatibility_report",
            Self::SignatureMetadata => "signature_metadata",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolCompatibilityProfile {
    pub version_range: ProtocolVersionRange,
    pub capabilities: Vec<ProtocolCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolNegotiationStatus {
    ExactMatch,
    CompatibleDryRun,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolNegotiationResult {
    pub status: ProtocolNegotiationStatus,
    pub selected_version: Option<u32>,
    pub shared_capabilities: Vec<ProtocolCapability>,
    pub reason: String,
}

pub fn current_protocol_profile() -> ProtocolCompatibilityProfile {
    ProtocolCompatibilityProfile {
        version_range: ProtocolVersionRange {
            min: PROTOCOL_VERSION,
            max: PROTOCOL_VERSION,
        },
        capabilities: all_login_capabilities(&current_login_capabilities()),
    }
}

pub fn current_capability_negotiation_policy() -> CapabilityNegotiationPolicy {
    let mut policy = CapabilityNegotiationPolicy {
        required: current_login_capabilities().required,
        optional: current_login_capabilities().optional,
    };
    policy.normalize();
    policy
}

pub fn evaluate_capability_negotiation(
    advertised: &LoginCapabilities,
    policy: &CapabilityNegotiationPolicy,
) -> CapabilityNegotiationReport {
    let mut normalized_advertised = advertised.clone();
    normalized_advertised.normalize();
    let mut normalized_policy = policy.clone();
    normalized_policy.normalize();

    let advertised_all = all_login_capabilities(&normalized_advertised);

    let mut required_supported = Vec::new();
    let mut required_missing = Vec::new();
    for capability in &normalized_policy.required {
        if advertised_all.contains(capability) {
            required_supported.push(capability.clone());
        } else {
            required_missing.push(capability.clone());
        }
    }

    let mut optional_supported = Vec::new();
    let mut optional_missing = Vec::new();
    for capability in &normalized_policy.optional {
        if advertised_all.contains(capability) {
            optional_supported.push(capability.clone());
        } else {
            optional_missing.push(capability.clone());
        }
    }

    let mut unsupported_client_optional: Vec<ProtocolCapability> = normalized_advertised
        .optional
        .iter()
        .filter(|capability| {
            !normalized_policy.required.contains(capability)
                && !normalized_policy.optional.contains(capability)
        })
        .cloned()
        .collect();
    unsupported_client_optional.sort();
    unsupported_client_optional.dedup();

    let unknown_feature_flags = normalized_advertised
        .feature_flags
        .clone()
        .unwrap_or_default();

    let mut warnings = Vec::new();
    for capability in &optional_missing {
        warnings.push(CapabilityNegotiationWarning::MissingOptionalCapability {
            capability: capability.clone(),
        });
    }
    for capability in &unsupported_client_optional {
        warnings.push(
            CapabilityNegotiationWarning::UnsupportedOptionalCapability {
                capability: capability.clone(),
            },
        );
    }
    for feature_flag in &unknown_feature_flags {
        warnings.push(CapabilityNegotiationWarning::UnknownFeatureFlag {
            feature_flag: feature_flag.clone(),
        });
    }
    warnings.sort();

    let mut violations = Vec::new();
    for capability in &required_missing {
        violations.push(CapabilityNegotiationViolation::MissingRequiredCapability {
            capability: capability.clone(),
        });
    }
    violations.sort();

    let decision = if !violations.is_empty() {
        CapabilityNegotiationDecision::WouldReject
    } else if !warnings.is_empty() {
        CapabilityNegotiationDecision::AcceptedWithWarnings
    } else {
        CapabilityNegotiationDecision::Accepted
    };

    CapabilityNegotiationReport {
        decision,
        required_supported,
        required_missing,
        optional_supported,
        optional_missing,
        unsupported_client_optional,
        unknown_feature_flags,
        warnings,
        violations,
    }
}

pub fn protocol_ranges_overlap(a: &ProtocolVersionRange, b: &ProtocolVersionRange) -> bool {
    a.min <= b.max && b.min <= a.max
}

pub fn negotiate_protocol_dry_run(
    client: &ProtocolCompatibilityProfile,
    server: &ProtocolCompatibilityProfile,
) -> ProtocolNegotiationResult {
    let overlap = protocol_ranges_overlap(&client.version_range, &server.version_range);

    if !overlap {
        return ProtocolNegotiationResult {
            status: ProtocolNegotiationStatus::Incompatible,
            selected_version: None,
            shared_capabilities: Vec::new(),
            reason: "no overlapping protocol version range".to_string(),
        };
    }

    let contains_current = client.version_range.min <= PROTOCOL_VERSION
        && client.version_range.max >= PROTOCOL_VERSION
        && server.version_range.min <= PROTOCOL_VERSION
        && server.version_range.max >= PROTOCOL_VERSION;

    if contains_current {
        let mut shared: Vec<ProtocolCapability> = client
            .capabilities
            .iter()
            .filter(|c| server.capabilities.contains(c))
            .cloned()
            .collect();
        shared.sort();
        shared.dedup();

        return ProtocolNegotiationResult {
            status: ProtocolNegotiationStatus::ExactMatch,
            selected_version: Some(PROTOCOL_VERSION),
            shared_capabilities: shared,
            reason: "both profiles include the current protocol version".to_string(),
        };
    }

    let highest_shared = std::cmp::max(client.version_range.min, server.version_range.min)
        ..=std::cmp::min(client.version_range.max, server.version_range.max);
    let selected_version = highest_shared
        .into_iter()
        .max()
        .expect("overlap guarantees at least one version");

    let mut shared: Vec<ProtocolCapability> = client
        .capabilities
        .iter()
        .filter(|c| server.capabilities.contains(c))
        .cloned()
        .collect();
    shared.sort();
    shared.dedup();

    ProtocolNegotiationResult {
        status: ProtocolNegotiationStatus::CompatibleDryRun,
        selected_version: Some(selected_version),
        shared_capabilities: shared,
        reason: format!(
            "compatible dry-run only: highest shared version is {}, not active",
            selected_version
        ),
    }
}

// ---------------------------------------------------------------------------
// Protocol Capability Gating (dry-run, report-only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolCapabilityError {
    MissingCapability { capability: ProtocolCapability },
}

impl std::fmt::Display for ProtocolCapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolCapabilityError::MissingCapability { capability } => {
                write!(f, "missing required capability: {capability:?}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityGateReport {
    pub capability: ProtocolCapability,
    pub supported: bool,
    pub reason: String,
}

pub fn profile_supports_capability(
    profile: &ProtocolCompatibilityProfile,
    capability: ProtocolCapability,
) -> bool {
    profile.capabilities.contains(&capability)
}

pub fn shared_capabilities(
    client: &ProtocolCompatibilityProfile,
    server: &ProtocolCompatibilityProfile,
) -> Vec<ProtocolCapability> {
    let mut shared: Vec<ProtocolCapability> = client
        .capabilities
        .iter()
        .filter(|c| server.capabilities.contains(c))
        .cloned()
        .collect();
    shared.sort();
    shared.dedup();
    shared
}

pub fn requires_capability(
    capability: ProtocolCapability,
    shared: &[ProtocolCapability],
) -> Result<(), ProtocolCapabilityError> {
    if shared.contains(&capability) {
        Ok(())
    } else {
        Err(ProtocolCapabilityError::MissingCapability { capability })
    }
}

pub fn capability_gate_report(
    capability: ProtocolCapability,
    shared: &[ProtocolCapability],
) -> CapabilityGateReport {
    let supported = shared.contains(&capability);
    let reason = if supported {
        format!("{capability:?} present in shared capability set")
    } else {
        format!("{capability:?} not in shared capability set; not enforced in this milestone")
    };
    CapabilityGateReport {
        capability,
        supported,
        reason,
    }
}

// ---------------------------------------------------------------------------
// Signature Verification Dry-Run Planner (M3.7 — no crypto, report-only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    pub key_id: String,
    pub algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureVerificationAction {
    VerifySignature,
    MissingSignature,
    UnsupportedAlgorithm,
    UnknownKeyId,
    MalformedSignature,
    WouldRejectUnsigned,
    ResourceDigestMismatchPrecheck,
}

impl fmt::Display for SignatureVerificationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VerifySignature => write!(f, "verify_signature"),
            Self::MissingSignature => write!(f, "missing_signature"),
            Self::UnsupportedAlgorithm => write!(f, "unsupported_algorithm"),
            Self::UnknownKeyId => write!(f, "unknown_key_id"),
            Self::MalformedSignature => write!(f, "malformed_signature"),
            Self::WouldRejectUnsigned => write!(f, "would_reject_unsigned"),
            Self::ResourceDigestMismatchPrecheck => {
                write!(f, "resource_digest_mismatch_precheck")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureVerificationPlanEntry {
    pub resource_name: String,
    pub action: SignatureVerificationAction,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureVerificationPlan {
    pub entries: Vec<SignatureVerificationPlanEntry>,
}

impl SignatureVerificationPlan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "signature verification plan: (empty, no resources)\n".to_string();
        }
        let mut lines = "signature verification plan:\n".to_string();
        for entry in &self.entries {
            lines.push_str(&format!(
                "  [{}] {} — {}\n",
                entry.action, entry.resource_name, entry.reason
            ));
        }
        lines.push_str(&format!("  total: {} resource(s)\n", self.entries.len()));
        lines
    }
}

pub fn build_signature_verification_plan(
    announcement: &ResourceAnnouncement,
    trusted_keys: &[TrustedKey],
    reject_unsigned: bool,
) -> SignatureVerificationPlan {
    let mut entries: Vec<SignatureVerificationPlanEntry> = announcement
        .resources
        .iter()
        .map(|resource| {
            let (action, reason) = match &announcement.signature {
                None => {
                    if reject_unsigned {
                        (
                            SignatureVerificationAction::WouldRejectUnsigned,
                            "announcement has no signature; policy would reject unsigned"
                                .to_string(),
                        )
                    } else {
                        (
                            SignatureVerificationAction::MissingSignature,
                            "announcement has no signature; would skip verification".to_string(),
                        )
                    }
                }
                Some(sig) => match validate_signature_metadata(sig) {
                    Err(err) => match &err {
                        SignatureMetadataError::UnsupportedAlgorithm(_) => (
                            SignatureVerificationAction::UnsupportedAlgorithm,
                            format!("signature metadata error: {err}"),
                        ),
                        _ => (
                            SignatureVerificationAction::MalformedSignature,
                            format!("signature metadata error: {err}"),
                        ),
                    },
                    Ok(()) => {
                        let key_trusted = trusted_keys
                            .iter()
                            .any(|k| k.key_id == sig.key_id && k.algorithm == sig.algorithm);
                        if key_trusted {
                            (
                                SignatureVerificationAction::VerifySignature,
                                format!(
                                    "algorithm '{}', key '{}': trusted, would verify",
                                    sig.algorithm, sig.key_id
                                ),
                            )
                        } else {
                            (
                                SignatureVerificationAction::UnknownKeyId,
                                format!(
                                    "algorithm '{}', key '{}': not in trusted key set",
                                    sig.algorithm, sig.key_id
                                ),
                            )
                        }
                    }
                },
            };
            SignatureVerificationPlanEntry {
                resource_name: resource.name.clone(),
                action,
                reason,
            }
        })
        .collect();

    entries.sort_by(|a, b| a.resource_name.cmp(&b.resource_name));
    SignatureVerificationPlan { entries }
}

pub mod signature_engine;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize_resource_announcement() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: None,
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };

        let line = encode_line(&ServerMessage::ResourceAnnouncement(announcement.clone())).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        match decoded {
            ServerMessage::ResourceAnnouncement(actual) => assert_eq!(actual, announcement),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn serialize_deserialize_resource_availability_report() {
        let report = ResourceAvailabilityReport {
            resources: vec![ResourceAvailabilityEntry {
                resource_name: "chat".to_string(),
                file_path: "resource.toml".to_string(),
                status: ResourceAvailabilityStatus::Available,
            }],
            is_fully_available: true,
        };

        let line = encode_line(&ClientMessage::ResourceAvailabilityReport(report.clone())).unwrap();
        let decoded = decode_client_line(line.trim()).unwrap();
        match decoded {
            ClientMessage::ResourceAvailabilityReport(actual) => assert_eq!(actual, report),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn protocol_messages_include_resource_types() {
        let server = ServerMessage::ResourceAnnouncement(ResourceAnnouncement {
            resources: vec![],
            signature: None,
        });
        let client = ClientMessage::ResourceAvailabilityReport(ResourceAvailabilityReport {
            resources: vec![],
            is_fully_available: true,
        });

        assert!(matches!(server, ServerMessage::ResourceAnnouncement(_)));
        assert!(matches!(
            client,
            ClientMessage::ResourceAvailabilityReport(_)
        ));
    }

    #[test]
    fn allowed_policy_maps_to_would_allow() {
        let decision = build_join_gate_decision(ResourcePolicyEvaluation {
            decision: ResourceJoinDecision::Allowed,
            missing_required: vec![],
            invalid_required: vec![],
            missing_optional: vec![],
            invalid_optional: vec![],
            missing_recommended: vec![],
            invalid_recommended: vec![],
        });
        assert_eq!(decision.outcome, JoinGateOutcome::WouldAllow);
        assert_eq!(decision.mode, JoinGateMode::DryRun);
    }

    #[test]
    fn warning_only_policy_maps_to_would_warn() {
        let decision = build_join_gate_decision(ResourcePolicyEvaluation {
            decision: ResourceJoinDecision::WarningOnly,
            missing_required: vec![],
            invalid_required: vec![],
            missing_optional: vec!["chat:resource.toml".to_string()],
            invalid_optional: vec![],
            missing_recommended: vec![],
            invalid_recommended: vec![],
        });
        assert_eq!(decision.outcome, JoinGateOutcome::WouldWarn);
    }

    #[test]
    fn blocked_policy_maps_to_would_block() {
        let decision = build_join_gate_decision(ResourcePolicyEvaluation {
            decision: ResourceJoinDecision::Blocked,
            missing_required: vec!["chat:resource.toml".to_string()],
            invalid_required: vec![],
            missing_optional: vec![],
            invalid_optional: vec![],
            missing_recommended: vec![],
            invalid_recommended: vec![],
        });
        assert_eq!(decision.outcome, JoinGateOutcome::WouldBlock);
    }

    #[test]
    fn join_gate_decision_serializes_deserializes() {
        let decision = build_join_gate_decision(ResourcePolicyEvaluation {
            decision: ResourceJoinDecision::Allowed,
            missing_required: vec![],
            invalid_required: vec![],
            missing_optional: vec![],
            invalid_optional: vec![],
            missing_recommended: vec![],
            invalid_recommended: vec![],
        });

        let line = encode_line(&ServerMessage::JoinGateDecision(decision.clone())).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        match decoded {
            ServerMessage::JoinGateDecision(actual) => assert_eq!(actual, decision),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn all_resources_available_is_allowed() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = sample_report(ResourceAvailabilityStatus::Available);
        let evaluation = evaluate_resource_policy(&announcement, &report);
        assert_eq!(evaluation.decision, ResourceJoinDecision::Allowed);
    }

    #[test]
    fn resource_announcement_serializes_without_signature() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let line = encode_line(&ServerMessage::ResourceAnnouncement(announcement.clone())).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        match decoded {
            ServerMessage::ResourceAnnouncement(actual) => assert_eq!(actual, announcement),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn resource_announcement_serializes_with_signature() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "deadbeef".to_string(),
        });
        let line = encode_line(&ServerMessage::ResourceAnnouncement(announcement.clone())).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        match decoded {
            ServerMessage::ResourceAnnouncement(actual) => assert_eq!(actual, announcement),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn missing_signature_returns_not_provided() {
        let report = check_announcement_signature_stub(&sample_announcement(
            ResourceRequirementLevel::Required,
        ));
        assert_eq!(report.status, SignatureVerificationStatus::NotProvided);
    }

    #[test]
    fn present_signature_returns_not_checked() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "deadbeef".to_string(),
        });
        let report = check_announcement_signature_stub(&announcement);
        assert_eq!(report.status, SignatureVerificationStatus::NotChecked);
    }

    #[test]
    fn missing_required_is_blocked() {
        let evaluation = evaluate_resource_policy(
            &sample_announcement(ResourceRequirementLevel::Required),
            &sample_report(ResourceAvailabilityStatus::Missing),
        );
        assert_eq!(evaluation.decision, ResourceJoinDecision::Blocked);
        assert_eq!(evaluation.missing_required, vec!["chat:resource.toml"]);
    }

    #[test]
    fn hash_mismatch_required_is_blocked() {
        let evaluation = evaluate_resource_policy(
            &sample_announcement(ResourceRequirementLevel::Required),
            &sample_report(ResourceAvailabilityStatus::HashMismatch),
        );
        assert_eq!(evaluation.decision, ResourceJoinDecision::Blocked);
        assert_eq!(evaluation.invalid_required, vec!["chat:resource.toml"]);
    }

    #[test]
    fn size_mismatch_required_is_blocked() {
        let evaluation = evaluate_resource_policy(
            &sample_announcement(ResourceRequirementLevel::Required),
            &sample_report(ResourceAvailabilityStatus::SizeMismatch),
        );
        assert_eq!(evaluation.decision, ResourceJoinDecision::Blocked);
        assert_eq!(evaluation.invalid_required, vec!["chat:resource.toml"]);
    }

    #[test]
    fn missing_optional_only_is_warning_only() {
        let evaluation = evaluate_resource_policy(
            &sample_announcement(ResourceRequirementLevel::Optional),
            &sample_report(ResourceAvailabilityStatus::Missing),
        );
        assert_eq!(evaluation.decision, ResourceJoinDecision::WarningOnly);
        assert_eq!(evaluation.missing_optional, vec!["chat:resource.toml"]);
    }

    #[test]
    fn missing_recommended_only_is_warning_only() {
        let evaluation = evaluate_resource_policy(
            &sample_announcement(ResourceRequirementLevel::Recommended),
            &sample_report(ResourceAvailabilityStatus::Missing),
        );
        assert_eq!(evaluation.decision, ResourceJoinDecision::WarningOnly);
        assert_eq!(evaluation.missing_recommended, vec!["chat:resource.toml"]);
    }

    #[test]
    fn missing_report_entry_counts_as_missing() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = ResourceAvailabilityReport {
            resources: vec![],
            is_fully_available: false,
        };
        let evaluation = evaluate_resource_policy(&announcement, &report);
        assert_eq!(evaluation.missing_required, vec!["chat:resource.toml"]);
    }

    #[test]
    fn extra_report_entry_ignored() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = ResourceAvailabilityReport {
            resources: vec![
                ResourceAvailabilityEntry {
                    resource_name: "chat".to_string(),
                    file_path: "resource.toml".to_string(),
                    status: ResourceAvailabilityStatus::Available,
                },
                ResourceAvailabilityEntry {
                    resource_name: "extra".to_string(),
                    file_path: "ignored.txt".to_string(),
                    status: ResourceAvailabilityStatus::Missing,
                },
            ],
            is_fully_available: false,
        };
        let evaluation = evaluate_resource_policy(&announcement, &report);
        assert_eq!(evaluation.decision, ResourceJoinDecision::Allowed);
    }

    #[test]
    fn deterministic_evaluation_ordering() {
        let announcement = ResourceAnnouncement {
            resources: vec![
                AnnouncedResource {
                    name: "alpha".to_string(),
                    version: "0.1.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "a.txt".to_string(),
                        size_bytes: 1,
                        sha256: "a".to_string(),
                        sources: None,
                    }],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
                AnnouncedResource {
                    name: "beta".to_string(),
                    version: "0.1.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "b.txt".to_string(),
                        size_bytes: 1,
                        sha256: "b".to_string(),
                        sources: None,
                    }],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
            ],
            signature: None,
        };
        let report = ResourceAvailabilityReport {
            resources: vec![],
            is_fully_available: false,
        };
        let evaluation = evaluate_resource_policy(&announcement, &report);
        assert_eq!(
            evaluation.missing_required,
            vec!["alpha:a.txt", "beta:b.txt"]
        );
    }

    // -----------------------------------------------------------------------
    // Protocol negotiation tests
    // -----------------------------------------------------------------------

    fn current_profile() -> super::ProtocolCompatibilityProfile {
        ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange {
                min: PROTOCOL_VERSION,
                max: PROTOCOL_VERSION,
            },
            capabilities: all_login_capabilities(&current_login_capabilities()),
        }
    }

    fn sample_login_capabilities() -> LoginCapabilities {
        LoginCapabilities {
            required: vec![
                ProtocolCapability::ResourceAvailabilityReport,
                ProtocolCapability::ResourceAnnouncement,
            ],
            optional: vec![
                ProtocolCapability::SignatureMetadata,
                ProtocolCapability::JoinGateDryRun,
                ProtocolCapability::JoinGateDryRun,
            ],
            feature_flags: Some(vec![
                "z_flag".to_string(),
                "a_flag".to_string(),
                "a_flag".to_string(),
            ]),
        }
    }

    fn sample_login() -> ClientMessage {
        let mut capabilities = sample_login_capabilities();
        capabilities.normalize();
        ClientMessage::Login {
            name: "alice".to_string(),
            protocol_version: PROTOCOL_VERSION,
            capabilities,
        }
    }

    #[test]
    fn negotiate_exact_current_protocol_match() {
        let client = current_profile();
        let server = current_profile();
        let result = negotiate_protocol_dry_run(&client, &server);
        assert_eq!(result.status, ProtocolNegotiationStatus::ExactMatch);
        assert_eq!(result.selected_version, Some(PROTOCOL_VERSION));
        assert!(result.reason.contains("current protocol version"));
    }

    #[test]
    fn negotiate_compatible_dry_run_overlap() {
        let client = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 0, max: 1 },
            capabilities: vec![ProtocolCapability::ResourceAnnouncement],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 1, max: 1 },
            capabilities: vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::JoinGateDryRun,
            ],
        };
        let result = negotiate_protocol_dry_run(&client, &server);
        assert_eq!(result.status, ProtocolNegotiationStatus::CompatibleDryRun);
        assert_eq!(result.selected_version, Some(1));
        assert!(result.reason.contains("dry-run"));
    }

    #[test]
    fn negotiate_incompatible_no_overlap() {
        let client = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 5, max: 10 },
            capabilities: vec![],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 1, max: 3 },
            capabilities: vec![],
        };
        let result = negotiate_protocol_dry_run(&client, &server);
        assert_eq!(result.status, ProtocolNegotiationStatus::Incompatible);
        assert_eq!(result.selected_version, None);
    }

    #[test]
    fn negotiate_selects_highest_shared_version() {
        let client = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 1, max: 5 },
            capabilities: vec![],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 3, max: 7 },
            capabilities: vec![],
        };
        let result = negotiate_protocol_dry_run(&client, &server);
        assert_eq!(result.status, ProtocolNegotiationStatus::CompatibleDryRun);
        assert_eq!(result.selected_version, Some(5));
    }

    #[test]
    fn negotiate_capability_intersection_deterministic() {
        let client = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 0, max: 1 },
            capabilities: vec![
                ProtocolCapability::JoinGateDryRun,
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::SignatureMetadata,
            ],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 0, max: 1 },
            capabilities: vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::JoinGateDryRun,
            ],
        };
        let result = negotiate_protocol_dry_run(&client, &server);
        assert_eq!(
            result.shared_capabilities,
            vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::JoinGateDryRun,
            ]
        );
    }

    #[test]
    fn negotiate_no_shared_capabilities() {
        let client = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 0, max: 2 },
            capabilities: vec![ProtocolCapability::JoinGateDryRun],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 0, max: 2 },
            capabilities: vec![ProtocolCapability::ResourceAnnouncement],
        };
        let result = negotiate_protocol_dry_run(&client, &server);
        assert!(result.shared_capabilities.is_empty());
    }

    #[test]
    fn protocol_ranges_overlap_true() {
        assert!(protocol_ranges_overlap(
            &ProtocolVersionRange { min: 1, max: 3 },
            &ProtocolVersionRange { min: 2, max: 4 },
        ));
        assert!(protocol_ranges_overlap(
            &ProtocolVersionRange { min: 2, max: 2 },
            &ProtocolVersionRange { min: 2, max: 2 },
        ));
    }

    #[test]
    fn protocol_ranges_overlap_false() {
        assert!(!protocol_ranges_overlap(
            &ProtocolVersionRange { min: 1, max: 2 },
            &ProtocolVersionRange { min: 3, max: 4 },
        ));
    }

    #[test]
    fn protocol_compatibility_profile_serializes_deserializes() {
        let profile = current_profile();
        let json = serde_json::to_string(&profile).unwrap();
        let decoded: ProtocolCompatibilityProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, profile);
    }

    #[test]
    fn protocol_negotiation_result_serializes_deserializes() {
        let result = negotiate_protocol_dry_run(&current_profile(), &current_profile());
        let json = serde_json::to_string(&result).unwrap();
        let decoded: ProtocolNegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
    }

    #[test]
    fn exact_match_result_has_current_version() {
        let result = negotiate_protocol_dry_run(
            &ProtocolCompatibilityProfile {
                version_range: ProtocolVersionRange {
                    min: PROTOCOL_VERSION,
                    max: PROTOCOL_VERSION,
                },
                capabilities: vec![],
            },
            &ProtocolCompatibilityProfile {
                version_range: ProtocolVersionRange {
                    min: PROTOCOL_VERSION,
                    max: PROTOCOL_VERSION,
                },
                capabilities: vec![],
            },
        );
        assert_eq!(result.status, ProtocolNegotiationStatus::ExactMatch);
        assert_eq!(result.selected_version, Some(PROTOCOL_VERSION));
    }

    #[test]
    fn login_with_capability_payload_roundtrips() {
        let json = encode_line(&sample_login()).unwrap();
        let decoded = decode_client_line(&json).unwrap();
        assert_eq!(decoded, sample_login());
    }

    #[test]
    fn login_required_capabilities_roundtrip() {
        let decoded = decode_client_line(
            r#"{"type":"login","name":"alice","protocol_version":2,"capabilities":{"required":["resource_availability_report","resource_announcement"],"optional":[]}}"#,
        )
        .unwrap();
        match decoded {
            ClientMessage::Login { capabilities, .. } => {
                assert_eq!(
                    capabilities.required,
                    vec![
                        ProtocolCapability::ResourceAnnouncement,
                        ProtocolCapability::ResourceAvailabilityReport,
                    ]
                );
                assert!(capabilities.optional.is_empty());
            }
            other => panic!("expected login, got {other:?}"),
        }
    }

    #[test]
    fn login_optional_capabilities_roundtrip() {
        let decoded = decode_client_line(
            r#"{"type":"login","name":"alice","protocol_version":2,"capabilities":{"required":[],"optional":["signature_metadata","join_gate_dry_run"]}}"#,
        )
        .unwrap();
        match decoded {
            ClientMessage::Login { capabilities, .. } => {
                assert!(capabilities.required.is_empty());
                assert_eq!(
                    capabilities.optional,
                    vec![
                        ProtocolCapability::JoinGateDryRun,
                        ProtocolCapability::SignatureMetadata,
                    ]
                );
            }
            other => panic!("expected login, got {other:?}"),
        }
    }

    #[test]
    fn login_feature_flags_roundtrip() {
        let decoded = decode_client_line(
            r#"{"type":"login","name":"alice","protocol_version":2,"capabilities":{"required":[],"optional":[],"feature_flags":["z_flag","a_flag"]}}"#,
        )
        .unwrap();
        match decoded {
            ClientMessage::Login { capabilities, .. } => {
                assert_eq!(
                    capabilities.feature_flags,
                    Some(vec!["a_flag".to_string(), "z_flag".to_string()])
                );
            }
            other => panic!("expected login, got {other:?}"),
        }
    }

    #[test]
    fn login_unknown_typed_capability_rejected() {
        let err = decode_client_line(
            r#"{"type":"login","name":"alice","protocol_version":2,"capabilities":{"required":["future_capability"],"optional":[]}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("future_capability"));
    }

    #[test]
    fn login_unknown_feature_flags_are_tolerated() {
        let decoded = decode_client_line(
            r#"{"type":"login","name":"alice","protocol_version":2,"capabilities":{"required":[],"optional":[],"feature_flags":["future_experiment"]}}"#,
        )
        .unwrap();
        match decoded {
            ClientMessage::Login { capabilities, .. } => {
                assert_eq!(
                    capabilities.feature_flags,
                    Some(vec!["future_experiment".to_string()])
                );
            }
            other => panic!("expected login, got {other:?}"),
        }
    }

    #[test]
    fn login_capability_payload_normalized_deterministically() {
        let decoded = decode_client_line(
            r#"{"type":"login","name":"alice","protocol_version":2,"capabilities":{"required":["resource_availability_report","resource_announcement","resource_announcement"],"optional":["signature_metadata","join_gate_dry_run","join_gate_dry_run"],"feature_flags":["z_flag","a_flag","a_flag"]}}"#,
        )
        .unwrap();
        assert_eq!(decoded, sample_login());
    }

    #[test]
    fn login_missing_capability_payload_rejected() {
        let err = decode_client_line(r#"{"type":"login","name":"alice","protocol_version":2}"#)
            .unwrap_err();
        assert!(err.to_string().contains("capabilities"));
    }

    #[test]
    fn all_login_capabilities_merges_deterministically() {
        assert_eq!(
            all_login_capabilities(&sample_login_capabilities()),
            vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::ResourceAvailabilityReport,
                ProtocolCapability::JoinGateDryRun,
                ProtocolCapability::SignatureMetadata,
            ]
        );
    }

    #[test]
    fn capability_negotiation_accepts_all_required_capabilities() {
        let report = evaluate_capability_negotiation(
            &current_login_capabilities(),
            &current_capability_negotiation_policy(),
        );
        assert_eq!(report.decision, CapabilityNegotiationDecision::Accepted);
        assert!(report.required_missing.is_empty());
        assert!(report.warnings.is_empty());
        assert!(report.violations.is_empty());
    }

    #[test]
    fn capability_negotiation_missing_required_reports_would_reject() {
        let report = evaluate_capability_negotiation(
            &LoginCapabilities {
                required: vec![ProtocolCapability::ResourceAnnouncement],
                optional: vec![],
                feature_flags: None,
            },
            &current_capability_negotiation_policy(),
        );
        assert_eq!(report.decision, CapabilityNegotiationDecision::WouldReject);
        assert_eq!(
            report.violations,
            vec![CapabilityNegotiationViolation::MissingRequiredCapability {
                capability: ProtocolCapability::ResourceAvailabilityReport,
            }]
        );
    }

    #[test]
    fn capability_negotiation_optional_and_feature_flag_warnings_are_report_only() {
        let report = evaluate_capability_negotiation(
            &LoginCapabilities {
                required: vec![
                    ProtocolCapability::ResourceAnnouncement,
                    ProtocolCapability::ResourceAvailabilityReport,
                ],
                optional: vec![ProtocolCapability::SignatureMetadata],
                feature_flags: Some(vec!["z_flag".to_string(), "a_flag".to_string()]),
            },
            &current_capability_negotiation_policy(),
        );
        assert_eq!(
            report.decision,
            CapabilityNegotiationDecision::AcceptedWithWarnings
        );
        assert_eq!(
            report.optional_missing,
            vec![
                ProtocolCapability::JoinGateDryRun,
                ProtocolCapability::ResourceCompatibilityReport,
            ]
        );
        assert_eq!(
            report.unknown_feature_flags,
            vec!["a_flag".to_string(), "z_flag".to_string()]
        );
    }

    #[test]
    fn capability_negotiation_unsupported_optional_is_warning_only() {
        let report = evaluate_capability_negotiation(
            &LoginCapabilities {
                required: current_login_capabilities().required,
                optional: vec![
                    ProtocolCapability::SignatureMetadata,
                    ProtocolCapability::ResourceAnnouncement,
                ],
                feature_flags: None,
            },
            &CapabilityNegotiationPolicy {
                required: current_login_capabilities().required,
                optional: vec![ProtocolCapability::JoinGateDryRun],
            },
        );
        assert_eq!(
            report.unsupported_client_optional,
            vec![ProtocolCapability::SignatureMetadata]
        );
        assert_eq!(
            report.decision,
            CapabilityNegotiationDecision::AcceptedWithWarnings
        );
    }

    #[test]
    fn capability_negotiation_report_text_is_deterministic() {
        let report = evaluate_capability_negotiation(
            &LoginCapabilities {
                required: vec![ProtocolCapability::ResourceAvailabilityReport],
                optional: vec![ProtocolCapability::SignatureMetadata],
                feature_flags: Some(vec!["z_flag".to_string(), "a_flag".to_string()]),
            },
            &current_capability_negotiation_policy(),
        );
        assert_eq!(report.to_text(), report.to_text());
        assert!(
            report
                .to_text()
                .contains("capability_negotiation_decision: would_reject")
        );
    }

    #[test]
    fn resource_download_preflight_all_available() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = sample_report(ResourceAvailabilityStatus::Available);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(
            plan.entries[0].action,
            ResourceDownloadPreflightAction::AlreadyAvailable
        );
    }

    #[test]
    fn resource_download_preflight_missing_file_fetch_and_verify() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        assert_eq!(plan.entries.len(), 2);
        assert_eq!(
            plan.entries[0].action,
            ResourceDownloadPreflightAction::FetchMissing
        );
        assert_eq!(
            plan.entries[1].action,
            ResourceDownloadPreflightAction::WouldVerifyAfterFetch
        );
    }

    #[test]
    fn resource_download_preflight_invalid_file_replace() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = sample_report(ResourceAvailabilityStatus::HashMismatch);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        assert_eq!(
            plan.entries[0].action,
            ResourceDownloadPreflightAction::ReplaceInvalid
        );
    }

    #[test]
    fn resource_download_preflight_strict_signature_blocks() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Invalid,
                reason: "signature invalid".to_string(),
            },
            &signature_engine::SignaturePolicy::Strict,
            None,
        );
        assert_eq!(
            plan.entries[0].action,
            ResourceDownloadPreflightAction::BlockedBySignaturePolicy
        );
    }

    #[test]
    fn resource_download_preflight_resource_policy_blocks() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let evaluation = ResourcePolicyEvaluation {
            decision: ResourceJoinDecision::Blocked,
            missing_required: vec!["chat:resource.toml".to_string()],
            invalid_required: vec![],
            missing_optional: vec![],
            invalid_optional: vec![],
            missing_recommended: vec![],
            invalid_recommended: vec![],
        };
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            Some(&evaluation),
        );
        assert_eq!(
            plan.entries[0].action,
            ResourceDownloadPreflightAction::BlockedByResourcePolicy
        );
    }

    #[test]
    fn resource_download_preflight_ordering_deterministic() {
        let announcement = ResourceAnnouncement {
            resources: vec![
                AnnouncedResource {
                    name: "zeta".to_string(),
                    version: "1.0.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "b.txt".to_string(),
                        size_bytes: 1,
                        sha256: "b".to_string(),
                        sources: None,
                    }],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
                AnnouncedResource {
                    name: "alpha".to_string(),
                    version: "1.0.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "a.txt".to_string(),
                        size_bytes: 1,
                        sha256: "a".to_string(),
                        sources: None,
                    }],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
            ],
            signature: None,
        };
        let report = ResourceAvailabilityReport {
            resources: vec![],
            is_fully_available: false,
        };
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        assert_eq!(plan.entries[0].resource_name, "alpha");
        assert_eq!(plan.to_text(), plan.to_text());
    }

    // -------------------------------------------------------------------
    // Resource download preflight — source metadata reporting (M6.5)
    // -------------------------------------------------------------------

    #[test]
    fn preflight_sources_valid_in_text() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![
                        ResourceFetchSource {
                            id: Some("primary".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://cdn.example.com/resource.toml".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(10),
                            mirrors: None,
                        },
                        ResourceFetchSource {
                            id: Some("fallback".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://backup.example.com/resource.toml".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(20),
                            mirrors: None,
                        },
                    ]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let text = plan.to_text();
        assert!(
            text.contains("sources: 2 validated"),
            "text should show validated source count:\n{text}"
        );
    }

    #[test]
    fn preflight_sources_valid_in_json() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![ResourceFetchSource {
                        id: Some("cdn".to_string()),
                        scheme: "https".to_string(),
                        uri: "https://cdn.example.com/resource.toml".to_string(),
                        size_bytes: Some(123),
                        sha256: Some("abc".to_string()),
                        compression: None,
                        media_type: None,
                        priority: Some(10),
                        mirrors: None,
                    }]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let json = serde_json::to_string_pretty(&plan).unwrap();
        assert!(
            json.contains("valid_sources"),
            "JSON should include valid_sources:\n{json}"
        );
        assert!(
            json.contains("https://cdn.example.com/resource.toml"),
            "JSON should include source URI:\n{json}"
        );
    }

    #[test]
    fn preflight_sources_invalid_scheme_in_text() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![ResourceFetchSource {
                        id: None,
                        scheme: "http".to_string(),
                        uri: "http://insecure.example.com/resource.toml".to_string(),
                        size_bytes: None,
                        sha256: None,
                        compression: None,
                        media_type: None,
                        priority: None,
                        mirrors: None,
                    }]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let text = plan.to_text();
        assert!(
            text.contains("source error"),
            "text should contain error for invalid scheme:\n{text}"
        );
        assert!(
            text.contains("unsupported scheme: http"),
            "text should name unsupported scheme:\n{text}"
        );
    }

    #[test]
    fn preflight_sources_duplicate_in_text() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![
                        ResourceFetchSource {
                            id: None,
                            scheme: "https".to_string(),
                            uri: "https://example.com/resource.toml".to_string(),
                            size_bytes: None,
                            sha256: None,
                            compression: None,
                            media_type: None,
                            priority: None,
                            mirrors: None,
                        },
                        ResourceFetchSource {
                            id: None,
                            scheme: "https".to_string(),
                            uri: "https://example.com/resource.toml".to_string(),
                            size_bytes: None,
                            sha256: None,
                            compression: None,
                            media_type: None,
                            priority: None,
                            mirrors: None,
                        },
                    ]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let text = plan.to_text();
        assert!(
            text.contains("source error"),
            "text should contain error for duplicate source:\n{text}"
        );
        assert!(
            text.contains("duplicate source"),
            "text should mention duplicate:\n{text}"
        );
    }

    #[test]
    fn preflight_sources_no_behavior_change() {
        // Verify that source metadata does NOT alter the action, reason,
        // entry count, or availability logic.
        let announcement_with_sources = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![ResourceFetchSource {
                        id: None,
                        scheme: "https".to_string(),
                        uri: "https://example.com/resource.toml".to_string(),
                        size_bytes: None,
                        sha256: None,
                        compression: None,
                        media_type: None,
                        priority: None,
                        mirrors: None,
                    }]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let announcement_no_sources = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: None,
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };

        for status in &[
            ResourceAvailabilityStatus::Available,
            ResourceAvailabilityStatus::Missing,
            ResourceAvailabilityStatus::SizeMismatch,
            ResourceAvailabilityStatus::HashMismatch,
        ] {
            let report = sample_report(status.clone());
            let plan_with = build_resource_download_preflight_plan(
                &announcement_with_sources,
                &report,
                &SignatureVerificationReport {
                    status: SignatureVerificationStatus::Valid,
                    reason: "signature valid".to_string(),
                },
                &signature_engine::SignaturePolicy::ReportOnly,
                None,
            );
            let plan_without = build_resource_download_preflight_plan(
                &announcement_no_sources,
                &report,
                &SignatureVerificationReport {
                    status: SignatureVerificationStatus::Valid,
                    reason: "signature valid".to_string(),
                },
                &signature_engine::SignaturePolicy::ReportOnly,
                None,
            );
            for (entry_with, entry_without) in
                plan_with.entries.iter().zip(plan_without.entries.iter())
            {
                assert_eq!(
                    entry_with.action, entry_without.action,
                    "action should match for status={:?}",
                    status
                );
                assert_eq!(
                    entry_with.reason, entry_without.reason,
                    "reason should match for status={:?}",
                    status
                );
            }
            assert_eq!(
                plan_with.entries.len(),
                plan_without.entries.len(),
                "entry count should match for status={:?}",
                status
            );
        }
    }

    #[test]
    fn preflight_sources_deterministic_output() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![
                        ResourceFetchSource {
                            id: Some("z".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://z.example.com/f".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(50),
                            mirrors: None,
                        },
                        ResourceFetchSource {
                            id: Some("a".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://a.example.com/f".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(10),
                            mirrors: None,
                        },
                    ]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        assert_eq!(
            plan.to_text(),
            plan.to_text(),
            "text output is deterministic"
        );
        let json = serde_json::to_string_pretty(&plan).unwrap();
        assert_eq!(
            json,
            serde_json::to_string_pretty(&plan).unwrap(),
            "JSON output is deterministic"
        );
    }

    // -----------------------------------------------------------------------
    // Fetch source selection planning tests (M6.6)
    // -----------------------------------------------------------------------

    #[test]
    fn select_fetch_source_lowest_priority_selected() {
        let mut sources = vec![
            ResourceFetchSource {
                id: Some("a".to_string()),
                scheme: "https".to_string(),
                uri: "https://a.example.com/f".to_string(),
                priority: Some(50),
                ..sample_source()
            },
            ResourceFetchSource {
                id: Some("b".to_string()),
                scheme: "https".to_string(),
                uri: "https://b.example.com/f".to_string(),
                priority: Some(10),
                ..sample_source()
            },
        ];
        // Simulate deterministic sort from validate_and_order_sources
        sort_sources(&mut sources);
        let (selected, fallbacks) = select_fetch_source(&sources);
        assert!(selected.is_some());
        assert_eq!(selected.as_ref().unwrap().uri, "https://b.example.com/f");
        assert_eq!(fallbacks.len(), 1);
        assert_eq!(fallbacks[0].uri, "https://a.example.com/f");
    }

    #[test]
    fn select_fetch_source_tie_break_by_id() {
        let mut sources = vec![
            ResourceFetchSource {
                id: Some("z".to_string()),
                scheme: "https".to_string(),
                uri: "https://z.example.com/f".to_string(),
                priority: Some(10),
                ..sample_source()
            },
            ResourceFetchSource {
                id: Some("a".to_string()),
                scheme: "https".to_string(),
                uri: "https://a.example.com/f".to_string(),
                priority: Some(10),
                ..sample_source()
            },
        ];
        sort_sources(&mut sources);
        let (selected, _) = select_fetch_source(&sources);
        assert_eq!(
            selected.as_ref().unwrap().id.as_deref(),
            Some("a"),
            "lower id wins tie-break"
        );
    }

    #[test]
    fn select_fetch_source_tie_break_by_uri() {
        let mut sources = vec![
            ResourceFetchSource {
                id: None,
                scheme: "https".to_string(),
                uri: "https://z.example.com/f".to_string(),
                priority: Some(10),
                ..sample_source()
            },
            ResourceFetchSource {
                id: None,
                scheme: "https".to_string(),
                uri: "https://a.example.com/f".to_string(),
                priority: Some(10),
                ..sample_source()
            },
        ];
        sort_sources(&mut sources);
        let (selected, _) = select_fetch_source(&sources);
        assert_eq!(
            selected.as_ref().unwrap().uri,
            "https://a.example.com/f",
            "lower uri wins final tie-break"
        );
    }

    #[test]
    fn select_fetch_source_empty_returns_none() {
        let (selected, fallbacks) = select_fetch_source(&[]);
        assert!(selected.is_none());
        assert!(fallbacks.is_empty());
    }

    #[test]
    fn preflight_selected_source_in_text() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![
                        ResourceFetchSource {
                            id: Some("primary".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://cdn.example.com/resource.toml".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(10),
                            mirrors: None,
                        },
                        ResourceFetchSource {
                            id: Some("fallback".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://backup.example.com/resource.toml".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(20),
                            mirrors: None,
                        },
                    ]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let text = plan.to_text();
        assert!(
            text.contains("selected source:"),
            "text should show selected source:\n{text}"
        );
        assert!(
            text.contains("https://cdn.example.com/resource.toml"),
            "text should show primary source URI:\n{text}"
        );
        assert!(
            text.contains("fallback sources: 1"),
            "text should show fallback count:\n{text}"
        );
    }

    #[test]
    fn preflight_no_selected_source_when_none_valid() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![ResourceFetchSource {
                        id: None,
                        scheme: "http".to_string(),
                        uri: "http://insecure.example.com/resource.toml".to_string(),
                        size_bytes: None,
                        sha256: None,
                        compression: None,
                        media_type: None,
                        priority: None,
                        mirrors: None,
                    }]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let text = plan.to_text();
        assert!(
            text.contains("source error"),
            "should show source error for invalid scheme:\n{text}"
        );
        assert!(
            !text.contains("selected source:"),
            "should NOT show selected source when none valid:\n{text}"
        );
        for entry in &plan.entries {
            assert!(entry.selected_source.is_none());
            assert!(entry.fallback_sources.is_empty());
        }
    }

    #[test]
    fn preflight_selected_source_no_behavior_change() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![ResourceFetchSource {
                        id: None,
                        scheme: "https".to_string(),
                        uri: "https://example.com/resource.toml".to_string(),
                        size_bytes: None,
                        sha256: None,
                        compression: None,
                        media_type: None,
                        priority: None,
                        mirrors: None,
                    }]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        for status in &[
            ResourceAvailabilityStatus::Available,
            ResourceAvailabilityStatus::Missing,
            ResourceAvailabilityStatus::SizeMismatch,
            ResourceAvailabilityStatus::HashMismatch,
        ] {
            let report = sample_report(status.clone());
            let plan = build_resource_download_preflight_plan(
                &announcement,
                &report,
                &SignatureVerificationReport {
                    status: SignatureVerificationStatus::Valid,
                    reason: "signature valid".to_string(),
                },
                &signature_engine::SignaturePolicy::ReportOnly,
                None,
            );
            for entry in &plan.entries {
                // Action/reason unchanged from pre-M6.6
                match entry.action {
                    ResourceDownloadPreflightAction::AlreadyAvailable => {
                        assert_eq!(entry.reason, "resource file already available locally");
                    }
                    ResourceDownloadPreflightAction::FetchMissing => {
                        assert_eq!(entry.reason, "resource file missing from local cache");
                    }
                    ResourceDownloadPreflightAction::ReplaceInvalid => {
                        assert_eq!(entry.reason, "resource file present but invalid locally");
                    }
                    ResourceDownloadPreflightAction::WouldVerifyAfterFetch => {
                        assert_eq!(
                            entry.reason,
                            "downloaded file would require post-fetch verification"
                        );
                    }
                    _ => {}
                }
                // Selected source should be present since source is valid
                assert!(
                    entry.selected_source.is_some(),
                    "should have selected source for status={:?}",
                    status
                );
            }
        }
    }

    #[test]
    fn preflight_selected_source_deterministic_output() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![
                        ResourceFetchSource {
                            id: Some("a".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://a.example.com/f".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(20),
                            mirrors: None,
                        },
                        ResourceFetchSource {
                            id: Some("b".to_string()),
                            scheme: "https".to_string(),
                            uri: "https://b.example.com/f".to_string(),
                            size_bytes: Some(123),
                            sha256: Some("abc".to_string()),
                            compression: None,
                            media_type: None,
                            priority: Some(10),
                            mirrors: None,
                        },
                    ]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        // Running twice gives same output
        assert_eq!(plan.to_text(), plan.to_text());
        let json = serde_json::to_string_pretty(&plan).unwrap();
        assert_eq!(json, serde_json::to_string_pretty(&plan).unwrap());
        // The lowest-priority source (10) should be selected
        assert_eq!(
            plan.entries[0].selected_source.as_ref().unwrap().uri,
            "https://b.example.com/f",
            "lowest priority source should be selected"
        );
        assert_eq!(plan.entries[0].fallback_sources.len(), 1);
        assert_eq!(
            plan.entries[0].fallback_sources[0].uri,
            "https://a.example.com/f"
        );
    }

    #[test]
    fn preflight_selected_source_json_serialization() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: Some(vec![ResourceFetchSource {
                        id: Some("cdn".to_string()),
                        scheme: "https".to_string(),
                        uri: "https://cdn.example.com/resource.toml".to_string(),
                        size_bytes: Some(123),
                        sha256: Some("abc".to_string()),
                        compression: None,
                        media_type: None,
                        priority: Some(10),
                        mirrors: None,
                    }]),
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        };
        let report = sample_report(ResourceAvailabilityStatus::Missing);
        let plan = build_resource_download_preflight_plan(
            &announcement,
            &report,
            &SignatureVerificationReport {
                status: SignatureVerificationStatus::Valid,
                reason: "signature valid".to_string(),
            },
            &signature_engine::SignaturePolicy::ReportOnly,
            None,
        );
        let json = serde_json::to_string_pretty(&plan).unwrap();
        assert!(
            json.contains("selected_source"),
            "JSON should include selected_source:\n{json}"
        );
        assert!(
            json.contains("https://cdn.example.com/resource.toml"),
            "JSON should include selected source URI:\n{json}"
        );
        // No fallbacks when only one source
        assert!(
            !json.contains("fallback_sources"),
            "JSON should omit empty fallback_sources:\n{json}"
        );
        // Round-trip: deserialize and re-serialize should match
        let deserialized: ResourceDownloadPreflightPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, plan);
    }

    fn sort_sources(sources: &mut [ResourceFetchSource]) {
        sources.sort_by(|a, b| {
            let pa = a.priority.unwrap_or(100);
            let pb = b.priority.unwrap_or(100);
            pa.cmp(&pb)
                .then(
                    a.id.clone()
                        .unwrap_or_default()
                        .cmp(&b.id.clone().unwrap_or_default()),
                )
                .then(a.uri.cmp(&b.uri))
        });
    }

    fn sample_source() -> ResourceFetchSource {
        ResourceFetchSource {
            id: None,
            scheme: "https".to_string(),
            uri: "https://example.com/f".to_string(),
            size_bytes: None,
            sha256: None,
            compression: None,
            media_type: None,
            priority: None,
            mirrors: None,
        }
    }

    fn sample_announcement(requirement_level: ResourceRequirementLevel) -> ResourceAnnouncement {
        ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: None,
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level,
            }],
            signature: None,
        }
    }

    fn sample_report(status: ResourceAvailabilityStatus) -> ResourceAvailabilityReport {
        ResourceAvailabilityReport {
            resources: vec![ResourceAvailabilityEntry {
                resource_name: "chat".to_string(),
                file_path: "resource.toml".to_string(),
                status,
            }],
            is_fully_available: false,
        }
    }

    // -----------------------------------------------------------------------
    // Capability gate tests
    // -----------------------------------------------------------------------

    #[test]
    fn profile_supports_existing_capability() {
        let profile = current_protocol_profile();
        assert!(profile_supports_capability(
            &profile,
            ProtocolCapability::ResourceAnnouncement
        ));
    }

    #[test]
    fn profile_does_not_support_missing_capability() {
        let profile = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 1, max: 1 },
            capabilities: vec![ProtocolCapability::ResourceAnnouncement],
        };
        assert!(!profile_supports_capability(
            &profile,
            ProtocolCapability::JoinGateDryRun
        ));
    }

    #[test]
    fn shared_capabilities_intersection_deterministic() {
        let client = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 1, max: 1 },
            capabilities: vec![
                ProtocolCapability::JoinGateDryRun,
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::SignatureMetadata,
            ],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 1, max: 1 },
            capabilities: vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::JoinGateDryRun,
            ],
        };
        let shared = shared_capabilities(&client, &server);
        assert_eq!(
            shared,
            vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::JoinGateDryRun,
            ]
        );
    }

    #[test]
    fn missing_required_capability_returns_error() {
        let shared = vec![ProtocolCapability::ResourceAnnouncement];
        let result = requires_capability(ProtocolCapability::JoinGateDryRun, &shared);
        assert!(matches!(
            result,
            Err(ProtocolCapabilityError::MissingCapability {
                capability: ProtocolCapability::JoinGateDryRun
            })
        ));
    }

    #[test]
    fn present_required_capability_succeeds() {
        let shared = vec![
            ProtocolCapability::ResourceAnnouncement,
            ProtocolCapability::JoinGateDryRun,
        ];
        assert!(requires_capability(ProtocolCapability::JoinGateDryRun, &shared).is_ok());
    }

    #[test]
    fn current_protocol_profile_includes_expected_capabilities() {
        let profile = current_protocol_profile();
        assert!(profile_supports_capability(
            &profile,
            ProtocolCapability::ResourceAnnouncement
        ));
        assert!(profile_supports_capability(
            &profile,
            ProtocolCapability::ResourceAvailabilityReport
        ));
        assert!(profile_supports_capability(
            &profile,
            ProtocolCapability::JoinGateDryRun
        ));
        assert!(profile_supports_capability(
            &profile,
            ProtocolCapability::ResourceCompatibilityReport
        ));
        assert!(profile_supports_capability(
            &profile,
            ProtocolCapability::SignatureMetadata
        ));
    }

    #[test]
    fn capability_gate_report_supported() {
        let shared = vec![ProtocolCapability::ResourceAnnouncement];
        let report = capability_gate_report(ProtocolCapability::ResourceAnnouncement, &shared);
        assert!(report.supported);
        assert_eq!(report.capability, ProtocolCapability::ResourceAnnouncement);
    }

    #[test]
    fn capability_gate_report_not_supported() {
        let shared: Vec<ProtocolCapability> = vec![];
        let report = capability_gate_report(ProtocolCapability::ResourceAnnouncement, &shared);
        assert!(!report.supported);
        assert_eq!(report.capability, ProtocolCapability::ResourceAnnouncement);
    }

    // -------------------------------------------------------------------
    // Signature metadata model tests (M3.6)
    // -------------------------------------------------------------------

    #[test]
    fn validate_signature_metadata_valid_ed25519() {
        let sig = ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        };
        assert!(validate_signature_metadata(&sig).is_ok());
    }

    #[test]
    fn validate_signature_metadata_empty_algorithm() {
        let sig = ResourceAnnouncementSignature {
            algorithm: "".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        };
        let err = validate_signature_metadata(&sig).unwrap_err();
        assert_eq!(err, SignatureMetadataError::EmptyAlgorithm);
    }

    #[test]
    fn validate_signature_metadata_empty_key_id() {
        let sig = ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        };
        let err = validate_signature_metadata(&sig).unwrap_err();
        assert_eq!(err, SignatureMetadataError::EmptyKeyId);
    }

    #[test]
    fn validate_signature_metadata_empty_signature() {
        let sig = ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "".to_string(),
        };
        let err = validate_signature_metadata(&sig).unwrap_err();
        assert_eq!(err, SignatureMetadataError::EmptySignature);
    }

    #[test]
    fn validate_signature_metadata_unknown_algorithm() {
        let sig = ResourceAnnouncementSignature {
            algorithm: "rsa".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        };
        let err = validate_signature_metadata(&sig).unwrap_err();
        assert!(matches!(
            err,
            SignatureMetadataError::UnsupportedAlgorithm(_)
        ));
    }

    #[test]
    fn signature_algorithm_display_and_from_str_roundtrip() {
        let alg = SignatureAlgorithm::Ed25519;
        assert_eq!(alg.to_string(), "ed25519");
        let parsed = SignatureAlgorithm::from_str("ed25519").unwrap();
        assert_eq!(parsed, SignatureAlgorithm::Ed25519);
    }

    #[test]
    fn signature_algorithm_unknown_from_str_fails() {
        let err = SignatureAlgorithm::from_str("rsa").unwrap_err();
        assert!(matches!(
            err,
            SignatureMetadataError::UnsupportedAlgorithm(_)
        ));
    }

    #[test]
    fn signature_algorithm_known_names_includes_ed25519() {
        assert!(SignatureAlgorithm::known_names().contains(&"ed25519"));
    }

    #[test]
    fn signature_algorithm_is_known() {
        assert!(SignatureAlgorithm::is_known("ed25519"));
        assert!(!SignatureAlgorithm::is_known("rsa"));
    }

    #[test]
    fn stub_returns_not_provided_when_no_signature() {
        let report = check_announcement_signature_stub(&sample_announcement(
            ResourceRequirementLevel::Required,
        ));
        assert_eq!(report.status, SignatureVerificationStatus::NotProvided);
    }

    #[test]
    fn stub_returns_not_checked_for_valid_metadata() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let report = check_announcement_signature_stub(&announcement);
        assert_eq!(report.status, SignatureVerificationStatus::NotChecked);
    }

    #[test]
    fn stub_returns_unsupported_algorithm_for_unknown_algorithm() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "rsa".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let report = check_announcement_signature_stub(&announcement);
        assert_eq!(
            report.status,
            SignatureVerificationStatus::UnsupportedAlgorithm
        );
    }

    #[test]
    fn stub_returns_invalid_for_empty_algorithm() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let report = check_announcement_signature_stub(&announcement);
        assert_eq!(report.status, SignatureVerificationStatus::Invalid);
    }

    #[test]
    fn stub_returns_invalid_for_empty_key_id() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let report = check_announcement_signature_stub(&announcement);
        assert_eq!(report.status, SignatureVerificationStatus::Invalid);
    }

    #[test]
    fn stub_returns_invalid_for_empty_signature() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "".to_string(),
        });
        let report = check_announcement_signature_stub(&announcement);
        assert_eq!(report.status, SignatureVerificationStatus::Invalid);
    }

    #[test]
    fn build_canonical_payload_basic_structure() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let payload = build_canonical_payload(&announcement).unwrap();
        assert_eq!(payload.protocol_version, PROTOCOL_VERSION);
        assert_eq!(payload.algorithm, "ed25519");
        assert_eq!(payload.key_id, "dev-key");
        assert_eq!(payload.resources.len(), 1);
        assert_eq!(payload.resources[0].name, "chat");
        assert_eq!(payload.resources[0].version, "0.1.0");
        assert_eq!(payload.resources[0].files.len(), 1);
        assert_eq!(payload.resources[0].files[0].relative_path, "resource.toml");
        assert_eq!(payload.resources[0].files[0].size_bytes, 123);
        assert_eq!(payload.resources[0].files[0].sha256, "abc");
    }

    #[test]
    fn build_canonical_payload_no_signature_returns_none() {
        let announcement = sample_announcement(ResourceRequirementLevel::Required);
        assert!(build_canonical_payload(&announcement).is_none());
    }

    #[test]
    fn build_canonical_payload_empty_algorithm_returns_none() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        assert!(build_canonical_payload(&announcement).is_none());
    }

    #[test]
    fn build_canonical_payload_deterministic_sorting() {
        let announcement = ResourceAnnouncement {
            resources: vec![
                AnnouncedResource {
                    name: "zeta".to_string(),
                    version: "1.0.0".to_string(),
                    files: vec![
                        AnnouncedResourceFile {
                            relative_path: "b.txt".to_string(),
                            size_bytes: 2,
                            sha256: "b".to_string(),
                            sources: None,
                        },
                        AnnouncedResourceFile {
                            relative_path: "a.txt".to_string(),
                            size_bytes: 1,
                            sha256: "a".to_string(),
                            sources: None,
                        },
                    ],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
                AnnouncedResource {
                    name: "alpha".to_string(),
                    version: "0.1.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "c.txt".to_string(),
                        size_bytes: 3,
                        sha256: "c".to_string(),
                        sources: None,
                    }],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
            ],
            signature: Some(ResourceAnnouncementSignature {
                algorithm: "ed25519".to_string(),
                key_id: "dev-key".to_string(),
                signature: "c29tZS1zaWc=".to_string(),
            }),
        };
        let payload = build_canonical_payload(&announcement).unwrap();
        // Resources sorted by name
        assert_eq!(payload.resources[0].name, "alpha");
        assert_eq!(payload.resources[1].name, "zeta");
        // Files sorted by relative_path
        assert_eq!(payload.resources[1].files[0].relative_path, "a.txt");
        assert_eq!(payload.resources[1].files[1].relative_path, "b.txt");
    }

    #[test]
    fn build_canonical_payload_json_serialization_roundtrip() {
        let mut announcement = sample_announcement(ResourceRequirementLevel::Required);
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let payload = build_canonical_payload(&announcement).unwrap();
        let json = serde_json::to_string(&payload).unwrap();
        let decoded: CanonicalAnnouncementPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn signature_metadata_error_display() {
        let err = SignatureMetadataError::UnsupportedAlgorithm("rsa".to_string());
        assert!(err.to_string().contains("unsupported"));
        assert!(err.to_string().contains("rsa"));

        let err = SignatureMetadataError::EmptyAlgorithm;
        assert!(err.to_string().contains("algorithm"));

        let err = SignatureMetadataError::EmptyKeyId;
        assert!(err.to_string().contains("key_id"));

        let err = SignatureMetadataError::EmptySignature;
        assert!(err.to_string().contains("signature"));
    }

    // -------------------------------------------------------------------
    // Signature verification plan tests (M3.7)
    // -------------------------------------------------------------------

    fn sample_plan_announcement() -> ResourceAnnouncement {
        ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                    sources: None,
                }],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: Some(ResourceAnnouncementSignature {
                algorithm: "ed25519".to_string(),
                key_id: "dev-key".to_string(),
                signature: "c29tZS1zaWc=".to_string(),
            }),
        }
    }

    fn sample_multi_resource_announcement() -> ResourceAnnouncement {
        ResourceAnnouncement {
            resources: vec![
                AnnouncedResource {
                    name: "admin".to_string(),
                    version: "1.0.0".to_string(),
                    files: vec![],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Optional,
                },
                AnnouncedResource {
                    name: "chat".to_string(),
                    version: "0.1.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "resource.toml".to_string(),
                        size_bytes: 123,
                        sha256: "abc".to_string(),
                        sources: None,
                    }],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
                AnnouncedResource {
                    name: "scoreboard".to_string(),
                    version: "2.0.0".to_string(),
                    files: vec![],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Optional,
                },
            ],
            signature: Some(ResourceAnnouncementSignature {
                algorithm: "ed25519".to_string(),
                key_id: "prod-key".to_string(),
                signature: "c29tZS1zaWc=".to_string(),
            }),
        }
    }

    // -------------------------------------------------------------------
    // Resource fetch metadata validation tests (M6.4)
    // -------------------------------------------------------------------

    #[test]
    fn validate_and_order_sources_deterministic_sort() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 10,
            sha256: "deadbeef".to_string(),
            sources: Some(vec![
                ResourceFetchSource {
                    id: Some("b".to_string()),
                    scheme: "https".to_string(),
                    uri: "https://example.com/z".to_string(),
                    size_bytes: Some(10),
                    sha256: Some("deadbeef".to_string()),
                    compression: None,
                    media_type: None,
                    priority: Some(50),
                    mirrors: None,
                },
                ResourceFetchSource {
                    id: Some("a".to_string()),
                    scheme: "https".to_string(),
                    uri: "https://example.com/a".to_string(),
                    size_bytes: Some(10),
                    sha256: Some("deadbeef".to_string()),
                    compression: None,
                    media_type: None,
                    priority: Some(10),
                    mirrors: None,
                },
                ResourceFetchSource {
                    id: Some("a".to_string()),
                    scheme: "https".to_string(),
                    uri: "https://example.com/b".to_string(),
                    size_bytes: Some(10),
                    sha256: Some("deadbeef".to_string()),
                    compression: None,
                    media_type: None,
                    priority: Some(10),
                    mirrors: None,
                },
            ]),
        };

        let ordered = validate_and_order_sources(&file).unwrap();
        let uris: Vec<String> = ordered.into_iter().map(|s| s.uri).collect();
        assert_eq!(
            uris,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
                "https://example.com/z".to_string(),
            ]
        );
    }

    #[test]
    fn validate_and_order_sources_rejects_unsupported_scheme() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 1,
            sha256: "x".to_string(),
            sources: Some(vec![ResourceFetchSource {
                id: None,
                scheme: "http".to_string(),
                uri: "http://example/1".to_string(),
                size_bytes: None,
                sha256: None,
                compression: None,
                media_type: None,
                priority: None,
                mirrors: None,
            }]),
        };

        let err = validate_and_order_sources(&file).unwrap_err();
        match err {
            ResourceFetchMetadataError::UnsupportedScheme(s) => assert!(s == "http"),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn validate_and_order_sources_rejects_duplicate_source() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 1,
            sha256: "x".to_string(),
            sources: Some(vec![
                ResourceFetchSource {
                    id: None,
                    scheme: "https".to_string(),
                    uri: "https://a".to_string(),
                    size_bytes: None,
                    sha256: None,
                    compression: None,
                    media_type: None,
                    priority: None,
                    mirrors: None,
                },
                ResourceFetchSource {
                    id: None,
                    scheme: "https".to_string(),
                    uri: "https://a".to_string(),
                    size_bytes: None,
                    sha256: None,
                    compression: None,
                    media_type: None,
                    priority: None,
                    mirrors: None,
                },
            ]),
        };

        let err = validate_and_order_sources(&file).unwrap_err();
        match err {
            ResourceFetchMetadataError::DuplicateSource { scheme, uri } => {
                assert_eq!(scheme, "https");
                assert_eq!(uri, "https://a")
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn validate_and_order_sources_rejects_size_mismatch() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 100,
            sha256: "abc".to_string(),
            sources: Some(vec![ResourceFetchSource {
                id: None,
                scheme: "https".to_string(),
                uri: "https://a".to_string(),
                size_bytes: Some(99),
                sha256: Some("abc".to_string()),
                compression: None,
                media_type: None,
                priority: None,
                mirrors: None,
            }]),
        };

        let err = validate_and_order_sources(&file).unwrap_err();
        match err {
            ResourceFetchMetadataError::SizeMismatch { expected, found } => {
                assert_eq!(expected, 100);
                assert_eq!(found, 99);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn validate_and_order_sources_rejects_sha_mismatch() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 1,
            sha256: "abc".to_string(),
            sources: Some(vec![ResourceFetchSource {
                id: None,
                scheme: "https".to_string(),
                uri: "https://a".to_string(),
                size_bytes: Some(1),
                sha256: Some("zzz".to_string()),
                compression: None,
                media_type: None,
                priority: None,
                mirrors: None,
            }]),
        };

        let err = validate_and_order_sources(&file).unwrap_err();
        match err {
            ResourceFetchMetadataError::DigestMismatch { expected, found } => {
                assert_eq!(expected, "abc".to_string());
                assert_eq!(found, "zzz".to_string());
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn validate_and_order_sources_rejects_file_path_traversal() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 1,
            sha256: "a".to_string(),
            sources: Some(vec![ResourceFetchSource {
                id: None,
                scheme: "file".to_string(),
                uri: "file:///tmp/../etc/passwd".to_string(),
                size_bytes: None,
                sha256: None,
                compression: None,
                media_type: None,
                priority: None,
                mirrors: None,
            }]),
        };

        let err = validate_and_order_sources(&file).unwrap_err();
        match err {
            ResourceFetchMetadataError::PathTraversalInFileUri(s) => assert!(s.contains("..")),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn validate_and_order_sources_missing_sources_ok() {
        let file_none = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 1,
            sha256: "a".to_string(),
            sources: None,
        };
        let file_empty = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 1,
            sha256: "a".to_string(),
            sources: Some(vec![]),
        };

        assert_eq!(validate_and_order_sources(&file_none).unwrap().len(), 0);
        assert_eq!(validate_and_order_sources(&file_empty).unwrap().len(), 0);
    }

    #[test]
    fn validate_and_order_sources_allowed_schemes_ok() {
        let file = AnnouncedResourceFile {
            relative_path: "f.txt".to_string(),
            size_bytes: 2,
            sha256: "a".to_string(),
            sources: Some(vec![
                ResourceFetchSource {
                    id: None,
                    scheme: "https".to_string(),
                    uri: "https://a".to_string(),
                    size_bytes: None,
                    sha256: None,
                    compression: None,
                    media_type: None,
                    priority: None,
                    mirrors: None,
                },
                ResourceFetchSource {
                    id: None,
                    scheme: "file".to_string(),
                    uri: "file:///tmp/x".to_string(),
                    size_bytes: None,
                    sha256: None,
                    compression: None,
                    media_type: None,
                    priority: None,
                    mirrors: None,
                },
                ResourceFetchSource {
                    id: None,
                    scheme: "ipfs".to_string(),
                    uri: "ipfs://Qm...".to_string(),
                    size_bytes: None,
                    sha256: None,
                    compression: None,
                    media_type: None,
                    priority: None,
                    mirrors: None,
                },
            ]),
        };

        let res = validate_and_order_sources(&file).unwrap();
        assert_eq!(res.len(), 3);
    }

    #[test]
    fn plan_valid_signed_announcement_trusted_key() {
        let announcement = sample_plan_announcement();
        let trusted = vec![TrustedKey {
            key_id: "dev-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::VerifySignature
        );
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_no_signature_no_reject_unsigned() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = None;
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::MissingSignature
        );
    }

    #[test]
    fn plan_no_signature_reject_unsigned() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = None;
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, true);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::WouldRejectUnsigned
        );
    }

    #[test]
    fn plan_unsupported_algorithm() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "rsa".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::UnsupportedAlgorithm
        );
    }

    #[test]
    fn plan_unknown_key_id() {
        let announcement = sample_plan_announcement();
        let trusted = vec![TrustedKey {
            key_id: "other-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::UnknownKeyId
        );
    }

    #[test]
    fn plan_malformed_signature_empty_algorithm() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "".to_string(),
            key_id: "dev-key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::MalformedSignature
        );
    }

    #[test]
    fn plan_malformed_signature_empty_key_id() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::MalformedSignature
        );
    }

    #[test]
    fn plan_malformed_signature_empty_sig() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "dev-key".to_string(),
            signature: "".to_string(),
        });
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::MalformedSignature
        );
    }

    #[test]
    fn plan_multiple_resources_sorted() {
        let announcement = sample_multi_resource_announcement();
        let trusted = vec![TrustedKey {
            key_id: "prod-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(plan.entries.len(), 3);
        // must be sorted by resource name
        assert_eq!(plan.entries[0].resource_name, "admin");
        assert_eq!(plan.entries[1].resource_name, "chat");
        assert_eq!(plan.entries[2].resource_name, "scoreboard");
        for entry in &plan.entries {
            assert_eq!(entry.action, SignatureVerificationAction::VerifySignature);
        }
    }

    #[test]
    fn plan_to_text_output_format() {
        let announcement = sample_plan_announcement();
        let trusted = vec![TrustedKey {
            key_id: "dev-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        let text = plan.to_text();
        assert!(text.contains("signature verification plan:"));
        assert!(text.contains("[verify_signature]"));
        assert!(text.contains("chat"));
        assert!(text.contains("total: 1 resource(s)"));
    }

    #[test]
    fn plan_to_text_empty() {
        let announcement = ResourceAnnouncement {
            resources: vec![],
            signature: None,
        };
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let text = plan.to_text();
        assert!(text.contains("(empty, no resources)"));
        assert!(plan.is_empty());
    }

    #[test]
    fn plan_action_display() {
        assert_eq!(
            SignatureVerificationAction::VerifySignature.to_string(),
            "verify_signature"
        );
        assert_eq!(
            SignatureVerificationAction::MissingSignature.to_string(),
            "missing_signature"
        );
        assert_eq!(
            SignatureVerificationAction::UnsupportedAlgorithm.to_string(),
            "unsupported_algorithm"
        );
        assert_eq!(
            SignatureVerificationAction::UnknownKeyId.to_string(),
            "unknown_key_id"
        );
        assert_eq!(
            SignatureVerificationAction::MalformedSignature.to_string(),
            "malformed_signature"
        );
        assert_eq!(
            SignatureVerificationAction::WouldRejectUnsigned.to_string(),
            "would_reject_unsigned"
        );
        assert_eq!(
            SignatureVerificationAction::ResourceDigestMismatchPrecheck.to_string(),
            "resource_digest_mismatch_precheck"
        );
    }

    #[test]
    fn plan_key_trusted_any_algorithm_match() {
        // key_id matches but algorithm differs → not trusted
        let announcement = sample_plan_announcement();
        let trusted = vec![TrustedKey {
            key_id: "dev-key".to_string(),
            algorithm: "rsa".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::UnknownKeyId
        );
    }

    #[test]
    fn plan_no_trusted_keys_all_unknown() {
        let announcement = sample_plan_announcement();
        let plan = build_signature_verification_plan(&announcement, &[], false);
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::UnknownKeyId
        );
    }

    #[test]
    fn plan_trusted_key_matches_case_sensitive() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "Dev-Key".to_string(),
            signature: "c29tZS1zaWc=".to_string(),
        });
        let trusted = vec![TrustedKey {
            key_id: "dev-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        // case-sensitive: Dev-Key != dev-key
        assert_eq!(
            plan.entries[0].action,
            SignatureVerificationAction::UnknownKeyId
        );
    }

    #[test]
    fn plan_empty_resources() {
        let announcement = ResourceAnnouncement {
            resources: vec![],
            signature: Some(ResourceAnnouncementSignature {
                algorithm: "ed25519".to_string(),
                key_id: "dev-key".to_string(),
                signature: "c29tZS1zaWc=".to_string(),
            }),
        };
        let plan = build_signature_verification_plan(&announcement, &[], false);
        assert!(plan.is_empty());
    }

    // --- M4.16: server-initiated heartbeat protocol round-trip tests ---

    #[test]
    fn server_ping_serializes_and_deserializes() {
        let msg = ServerMessage::ServerPing { sequence: 42 };
        let line = encode_line(&msg).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        match decoded {
            ServerMessage::ServerPing { sequence } => assert_eq!(sequence, 42),
            other => panic!("expected ServerPing, got {other:?}"),
        }
    }

    #[test]
    fn server_pong_client_message_serializes_and_deserializes() {
        let msg = ClientMessage::ServerPong { sequence: 7 };
        let line = encode_line(&msg).unwrap();
        let decoded = decode_client_line(line.trim()).unwrap();
        match decoded {
            ClientMessage::ServerPong { sequence } => assert_eq!(sequence, 7),
            other => panic!("expected ServerPong, got {other:?}"),
        }
    }

    #[test]
    fn server_ping_sequence_zero_round_trips() {
        let msg = ServerMessage::ServerPing { sequence: 0 };
        let line = encode_line(&msg).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        assert!(matches!(decoded, ServerMessage::ServerPing { sequence: 0 }));
    }

    #[test]
    fn server_pong_sequence_max_round_trips() {
        let msg = ClientMessage::ServerPong { sequence: u64::MAX };
        let line = encode_line(&msg).unwrap();
        let decoded = decode_client_line(line.trim()).unwrap();
        assert!(matches!(
            decoded,
            ClientMessage::ServerPong { sequence: u64::MAX }
        ));
    }

    #[test]
    fn client_ping_round_trip_unchanged_by_m4_16() {
        let msg = ClientMessage::Ping { sequence: 99 };
        let line = encode_line(&msg).unwrap();
        let decoded = decode_client_line(line.trim()).unwrap();
        assert!(matches!(decoded, ClientMessage::Ping { sequence: 99 }));
    }

    #[test]
    fn server_pong_round_trip_unchanged_by_m4_16() {
        let msg = ServerMessage::Pong { sequence: 55 };
        let line = encode_line(&msg).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        assert!(matches!(decoded, ServerMessage::Pong { sequence: 55 }));
    }

    #[test]
    fn server_ping_has_distinct_serde_type_from_client_ping() {
        let server_ping = encode_line(&ServerMessage::ServerPing { sequence: 1 }).unwrap();
        let client_ping = encode_line(&ClientMessage::Ping { sequence: 1 }).unwrap();
        // Wire formats differ by the "type" field
        assert!(server_ping.contains("server_ping"));
        assert!(client_ping.contains("\"ping\""));
        assert!(!server_ping.contains("\"ping\""));
    }

    #[test]
    fn server_pong_client_has_distinct_serde_type_from_server_pong() {
        let client_server_pong = encode_line(&ClientMessage::ServerPong { sequence: 1 }).unwrap();
        let server_pong = encode_line(&ServerMessage::Pong { sequence: 1 }).unwrap();
        assert!(client_server_pong.contains("server_pong"));
        assert!(server_pong.contains("\"pong\""));
        assert!(!client_server_pong.contains("\"pong\""));
    }
}
