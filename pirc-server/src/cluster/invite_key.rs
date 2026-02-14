use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use pirc_common::{InviteKeyError, ServerId};

const INVITE_KEYS_FILENAME: &str = "invite_keys.json";

/// Default invite key validity period (24 hours).
const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// A cryptographic invite key token (32 random bytes, base64url-encoded).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InviteKey(String);

impl InviteKey {
    /// Generate a new random invite key.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Create an `InviteKey` from an existing token string.
    ///
    /// This does not validate the token format — use it for deserialization
    /// or testing only.
    pub fn from_token(token: String) -> Self {
        Self(token)
    }

    /// Return the base64url-encoded token string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InviteKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Metadata record for a stored invite key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteKeyRecord {
    pub key: InviteKey,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    pub single_use: bool,
    pub used: bool,
    pub revoked: bool,
    pub creator: ServerId,
}

impl InviteKeyRecord {
    /// Check whether this key has expired relative to `now`.
    pub fn is_expired(&self, now: SystemTime) -> bool {
        now >= self.expires_at
    }
}

/// In-memory store for invite keys.
#[derive(Debug)]
pub struct InviteKeyStore {
    keys: HashMap<String, InviteKeyRecord>,
}

impl InviteKeyStore {
    /// Create an empty invite key store.
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    /// Generate and store a new invite key.
    ///
    /// Returns the generated key. The key expires after `ttl` (defaults to 24 h)
    /// and is single-use unless `single_use` is set to `false`.
    pub fn create(
        &mut self,
        creator: ServerId,
        ttl: Option<Duration>,
        single_use: bool,
    ) -> InviteKey {
        let key = InviteKey::generate();
        let now = SystemTime::now();
        let ttl = ttl.unwrap_or(DEFAULT_TTL);

        let record = InviteKeyRecord {
            key: key.clone(),
            created_at: now,
            expires_at: now + ttl,
            single_use,
            used: false,
            revoked: false,
            creator,
        };
        self.keys.insert(key.as_str().to_owned(), record);
        key
    }

    /// Validate and consume an invite key.
    ///
    /// On success, marks single-use keys as used and returns the record.
    /// On failure, returns an appropriate [`InviteKeyError`].
    pub fn validate(&mut self, token: &str) -> Result<&InviteKeyRecord, InviteKeyError> {
        // Look up key — must exist
        let record = self.keys.get_mut(token).ok_or(InviteKeyError::NotFound)?;

        // Check revoked before other states
        if record.revoked {
            return Err(InviteKeyError::Revoked);
        }

        // Check expiry
        if record.is_expired(SystemTime::now()) {
            return Err(InviteKeyError::Expired);
        }

        // Check single-use enforcement
        if record.single_use && record.used {
            return Err(InviteKeyError::AlreadyUsed);
        }

        // Mark as used
        record.used = true;

        // Return immutable ref (reborrow from map)
        let record = &self.keys[token];
        Ok(record)
    }

    /// Revoke an invite key so it can no longer be used.
    ///
    /// Returns `true` if the key existed and was revoked, `false` if not found.
    pub fn revoke(&mut self, token: &str) -> bool {
        if let Some(record) = self.keys.get_mut(token) {
            record.revoked = true;
            true
        } else {
            false
        }
    }

    /// List all stored invite key records.
    pub fn list(&self) -> Vec<&InviteKeyRecord> {
        self.keys.values().collect()
    }

    /// Get a specific key record by token without consuming it.
    pub fn get(&self, token: &str) -> Option<&InviteKeyRecord> {
        self.keys.get(token)
    }

    /// Remove expired and used single-use keys from the store.
    pub fn purge_expired(&mut self) {
        let now = SystemTime::now();
        self.keys.retain(|_, record| {
            let expired = record.is_expired(now);
            let consumed = record.single_use && record.used;
            !expired && !consumed
        });
    }

    /// Returns the file path for persisted invite keys within the given data directory.
    pub fn file_path(data_dir: &Path) -> PathBuf {
        data_dir.join(INVITE_KEYS_FILENAME)
    }

