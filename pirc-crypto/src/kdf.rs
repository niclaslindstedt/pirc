//! HKDF-based key derivation.
//!
//! Wraps HKDF-SHA-256 to derive symmetric keys and chain keys from
//! shared secrets produced by X25519 or ML-KEM. Implements the
//! KDF chains used by the symmetric-key ratchet.

use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::{CryptoError, Result};

/// Size of a derived key in bytes (256 bits).
pub const KEY_SIZE: usize = 32;

/// Maximum output length for HKDF-Expand with SHA-256 (255 * 32 = 8160 bytes).
pub const MAX_OUTPUT_LEN: usize = 255 * KEY_SIZE;

/// Info string used when deriving the next chain key in a KDF chain.
const CHAIN_KEY_INFO: &[u8] = b"pirc-chain-key";

/// Info string used when deriving a message key in a KDF chain.
const MESSAGE_KEY_INFO: &[u8] = b"pirc-message-key";

/// Perform the HKDF-Extract step using HMAC-SHA-256.
///
/// Extracts a pseudorandom key (PRK) from the input key material and
/// an optional salt. The PRK has uniformly distributed entropy
/// regardless of the structure of the input key material.
///
/// # Arguments
///
/// * `salt` — optional salt value (can be empty; a zero-filled salt
///   of hash length is used internally when empty)
/// * `ikm` — input key material (e.g. a shared secret from DH or KEM)
#[must_use]
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; KEY_SIZE] {
    let (prk, _) = Hkdf::<Sha256>::extract(Some(salt), ikm);
    let mut out = [0u8; KEY_SIZE];
    out.copy_from_slice(prk.as_slice());
    out
}

/// Perform the HKDF-Expand step to derive output key material.
///
/// Expands a pseudorandom key (PRK) into output key material of the
/// requested length, using the `info` parameter for domain separation.
///
/// # Arguments
///
/// * `prk` — pseudorandom key (typically from [`hkdf_extract`])
/// * `info` — context and application-specific information for domain
///   separation (can be empty)
/// * `length` — desired output length in bytes (1..=[`MAX_OUTPUT_LEN`])
///
/// # Errors
///
/// Returns [`CryptoError::KeyDerivation`] if `length` is zero or exceeds
/// [`MAX_OUTPUT_LEN`] (8160 bytes for SHA-256).
pub fn hkdf_expand(prk: &[u8; KEY_SIZE], info: &[u8], length: usize) -> Result<Vec<u8>> {
    if length == 0 {
        return Err(CryptoError::KeyDerivation(
            "output length must be at least 1".into(),
        ));
    }
    let hk = Hkdf::<Sha256>::from_prk(prk).map_err(|e| {
        CryptoError::KeyDerivation(format!("invalid PRK: {e}"))
    })?;
    let mut okm = vec![0u8; length];
    hk.expand(info, &mut okm).map_err(|e| {
        CryptoError::KeyDerivation(format!("expand failed: {e}"))
    })?;
    Ok(okm)
}

/// HKDF-Expand into a caller-provided buffer, avoiding heap allocation.
///
/// This is the zero-allocation variant of [`hkdf_expand`] for callers
/// that know the output size at compile time or can provide a stack buffer.
///
/// # Errors
///
/// Returns [`CryptoError::KeyDerivation`] if the PRK is invalid or
/// the output buffer exceeds [`MAX_OUTPUT_LEN`].
pub fn hkdf_expand_into(prk: &[u8; KEY_SIZE], info: &[u8], out: &mut [u8]) -> Result<()> {
    if out.is_empty() {
        return Err(CryptoError::KeyDerivation(
            "output length must be at least 1".into(),
        ));
    }
    let hk = Hkdf::<Sha256>::from_prk(prk).map_err(|e| {
        CryptoError::KeyDerivation(format!("invalid PRK: {e}"))
    })?;
    hk.expand(info, out).map_err(|e| {
        CryptoError::KeyDerivation(format!("expand failed: {e}"))
    })?;
    Ok(())
}

/// Derive key material into a caller-provided buffer (zero-allocation).
///
/// Combined Extract-then-Expand that writes directly into `out`,
/// avoiding the heap allocation of [`derive_key`].
///
/// # Errors
///
/// Returns [`CryptoError::KeyDerivation`] if the output buffer is empty
/// or exceeds [`MAX_OUTPUT_LEN`].
pub fn derive_key_into(salt: &[u8], ikm: &[u8], info: &[u8], out: &mut [u8]) -> Result<()> {
    let prk = hkdf_extract(salt, ikm);
    hkdf_expand_into(&prk, info, out)
}

