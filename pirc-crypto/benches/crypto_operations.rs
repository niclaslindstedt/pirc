use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pirc_crypto::aead;
use pirc_crypto::identity::IdentityKeyPair;
use pirc_crypto::kdf;
use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::prekey::{KemPreKey, OneTimePreKey, PreKeyBundle, SignedPreKey};
use pirc_crypto::signing::SigningKeyPair;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519::{self, KeyPair};
use pirc_crypto::x3dh;

// ── AEAD (AES-256-GCM) ─────────────────────────────────────────

fn bench_aead_encrypt(c: &mut Criterion) {
    let key = [0x42u8; 32];
    let nonce = aead::generate_nonce();
    let plaintext = b"Hello, world! This is a typical chat message for encryption.";
    let aad = b"channel:#general";

    c.bench_function("aead_encrypt_60B", |b| {
        b.iter(|| aead::encrypt(black_box(&key), black_box(&nonce), black_box(plaintext), black_box(aad)).unwrap());
    });
}

fn bench_aead_decrypt(c: &mut Criterion) {
    let key = [0x42u8; 32];
    let nonce = aead::generate_nonce();
    let plaintext = b"Hello, world! This is a typical chat message for encryption.";
    let aad = b"channel:#general";
    let ciphertext = aead::encrypt(&key, &nonce, plaintext, aad).unwrap();

    c.bench_function("aead_decrypt_60B", |b| {
        b.iter(|| aead::decrypt(black_box(&key), black_box(&nonce), black_box(&ciphertext), black_box(aad)).unwrap());
    });
}

fn bench_aead_encrypt_1kb(c: &mut Criterion) {
    let key = [0x42u8; 32];
    let nonce = aead::generate_nonce();
    let plaintext = vec![0xABu8; 1024];
    let aad = b"";

    c.bench_function("aead_encrypt_1KB", |b| {
        b.iter(|| aead::encrypt(black_box(&key), black_box(&nonce), black_box(&plaintext), black_box(aad)).unwrap());
    });
}

fn bench_aead_decrypt_1kb(c: &mut Criterion) {
    let key = [0x42u8; 32];
    let nonce = aead::generate_nonce();
    let plaintext = vec![0xABu8; 1024];
    let aad = b"";
    let ciphertext = aead::encrypt(&key, &nonce, &plaintext, aad).unwrap();

    c.bench_function("aead_decrypt_1KB", |b| {
        b.iter(|| aead::decrypt(black_box(&key), black_box(&nonce), black_box(&ciphertext), black_box(aad)).unwrap());
    });
}

fn bench_aead_nonce_generation(c: &mut Criterion) {
    c.bench_function("aead_generate_nonce", |b| {
        b.iter(|| black_box(aead::generate_nonce()));
    });
}

// ── X25519 Diffie-Hellman ───────────────────────────────────────

fn bench_x25519_keygen(c: &mut Criterion) {
    c.bench_function("x25519_keygen", |b| {
        b.iter(|| black_box(KeyPair::generate()));
    });
}

fn bench_x25519_diffie_hellman(c: &mut Criterion) {
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    c.bench_function("x25519_diffie_hellman", |b| {
        b.iter(|| {
            x25519::diffie_hellman(black_box(&alice.secret_key()), black_box(&bob.public_key()))
                .unwrap()
        });
    });
}

// ── ML-KEM (Post-Quantum KEM) ──────────────────────────────────

fn bench_kem_keygen(c: &mut Criterion) {
    c.bench_function("mlkem768_keygen", |b| {
        b.iter(|| black_box(KemKeyPair::generate()));
    });
}

fn bench_kem_encapsulate(c: &mut Criterion) {
    let kp = KemKeyPair::generate();
    let pk = kp.public_key();

    c.bench_function("mlkem768_encapsulate", |b| {
        b.iter(|| pk.encapsulate().unwrap());
    });
}

