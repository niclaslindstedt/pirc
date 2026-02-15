//! Error types for the `pirc-crypto` crate.
//!
//! Provides [`CryptoError`] with variants for each cryptographic subsystem:
//! key exchange, authenticated encryption, key derivation, post-quantum KEM,
//! post-quantum signatures, and the triple ratchet protocol.

/// Errors produced by cryptographic operations.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// X25519 Diffie-Hellman key exchange failure.
    #[error("key exchange error: {0}")]
    KeyExchange(String),

    /// AES-256-GCM encryption or decryption failure.
    #[error("AEAD error: {0}")]
    Aead(String),

    /// HKDF key derivation failure.
    #[error("key derivation error: {0}")]
    KeyDerivation(String),

    /// ML-KEM (Kyber) key encapsulation failure.
    #[error("KEM error: {0}")]
    Kem(String),

    /// ML-DSA (Dilithium) signature failure.
    #[error("signature error: {0}")]
    Signature(String),

    /// Triple ratchet session state error.
    #[error("ratchet error: {0}")]
    Ratchet(String),

    /// Header encryption or decryption failure.
    #[error("header encryption error: {0}")]
    HeaderEncryption(String),

    /// Serialization or deserialization failure.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Invalid key material (wrong length, all zeros, etc.).
    #[error("invalid key: {0}")]
    InvalidKey(String),
}

/// A convenience result type that uses [`CryptoError`] as the error variant.
pub type Result<T> = std::result::Result<T, CryptoError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_exchange_error_display() {
        let err = CryptoError::KeyExchange("low-order point".into());
        assert_eq!(err.to_string(), "key exchange error: low-order point");
    }

    #[test]
    fn aead_error_display() {
        let err = CryptoError::Aead("tag mismatch".into());
        assert_eq!(err.to_string(), "AEAD error: tag mismatch");
    }

    #[test]
    fn key_derivation_error_display() {
        let err = CryptoError::KeyDerivation("invalid length".into());
        assert_eq!(err.to_string(), "key derivation error: invalid length");
    }

    #[test]
    fn kem_error_display() {
        let err = CryptoError::Kem("decapsulation failed".into());
        assert_eq!(err.to_string(), "KEM error: decapsulation failed");
    }

    #[test]
    fn signature_error_display() {
        let err = CryptoError::Signature("verification failed".into());
        assert_eq!(err.to_string(), "signature error: verification failed");
    }

    #[test]
    fn ratchet_error_display() {
        let err = CryptoError::Ratchet("chain exhausted".into());
        assert_eq!(err.to_string(), "ratchet error: chain exhausted");
    }

    #[test]
    fn header_encryption_error_display() {
        let err = CryptoError::HeaderEncryption("decrypt failed".into());
        assert_eq!(err.to_string(), "header encryption error: decrypt failed");
    }

    #[test]
    fn serialization_error_display() {
        let err = CryptoError::Serialization("invalid format".into());
        assert_eq!(err.to_string(), "serialization error: invalid format");
    }

    #[test]
    fn invalid_key_error_display() {
        let err = CryptoError::InvalidKey("wrong length".into());
        assert_eq!(err.to_string(), "invalid key: wrong length");
    }

    #[test]
    fn crypto_error_is_std_error() {
        let err: Box<dyn std::error::Error> =
            Box::new(CryptoError::Aead("test".into()));
        assert_eq!(err.to_string(), "AEAD error: test");
    }
}
