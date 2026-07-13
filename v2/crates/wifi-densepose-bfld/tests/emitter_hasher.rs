//! Acceptance tests for ADR-120 §2.3 ↔ ADR-118 §2.1 wiring — `SignatureHasher`
//! derives `rf_signature_hash` end-to-end through `BfldEmitter`.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    BfldEmitter, IdentityEmbedding, SensingInputs, SignatureHasher, EMBEDDING_DIM, SITE_SALT_LEN,
};

fn salt(seed: u8) -> [u8; SITE_SALT_LEN] {
    let mut s = [0u8; SITE_SALT_LEN];
    for (i, b) in s.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8);
    }
    s
}

fn embedding(seed: u8) -> IdentityEmbedding {
    let mut a = [0.0f32; EMBEDDING_DIM];
    for (i, v) in a.iter_mut().enumerate() {
        *v = (i as f32 + seed as f32) * 0.001;
    }
    IdentityEmbedding::from_raw(a)
}

fn inputs(seed: u8) -> SensingInputs {
    SensingInputs {
        timestamp_ns: 1_700_000_000_000_000_000 + (seed as u64) * 1_000_000_000,
        presence: true,
        motion: 0.5,
        person_count: 1,
        sensing_confidence: 0.9,
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: Some([0xFF; 32]), // caller-supplied "wrong" hash
    }
}

#[test]
fn no_hasher_passes_caller_supplied_hash_through() {
    let mut e = BfldEmitter::new("seed-01");
    let out = e.emit(inputs(0), Some(embedding(0))).unwrap();
    assert_eq!(out.rf_signature_hash, Some([0xFF; 32]));
}

#[test]
fn installed_hasher_overrides_caller_supplied_hash() {
    let mut e = BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(7)));
    let out = e.emit(inputs(0), Some(embedding(0))).unwrap();
    let hash = out.rf_signature_hash.unwrap();
    assert_ne!(
        hash, [0xFF; 32],
        "derived hash must override caller-supplied"
    );
    assert_ne!(hash, [0x00; 32], "derived hash must be non-trivial");
}

#[test]
fn same_emitter_same_inputs_produce_same_hash() {
    let mut e_a = BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(7)));
    let mut e_b = BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(7)));
    let a = e_a.emit(inputs(0), Some(embedding(0))).unwrap();
    let b = e_b.emit(inputs(0), Some(embedding(0))).unwrap();
    assert_eq!(a.rf_signature_hash, b.rf_signature_hash);
}

#[test]
fn different_site_salts_produce_different_hashes_end_to_end() {
    let mut e_a = BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(1)));
    let mut e_b = BfldEmitter::new("seed-02").with_signature_hasher(SignatureHasher::new(salt(2)));
    // Same embedding, same inputs → different sites must produce different hashes.
    let a = e_a.emit(inputs(0), Some(embedding(0))).unwrap();
    let b = e_b.emit(inputs(0), Some(embedding(0))).unwrap();
    assert_ne!(
        a.rf_signature_hash, b.rf_signature_hash,
        "cross-site emit must produce uncorrelated hashes",
    );
}

#[test]
fn no_embedding_falls_back_to_risk_factor_bytes() {
    let mut e = BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(5)));
    let out = e.emit(inputs(0), None).unwrap();
    let hash = out.rf_signature_hash.unwrap();
    assert_ne!(hash, [0xFF; 32]); // still derived (fallback path), not caller-supplied
}

#[test]
fn fallback_hash_differs_from_embedding_hash() {
    let mut e_with =
        BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(9)));
    let mut e_without =
        BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt(9)));
    let with_emb = e_with.emit(inputs(0), Some(embedding(0))).unwrap();
    let no_emb = e_without.emit(inputs(0), None).unwrap();
    assert_ne!(
        with_emb.rf_signature_hash, no_emb.rf_signature_hash,
        "embedding bytes and risk-factor bytes should hash to different values",
    );
}