fn bench_kem_decapsulate(c: &mut Criterion) {
    let kp = KemKeyPair::generate();
    let (ct, _) = kp.public_key().encapsulate().unwrap();

    c.bench_function("mlkem768_decapsulate", |b| {
        b.iter(|| kp.decapsulate(black_box(&ct)).unwrap());
    });
}

// ── ML-DSA (Post-Quantum Signatures) ───────────────────────────

fn bench_signing_keygen(c: &mut Criterion) {
    c.bench_function("mldsa65_keygen", |b| {
        b.iter(|| black_box(SigningKeyPair::generate()));
    });
}

fn bench_signing_sign(c: &mut Criterion) {
    let kp = SigningKeyPair::generate();
    let message = b"This is a message to be signed for authentication.";

    c.bench_function("mldsa65_sign", |b| {
        b.iter(|| kp.sign(black_box(message)).unwrap());
    });
}

fn bench_signing_verify(c: &mut Criterion) {
    let kp = SigningKeyPair::generate();
    let message = b"This is a message to be signed for authentication.";
    let sig = kp.sign(message).unwrap();
    let vk = kp.verifying_key();

    c.bench_function("mldsa65_verify", |b| {
        b.iter(|| vk.verify(black_box(message), black_box(&sig)).unwrap());
    });
}

// ── HKDF Key Derivation ────────────────────────────────────────

fn bench_hkdf_extract(c: &mut Criterion) {
    let salt = [0x42u8; 32];
    let ikm = [0xABu8; 32];

    c.bench_function("hkdf_extract", |b| {
        b.iter(|| kdf::hkdf_extract(black_box(&salt), black_box(&ikm)));
    });
}

fn bench_hkdf_expand(c: &mut Criterion) {
    let prk = kdf::hkdf_extract(&[0x42u8; 32], &[0xABu8; 32]);

    c.bench_function("hkdf_expand_32B", |b| {
        b.iter(|| kdf::hkdf_expand(black_box(&prk), black_box(b"info"), 32).unwrap());
    });
}

fn bench_hkdf_expand_into(c: &mut Criterion) {
    let prk = kdf::hkdf_extract(&[0x42u8; 32], &[0xABu8; 32]);

    c.bench_function("hkdf_expand_into_32B", |b| {
        b.iter(|| {
            let mut out = [0u8; 32];
            kdf::hkdf_expand_into(black_box(&prk), black_box(b"info"), &mut out).unwrap();
            out
        });
    });
}

fn bench_kdf_chain(c: &mut Criterion) {
    let chain_key = [0xAAu8; 32];

    c.bench_function("kdf_chain", |b| {
        b.iter(|| kdf::kdf_chain(black_box(&chain_key), black_box(&[])));
    });
}

fn bench_derive_key(c: &mut Criterion) {
    let salt = [0x42u8; 32];
    let ikm = [0xABu8; 32];

    c.bench_function("derive_key_32B", |b| {
        b.iter(|| {
            kdf::derive_key(black_box(&salt), black_box(&ikm), black_box(b"info"), 32).unwrap()
        });
    });
}

fn bench_derive_key_into(c: &mut Criterion) {
    let salt = [0x42u8; 32];
    let ikm = [0xABu8; 32];

    c.bench_function("derive_key_into_32B", |b| {
        b.iter(|| {
            let mut out = [0u8; 32];
            kdf::derive_key_into(
                black_box(&salt),
                black_box(&ikm),
                black_box(b"info"),
                &mut out,
            )
            .unwrap();
            out
        });
    });
}

fn bench_derive_key_96(c: &mut Criterion) {
    let salt = [0x42u8; 32];
    let ikm = [0xABu8; 32];

    c.bench_function("derive_key_into_96B", |b| {
        b.iter(|| {
            let mut out = [0u8; 96];
            kdf::derive_key_into(
                black_box(&salt),
                black_box(&ikm),
                black_box(b"root-kdf"),
                &mut out,
            )
            .unwrap();
            out
        });
    });
}

// ── X3DH Key Exchange ──────────────────────────────────────────