    /// Save all invite keys to `data_dir/invite_keys.json`.
    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        let path = Self::file_path(data_dir);
        let records: Vec<&InviteKeyRecord> = self.keys.values().collect();
        let json = serde_json::to_string_pretty(&records)
            .map_err(io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load invite keys from `data_dir/invite_keys.json`.
    ///
    /// Returns a new `InviteKeyStore` populated with the persisted keys.
    /// Returns an empty store if the file does not exist.
    pub fn load(data_dir: &Path) -> io::Result<Self> {
        let path = Self::file_path(data_dir);
        if !path.exists() {
            return Ok(Self::new());
        }
        let json = std::fs::read_to_string(path)?;
        let records: Vec<InviteKeyRecord> = serde_json::from_str(&json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut keys = HashMap::new();
        for record in records {
            keys.insert(record.key.as_str().to_owned(), record);
        }
        Ok(Self { keys })
    }
}

impl Default for InviteKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- InviteKey generation ----

    #[test]
    fn generate_produces_unique_keys() {
        let a = InviteKey::generate();
        let b = InviteKey::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn generated_key_is_base64url_encoded() {
        let key = InviteKey::generate();
        let token = key.as_str();
        // 32 bytes -> 43 base64url chars (no padding)
        assert_eq!(token.len(), 43);
        // Should decode back to 32 bytes
        let decoded = URL_SAFE_NO_PAD.decode(token).unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn from_token_roundtrip() {
        let key = InviteKey::generate();
        let reconstructed = InviteKey::from_token(key.as_str().to_owned());
        assert_eq!(key, reconstructed);
    }

    #[test]
    fn display_matches_as_str() {
        let key = InviteKey::generate();
        assert_eq!(key.to_string(), key.as_str());
    }

    #[test]
    fn serde_roundtrip() {
        let key = InviteKey::generate();
        let json = serde_json::to_string(&key).unwrap();
        let deserialized: InviteKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, deserialized);
    }

    // ---- InviteKeyStore::create ----

    #[test]
    fn create_stores_key() {
        let mut store = InviteKeyStore::new();
        let creator = ServerId::new(1);
        let key = store.create(creator, None, true);
        assert!(store.get(key.as_str()).is_some());
    }

    #[test]
    fn create_with_custom_ttl() {
        let mut store = InviteKeyStore::new();
        let creator = ServerId::new(1);
        let ttl = Duration::from_secs(60);
        let key = store.create(creator, Some(ttl), true);
        let record = store.get(key.as_str()).unwrap();
        let diff = record
            .expires_at
            .duration_since(record.created_at)
            .unwrap();
        // Allow small timing variance
        assert!(diff.as_secs() <= 60);
        assert!(diff.as_secs() >= 59);
    }

    #[test]
    fn create_sets_metadata() {
        let mut store = InviteKeyStore::new();
        let creator = ServerId::new(42);
        let key = store.create(creator, None, true);
        let record = store.get(key.as_str()).unwrap();
        assert_eq!(record.creator, creator);
        assert!(record.single_use);
        assert!(!record.used);
        assert!(!record.revoked);
    }

    #[test]
    fn create_multi_use_key() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, false);
        let record = store.get(key.as_str()).unwrap();
        assert!(!record.single_use);
    }

    // ---- InviteKeyStore::validate ----

