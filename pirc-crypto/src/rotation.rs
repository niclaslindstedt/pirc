//! Key rotation policy and scheduling.
//!
//! Provides [`RotationPolicy`] for configuring how often signed pre-keys
//! and KEM pre-keys should be rotated, how long old keys are retained
//! during a grace period, and when one-time pre-key supplies should be
//! replenished. The [`check_rotation`] function inspects a [`KeyStore`]
//! and returns a [`RotationCheck`] indicating what actions are needed.

use crate::key_storage::KeyStore;

/// Default signed pre-key maximum age: 7 days in seconds.
const DEFAULT_SIGNED_PRE_KEY_MAX_AGE: u64 = 604_800;

/// Default KEM pre-key maximum age: 7 days in seconds.
const DEFAULT_KEM_PRE_KEY_MAX_AGE: u64 = 604_800;

/// Default grace period for retaining old pre-keys: 2 days in seconds.
const DEFAULT_GRACE_PERIOD: u64 = 172_800;

/// Default minimum one-time pre-key count before replenishment.
const DEFAULT_MIN_ONE_TIME_PRE_KEYS: u32 = 25;

/// Default batch size when generating new one-time pre-keys.
const DEFAULT_ONE_TIME_PRE_KEY_BATCH_SIZE: u32 = 100;

/// Configuration for key rotation schedules.
///
/// Controls how often signed pre-keys and KEM pre-keys are rotated,
/// how long old keys are retained after rotation (grace period), and
/// when one-time pre-key supplies should be replenished.
#[derive(Debug, Clone)]
pub struct RotationPolicy {
    /// Maximum age (in seconds) before a signed pre-key should be rotated.
    pub signed_pre_key_max_age_secs: u64,
    /// Maximum age (in seconds) before a KEM pre-key should be rotated.
    pub kem_pre_key_max_age_secs: u64,
    /// How long (in seconds) to retain old pre-keys after rotation.
    pub grace_period_secs: u64,
    /// Minimum one-time pre-key count before replenishment is triggered.
    pub min_one_time_pre_keys: u32,
    /// How many one-time pre-keys to generate per replenishment batch.
    pub one_time_pre_key_batch_size: u32,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            signed_pre_key_max_age_secs: DEFAULT_SIGNED_PRE_KEY_MAX_AGE,
            kem_pre_key_max_age_secs: DEFAULT_KEM_PRE_KEY_MAX_AGE,
            grace_period_secs: DEFAULT_GRACE_PERIOD,
            min_one_time_pre_keys: DEFAULT_MIN_ONE_TIME_PRE_KEYS,
            one_time_pre_key_batch_size: DEFAULT_ONE_TIME_PRE_KEY_BATCH_SIZE,
        }
    }
}

/// Result of checking what key rotation actions are needed.
///
/// Returned by [`check_rotation`] after inspecting the current
/// [`KeyStore`] state against a [`RotationPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationCheck {
    /// Whether the current signed pre-key has exceeded its maximum age.
    pub needs_signed_pre_key_rotation: bool,
    /// Whether the current KEM pre-key has exceeded its maximum age.
    pub needs_kem_pre_key_rotation: bool,
    /// Whether one-time pre-key supply is below the minimum threshold.
    pub needs_one_time_pre_key_replenishment: bool,
    /// Number of retained pre-keys that have exceeded the grace period.
    pub expired_pre_key_count: usize,
}

