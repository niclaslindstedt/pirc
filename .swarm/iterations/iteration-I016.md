# Iteration I016 Analysis

## Summary

Iteration I016 completed Epic E016 (Triple Ratchet Encryption Core), delivering the full cryptographic foundation for PIRC's end-to-end encryption in the `pirc-crypto` crate. This epic implements the triple ratchet protocol combining classical Diffie-Hellman, post-quantum ML-KEM (Kyber), and symmetric KDF chains — providing forward secrecy and post-quantum resistance. The implementation spans 16 tickets across 12 merged CRs, building up from low-level primitives (X25519, AES-256-GCM, HKDF) through the three ratchet mechanisms to a unified session with header encryption, out-of-order message handling, and key erasure guarantees. All 190 pirc-crypto tests pass with clean clippy.

## Completed Work

### Feature Tickets (via CRs)
- **T159** (CR134): Crate setup — added cryptographic dependencies (x25519-dalek, aes-gcm, hkdf, sha2, ml-kem, ml-dsa, zeroize), created 12 module stubs, defined `CryptoError` enum, bumped workspace MSRV to 1.85
- **T160** (CR135): X25519 DH wrapper — `KeyPair` generation, `SharedSecret` with constant-time DH, `Zeroize`/`ZeroizeOnDrop` on all secret types, serialization support
- **T161** (CR136): AES-256-GCM AEAD — encrypt/decrypt with associated data, nonce generation, tamper detection tests
- **T162** (CR137): HKDF key derivation — extract/expand matching RFC 5869 test vectors, `kdf_chain()` for symmetric ratchet advancement
- **T163** (CR138): Symmetric-key ratchet — `SymmetricRatchet` with `ChainKey`/`MessageKey` types, per-message key derivation, `skip_to()` for out-of-order handling with MAX_SKIP=1000
- **T164** (CR139): DH ratchet — `DhRatchetState` combining X25519 key exchange with root key evolution, automatic chain key reset on sender change, break-in recovery
- **T165** (CR140): ML-KEM (Kyber) wrapper — ML-KEM-768 key generation, encapsulate/decapsulate round-trip, secret key zeroization
- **T166** (CR141): ML-DSA (Dilithium) wrapper — ML-DSA-65 sign/verify, key fingerprinting via SHA-256, tamper detection
- **T167** (CR142): Post-quantum ratchet — `PqRatchetState` with periodic KEM encapsulation, chain key evolution via HKDF, step counter for PQ interval control
- **T168** (CR143): Message types and header encryption — `MessageHeader`/`EncryptedMessage` wire types, `HeaderKey` with AES-256-GCM encryption, trial decryption with multiple header keys
- **T169** (CR145): Unified triple ratchet session — `TripleRatchetSession` combining all three ratchets, bidirectional messaging, automatic DH/PQ stepping, skipped key cache with HashMap storage, header key rotation
- **T170** (CR147): Forward secrecy and key erasure — `Zeroize`/`ZeroizeOnDrop` audit across all key types, `MessageKey` made non-Clone for single-use enforcement, `SkippedKey` with age-based purging, `SessionInfo` for state monitoring

### Follow-up/Cleanup Tickets (closed directly)
- **T171**: Comprehensive test vectors and property-based tests — auto-closed, testing was folded into each implementation ticket (190 total tests)
- **T172**: Fix out-of-order message key handling in decrypt — review feedback fix merged into CR143
- **T173**: Add previous receiving header key for trial decryption — review feedback fix merged into CR145
- **T174**: Split triple_ratchet.rs tests into separate file — review feedback fix merged into CR145

## Challenges

- **MSRV bump to 1.85**: The `ml-dsa` crate requires Rust 1.85, which forced a workspace-wide MSRV increase from 1.80. This was a necessary trade-off for post-quantum support via RustCrypto's native implementations.
- **Header trial decryption**: The initial implementation missed that a receiver needs to try both the current and previous receiving header keys when a DH ratchet step occurs. This was identified during review and fixed via T173.
- **Out-of-order message handling**: The initial skipped key storage in the decrypt path had issues with properly caching and retrieving keys for out-of-order messages. T172 fixed the storage and lookup logic.
- **Non-Clone MessageKey design**: Making `MessageKey` non-Clone to enforce single-use semantics required careful restructuring of the encrypt/decrypt paths to avoid the compiler demanding copies, but produces a stronger forward secrecy guarantee.

## Learnings

- **Bottom-up crypto construction**: Building from primitives (X25519, AES-GCM, HKDF) through ratchets to the unified session allowed each layer to be independently tested and reasoned about. The layered architecture made the complex triple ratchet manageable.
- **Zeroize/ZeroizeOnDrop pattern**: Rust's ownership model combined with `zeroize` provides compile-time-enforceable key erasure — key material is provably zeroed when the owning struct is dropped, with no runtime overhead for tracking.
- **Trial decryption for header keys**: The double ratchet pattern requires trying multiple header keys because the receiver doesn't know in advance whether a DH ratchet step occurred. Keeping both current and previous header keys handles this cleanly.
- **Post-quantum hybrid approach**: Combining classical DH (X25519) with post-quantum KEM (ML-KEM-768) provides defense in depth — security holds as long as either primitive remains secure, protecting against both current and future quantum threats.
- **Review-driven quality**: Several critical correctness issues (T172, T173) were caught during code review, reinforcing the value of the review step for cryptographic code.

## Recommendations

- **Integration with protocol layer**: The next step is integrating `pirc-crypto` with `pirc-protocol` to encrypt/decrypt IRC messages on the wire, including key exchange handshake during connection setup.
- **Key exchange protocol**: A higher-level key exchange protocol is needed to establish the initial shared secret between two parties (likely using X3DH or similar), which feeds into the triple ratchet session initialization.
- **Group encryption**: The current session is pairwise (1:1). For IRC channels, either a sender-keys approach or a tree-based group ratchet will be needed to extend encryption to multi-party conversations.
- **Persistent session storage**: Sessions will need to be serializable to disk so that encryption state survives client restarts, requiring careful handling of key material at rest (possibly with a passphrase-derived wrapping key).
- **Performance benchmarks**: ML-KEM and ML-DSA operations are more expensive than classical crypto. Benchmarking the PQ ratchet step frequency against real-world message patterns will help tune the `pq_step_interval` parameter.
