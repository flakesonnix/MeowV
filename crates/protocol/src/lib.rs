use serde::{Deserialize, Serialize};
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
        Some(signature) => SignatureVerificationReport {
            status: SignatureVerificationStatus::NotChecked,
            reason: format!(
                "signature verification for algorithm '{}' and key '{}' is not enforced in this milestone",
                signature.algorithm, signature.key_id
            ),
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
}
