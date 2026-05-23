use base64::Engine as _;
use ed25519_dalek::Signer;
use protocol::signature_engine::{
    evaluate_signature_policy, execute_verification_plan, TrustedPublicKey, SignaturePolicy,
};
use protocol::{
    build_canonical_payload, build_signature_verification_plan, AnnouncedResource,
    ResourceAnnouncement, ResourceAnnouncementSignature, ResourceRequirementLevel, TrustedKey,
    PROTOCOL_VERSION,
};

fn test_signing_key(seed: u8) -> ed25519_dalek::SigningKey {
    let secret = [seed; 32];
    ed25519_dalek::SigningKey::from_bytes(&secret)
}

fn test_trusted_key(key_id: &str, seed: u8) -> TrustedPublicKey {
    let sk = test_signing_key(seed);
    TrustedPublicKey {
        key_id: key_id.to_string(),
        algorithm: "ed25519".to_string(),
        public_key: sk.verifying_key().to_bytes().to_vec(),
    }
}

fn sign_announcement(
    announcement: &mut ResourceAnnouncement,
    signing_key: &ed25519_dalek::SigningKey,
    key_id: &str,
) {
    announcement.signature = Some(ResourceAnnouncementSignature {
        algorithm: "ed25519".to_string(),
        key_id: key_id.to_string(),
        signature: String::new(),
    });
    let payload = build_canonical_payload(announcement).unwrap();
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    let sig: ed25519_dalek::Signature = signing_key.sign(&payload_bytes);
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
    announcement.signature = Some(ResourceAnnouncementSignature {
        algorithm: "ed25519".to_string(),
        key_id: key_id.to_string(),
        signature: sig_b64,
    });
}

fn unsigned_announcement() -> ResourceAnnouncement {
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

fn signed_announcement() -> (ResourceAnnouncement, Vec<TrustedPublicKey>) {
    let mut announcement = unsigned_announcement();
    let sk = test_signing_key(42);
    let key_id = "test-key";
    sign_announcement(&mut announcement, &sk, key_id);
    let trusted = vec![test_trusted_key(key_id, 42)];
    (announcement, trusted)
}

#[test]
fn e2e_signed_announcement_round_trip_verify() {
    let (announcement, trusted) = signed_announcement();

    let json = serde_json::to_string(&announcement).unwrap();
    let deserialized: ResourceAnnouncement = serde_json::from_str(&json).unwrap();

    let key_identities: Vec<TrustedKey> = trusted
        .iter()
        .map(|k| TrustedKey {
            key_id: k.key_id.clone(),
            algorithm: k.algorithm.clone(),
        })
        .collect();

    let plan = build_signature_verification_plan(&deserialized, &key_identities, false);
    assert!(!plan.entries.is_empty(), "plan should have entries");
    assert_eq!(plan.entries[0].action, protocol::SignatureVerificationAction::VerifySignature);

    let report = execute_verification_plan(&deserialized, &plan, &trusted);
    assert!(report.all_valid(), "report should be all_valid: {:?}", report);

    assert!(evaluate_signature_policy(&report, &SignaturePolicy::ReportOnly).is_ok());
    assert!(evaluate_signature_policy(&report, &SignaturePolicy::Strict).is_ok());
}

#[test]
fn e2e_tampered_announcement_rejected_under_strict() {
    let (announcement, trusted) = signed_announcement();

    let json = serde_json::to_string(&announcement).unwrap();
    let tampered_json = json.replace("\"chat\"", "\"hacked\"");
    let deserialized: ResourceAnnouncement = serde_json::from_str(&tampered_json).unwrap();

    let key_identities: Vec<TrustedKey> = trusted
        .iter()
        .map(|k| TrustedKey {
            key_id: k.key_id.clone(),
            algorithm: k.algorithm.clone(),
        })
        .collect();

    let plan = build_signature_verification_plan(&deserialized, &key_identities, false);
    let report = execute_verification_plan(&deserialized, &plan, &trusted);
    assert!(!report.all_valid(), "tampered announcement should not be valid");

    assert!(evaluate_signature_policy(&report, &SignaturePolicy::ReportOnly).is_ok());
    assert!(evaluate_signature_policy(&report, &SignaturePolicy::Strict).is_err());
}

#[test]
fn e2e_unsigned_announcement_strict_rejects() {
    let announcement = unsigned_announcement();

    let json = serde_json::to_string(&announcement).unwrap();
    let deserialized: ResourceAnnouncement = serde_json::from_str(&json).unwrap();

    let plan = build_signature_verification_plan(&deserialized, &[], false);
    let report = execute_verification_plan(&deserialized, &plan, &[]);
    assert!(!report.all_valid(), "unsigned should not be all_valid");

    assert!(evaluate_signature_policy(&report, &SignaturePolicy::ReportOnly).is_ok());
    assert!(evaluate_signature_policy(&report, &SignaturePolicy::Strict).is_err());
}

#[test]
fn e2e_canonical_payload_stable_across_round_trip() {
    let (announcement, _trusted) = signed_announcement();

    let original_payload = build_canonical_payload(&announcement).unwrap();
    let original_bytes = serde_json::to_vec(&original_payload).unwrap();

    let json = serde_json::to_string(&announcement).unwrap();
    let deserialized: ResourceAnnouncement = serde_json::from_str(&json).unwrap();
    let round_trip_payload = build_canonical_payload(&deserialized).unwrap();
    let round_trip_bytes = serde_json::to_vec(&round_trip_payload).unwrap();

    assert_eq!(
        original_bytes, round_trip_bytes,
        "canonical payload must be identical after JSON round-trip"
    );
}