fn make_receiver_bundle() -> (
    IdentityKeyPair,
    SignedPreKey,
    KemPreKey,
    OneTimePreKey,
    PreKeyBundle,
) {
    let identity = IdentityKeyPair::generate();
    let spk = SignedPreKey::generate(1, &identity, 1_700_000_000).expect("spk failed");
    let kpk = KemPreKey::generate(1, &identity).expect("kpk failed");
    let otpk = OneTimePreKey::generate(1);

    let bundle = PreKeyBundle::new(
        identity.public_identity(),
        spk.to_public(),
        kpk.to_public(),
        Some(otpk.to_public()),
    );

    (identity, spk, kpk, otpk, bundle)
}

fn bench_x3dh_sender(c: &mut Criterion) {
    let alice_identity = IdentityKeyPair::generate();
    let (_, _, _, _, bob_bundle) = make_receiver_bundle();

    c.bench_function("x3dh_sender", |b| {
        b.iter(|| x3dh::x3dh_sender(black_box(&alice_identity), black_box(&bob_bundle)).unwrap());
    });
}

fn bench_x3dh_receiver(c: &mut Criterion) {
    let alice_identity = IdentityKeyPair::generate();
    let (bob_identity, bob_spk, bob_kpk, bob_otpk, bob_bundle) = make_receiver_bundle();

    let (_, init_msg) = x3dh::x3dh_sender(&alice_identity, &bob_bundle).expect("sender failed");

    c.bench_function("x3dh_receiver", |b| {
        b.iter(|| {
            x3dh::x3dh_receiver(
                black_box(&bob_identity),
                black_box(&bob_spk),
                black_box(&bob_kpk),
                black_box(Some(&bob_otpk)),
                black_box(&init_msg),
            )
            .unwrap()
        });
    });
}

fn bench_x3dh_full_exchange(c: &mut Criterion) {
    c.bench_function("x3dh_full_exchange", |b| {
        b.iter(|| {
            let alice_identity = IdentityKeyPair::generate();
            let (bob_identity, bob_spk, bob_kpk, bob_otpk, bob_bundle) = make_receiver_bundle();
            let (_, init_msg) = x3dh::x3dh_sender(&alice_identity, &bob_bundle).unwrap();
            x3dh::x3dh_receiver(&bob_identity, &bob_spk, &bob_kpk, Some(&bob_otpk), &init_msg)
                .unwrap()
        });
    });
}

// ── Triple Ratchet Session ─────────────────────────────────────

fn make_session_pair() -> (TripleRatchetSession, TripleRatchetSession) {
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();
    let secret = [0x42u8; 32];

    let alice =
        TripleRatchetSession::init_sender(&secret, bob_dh.public_key(), bob_kem.public_key())
            .expect("init sender failed");
    let bob = TripleRatchetSession::init_receiver(&secret, bob_dh, bob_kem)
        .expect("init receiver failed");

    (alice, bob)
}

fn bench_triple_ratchet_encrypt(c: &mut Criterion) {
    let (mut alice, _bob) = make_session_pair();
    let plaintext = b"Hello, Bob! This is a typical chat message.";

    c.bench_function("triple_ratchet_encrypt", |b| {
        b.iter(|| alice.encrypt(black_box(plaintext)).unwrap());
    });
}

fn bench_triple_ratchet_decrypt(c: &mut Criterion) {
    let (mut alice, mut bob) = make_session_pair();
    // Pre-generate a batch of messages to decrypt
    let messages: Vec<_> = (0..100)
        .map(|i| {
            alice
                .encrypt(format!("message {i}").as_bytes())
                .unwrap()
        })
        .collect();

    let mut idx = 0;
    c.bench_function("triple_ratchet_decrypt", |b| {
        b.iter(|| {
            let msg = &messages[idx % messages.len()];
            let _ = bob.decrypt(black_box(msg));
            idx += 1;
        });
    });
}

