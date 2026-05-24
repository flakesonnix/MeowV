use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};

use crate::{
    ResourceAnnouncement, SignatureMetadataError, SignatureVerificationPlan,
    build_canonical_payload, validate_signature_metadata,
};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationError {
    UnsupportedAlgorithm(String),
    UnknownKeyId(String),
    InvalidSignature,
    MalformedKeyMaterial(String),
    MalformedSignatureBytes(String),
    SignedPayloadMismatch(String),
    CanonicalPayloadMissing,
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedAlgorithm(alg) => {
                write!(f, "unsupported signature algorithm: '{alg}'")
            }
            Self::UnknownKeyId(kid) => write!(f, "unknown key ID: '{kid}'"),
            Self::InvalidSignature => write!(f, "signature is invalid"),
            Self::MalformedKeyMaterial(msg) => {
                write!(f, "malformed key material: {msg}")
            }
            Self::MalformedSignatureBytes(msg) => {
                write!(f, "malformed signature bytes: {msg}")
            }
            Self::SignedPayloadMismatch(msg) => {
                write!(f, "signed payload mismatch: {msg}")
            }
            Self::CanonicalPayloadMissing => {
                write!(f, "cannot build canonical payload (empty algorithm/key_id)")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Key type with material
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TrustedPublicKey {
    pub key_id: String,
    pub algorithm: String,
    pub public_key: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Key config validation (M4.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyConfigError {
    EmptyConfig,
    DuplicateKeyId {
        key_id: String,
    },
    UnsupportedAlgorithm {
        key_id: String,
        algorithm: String,
    },
    MalformedKeyMaterial {
        key_id: String,
        algorithm: String,
        detail: String,
    },
}

impl std::fmt::Display for KeyConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyConfig => write!(f, "trusted key config is empty"),
            Self::DuplicateKeyId { key_id } => {
                write!(f, "duplicate trusted key ID: '{key_id}'")
            }
            Self::UnsupportedAlgorithm { key_id, algorithm } => {
                write!(
                    f,
                    "trusted key '{key_id}' has unsupported algorithm '{algorithm}'"
                )
            }
            Self::MalformedKeyMaterial {
                key_id,
                algorithm,
                detail,
            } => {
                write!(
                    f,
                    "trusted key '{key_id}' has invalid material for '{algorithm}': {detail}"
                )
            }
        }
    }
}

pub fn validate_trusted_key_config(keys: &[TrustedPublicKey]) -> Result<(), KeyConfigError> {
    if keys.is_empty() {
        return Err(KeyConfigError::EmptyConfig);
    }

    let mut seen = std::collections::HashSet::new();
    for key in keys {
        if !seen.insert(&key.key_id) {
            return Err(KeyConfigError::DuplicateKeyId {
                key_id: key.key_id.clone(),
            });
        }
        if key.algorithm != "ed25519" {
            return Err(KeyConfigError::UnsupportedAlgorithm {
                key_id: key.key_id.clone(),
                algorithm: key.algorithm.clone(),
            });
        }
        if key.algorithm == "ed25519" && key.public_key.len() != 32 {
            return Err(KeyConfigError::MalformedKeyMaterial {
                key_id: key.key_id.clone(),
                algorithm: key.algorithm.clone(),
                detail: format!("expected 32-byte key, got {} bytes", key.public_key.len()),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Verification outcome and report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Valid,
    Invalid { error: VerificationError },
    Skipped { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationEntry {
    pub resource_name: String,
    pub outcome: VerificationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub entries: Vec<VerificationEntry>,
}

impl VerificationReport {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn all_valid(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.outcome == VerificationOutcome::Valid)
    }

    pub fn to_text(&self) -> String {
        if self.entries.is_empty() {
            return "signature verification report: (empty, no resources)\n".to_string();
        }
        let mut lines = format!("signature verification report:\n");
        for entry in &self.entries {
            let label = match &entry.outcome {
                VerificationOutcome::Valid => "valid".to_string(),
                VerificationOutcome::Invalid { error } => format!("invalid: {error}"),
                VerificationOutcome::Skipped { reason } => format!("skipped: {reason}"),
            };
            lines.push_str(&format!("  [{}] {}\n", label, entry.resource_name));
        }
        lines.push_str(&format!(
            "  total: {} resource(s), all valid: {}\n",
            self.entries.len(),
            self.all_valid(),
        ));
        lines
    }
}

// ---------------------------------------------------------------------------
// Core verification helpers
// ---------------------------------------------------------------------------

/// Verify an Ed25519 signature against canonical payload bytes.
fn verify_ed25519_raw(
    payload_bytes: &[u8],
    signature_bytes: &[u8],
    public_key_bytes: &[u8],
) -> Result<(), VerificationError> {
    let pk_array: [u8; 32] = public_key_bytes.try_into().map_err(|_| {
        VerificationError::MalformedKeyMaterial(format!(
            "expected 32-byte Ed25519 public key, got {} bytes",
            public_key_bytes.len()
        ))
    })?;

    let verifying_key = VerifyingKey::from_bytes(&pk_array).map_err(|e| {
        VerificationError::MalformedKeyMaterial(format!("failed to parse public key: {e}"))
    })?;

    let signature = Signature::from_slice(signature_bytes).map_err(|e| {
        VerificationError::MalformedSignatureBytes(format!("failed to parse signature: {e}"))
    })?;

    verifying_key
        .verify(payload_bytes, &signature)
        .map_err(|_| VerificationError::InvalidSignature)
}

/// Decode a base64 signature string and verify it against canonical payload bytes.
pub fn verify_ed25519_signature(
    payload_bytes: &[u8],
    signature_b64: &str,
    public_key_bytes: &[u8],
) -> Result<(), VerificationError> {
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|e| {
            VerificationError::MalformedSignatureBytes(format!("base64 decode failed: {e}"))
        })?;

    if sig_bytes.len() != 64 {
        return Err(VerificationError::MalformedSignatureBytes(format!(
            "expected 64-byte Ed25519 signature, got {} bytes",
            sig_bytes.len(),
        )));
    }

    verify_ed25519_raw(payload_bytes, &sig_bytes, public_key_bytes)
}

// ---------------------------------------------------------------------------
// Plan execution
// ---------------------------------------------------------------------------

/// Execute an M3.7 verification plan against actual key material.
///
/// All plan entries receive the same announcement-level verification outcome
/// (announcement-level signature covers all resources equally).
pub fn execute_verification_plan(
    announcement: &ResourceAnnouncement,
    plan: &SignatureVerificationPlan,
    trusted_keys: &[TrustedPublicKey],
) -> VerificationReport {
    // Determine announcement-level outcome once — independent of plan actions
    let announcement_outcome = match &announcement.signature {
        None => VerificationOutcome::Skipped {
            reason: "announcement has no signature".to_string(),
        },
        Some(sig) => match validate_signature_metadata(sig) {
            Err(err) => {
                let error = match err {
                    SignatureMetadataError::UnsupportedAlgorithm(alg) => {
                        VerificationError::UnsupportedAlgorithm(alg)
                    }
                    _ => VerificationError::MalformedSignatureBytes(err.to_string()),
                };
                VerificationOutcome::Invalid { error }
            }
            Ok(()) => {
                // Find matching trusted public key
                let trusted = trusted_keys
                    .iter()
                    .find(|k| k.key_id == sig.key_id && k.algorithm == sig.algorithm);
                match trusted {
                    None => VerificationOutcome::Invalid {
                        error: VerificationError::UnknownKeyId(sig.key_id.clone()),
                    },
                    Some(key) => {
                        // Build canonical payload
                        match build_canonical_payload(announcement) {
                            None => VerificationOutcome::Invalid {
                                error: VerificationError::CanonicalPayloadMissing,
                            },
                            Some(payload) => {
                                let payload_bytes =
                                    serde_json::to_vec(&payload).unwrap_or_default();
                                match verify_ed25519_signature(
                                    &payload_bytes,
                                    &sig.signature,
                                    &key.public_key,
                                ) {
                                    Ok(()) => VerificationOutcome::Valid,
                                    Err(error) => VerificationOutcome::Invalid { error },
                                }
                            }
                        }
                    }
                }
            }
        },
    };

    // Map all plan entries to report entries (same outcome for all)
    let entries: Vec<VerificationEntry> = plan
        .entries
        .iter()
        .map(|plan_entry| VerificationEntry {
            resource_name: plan_entry.resource_name.clone(),
            outcome: announcement_outcome.clone(),
        })
        .collect();

    VerificationReport { entries }
}

// ---------------------------------------------------------------------------
// Signature policy enforcement gate (M4.0)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignaturePolicy {
    ReportOnly,
    Strict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturePolicyViolation {
    pub message: String,
}

pub fn evaluate_signature_policy(
    report: &VerificationReport,
    policy: &SignaturePolicy,
) -> Result<(), SignaturePolicyViolation> {
    match policy {
        SignaturePolicy::ReportOnly => Ok(()),
        SignaturePolicy::Strict => {
            if report.all_valid() || report.is_empty() {
                Ok(())
            } else {
                let failed_count = report
                    .entries
                    .iter()
                    .filter(|e| e.outcome != VerificationOutcome::Valid)
                    .count();
                Err(SignaturePolicyViolation {
                    message: format!(
                        "signature policy violation (strict): {}/{} resource(s) rejected\n\
                         (report-only: no enforcement was applied to the server, \
                         this client refuses this announcement){extra}",
                        failed_count,
                        report.entries.len(),
                        extra = "",
                    ),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AnnouncedResource, PROTOCOL_VERSION, ResourceAnnouncement, ResourceAnnouncementSignature,
        ResourceRequirementLevel, TrustedKey, build_signature_verification_plan,
    };
    use ed25519_dalek::Signer;

    fn test_keypair(seed: u8) -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
        let secret = [seed; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&secret);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    fn sign_announcement(
        announcement: &mut ResourceAnnouncement,
        signing_key: &ed25519_dalek::SigningKey,
        key_id: &str,
    ) {
        // Set algorithm + key_id first (build_canonical_payload requires them)
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: key_id.to_string(),
            signature: String::new(),
        });
        let payload = build_canonical_payload(announcement).unwrap();
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature: ed25519_dalek::Signature = signing_key.sign(&payload_bytes);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: key_id.to_string(),
            signature: sig_b64,
        });
    }

    fn test_announcement() -> ResourceAnnouncement {
        ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: None,
        }
    }

    fn test_multi_resource_announcement() -> ResourceAnnouncement {
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
                    files: vec![],
                    protocol_version: PROTOCOL_VERSION,
                    requirement_level: ResourceRequirementLevel::Required,
                },
            ],
            signature: None,
        }
    }

    #[test]
    fn valid_signature_roundtrip() {
        let (sk, vk) = test_keypair(42);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "test-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, VerificationOutcome::Valid);
        assert!(report.all_valid());
    }

    #[test]
    fn wrong_key_fails() {
        let (sk, _vk) = test_keypair(1);
        let (_wrong_sk, wrong_vk) = test_keypair(99);

        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "test-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: wrong_vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 1);
        assert_eq!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid {
                error: VerificationError::InvalidSignature,
            }
        );
        assert!(!report.all_valid());
    }

    #[test]
    fn corrupted_payload_fails() {
        let (sk, vk) = test_keypair(7);

        let mut announcement = test_announcement();
        // Set algorithm + key_id so build_canonical_payload succeeds
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "test-key".to_string(),
            signature: String::new(),
        });
        let payload = build_canonical_payload(&announcement).unwrap();
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature: ed25519_dalek::Signature = sk.sign(&payload_bytes);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

        // Corrupt the announcement AFTER signing
        announcement.resources[0].version = "9.9.9".to_string();
        announcement.signature = Some(ResourceAnnouncementSignature {
            algorithm: "ed25519".to_string(),
            key_id: "test-key".to_string(),
            signature: sig_b64,
        });

        let trusted_identity = vec![TrustedKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid { .. }
        ));
        assert!(!report.all_valid());
    }

    #[test]
    fn unsupported_algorithm_in_engine() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: Some(ResourceAnnouncementSignature {
                algorithm: "rsa".to_string(),
                key_id: "test-key".to_string(),
                signature: "AAAA".to_string(),
            }),
        };

        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);

        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid {
                error: VerificationError::UnsupportedAlgorithm(_),
            }
        ));
    }

    #[test]
    fn unknown_key_id_in_engine() {
        let (sk, _vk) = test_keypair(3);

        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "unknown-key");

        let plan = build_signature_verification_plan(&announcement, &[], false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "different-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: sk.verifying_key().to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid {
                error: VerificationError::UnknownKeyId(_),
            }
        ));
    }

    #[test]
    fn skipped_entries_not_verified() {
        let announcement = test_announcement();
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);

        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Skipped { .. }
        ));
        assert!(!report.all_valid());
    }

    #[test]
    fn plan_mirroring_multi_resource() {
        let (sk, vk) = test_keypair(13);

        let mut announcement = test_multi_resource_announcement();
        sign_announcement(&mut announcement, &sk, "multi-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "multi-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "multi-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 2);
        assert_eq!(report.entries[0].resource_name, "admin");
        assert_eq!(report.entries[1].resource_name, "chat");
        assert!(report.all_valid());
    }

    #[test]
    fn report_to_text_valid() {
        let (sk, vk) = test_keypair(42);

        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "test-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        let text = report.to_text();
        assert!(text.contains("signature verification report:"));
        assert!(text.contains("[valid]"));
        assert!(text.contains("chat"));
        assert!(text.contains("all valid: true"));
    }

    #[test]
    fn report_to_text_invalid() {
        let announcement = test_announcement();
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);

        let text = report.to_text();
        assert!(text.contains("all valid: false"));
    }

    #[test]
    fn report_is_empty() {
        let announcement = ResourceAnnouncement {
            resources: vec![],
            signature: None,
        };
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert!(report.is_empty());

        let text = report.to_text();
        assert!(text.contains("(empty, no resources)"));
    }

    #[test]
    fn malformed_signature_bytes() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![],
                protocol_version: PROTOCOL_VERSION,
                requirement_level: ResourceRequirementLevel::Required,
            }],
            signature: Some(ResourceAnnouncementSignature {
                algorithm: "ed25519".to_string(),
                key_id: "test-key".to_string(),
                signature: "!!!not-valid-base64!!!".to_string(),
            }),
        };

        let trusted_identity = vec![TrustedKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vec![0u8; 32],
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid {
                error: VerificationError::MalformedSignatureBytes(_),
            }
        ));
    }

    #[test]
    fn verification_error_display() {
        let err = VerificationError::UnsupportedAlgorithm("rsa".to_string());
        assert!(err.to_string().contains("unsupported"));

        let err = VerificationError::UnknownKeyId("k".to_string());
        assert!(err.to_string().contains("unknown key ID"));

        let err = VerificationError::InvalidSignature;
        assert_eq!(err.to_string(), "signature is invalid");

        let err = VerificationError::MalformedKeyMaterial("bad".to_string());
        assert!(err.to_string().contains("malformed key material"));

        let err = VerificationError::MalformedSignatureBytes("bad".to_string());
        assert!(err.to_string().contains("malformed signature bytes"));

        let err = VerificationError::SignedPayloadMismatch("bad".to_string());
        assert!(err.to_string().contains("signed payload mismatch"));

        let err = VerificationError::CanonicalPayloadMissing;
        assert!(err.to_string().contains("cannot build canonical payload"));
    }

    #[test]
    fn empty_plan_skipped() {
        let announcement = test_announcement();
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Skipped { .. }
        ));
    }

    #[test]
    fn malformed_key_material_wrong_length() {
        let (sk, _vk) = test_keypair(5);

        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "test-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        let trusted_material = vec![TrustedPublicKey {
            key_id: "test-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vec![0u8; 31],
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid {
                error: VerificationError::MalformedKeyMaterial(_),
            }
        ));
    }

    #[test]
    fn full_flow_plan_and_engine_valid() {
        let (sk, vk) = test_keypair(10);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "my-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "my-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);

        assert_eq!(plan.entries.len(), 1);
        assert_eq!(
            plan.entries[0].action,
            crate::SignatureVerificationAction::VerifySignature
        );

        let trusted_material = vec![TrustedPublicKey {
            key_id: "my-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].outcome, VerificationOutcome::Valid);
        assert!(report.all_valid());
        assert!(!report.is_empty());
    }

    #[test]
    fn full_flow_reject_unsigned() {
        let announcement = test_announcement(); // no signature
        let plan = build_signature_verification_plan(&announcement, &[], true);

        assert_eq!(
            plan.entries[0].action,
            crate::SignatureVerificationAction::WouldRejectUnsigned
        );

        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Skipped { .. }
        ));
        assert!(!report.all_valid());
    }

    #[test]
    fn full_flow_no_trusted_keys_skipped() {
        let (sk, _vk) = test_keypair(20);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "orphan-key");

        // No trusted identity → plan says UnknownKeyId → engine has no matching TrustedPublicKey
        let plan = build_signature_verification_plan(&announcement, &[], false);
        assert_eq!(
            plan.entries[0].action,
            crate::SignatureVerificationAction::UnknownKeyId
        );

        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert_eq!(report.entries.len(), 1);
        assert!(matches!(
            report.entries[0].outcome,
            VerificationOutcome::Invalid {
                error: VerificationError::UnknownKeyId(_),
            }
        ));
    }

    #[test]
    fn report_to_text_contains_all_fields() {
        let (sk, vk) = test_keypair(30);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "key-30");

        let trusted_identity = vec![TrustedKey {
            key_id: "key-30".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);
        let trusted_material = vec![TrustedPublicKey {
            key_id: "key-30".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);

        let text = report.to_text();
        assert!(text.contains("signature verification report:"));
        assert!(text.contains("[valid]"));
        assert!(text.contains("chat"));
        assert!(text.contains("total:"));
        assert!(text.contains("all valid: true"));
    }

    #[test]
    fn report_to_text_empty() {
        let announcement = ResourceAnnouncement {
            resources: vec![],
            signature: None,
        };
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert!(report.is_empty());
        assert!(report.all_valid()); // empty → vacuously true
        assert!(report.to_text().contains("(empty, no resources)"));
    }

    // -------------------------------------------------------------------
    // Signature policy tests (M4.0)
    // -------------------------------------------------------------------

    #[test]
    fn policy_report_only_never_rejects() {
        let (sk, _vk) = test_keypair(1);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "key-99");

        // Engine has no matching key → UnknownKeyId → report not all_valid
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert!(!report.all_valid());

        // ReportOnly → always Ok
        assert!(evaluate_signature_policy(&report, &SignaturePolicy::ReportOnly).is_ok());
    }

    #[test]
    fn policy_strict_rejects_invalid() {
        let (sk, _vk) = test_keypair(2);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "key-99");

        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert!(!report.all_valid());

        let result = evaluate_signature_policy(&report, &SignaturePolicy::Strict);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("signature policy violation")
        );
    }

    #[test]
    fn policy_strict_rejects_unsigned() {
        let announcement = test_announcement(); // no signature

        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert!(!report.all_valid());

        let result = evaluate_signature_policy(&report, &SignaturePolicy::Strict);
        assert!(result.is_err());
    }

    #[test]
    fn policy_strict_allows_valid() {
        let (sk, vk) = test_keypair(3);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "good-key");

        let trusted_identity = vec![TrustedKey {
            key_id: "good-key".to_string(),
            algorithm: "ed25519".to_string(),
        }];
        let plan = build_signature_verification_plan(&announcement, &trusted_identity, false);
        let trusted_material = vec![TrustedPublicKey {
            key_id: "good-key".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vk.to_bytes().to_vec(),
        }];
        let report = execute_verification_plan(&announcement, &plan, &trusted_material);
        assert!(report.all_valid());

        assert!(evaluate_signature_policy(&report, &SignaturePolicy::Strict).is_ok());
    }

    #[test]
    fn policy_strict_allows_empty() {
        let announcement = ResourceAnnouncement {
            resources: vec![],
            signature: None,
        };
        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        assert!(report.is_empty());
        assert!(report.all_valid());

        assert!(evaluate_signature_policy(&report, &SignaturePolicy::Strict).is_ok());
    }

    #[test]
    fn policy_violation_message_contains_details() {
        let (sk, _vk) = test_keypair(4);
        let mut announcement = test_announcement();
        sign_announcement(&mut announcement, &sk, "bad-key");

        let plan = build_signature_verification_plan(&announcement, &[], false);
        let report = execute_verification_plan(&announcement, &plan, &[]);
        let err = evaluate_signature_policy(&report, &SignaturePolicy::Strict).unwrap_err();
        assert!(err.message.contains("signature policy violation"));
        assert!(err.message.contains("rejected"));
    }

    #[test]
    fn policy_violation_implements_debug_and_eq() {
        let v = SignaturePolicyViolation {
            message: "test".to_string(),
        };
        let v2 = SignaturePolicyViolation {
            message: "test".to_string(),
        };
        assert_eq!(v, v2);
        assert!(!format!("{v:?}").is_empty());
    }

    // -------------------------------------------------------------------
    // Key config validation tests (M4.1)
    // -------------------------------------------------------------------

    fn make_valid_key(key_id: &str, seed: u8) -> TrustedPublicKey {
        let secret = [seed; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&secret);
        TrustedPublicKey {
            key_id: key_id.to_string(),
            algorithm: "ed25519".to_string(),
            public_key: sk.verifying_key().to_bytes().to_vec(),
        }
    }

    #[test]
    fn key_config_valid_passes() {
        let keys = vec![make_valid_key("a", 1), make_valid_key("b", 2)];
        assert!(validate_trusted_key_config(&keys).is_ok());
    }

    #[test]
    fn key_config_empty_rejected() {
        let err = validate_trusted_key_config(&[]).unwrap_err();
        assert_eq!(err, KeyConfigError::EmptyConfig);
    }

    #[test]
    fn key_config_duplicate_key_id_rejected() {
        let keys = vec![make_valid_key("dup", 1), make_valid_key("dup", 2)];
        let err = validate_trusted_key_config(&keys).unwrap_err();
        assert!(matches!(err, KeyConfigError::DuplicateKeyId { key_id } if key_id == "dup"));
    }

    #[test]
    fn key_config_unsupported_algorithm_rejected() {
        let keys = vec![TrustedPublicKey {
            key_id: "bad".to_string(),
            algorithm: "rsa".to_string(),
            public_key: vec![0u8; 32],
        }];
        let err = validate_trusted_key_config(&keys).unwrap_err();
        assert!(matches!(
            err,
            KeyConfigError::UnsupportedAlgorithm { key_id, algorithm }
                if key_id == "bad" && algorithm == "rsa"
        ));
    }

    #[test]
    fn key_config_wrong_key_length_rejected() {
        let keys = vec![TrustedPublicKey {
            key_id: "short".to_string(),
            algorithm: "ed25519".to_string(),
            public_key: vec![0u8; 31],
        }];
        let err = validate_trusted_key_config(&keys).unwrap_err();
        assert!(matches!(
            err,
            KeyConfigError::MalformedKeyMaterial { key_id, .. }
                if key_id == "short"
        ));
    }

    #[test]
    fn key_config_error_display() {
        let e = KeyConfigError::EmptyConfig;
        assert!(e.to_string().contains("empty"));

        let e = KeyConfigError::DuplicateKeyId {
            key_id: "k".to_string(),
        };
        assert!(e.to_string().contains("duplicate"));

        let e = KeyConfigError::UnsupportedAlgorithm {
            key_id: "k".to_string(),
            algorithm: "rsa".to_string(),
        };
        assert!(e.to_string().contains("unsupported"));

        let e = KeyConfigError::MalformedKeyMaterial {
            key_id: "k".to_string(),
            algorithm: "ed25519".to_string(),
            detail: "bad".to_string(),
        };
        assert!(e.to_string().contains("invalid material"));
    }
}
