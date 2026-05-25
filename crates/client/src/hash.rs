//! Journal hash primitive: SHA-256 chain link with canonical `sha256:hex` encoding.
//!
//! Only the `hash_chain_link` function and `Hash` type are the canonical primitives.
//! All journal chain integrity derives from this module.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// SHA-256 hash with canonical `sha256:hex` encoding.
///
/// # Invariants
///
/// - Wraps exactly 32 raw bytes.
/// - Display/Serialize always produces `sha256:[0-9a-f]{64}`.
/// - `FromStr` rejects any string not matching that format.
/// - `GENESIS` is the all-zero hash (first entry in a journal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash([u8; 32]);

impl Hash {
    /// Genesis hash all-zero — used as prev_hash for first journal entry.
    pub const GENESIS: Hash = Hash([0u8; 32]);

    /// Construct from raw 32 bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw 32 bytes (e.g. for feeding into SHA-256).
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Canonical `sha256:hex` string.
    pub fn prefixed_hex(&self) -> String {
        self.to_string()
    }

    /// Parse from `sha256:hex` string. Returns `None` on invalid format.
    pub fn from_prefixed_hex(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:")?;
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Hash {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s
            .strip_prefix("sha256:")
            .ok_or_else(|| format!("Hash must start with 'sha256:': got '{s}'"))?;
        if hex.len() != 64 {
            return Err(format!(
                "Hash hex must be exactly 64 characters, got {}",
                hex.len()
            ));
        }
        let mut arr = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let Ok(hex_str) = std::str::from_utf8(chunk) else {
                return Err("non-utf8 in hash hex".to_string());
            };
            arr[i] = u8::from_str_radix(hex_str, 16)
                .map_err(|_| format!("invalid hex at position {}: '{hex_str}'", i * 2))?;
        }
        Ok(Self(arr))
    }
}

impl Serialize for Hash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Deterministic SHA-256 chain link.
///
/// Computes: `SHA-256(prev_hash_raw_bytes || payload)`.
///
/// # Invariants
///
/// - Pure: no IO, no state, no randomness.
/// - Total: always returns a valid `Hash`.
/// - Never changes: this function is the journal's cryptographic identity.
pub fn hash_chain_link(prev_hash: &Hash, payload: &[u8]) -> Hash {
    let mut hasher = sha2::Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(payload);
    Hash::from_bytes(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_is_all_zeros() {
        let raw = Hash::GENESIS.as_bytes();
        assert_eq!(raw.len(), 32);
        assert!(raw.iter().all(|&b| b == 0));
    }

    #[test]
    fn display_and_parse_roundtrip() {
        let h = Hash::from_bytes([42u8; 32]);
        let s = h.to_string();
        assert!(s.starts_with("sha256:"));
        assert_eq!(s.len(), 71); // "sha256:" + 64 hex chars

        let parsed: Hash = s.parse().unwrap();
        assert_eq!(parsed, h);
    }

    #[test]
    fn display_matches_expected_format() {
        let h = Hash::from_bytes([0xab; 32]);
        let s = h.to_string();
        // Every byte is 0xab so hex is "ab" repeated 32 times
        let expected_suffix = "ab".repeat(32);
        assert_eq!(s, format!("sha256:{expected_suffix}"));
        assert_eq!(s.len(), 71);
    }

    #[test]
    fn reject_missing_prefix() {
        let err = "deadbeef".parse::<Hash>().unwrap_err();
        assert!(err.contains("sha256:"));
    }

    #[test]
    fn reject_wrong_hex_length() {
        let err = "sha256:abc".parse::<Hash>().unwrap_err();
        assert!(err.contains("64"));
    }

    #[test]
    fn reject_invalid_hex_chars() {
        let input = format!("sha256:{}", "zz".repeat(32));
        let err = input.parse::<Hash>().unwrap_err();
        assert!(err.contains("invalid hex"));
    }

    #[test]
    fn serde_roundtrip() {
        let h = Hash::from_bytes([1u8; 32]);
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, format!("\"sha256:{}\"", "01".repeat(32)));

        let deserialized: Hash = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, h);
    }

    #[test]
    fn hash_chain_link_is_deterministic() {
        let prev = Hash::GENESIS;
        let payload = b"hello world";
        let h1 = hash_chain_link(&prev, payload);
        let h2 = hash_chain_link(&prev, payload);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_chain_link_changes_with_prev() {
        let prev_a = Hash::GENESIS;
        let prev_b = Hash::from_bytes([1u8; 32]);
        let payload = b"hello world";
        let ha = hash_chain_link(&prev_a, payload);
        let hb = hash_chain_link(&prev_b, payload);
        assert_ne!(ha, hb);
    }

    #[test]
    fn hash_chain_link_changes_with_payload() {
        let prev = Hash::GENESIS;
        let h1 = hash_chain_link(&prev, b"hello");
        let h2 = hash_chain_link(&prev, b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_chain_link_produces_valid_hash_format() {
        let prev = Hash::GENESIS;
        let h = hash_chain_link(&prev, b"test");
        let s = h.to_string();
        assert!(s.starts_with("sha256:"));
        assert_eq!(s.len(), 71);
        // Should be parseable back
        let _: Hash = s.parse().unwrap();
    }
}
