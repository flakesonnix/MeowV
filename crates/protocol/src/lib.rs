use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

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
            other => Err(SignatureMetadataError::UnsupportedAlgorithm(other.to_string())),
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

pub fn build_canonical_payload(announcement: &ResourceAnnouncement) -> Option<CanonicalAnnouncementPayload> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Login { name: String, protocol_version: u32 },
    /// Heartbeat ping from client to server. Server should reply with Pong(sequence).
    Ping { sequence: u64 },
    /// Reply to a server-initiated ServerPing. Client echoes the sequence back.
    /// This is the authoritative liveness path: the server owns the timer and
    /// can detect missed replies independently of the client.
    ServerPong { sequence: u64 },
    Chat { message: String },
    ResourceAvailabilityReport(ResourceAvailabilityReport),
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
    Pong { sequence: u64 },
    /// Server-initiated heartbeat ping. Client must reply with ServerPong(sequence).
    /// This is the authoritative liveness direction: the server owns the timer and
    /// can measure missed replies without relying on client-reported data.
    /// Inert stub — no live timer or enforcement in current milestone.
    ServerPing { sequence: u64 },
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
    serde_json::from_str(line)
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
        capabilities: vec![
            ProtocolCapability::ResourceAnnouncement,
            ProtocolCapability::ResourceAvailabilityReport,
            ProtocolCapability::JoinGateDryRun,
            ProtocolCapability::ResourceCompatibilityReport,
            ProtocolCapability::SignatureMetadata,
        ],
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
        let mut lines = format!("signature verification plan:\n");
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
            capabilities: vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::ResourceAvailabilityReport,
                ProtocolCapability::JoinGateDryRun,
                ProtocolCapability::ResourceCompatibilityReport,
                ProtocolCapability::SignatureMetadata,
            ],
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
            version_range: ProtocolVersionRange { min: 0, max: 2 },
            capabilities: vec![ProtocolCapability::ResourceAnnouncement],
        };
        let server = ProtocolCompatibilityProfile {
            version_range: ProtocolVersionRange { min: 2, max: 3 },
            capabilities: vec![
                ProtocolCapability::ResourceAnnouncement,
                ProtocolCapability::JoinGateDryRun,
            ],
        };
        let result = negotiate_protocol_dry_run(&client, &server);
        assert_eq!(result.status, ProtocolNegotiationStatus::CompatibleDryRun);
        assert_eq!(result.selected_version, Some(2));
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

    fn sample_announcement(requirement_level: ResourceRequirementLevel) -> ResourceAnnouncement {
        ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
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
                        },
                        AnnouncedResourceFile {
                            relative_path: "a.txt".to_string(),
                            size_bytes: 1,
                            sha256: "a".to_string(),
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
            resources: vec![
                AnnouncedResource {
                    name: "chat".to_string(),
                    version: "0.1.0".to_string(),
                    files: vec![AnnouncedResourceFile {
                        relative_path: "resource.toml".to_string(),
                        size_bytes: 123,
                        sha256: "abc".to_string(),
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

    #[test]
    fn plan_valid_signed_announcement_trusted_key() {
        let announcement = sample_plan_announcement();
        let trusted = vec![TrustedKey {
            key_id: "dev-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].action, SignatureVerificationAction::VerifySignature);
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_no_signature_no_reject_unsigned() {
        let mut announcement = sample_plan_announcement();
        announcement.signature = None;
        let trusted = vec![];
        let plan = build_signature_verification_plan(&announcement, &trusted, false);
        assert_eq!(plan.entries[0].action, SignatureVerificationAction::MissingSignature);
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
        assert_eq!(plan.entries[0].action, SignatureVerificationAction::UnknownKeyId);
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
        assert_eq!(plan.entries[0].action, SignatureVerificationAction::UnknownKeyId);
    }

    #[test]
    fn plan_no_trusted_keys_all_unknown() {
        let announcement = sample_plan_announcement();
        let plan = build_signature_verification_plan(&announcement, &[], false);
        assert_eq!(plan.entries[0].action, SignatureVerificationAction::UnknownKeyId);
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
        assert_eq!(plan.entries[0].action, SignatureVerificationAction::UnknownKeyId);
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
}