/// Check the [`KeyStore`] against a [`RotationPolicy`] and return what
/// actions are needed.
///
/// `now_secs` is the current Unix timestamp in seconds. This is passed
/// explicitly (rather than using the system clock) to make the function
/// deterministic and testable.
#[must_use]
pub fn check_rotation(store: &KeyStore, policy: &RotationPolicy, now_secs: u64) -> RotationCheck {
    let needs_signed_pre_key_rotation = store
        .current_signed_pre_key_timestamp()
        .is_none_or(|ts| now_secs.saturating_sub(ts) >= policy.signed_pre_key_max_age_secs);

    let needs_kem_pre_key_rotation = store
        .current_kem_pre_key_timestamp()
        .is_none_or(|ts| now_secs.saturating_sub(ts) >= policy.kem_pre_key_max_age_secs);

    #[allow(clippy::cast_possible_truncation)] // OPK count is always small
    let needs_one_time_pre_key_replenishment =
        (store.one_time_pre_key_count() as u32) < policy.min_one_time_pre_keys;

    let expired_pre_key_count = store.count_expired_pre_keys(
        now_secs.saturating_sub(policy.grace_period_secs),
    );

    RotationCheck {
        needs_signed_pre_key_rotation,
        needs_kem_pre_key_rotation,
        needs_one_time_pre_key_replenishment,
        expired_pre_key_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_policy_default_values() {
        let policy = RotationPolicy::default();
        assert_eq!(policy.signed_pre_key_max_age_secs, 604_800);
        assert_eq!(policy.kem_pre_key_max_age_secs, 604_800);
        assert_eq!(policy.grace_period_secs, 172_800);
        assert_eq!(policy.min_one_time_pre_keys, 25);
        assert_eq!(policy.one_time_pre_key_batch_size, 100);
    }

    #[test]
    fn check_rotation_fresh_store_needs_rotation_at_zero_timestamp() {
        // A fresh store has timestamp 0, so at time 0 it should NOT need
        // rotation (age = 0 < max_age).
        let store = KeyStore::create().expect("create failed");
        let policy = RotationPolicy {
            signed_pre_key_max_age_secs: 100,
            kem_pre_key_max_age_secs: 100,
            grace_period_secs: 50,
            min_one_time_pre_keys: 0,
            one_time_pre_key_batch_size: 10,
        };

        let check = check_rotation(&store, &policy, 0);
        assert!(!check.needs_signed_pre_key_rotation);
        assert!(!check.needs_kem_pre_key_rotation);
        assert!(!check.needs_one_time_pre_key_replenishment);
        assert_eq!(check.expired_pre_key_count, 0);
    }

    #[test]
    fn check_rotation_detects_aged_keys() {
        let store = KeyStore::create().expect("create failed");
        let policy = RotationPolicy {
            signed_pre_key_max_age_secs: 100,
            kem_pre_key_max_age_secs: 200,
            grace_period_secs: 50,
            min_one_time_pre_keys: 0,
            one_time_pre_key_batch_size: 10,
        };

        // At time 100, signed pre-key (created at 0) is exactly max_age -> rotate
        let check = check_rotation(&store, &policy, 100);
        assert!(check.needs_signed_pre_key_rotation);
        assert!(!check.needs_kem_pre_key_rotation);

        // At time 200, both need rotation
        let check = check_rotation(&store, &policy, 200);
        assert!(check.needs_signed_pre_key_rotation);
        assert!(check.needs_kem_pre_key_rotation);
    }

    #[test]
    fn check_rotation_detects_low_otpk() {
        let store = KeyStore::create().expect("create failed");
        let policy = RotationPolicy {
            signed_pre_key_max_age_secs: 604_800,
            kem_pre_key_max_age_secs: 604_800,
            grace_period_secs: 172_800,
            min_one_time_pre_keys: 25,
            one_time_pre_key_batch_size: 100,
        };

        let check = check_rotation(&store, &policy, 0);
        assert!(check.needs_one_time_pre_key_replenishment);
    }

    #[test]
    fn check_rotation_otpk_above_threshold() {
        let mut store = KeyStore::create().expect("create failed");
        store.generate_one_time_pre_keys(30);

        let policy = RotationPolicy {
            signed_pre_key_max_age_secs: 604_800,
            kem_pre_key_max_age_secs: 604_800,
            grace_period_secs: 172_800,
            min_one_time_pre_keys: 25,
            one_time_pre_key_batch_size: 100,
        };

        let check = check_rotation(&store, &policy, 0);
        assert!(!check.needs_one_time_pre_key_replenishment);
    }

    #[test]
    fn check_rotation_detects_expired_retained_keys() {
        let mut store = KeyStore::create().expect("create failed");

        // Rotate signed pre-key at time 100
        store.rotate_signed_pre_key(100).expect("rotate failed");
        // Now there's 1 retained signed pre-key with timestamp 0.

        let policy = RotationPolicy {
            signed_pre_key_max_age_secs: 604_800,
            kem_pre_key_max_age_secs: 604_800,
            grace_period_secs: 50,
            min_one_time_pre_keys: 0,
            one_time_pre_key_batch_size: 10,
        };

        // At time 51, the retained key (timestamp 0) is older than grace_period cutoff (51 - 50 = 1)
        // So it should be counted as expired
        let check = check_rotation(&store, &policy, 51);
        assert_eq!(check.expired_pre_key_count, 1);

        // At time 40, the retained key is still within grace
        let check = check_rotation(&store, &policy, 40);
        assert_eq!(check.expired_pre_key_count, 0);
    }

    #[test]
    fn rotation_check_derives_eq() {
        let a = RotationCheck {
            needs_signed_pre_key_rotation: true,
            needs_kem_pre_key_rotation: false,
            needs_one_time_pre_key_replenishment: true,
            expired_pre_key_count: 3,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
