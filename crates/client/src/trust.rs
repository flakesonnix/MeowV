//! Typestate trust machine for ResourceAnnouncement.
//!
//! All paths to execute_cache_repair must flow through:
//!
//!   Unverified → Parsed → PolicyChecked → Trusted
//!
//! Passing an announcement in any other state is a compile error.
//! No implicit trust transitions. No "existence == trust" shortcuts.

use std::marker::PhantomData;

use anyhow::Result;
use protocol::{
    ResourceAnnouncement, TrustedKey, build_signature_verification_plan,
    signature_engine::{
        SignaturePolicy, TrustedPublicKey, evaluate_signature_policy, execute_verification_plan,
    },
};

// ── State markers ─────────────────────────────────────────────────────────────

/// Raw bytes received — nothing validated.
pub struct Unverified;
/// JSON parsed, structural schema verified.
pub struct Parsed;
/// Policy pre-conditions checked (key presence, non-empty key set for Strict).
pub struct PolicyChecked;
/// Signature verified (or policy explicitly does not require it). Safe for mutation use.
pub struct Trusted;

// ── Wrapper ───────────────────────────────────────────────────────────────────

pub struct Announcement<S> {
    inner: ResourceAnnouncement,
    _state: PhantomData<S>,
}

/// Reason a trust transition was rejected.
#[derive(Debug, Clone)]
pub struct TrustRejected {
    pub reason: String,
}

impl std::fmt::Display for TrustRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "trust rejected: {}", self.reason)
    }
}

impl std::error::Error for TrustRejected {}

// ── Unverified → Parsed ───────────────────────────────────────────────────────

impl Announcement<Unverified> {
    /// Entry point. Parse raw JSON into a structurally valid announcement.
    /// On success advances to `Parsed`. Fails on malformed JSON or schema errors.
    pub fn from_raw(raw: &str) -> Result<Announcement<Parsed>> {
        let inner: ResourceAnnouncement = serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("failed to parse announcement: {e}"))?;
        Ok(Announcement {
            inner,
            _state: PhantomData,
        })
    }

    /// Wrap a programmatically constructed `ResourceAnnouncement`, advancing directly to
    /// `Parsed`. Valid because Rust's type system guarantees structural correctness of
    /// typed values — JSON parsing is unnecessary for in-process construction.
    ///
    /// Use for tests and in-process announcement construction.
    /// For untrusted external input, always use `from_raw`.
    pub fn from_constructed(ann: ResourceAnnouncement) -> Announcement<Parsed> {
        Announcement {
            inner: ann,
            _state: PhantomData,
        }
    }
}

// ── Parsed → PolicyChecked ────────────────────────────────────────────────────

impl Announcement<Parsed> {
    /// Check policy pre-conditions (key presence and non-empty key set for Strict).
    /// Does not perform cryptographic verification.
    pub fn check_policy(
        self,
        policy: &SignaturePolicy,
        keys: Option<&[TrustedPublicKey]>,
    ) -> Result<Announcement<PolicyChecked>, TrustRejected> {
        match (policy, keys) {
            (SignaturePolicy::Strict, None) => Err(TrustRejected {
                reason: "--signature-policy strict requires --trusted-keys <path>".to_string(),
            }),
            (SignaturePolicy::Strict, Some([])) => Err(TrustRejected {
                reason: "--signature-policy strict requires at least one trusted key".to_string(),
            }),
            _ => Ok(Announcement {
                inner: self.inner,
                _state: PhantomData,
            }),
        }
    }

    /// Test helper to advance to PolicyChecked without key checks.
    /// Not for production trust resolution paths.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn skip_policy_check(self) -> Announcement<PolicyChecked> {
        Announcement {
            inner: self.inner,
            _state: PhantomData,
        }
    }
}

// ── PolicyChecked → Trusted ───────────────────────────────────────────────────

impl Announcement<PolicyChecked> {
    /// Advance to Trusted without cryptographic verification.
    /// Test helper for constructing trusted announcements without external key
    /// material. Not for production trust resolution paths.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn trust_relaxed_for_testing(self) -> Announcement<Trusted> {
        Announcement {
            inner: self.inner,
            _state: PhantomData,
        }
    }

    /// Resolve to Trusted by running the full verification pipeline.
    ///
    /// - `Strict`:     signature must be present and valid against provided keys.
    /// - `ReportOnly`: verification runs but non-valid outcomes do not reject.
    pub fn resolve_trust(
        self,
        policy: &SignaturePolicy,
        keys: Option<&[TrustedPublicKey]>,
    ) -> Result<Announcement<Trusted>, TrustRejected> {
        match policy {
            SignaturePolicy::ReportOnly => {
                // Report-only: run verification but never reject.
                if let Some(keys) = keys {
                    let trusted: Vec<TrustedKey> = keys
                        .iter()
                        .map(|k| TrustedKey {
                            key_id: k.key_id.clone(),
                            algorithm: k.algorithm.clone(),
                        })
                        .collect();
                    let plan = build_signature_verification_plan(&self.inner, &trusted, false);
                    let report = execute_verification_plan(&self.inner, &plan, keys);
                    tracing::debug!(
                        valid = report.all_valid(),
                        "report-only signature verification complete"
                    );
                }
                Ok(Announcement {
                    inner: self.inner,
                    _state: PhantomData,
                })
            }
            SignaturePolicy::Strict => {
                let keys = keys.ok_or_else(|| TrustRejected {
                    reason: "strict policy requires trusted keys at resolve_trust".to_string(),
                })?;
                let trusted: Vec<TrustedKey> = keys
                    .iter()
                    .map(|k| TrustedKey {
                        key_id: k.key_id.clone(),
                        algorithm: k.algorithm.clone(),
                    })
                    .collect();
                let reject_unsigned = true;
                let plan =
                    build_signature_verification_plan(&self.inner, &trusted, reject_unsigned);
                let report = execute_verification_plan(&self.inner, &plan, keys);
                evaluate_signature_policy(&report, policy)
                    .map_err(|e| TrustRejected { reason: e.message })?;
                Ok(Announcement {
                    inner: self.inner,
                    _state: PhantomData,
                })
            }
        }
    }
}

// ── Trusted ───────────────────────────────────────────────────────────────────

impl Announcement<Trusted> {
    /// Access the verified announcement. Only callable in Trusted state.
    pub fn as_announcement(&self) -> &ResourceAnnouncement {
        &self.inner
    }

    /// Consume and return the inner announcement.
    #[doc(hidden)]
    pub fn into_announcement(self) -> ResourceAnnouncement {
        self.inner
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convenience function for the standard CLI trust resolution flow.
///
/// Runs: parse → check_policy → resolve_trust
/// Returns `Announcement<Trusted>` or an error describing the rejection.
pub fn resolve_announcement_trust(
    raw: &str,
    policy: &SignaturePolicy,
    keys: Option<&[TrustedPublicKey]>,
) -> Result<Announcement<Trusted>> {
    let parsed = Announcement::<Unverified>::from_raw(raw)?;
    let policy_checked = parsed
        .check_policy(policy, keys)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let trusted = policy_checked
        .resolve_trust(policy, keys)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(trusted)
}
