use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::fetch::CacheManifestEntry;
use crate::hash::{Hash, hash_chain_link};
use crate::state::{new_id, now_unix_ms};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationType {
    ManifestEntryRepair,
    FileRefetch,
    ManifestRebuild,
}

impl MutationType {
    pub fn label(&self) -> &str {
        match self {
            Self::ManifestEntryRepair => "manifest_entry_repair",
            Self::FileRefetch => "file_refetch",
            Self::ManifestRebuild => "manifest_rebuild",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationScope {
    pub resource_name: String,
    /// None = entire resource scope.
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub schema_version: u32,
    pub entry_id: String,
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub mutation_type: MutationType,
    pub target_scope: MutationScope,
    /// sha256 of canonical manifest JSON before this entry is applied.
    pub precondition_hash: String,
    /// sha256 of canonical manifest JSON after this entry is applied.
    pub postcondition_hash: String,
    pub operation_payload: serde_json::Value,
    /// sha256(mutation_type + canonical(target_scope) + postcondition_hash)
    pub idempotency_key: String,
    /// sha256 of canonical JSON of this entry with entry_hash set to "".
    pub entry_hash: String,
    /// entry_hash of preceding entry; "genesis" for first.
    pub prev_entry_hash: String,
    /// Reserved: M6.17 Merkle upgrade. Must be null in M6.16.
    pub chain_hash: Option<String>,
    pub lock_scope: Option<LockScope>,
    pub checkpoint_state: Option<serde_json::Value>,
    pub retry_count: u32,
    pub origin_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockScope {
    pub resource_name: String,
    /// None = entire resource.
    pub file_path: Option<String>,
}

// ── Operation payloads ────────────────────────────────────────────────────────

/// Payload for `ManifestEntryRepair` and `FileRefetch` mutations.
/// Upserts a single manifest entry (matched by resource_name + file_path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryUpsertPayload {
    pub entry: CacheManifestEntry,
}

/// Payload for `ManifestRebuild` mutations.
/// Replaces the entire manifest entry set atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestRebuildPayload {
    pub entries: Vec<CacheManifestEntry>,
}

impl JournalEntry {
    /// Compute `entry_hash` using the hash chain primitive.
    ///
    /// Encodes the entry as canonical JSON with `entry_hash` zeroed, then applies
    /// `hash_chain_link(prev_hash, canonical_json_bytes)`.
    pub fn compute_hash(entry: &Self, prev_hash: &Hash) -> Result<String> {
        let mut copy = entry.clone();
        copy.entry_hash = String::new();
        let payload = serde_json::to_vec(&copy)?;
        Ok(hash_chain_link(prev_hash, &payload).prefixed_hex())
    }

    /// Verify that this entry's `entry_hash` matches `hash_chain_link(prev_hash, payload)`.
    pub fn verify_hash(&self, prev_hash: &Hash) -> Result<()> {
        let expected = Self::compute_hash(self, prev_hash)?;
        if self.entry_hash != expected {
            anyhow::bail!(
                "entry {} hash mismatch: expected={expected} actual={}",
                self.entry_id,
                self.entry_hash
            );
        }
        Ok(())
    }

    /// Verify that this entry's `prev_entry_hash` chains correctly from the previous hash.
    ///
    /// Accepts `"genesis"` as a canonical alias for `Hash::GENESIS` — journal entries
    /// created from a genesis snapshot store `"genesis"` as the human-readable sentinel.
    pub fn verify_prev_link(&self, prev_hash: &Hash) -> Result<()> {
        let genesis_alias = *prev_hash == Hash::GENESIS && self.prev_entry_hash == "genesis";
        if genesis_alias {
            return Ok(());
        }
        let expected = prev_hash.prefixed_hex();
        if self.prev_entry_hash != expected {
            anyhow::bail!(
                "entry {} prev_entry_hash mismatch: expected={expected} actual={}",
                self.entry_id,
                self.prev_entry_hash
            );
        }
        Ok(())
    }

    pub fn compute_idempotency_key(
        mutation_type: &MutationType,
        scope: &MutationScope,
        postcondition_hash: &str,
    ) -> Result<String> {
        use sha2::Digest;
        let scope_json = serde_json::to_string(scope)?;
        let combined = format!(
            "{}{}{}",
            mutation_type.label(),
            scope_json,
            postcondition_hash
        );
        let digest = sha2::Sha256::digest(combined.as_bytes());
        Ok(format!("sha256:{:x}", digest))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sequence: u64,
        mutation_type: MutationType,
        target_scope: MutationScope,
        precondition_hash: String,
        postcondition_hash: String,
        operation_payload: serde_json::Value,
        prev_entry_hash: String,
        lock_scope: Option<LockScope>,
        checkpoint_state: Option<serde_json::Value>,
        origin_context: Option<String>,
    ) -> Result<Self> {
        let idempotency_key =
            Self::compute_idempotency_key(&mutation_type, &target_scope, &postcondition_hash)?;
        let mut entry = Self {
            schema_version: 1,
            entry_id: new_id(),
            sequence,
            timestamp_unix_ms: now_unix_ms(),
            mutation_type,
            target_scope,
            precondition_hash,
            postcondition_hash,
            operation_payload,
            idempotency_key,
            entry_hash: String::new(),
            prev_entry_hash,
            chain_hash: None, // reserved: M6.17
            lock_scope,
            checkpoint_state,
            retry_count: 0,
            origin_context,
        };
        let prev_hash = if entry.prev_entry_hash == "genesis" {
            Hash::GENESIS
        } else {
            Hash::from_prefixed_hex(&entry.prev_entry_hash).ok_or_else(|| {
                anyhow::anyhow!("invalid prev_entry_hash: {}", entry.prev_entry_hash)
            })?
        };
        entry.entry_hash = Self::compute_hash(&entry, &prev_hash)?;
        Ok(entry)
    }
}

/// Append-only journal writer.
pub struct JournalWriter {
    file: tokio::fs::File,
}

impl JournalWriter {
    pub async fn open(path: &std::path::Path) -> Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .with_context(|| format!("failed to open journal for append: {}", path.display()))?;
        Ok(Self { file })
    }

    pub async fn append(&mut self, entry: &JournalEntry) -> Result<()> {
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .await
            .context("failed to append journal entry")?;
        self.file.flush().await.context("failed to flush journal")?;
        Ok(())
    }
}

/// Sequential journal reader from a given sequence boundary.
pub struct JournalReader {
    path: std::path::PathBuf,
}

impl JournalReader {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }

