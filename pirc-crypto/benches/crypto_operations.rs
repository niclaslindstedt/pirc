use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pirc_crypto::aead;
use pirc_crypto::kem::KemKeyPair;
use pirc_crypto::signing::SigningKeyPair;
use pirc_crypto::triple_ratchet::TripleRatchetSession;
use pirc_crypto::x25519::{self, KeyPair};

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

criterion_group!(
    benches,
    bench_aead_encrypt,
    bench_aead_decrypt,
    bench_aead_encrypt_1kb,
    bench_aead_decrypt_1kb,
    bench_aead_nonce_generation,
    bench_x25519_keygen,
    bench_x25519_diffie_hellman,
    bench_kem_keygen,
    bench_kem_encapsulate,
    bench_kem_decapsulate,
    bench_signing_keygen,
    bench_signing_sign,
    bench_signing_verify,
    bench_triple_ratchet_encrypt,
    bench_triple_ratchet_decrypt,
    bench_triple_ratchet_roundtrip,
    bench_triple_ratchet_init_sender,
);
criterion_main!(benches);