fn bench_triple_ratchet_roundtrip(c: &mut Criterion) {
    c.bench_function("triple_ratchet_roundtrip", |b| {
        let (mut alice, mut bob) = make_session_pair();
        let plaintext = b"Hello, Bob!";
        b.iter(|| {
            let msg = alice.encrypt(black_box(plaintext)).unwrap();
            bob.decrypt(black_box(&msg)).unwrap()
        });
    });
}

fn bench_triple_ratchet_init_sender(c: &mut Criterion) {
    let bob_dh = KeyPair::generate();
    let bob_kem = KemKeyPair::generate();
    let secret = [0x42u8; 32];
    let bob_dh_pub = bob_dh.public_key();
    let bob_kem_pub = bob_kem.public_key();

    c.bench_function("triple_ratchet_init_sender", |b| {
        b.iter(|| {
            TripleRatchetSession::init_sender(
                black_box(&secret),
                black_box(bob_dh_pub),
                black_box(bob_kem_pub.clone()),
            )
            .unwrap()
        });
    });
}

fn bench_triple_ratchet_init_receiver(c: &mut Criterion) {
    let secret = [0x42u8; 32];

    c.bench_function("triple_ratchet_init_receiver", |b| {
        b.iter(|| {
            let bob_dh = KeyPair::generate();
            let bob_kem = KemKeyPair::generate();
            TripleRatchetSession::init_receiver(
                black_box(&secret),
                black_box(bob_dh),
                black_box(bob_kem),
            )
            .unwrap()
        });
    });
}

// ── Header Encryption ──────────────────────────────────────────

fn bench_header_encrypt(c: &mut Criterion) {
    use pirc_crypto::header::{self, HeaderKey};
    use pirc_crypto::message::MessageHeader;

    let key = HeaderKey::from_bytes([0x42u8; 32]);
    let header = MessageHeader {
        dh_public: KeyPair::generate().public_key(),
        message_number: 42,
        previous_chain_length: 7,
        kem_ciphertext: None,
        kem_public: None,
    };

    c.bench_function("header_encrypt", |b| {
        b.iter(|| header::encrypt_header(black_box(&key), black_box(&header)).unwrap());
    });
}

fn bench_header_decrypt(c: &mut Criterion) {
    use pirc_crypto::header::{self, HeaderKey};
    use pirc_crypto::message::MessageHeader;

    let key = HeaderKey::from_bytes([0x42u8; 32]);
    let header = MessageHeader {
        dh_public: KeyPair::generate().public_key(),
        message_number: 42,
        previous_chain_length: 7,
        kem_ciphertext: None,
        kem_public: None,
    };
    let (encrypted, nonce) = header::encrypt_header(&key, &header).unwrap();

    c.bench_function("header_decrypt", |b| {
        b.iter(|| {
            header::decrypt_header(black_box(&key), black_box(&encrypted), black_box(&nonce))
                .unwrap()
        });
    });
}

criterion_group!(
    benches,
    // AEAD
    bench_aead_encrypt,
    bench_aead_decrypt,
    bench_aead_encrypt_1kb,
    bench_aead_decrypt_1kb,
    bench_aead_nonce_generation,
    // X25519
    bench_x25519_keygen,
    bench_x25519_diffie_hellman,
    // ML-KEM
    bench_kem_keygen,
    bench_kem_encapsulate,
    bench_kem_decapsulate,
    // ML-DSA
    bench_signing_keygen,
    bench_signing_sign,
    bench_signing_verify,
    // HKDF
    bench_hkdf_extract,
    bench_hkdf_expand,
    bench_hkdf_expand_into,
    bench_kdf_chain,
    bench_derive_key,
    bench_derive_key_into,
    bench_derive_key_96,
    // X3DH
    bench_x3dh_sender,
    bench_x3dh_receiver,
    bench_x3dh_full_exchange,
    // Triple Ratchet
    bench_triple_ratchet_encrypt,
    bench_triple_ratchet_decrypt,
    bench_triple_ratchet_roundtrip,
    bench_triple_ratchet_init_sender,
    bench_triple_ratchet_init_receiver,
    // Header encryption
    bench_header_encrypt,
    bench_header_decrypt,
);
criterion_main!(benches);