    /// Verify the full hash chain from genesis to the last entry.
    ///
    /// Checks:
    /// 1. Each entry's `prev_entry_hash` matches the previous entry's `entry_hash`.
    /// 2. Each entry's `entry_hash` matches `hash_chain_link(prev_hash, payload)`.
    ///
    /// Returns the number of verified entries on success.
    pub async fn verify(&self) -> Result<u64> {
        let entries = self.read_after(0).await?;
        let mut prev = Hash::GENESIS;
        for entry in &entries {
            entry.verify_prev_link(&prev)?;
            entry.verify_hash(&prev)?;
            prev = Hash::from_prefixed_hex(&entry.entry_hash).ok_or_else(|| {
                anyhow::anyhow!(
                    "entry {} has invalid entry_hash format: {}",
                    entry.entry_id,
                    entry.entry_hash
                )
            })?;
        }
        Ok(entries.len() as u64)
    }

    /// Read all entries with sequence > after_sequence, in order.
    pub async fn read_after(&self, after_sequence: u64) -> Result<Vec<JournalEntry>> {
        let file = match tokio::fs::File::open(&self.path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).context("failed to open journal for reading"),
        };
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut entries = Vec::new();
        while let Some(line) = lines.next_line().await.context("journal read error")? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            let entry: JournalEntry =
                serde_json::from_str(&line).context("malformed journal entry")?;
            if entry.sequence > after_sequence {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|e| e.sequence);
        Ok(entries)
    }
}
