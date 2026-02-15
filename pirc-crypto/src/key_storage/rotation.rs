//! Key rotation methods for [`KeyStore`].
//!
//! Provides signed pre-key and KEM pre-key rotation, cleanup of expired
//! retained keys, lookup by ID, and one-time pre-key replenishment.

use crate::error::Result;
use crate::prekey::{
    KemPreKey, KemPreKeyPublic, OneTimePreKeyPublic, SignedPreKey, SignedPreKeyPublic,
};

use super::KeyStore;

impl KeyStore {
    // ── Rotation methods ─────────────────────────────────────────────

    /// Rotate the signed pre-key.
    ///
    /// Generates a new signed pre-key with the next available ID, signs
    /// it with the identity key, and sets it as the current (first)
    /// signed pre-key. The previous current key is retained for the
    /// grace period so in-flight key exchanges can still complete.
    ///
    /// Returns the public portion of the new signed pre-key (to publish
    /// to the server).
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`](crate::error::CryptoError) if key generation or signing fails.
    pub fn rotate_signed_pre_key(&mut self, timestamp: u64) -> Result<SignedPreKeyPublic> {
        let id = self.bundle.next_pre_key_id;
        self.bundle.next_pre_key_id += 1;

        let spk = SignedPreKey::generate(id, &self.bundle.identity, timestamp)?;
        let public = spk.to_public();

        // Insert at front; old keys shift to retained positions
        self.bundle.signed_pre_keys.insert(0, spk);

        Ok(public)
    }

    /// Rotate the KEM pre-key.
    ///
    /// Generates a new KEM pre-key with the next available ID, signs it
    /// with the identity key, and sets it as the current (first) KEM
    /// pre-key. The previous current key is retained for the grace
    /// period.
    ///
    /// Returns the public portion of the new KEM pre-key (to publish to
    /// the server).
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError`](crate::error::CryptoError) if key generation or signing fails.
    pub fn rotate_kem_pre_key(&mut self, timestamp: u64) -> Result<KemPreKeyPublic> {
        let id = self.bundle.next_pre_key_id;
        self.bundle.next_pre_key_id += 1;

        let kpk = KemPreKey::generate(id, &self.bundle.identity)?;
        let public = kpk.to_public();

        // Insert at front; old keys shift to retained positions
        self.bundle.kem_pre_keys.insert(0, kpk);
        self.bundle.kem_pre_key_timestamps.insert(0, timestamp);

        Ok(public)
    }

    /// Remove retained pre-keys that have expired past the grace period.
    ///
    /// Keys with a creation timestamp strictly less than `cutoff_timestamp`
    /// are removed. The current (first) signed pre-key and current (first)
    /// KEM pre-key are never removed regardless of their timestamp.
    ///
    /// Removed key material is zeroized via the underlying key types.
    /// Returns the number of keys removed.
    pub fn cleanup_expired_pre_keys(&mut self, cutoff_timestamp: u64) -> usize {
        let mut removed = 0;

        // Keep the first signed pre-key (current), remove expired retained ones.
        // Signed pre-keys have timestamps we can compare.
        if self.bundle.signed_pre_keys.len() > 1 {
            let before = self.bundle.signed_pre_keys.len();
            self.bundle.signed_pre_keys.retain({
                let mut first = true;
                move |spk| {
                    if first {
                        first = false;
                        return true; // always keep current
                    }
                    spk.timestamp() >= cutoff_timestamp
                }
            });
            removed += before - self.bundle.signed_pre_keys.len();
        }

        // Keep the first KEM pre-key (current), remove retained ones older
        // than the cutoff based on their creation timestamps.
        if self.bundle.kem_pre_keys.len() > 1 {
            let before = self.bundle.kem_pre_keys.len();
            let mut i = 1;
            while i < self.bundle.kem_pre_keys.len() {
                let ts = self.bundle.kem_pre_key_timestamps.get(i).copied().unwrap_or(0);
                if ts < cutoff_timestamp {
                    self.bundle.kem_pre_keys.remove(i);
                    self.bundle.kem_pre_key_timestamps.remove(i);
                } else {
                    i += 1;
                }
            }
            removed += before - self.bundle.kem_pre_keys.len();
        }

        removed
    }

    /// Look up a signed pre-key by ID.
    ///
    /// Searches both the current key and retained keys. Returns `None`
    /// if no key with that ID exists.
    #[must_use]
    pub fn find_signed_pre_key(&self, id: u32) -> Option<&SignedPreKey> {
        self.bundle.signed_pre_keys.iter().find(|k| k.id() == id)
    }

    /// Look up a KEM pre-key by ID.
    ///
    /// Searches both the current key and retained keys. Returns `None`
    /// if no key with that ID exists.
    #[must_use]
    pub fn find_kem_pre_key(&self, id: u32) -> Option<&KemPreKey> {
        self.bundle.kem_pre_keys.iter().find(|k| k.id() == id)
    }

    /// Replenish one-time pre-keys up to `target_count`.
    ///
    /// If the current supply is already at or above `target_count`,
    /// no keys are generated. Otherwise, generates enough keys to reach
    /// `target_count` and returns their public halves (to publish to the
    /// server).
    pub fn replenish_one_time_pre_keys(&mut self, target_count: u32) -> Vec<OneTimePreKeyPublic> {
        #[allow(clippy::cast_possible_truncation)] // OPK count is always small
        let current = self.bundle.one_time_pre_keys.len() as u32;
        if current >= target_count {
            return Vec::new();
        }
        let needed = target_count - current;
        self.generate_one_time_pre_keys(needed)
    }

    /// Return the timestamp of the current (first) signed pre-key, or
    /// `None` if there are no signed pre-keys.
    #[must_use]
    pub fn current_signed_pre_key_timestamp(&self) -> Option<u64> {
        self.bundle.signed_pre_keys.first().map(SignedPreKey::timestamp)
    }

    /// Return the creation timestamp of the current KEM pre-key, or
    /// `None` if there are no KEM pre-keys.
    #[must_use]
    pub fn current_kem_pre_key_timestamp(&self) -> Option<u64> {
        self.bundle.kem_pre_key_timestamps.first().copied()
    }

    /// Count retained pre-keys with timestamps older than `cutoff_secs`.
    ///
    /// Only counts retained (non-current) signed pre-keys and retained
    /// KEM pre-keys. Used by [`crate::rotation::check_rotation`] to
    /// report how many expired keys are waiting for cleanup.
    #[must_use]
    pub fn count_expired_pre_keys(&self, cutoff_secs: u64) -> usize {
        let mut count = 0;

        // Count retained signed pre-keys older than cutoff
        for spk in self.bundle.signed_pre_keys.iter().skip(1) {
            if spk.timestamp() < cutoff_secs {
                count += 1;
            }
        }

        // Count retained KEM pre-keys older than cutoff
        for ts in self.bundle.kem_pre_key_timestamps.iter().skip(1) {
            if *ts < cutoff_secs {
                count += 1;
            }
        }

        count
    }

    /// Return the number of signed pre-keys (current + retained).
    #[must_use]
    pub fn signed_pre_key_count(&self) -> usize {
        self.bundle.signed_pre_keys.len()
    }

    /// Return the number of KEM pre-keys (current + retained).
    #[must_use]
    pub fn kem_pre_key_count(&self) -> usize {
        self.bundle.kem_pre_keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Signed pre-key rotation ──────────────────────────────────────

    #[test]
    fn rotate_signed_pre_key_creates_new_current() {
        let mut store = KeyStore::create().expect("create failed");
        let original_id = store.public_bundle().expect("bundle").signed_pre_key().id();

        let new_pub = store.rotate_signed_pre_key(1000).expect("rotate failed");

        // New key is now current
        let bundle = store.public_bundle().expect("bundle");
        assert_eq!(bundle.signed_pre_key().id(), new_pub.id());
        assert_ne!(new_pub.id(), original_id);
        assert_eq!(bundle.signed_pre_key().timestamp(), 1000);
    }

    #[test]
    fn rotate_signed_pre_key_retains_old() {
        let mut store = KeyStore::create().expect("create failed");
        let original_id = store.public_bundle().expect("bundle").signed_pre_key().id();

        store.rotate_signed_pre_key(1000).expect("rotate failed");

        // Old key is still findable
        assert!(store.find_signed_pre_key(original_id).is_some());
        assert_eq!(store.signed_pre_key_count(), 2);
    }

    #[test]
    fn rotate_signed_pre_key_increments_ids() {
        let mut store = KeyStore::create().expect("create failed");

        let pub1 = store.rotate_signed_pre_key(100).expect("rotate 1");
        let pub2 = store.rotate_signed_pre_key(200).expect("rotate 2");
        let pub3 = store.rotate_signed_pre_key(300).expect("rotate 3");

        assert!(pub1.id() < pub2.id());
        assert!(pub2.id() < pub3.id());
    }

    #[test]
    fn find_signed_pre_key_finds_current_and_retained() {
        let mut store = KeyStore::create().expect("create failed");
        let original_id = store.public_bundle().expect("bundle").signed_pre_key().id();
        let new_pub = store.rotate_signed_pre_key(1000).expect("rotate");

        assert!(store.find_signed_pre_key(original_id).is_some());
        assert!(store.find_signed_pre_key(new_pub.id()).is_some());
        assert!(store.find_signed_pre_key(9999).is_none());
    }

    // ── KEM pre-key rotation ────────────────────────────────────────

    #[test]
    fn rotate_kem_pre_key_creates_new_current() {
        let mut store = KeyStore::create().expect("create failed");
        let original_id = store.public_bundle().expect("bundle").kem_pre_key().id();

        let new_pub = store.rotate_kem_pre_key(2000).expect("rotate failed");

        let bundle = store.public_bundle().expect("bundle");
        assert_eq!(bundle.kem_pre_key().id(), new_pub.id());
        assert_ne!(new_pub.id(), original_id);
    }

    #[test]
    fn rotate_kem_pre_key_retains_old() {
        let mut store = KeyStore::create().expect("create failed");
        let original_id = store.public_bundle().expect("bundle").kem_pre_key().id();

        store.rotate_kem_pre_key(2000).expect("rotate failed");

        assert!(store.find_kem_pre_key(original_id).is_some());
        assert_eq!(store.kem_pre_key_count(), 2);
    }

    #[test]
    fn rotate_kem_pre_key_increments_ids() {
        let mut store = KeyStore::create().expect("create failed");

        let pub1 = store.rotate_kem_pre_key(100).expect("rotate 1");
        let pub2 = store.rotate_kem_pre_key(200).expect("rotate 2");

        assert!(pub1.id() < pub2.id());
    }

    #[test]
    fn find_kem_pre_key_finds_current_and_retained() {
        let mut store = KeyStore::create().expect("create failed");
        let original_id = store.public_bundle().expect("bundle").kem_pre_key().id();
        let new_pub = store.rotate_kem_pre_key(2000).expect("rotate");

        assert!(store.find_kem_pre_key(original_id).is_some());
        assert!(store.find_kem_pre_key(new_pub.id()).is_some());
        assert!(store.find_kem_pre_key(9999).is_none());
    }

    // ── Cleanup expired pre-keys ────────────────────────────────────

    #[test]
    fn cleanup_removes_old_signed_pre_keys() {
        let mut store = KeyStore::create().expect("create failed");

        // Rotate at timestamp 100, old key has timestamp 0
        store.rotate_signed_pre_key(100).expect("rotate");
        assert_eq!(store.signed_pre_key_count(), 2);

        // Cleanup with cutoff at 50: keys with timestamp < 50 are removed
        let removed = store.cleanup_expired_pre_keys(50);
        assert_eq!(removed, 1);
        assert_eq!(store.signed_pre_key_count(), 1);
    }

    #[test]
    fn cleanup_keeps_current_signed_pre_key() {
        let mut store = KeyStore::create().expect("create failed");

        // Even if current key has timestamp 0 and cutoff is 100,
        // the current key (first) is never removed.
        let removed = store.cleanup_expired_pre_keys(100);
        assert_eq!(removed, 0);
        assert_eq!(store.signed_pre_key_count(), 1);
    }

    #[test]
    fn cleanup_removes_old_kem_pre_keys() {
        let mut store = KeyStore::create().expect("create failed");

        // Rotate KEM at timestamp 200, old key had timestamp 0
        store.rotate_kem_pre_key(200).expect("rotate");
        assert_eq!(store.kem_pre_key_count(), 2);

        // Cleanup with cutoff at 50: old key (timestamp 0) removed
        let removed = store.cleanup_expired_pre_keys(50);
        assert_eq!(removed, 1);
        assert_eq!(store.kem_pre_key_count(), 1);
    }

    #[test]
    fn cleanup_keeps_recent_retained_keys() {
        let mut store = KeyStore::create().expect("create failed");

        // Rotate at timestamp 100
        store.rotate_signed_pre_key(100).expect("rotate");

        // Cleanup with cutoff at 0: nothing older than 0, nothing removed
        let removed = store.cleanup_expired_pre_keys(0);
        assert_eq!(removed, 0);
        assert_eq!(store.signed_pre_key_count(), 2);
    }

    // ── Replenish one-time pre-keys ─────────────────────────────────

    #[test]
    fn replenish_generates_up_to_target() {
        let mut store = KeyStore::create().expect("create failed");
        assert_eq!(store.one_time_pre_key_count(), 0);

        let new_keys = store.replenish_one_time_pre_keys(10);
        assert_eq!(new_keys.len(), 10);
        assert_eq!(store.one_time_pre_key_count(), 10);
    }

    #[test]
    fn replenish_does_nothing_when_above_target() {
        let mut store = KeyStore::create().expect("create failed");
        store.generate_one_time_pre_keys(20);

        let new_keys = store.replenish_one_time_pre_keys(10);
        assert!(new_keys.is_empty());
        assert_eq!(store.one_time_pre_key_count(), 20);
    }

    #[test]
    fn replenish_tops_up_partial() {
        let mut store = KeyStore::create().expect("create failed");
        store.generate_one_time_pre_keys(7);

        let new_keys = store.replenish_one_time_pre_keys(10);
        assert_eq!(new_keys.len(), 3);
        assert_eq!(store.one_time_pre_key_count(), 10);
    }

    // ── Timestamp accessors ─────────────────────────────────────────

    #[test]
    fn current_signed_pre_key_timestamp_returns_first() {
        let mut store = KeyStore::create().expect("create failed");
        assert_eq!(store.current_signed_pre_key_timestamp(), Some(0));

        store.rotate_signed_pre_key(5000).expect("rotate");
        assert_eq!(store.current_signed_pre_key_timestamp(), Some(5000));
    }

    #[test]
    fn current_kem_pre_key_timestamp_returns_first() {
        let mut store = KeyStore::create().expect("create failed");
        assert_eq!(store.current_kem_pre_key_timestamp(), Some(0));

        store.rotate_kem_pre_key(7000).expect("rotate");
        assert_eq!(store.current_kem_pre_key_timestamp(), Some(7000));
    }

    // ── Rotation survives save/open ─────────────────────────────────

    #[test]
    fn rotated_keys_survive_save_open() {
        let mut store = KeyStore::create().expect("create failed");
        let original_spk_id = store.public_bundle().expect("bundle").signed_pre_key().id();
        let original_kpk_id = store.public_bundle().expect("bundle").kem_pre_key().id();

        // Rotate both key types
        let new_spk = store.rotate_signed_pre_key(1000).expect("rotate spk");
        let new_kpk = store.rotate_kem_pre_key(2000).expect("rotate kpk");

        // Generate some OTPs
        store.generate_one_time_pre_keys(5);

        // Save and reopen
        let encrypted = store.save(b"pass").expect("save");
        let restored = KeyStore::open(&encrypted, b"pass").expect("open");

        // Current keys match
        assert_eq!(
            restored.public_bundle().expect("bundle").signed_pre_key().id(),
            new_spk.id()
        );
        assert_eq!(
            restored.public_bundle().expect("bundle").kem_pre_key().id(),
            new_kpk.id()
        );

        // Retained keys still findable
        assert!(restored.find_signed_pre_key(original_spk_id).is_some());
        assert!(restored.find_kem_pre_key(original_kpk_id).is_some());

        // Counts preserved
        assert_eq!(restored.signed_pre_key_count(), 2);
        assert_eq!(restored.kem_pre_key_count(), 2);
        assert_eq!(restored.one_time_pre_key_count(), 5);

        // Timestamps preserved
        assert_eq!(restored.current_signed_pre_key_timestamp(), Some(1000));
        assert_eq!(restored.current_kem_pre_key_timestamp(), Some(2000));
    }

    #[test]
    fn rotated_bundle_still_validates() {
        let mut store = KeyStore::create().expect("create failed");
        store.rotate_signed_pre_key(1000).expect("rotate spk");
        store.rotate_kem_pre_key(2000).expect("rotate kpk");

        let bundle = store.public_bundle().expect("bundle");
        bundle.validate().expect("validation should pass after rotation");
    }

    // ── Count expired pre-keys ──────────────────────────────────────

    #[test]
    fn count_expired_pre_keys_counts_old_retained() {
        let mut store = KeyStore::create().expect("create failed");

        // Rotate signed at 100, retained has timestamp 0
        store.rotate_signed_pre_key(100).expect("rotate spk");
        // Rotate KEM at 200, retained has timestamp 0
        store.rotate_kem_pre_key(200).expect("rotate kpk");

        // Cutoff at 50: both retained keys (timestamp 0) are expired
        assert_eq!(store.count_expired_pre_keys(50), 2);

        // Cutoff at 0: nothing expired (timestamps must be strictly less)
        assert_eq!(store.count_expired_pre_keys(0), 0);
    }
}