    #[test]
    fn validate_succeeds_for_fresh_key() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        let result = store.validate(key.as_str());
        assert!(result.is_ok());
    }

    #[test]
    fn validate_marks_key_as_used() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        store.validate(key.as_str()).unwrap();
        let record = store.get(key.as_str()).unwrap();
        assert!(record.used);
    }

    #[test]
    fn validate_rejects_unknown_key() {
        let mut store = InviteKeyStore::new();
        let result = store.validate("nonexistent-token");
        assert_eq!(result.unwrap_err(), InviteKeyError::NotFound);
    }

    #[test]
    fn validate_rejects_used_single_use_key() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        store.validate(key.as_str()).unwrap();
        let result = store.validate(key.as_str());
        assert_eq!(result.unwrap_err(), InviteKeyError::AlreadyUsed);
    }

    #[test]
    fn validate_allows_multi_use_key_reuse() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, false);
        store.validate(key.as_str()).unwrap();
        // Second use should succeed
        let result = store.validate(key.as_str());
        assert!(result.is_ok());
    }

    #[test]
    fn validate_rejects_expired_key() {
        let mut store = InviteKeyStore::new();
        // Create a key that already expired (zero TTL)
        let key = InviteKey::generate();
        let now = SystemTime::now();
        let record = InviteKeyRecord {
            key: key.clone(),
            created_at: now - Duration::from_secs(120),
            expires_at: now - Duration::from_secs(60),
            single_use: true,
            used: false,
            revoked: false,
            creator: ServerId::new(1),
        };
        store.keys.insert(key.as_str().to_owned(), record);

        let result = store.validate(key.as_str());
        assert_eq!(result.unwrap_err(), InviteKeyError::Expired);
    }

    #[test]
    fn validate_rejects_revoked_key() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        store.revoke(key.as_str());
        let result = store.validate(key.as_str());
        assert_eq!(result.unwrap_err(), InviteKeyError::Revoked);
    }

    // ---- InviteKeyStore::revoke ----

    #[test]
    fn revoke_existing_key() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        assert!(store.revoke(key.as_str()));
        let record = store.get(key.as_str()).unwrap();
        assert!(record.revoked);
    }

    #[test]
    fn revoke_nonexistent_key() {
        let mut store = InviteKeyStore::new();
        assert!(!store.revoke("does-not-exist"));
    }

    // ---- InviteKeyStore::list ----

    #[test]
    fn list_empty_store() {
        let store = InviteKeyStore::new();
        assert!(store.list().is_empty());
    }

    #[test]
    fn list_returns_all_keys() {
        let mut store = InviteKeyStore::new();
        store.create(ServerId::new(1), None, true);
        store.create(ServerId::new(1), None, true);
        store.create(ServerId::new(2), None, false);
        assert_eq!(store.list().len(), 3);
    }

    // ---- InviteKeyStore::purge_expired ----

    #[test]
    fn purge_removes_expired_keys() {
        let mut store = InviteKeyStore::new();
        // Insert an already-expired key
        let key = InviteKey::generate();
        let now = SystemTime::now();
        let record = InviteKeyRecord {
            key: key.clone(),
            created_at: now - Duration::from_secs(120),
            expires_at: now - Duration::from_secs(60),
            single_use: true,
            used: false,
            revoked: false,
            creator: ServerId::new(1),
        };
        store.keys.insert(key.as_str().to_owned(), record);

        // Insert a valid key
        let valid_key = store.create(ServerId::new(1), None, true);

        store.purge_expired();

        assert!(store.get(key.as_str()).is_none());
        assert!(store.get(valid_key.as_str()).is_some());
    }

    #[test]
    fn purge_removes_used_single_use_keys() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        store.validate(key.as_str()).unwrap();

        store.purge_expired();

        assert!(store.get(key.as_str()).is_none());
    }

    #[test]
    fn purge_keeps_used_multi_use_keys() {
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, false);
        store.validate(key.as_str()).unwrap();

        store.purge_expired();

        assert!(store.get(key.as_str()).is_some());
    }

    // ---- InviteKeyRecord::is_expired ----

    #[test]
    fn record_not_expired_before_deadline() {
        let now = SystemTime::now();
        let record = InviteKeyRecord {
            key: InviteKey::from_token("test".into()),
            created_at: now,
            expires_at: now + Duration::from_secs(3600),
            single_use: true,
            used: false,
            revoked: false,
            creator: ServerId::new(1),
        };
        assert!(!record.is_expired(now));
    }

    #[test]
    fn record_expired_after_deadline() {
        let now = SystemTime::now();
        let record = InviteKeyRecord {
            key: InviteKey::from_token("test".into()),
            created_at: now - Duration::from_secs(7200),
            expires_at: now - Duration::from_secs(3600),
            single_use: true,
            used: false,
            revoked: false,
            creator: ServerId::new(1),
        };
        assert!(record.is_expired(now));
    }

    #[test]
    fn record_expired_at_exact_deadline() {
        let now = SystemTime::now();
        let record = InviteKeyRecord {
            key: InviteKey::from_token("test".into()),
            created_at: now - Duration::from_secs(3600),
            expires_at: now,
            single_use: true,
            used: false,
            revoked: false,
            creator: ServerId::new(1),
        };
        assert!(record.is_expired(now));
    }

    // ---- InviteKeyRecord serde ----

    #[test]
    fn record_serde_roundtrip() {
        let now = SystemTime::now();
        let record = InviteKeyRecord {
            key: InviteKey::generate(),
            created_at: now,
            expires_at: now + Duration::from_secs(3600),
            single_use: true,
            used: false,
            revoked: false,
            creator: ServerId::new(5),
        };
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: InviteKeyRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record.key, deserialized.key);
        assert_eq!(record.single_use, deserialized.single_use);
        assert_eq!(record.creator, deserialized.creator);
    }

    // ---- Default trait ----

    #[test]
    fn default_creates_empty_store() {
        let store = InviteKeyStore::default();
        assert!(store.list().is_empty());
    }

    // ---- Persistence: save / load ----

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = InviteKeyStore::new();
        let creator = ServerId::new(1);
        let key1 = store.create(creator, None, true);
        let key2 = store.create(creator, None, false);

        store.save(dir.path()).expect("save");

        let loaded = InviteKeyStore::load(dir.path()).expect("load");
        assert_eq!(loaded.list().len(), 2);
        assert!(loaded.get(key1.as_str()).is_some());
        assert!(loaded.get(key2.as_str()).is_some());

        let r1 = loaded.get(key1.as_str()).unwrap();
        assert!(r1.single_use);
        assert!(!r1.used);

        let r2 = loaded.get(key2.as_str()).unwrap();
        assert!(!r2.single_use);
    }

    #[test]
    fn load_returns_empty_store_when_no_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let loaded = InviteKeyStore::load(dir.path()).expect("load");
        assert!(loaded.list().is_empty());
    }

    #[test]
    fn save_preserves_used_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = InviteKeyStore::new();
        let key = store.create(ServerId::new(1), None, true);
        store.validate(key.as_str()).unwrap();

        store.save(dir.path()).expect("save");

        let loaded = InviteKeyStore::load(dir.path()).expect("load");
        let record = loaded.get(key.as_str()).unwrap();
        assert!(record.used);
    }

    #[test]
    fn file_path_uses_correct_filename() {
        let dir = std::path::Path::new("/data/raft");
        assert_eq!(
            InviteKeyStore::file_path(dir),
            std::path::PathBuf::from("/data/raft/invite_keys.json")
        );
    }
}