/// Derive key material using the combined HKDF Extract-then-Expand flow.
///
/// This is a convenience function that chains [`hkdf_extract`] and
/// [`hkdf_expand`] into a single call, matching the full HKDF
/// construction from RFC 5869.
///
/// # Arguments
///
/// * `salt` — optional salt value (can be empty)
/// * `ikm` — input key material
/// * `info` — context/domain-separation string
/// * `length` — desired output length in bytes (1..=[`MAX_OUTPUT_LEN`])
///
/// # Errors
///
/// Returns [`CryptoError::KeyDerivation`] if `length` is zero or exceeds
/// [`MAX_OUTPUT_LEN`].
pub fn derive_key(salt: &[u8], ikm: &[u8], info: &[u8], length: usize) -> Result<Vec<u8>> {
    let prk = hkdf_extract(salt, ikm);
    hkdf_expand(&prk, info, length)
}

/// Advance a KDF chain by one step, producing a new chain key and a message key.
///
/// Takes the current chain key and input material, then derives two
/// independent 32-byte keys using distinct info strings for domain
/// separation:
///
/// - **chain key** — used as input to the next `kdf_chain` call
/// - **message key** — used to encrypt or decrypt a single message
///
/// This is the core primitive of the symmetric-key ratchet: each
/// ratchet step calls `kdf_chain` to advance the chain and obtain
/// a one-time message key.
///
/// # Arguments
///
/// * `chain_key` — the current 32-byte chain key
/// * `input` — additional input material (e.g. a DH shared secret)
#[must_use]
pub fn kdf_chain(chain_key: &[u8; KEY_SIZE], input: &[u8]) -> ([u8; KEY_SIZE], [u8; KEY_SIZE]) {
    let prk = hkdf_extract(chain_key, input);

    let mut new_chain_key = [0u8; KEY_SIZE];
    // These expand calls use a valid PRK and request exactly 32 bytes,
    // which is well within the HKDF-SHA-256 limit, so they cannot fail.
    let hk = Hkdf::<Sha256>::from_prk(&prk).expect("PRK is valid 32 bytes");
    hk.expand(CHAIN_KEY_INFO, &mut new_chain_key)
        .expect("32 bytes is within HKDF-SHA256 limit");

    let mut message_key = [0u8; KEY_SIZE];
    hk.expand(MESSAGE_KEY_INFO, &mut message_key)
        .expect("32 bytes is within HKDF-SHA256 limit");

    (new_chain_key, message_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------
    // RFC 5869 test vectors (HKDF-SHA-256)
    // -----------------------------------------------------------

    /// RFC 5869 Test Case 1.
    #[test]
    fn rfc5869_test_case_1() {
        let ikm = [0x0bu8; 22];
        let salt = hex_to_bytes("000102030405060708090a0b0c");
        let info = hex_to_bytes("f0f1f2f3f4f5f6f7f8f9");
        let expected_prk =
            hex_to_bytes("077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5");
        let expected_okm = hex_to_bytes(
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865",
        );

        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(prk.to_vec(), expected_prk, "PRK mismatch for test case 1");

        let okm = hkdf_expand(&prk, &info, 42).expect("expand failed");
        assert_eq!(okm, expected_okm, "OKM mismatch for test case 1");

        // Also verify the combined derive_key function
        let okm2 = derive_key(&salt, &ikm, &info, 42).expect("derive_key failed");
        assert_eq!(okm2, expected_okm, "derive_key mismatch for test case 1");
    }

    /// RFC 5869 Test Case 2 (longer inputs/outputs).
    #[test]
    fn rfc5869_test_case_2() {
        let ikm = hex_to_bytes(
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\
             202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f\
             404142434445464748494a4b4c4d4e4f",
        );
        let salt = hex_to_bytes(
            "606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f\
             808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f\
             a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
        );
        let info = hex_to_bytes(
            "b0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecf\
             d0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeef\
             f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
        );
        let expected_prk =
            hex_to_bytes("06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244");
        let expected_okm = hex_to_bytes(
            "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
             59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
             cc30c58179ec3e87c14c01d5c1f3434f1d87",
        );

        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(prk.to_vec(), expected_prk, "PRK mismatch for test case 2");

        let okm = hkdf_expand(&prk, &info, 82).expect("expand failed");
        assert_eq!(okm, expected_okm, "OKM mismatch for test case 2");
    }

    /// RFC 5869 Test Case 3 (zero-length salt and info).
    #[test]
    fn rfc5869_test_case_3() {
        let ikm = [0x0bu8; 22];
        let salt = b"";
        let info = b"";
        let expected_prk =
            hex_to_bytes("19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04");
        let expected_okm = hex_to_bytes(
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d\
             9d201395faa4b61a96c8",
        );

        let prk = hkdf_extract(salt, &ikm);
        assert_eq!(prk.to_vec(), expected_prk, "PRK mismatch for test case 3");

        let okm = hkdf_expand(&prk, info, 42).expect("expand failed");
        assert_eq!(okm, expected_okm, "OKM mismatch for test case 3");
    }

    // -----------------------------------------------------------
    // KDF chain tests
    // -----------------------------------------------------------

    #[test]
    fn kdf_chain_produces_two_distinct_keys() {
        let chain_key = [0xAAu8; KEY_SIZE];
        let input = b"shared-secret";

        let (new_chain_key, message_key) = kdf_chain(&chain_key, input);

        assert_ne!(new_chain_key, message_key, "chain key and message key must differ");
        assert_ne!(new_chain_key, chain_key, "new chain key must differ from input chain key");
    }

    #[test]
    fn kdf_chain_is_deterministic() {
        let chain_key = [0xBBu8; KEY_SIZE];
        let input = b"determinism-test";

        let (ck1, mk1) = kdf_chain(&chain_key, input);
        let (ck2, mk2) = kdf_chain(&chain_key, input);

        assert_eq!(ck1, ck2, "chain key must be deterministic");
        assert_eq!(mk1, mk2, "message key must be deterministic");
    }

    #[test]
    fn kdf_chain_different_inputs_produce_different_keys() {
        let chain_key = [0xCCu8; KEY_SIZE];

        let (ck1, mk1) = kdf_chain(&chain_key, b"input-a");
        let (ck2, mk2) = kdf_chain(&chain_key, b"input-b");

        assert_ne!(ck1, ck2, "different inputs should produce different chain keys");
        assert_ne!(mk1, mk2, "different inputs should produce different message keys");
    }

    #[test]
    fn kdf_chain_different_chain_keys_produce_different_keys() {
        let ck_a = [0x01u8; KEY_SIZE];
        let ck_b = [0x02u8; KEY_SIZE];
        let input = b"same-input";

        let (new_a, msg_a) = kdf_chain(&ck_a, input);
        let (new_b, msg_b) = kdf_chain(&ck_b, input);

        assert_ne!(new_a, new_b, "different chain keys should produce different outputs");
        assert_ne!(msg_a, msg_b, "different chain keys should produce different message keys");
    }

    #[test]
    fn kdf_chain_advancement() {
        let initial_ck = [0xDDu8; KEY_SIZE];
        let input = b"ratchet-step";

        // Advance the chain three times
        let (ck1, mk1) = kdf_chain(&initial_ck, input);
        let (ck2, mk2) = kdf_chain(&ck1, input);
        let (ck3, mk3) = kdf_chain(&ck2, input);

        // All chain keys must be distinct
        assert_ne!(initial_ck, ck1);
        assert_ne!(ck1, ck2);
        assert_ne!(ck2, ck3);

        // All message keys must be distinct
        assert_ne!(mk1, mk2);
        assert_ne!(mk2, mk3);
        assert_ne!(mk1, mk3);
    }

    // -----------------------------------------------------------
    // derive_key / hkdf_expand edge cases
    // -----------------------------------------------------------

    #[test]
    fn derive_key_single_byte_output() {
        let result = derive_key(b"salt", b"ikm", b"info", 1);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn derive_key_max_output() {
        let result = derive_key(b"salt", b"ikm", b"info", MAX_OUTPUT_LEN);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), MAX_OUTPUT_LEN);
    }

    #[test]
    fn hkdf_expand_rejects_zero_length() {
        let prk = [0x42u8; KEY_SIZE];
        let result = hkdf_expand(&prk, b"info", 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("at least 1"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn hkdf_expand_rejects_excessive_length() {
        let prk = [0x42u8; KEY_SIZE];
        let result = hkdf_expand(&prk, b"info", MAX_OUTPUT_LEN + 1);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("expand failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn different_info_strings_produce_different_output() {
        let prk = hkdf_extract(b"salt", b"key-material");

        let okm1 = hkdf_expand(&prk, b"purpose-a", 32).expect("expand failed");
        let okm2 = hkdf_expand(&prk, b"purpose-b", 32).expect("expand failed");

        assert_ne!(okm1, okm2, "different info strings must produce different keys");
    }

    #[test]
    fn empty_salt_is_handled() {
        // Empty salt should not panic and should produce a valid PRK
        let prk = hkdf_extract(b"", b"some-key-material");
        assert_ne!(prk, [0u8; KEY_SIZE], "PRK should not be all zeros");
    }

    #[test]
    fn empty_ikm_is_handled() {
        // Empty IKM should not panic (though it has no entropy)
        let prk = hkdf_extract(b"salt", b"");
        // Should still produce a deterministic non-zero output
        let prk2 = hkdf_extract(b"salt", b"");
        assert_eq!(prk, prk2, "extract should be deterministic");
    }

    // -----------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
            .collect()
    }
}
