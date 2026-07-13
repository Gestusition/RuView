//! Acceptance tests for ADR-120 §2.3 — `IdentityFeatures` canonical-bytes encoder.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    IdentityEmbedding, IdentityFeatures, SignatureHasher, EMBEDDING_DIM, RISK_FACTOR_BYTES,
    SITE_SALT_LEN,
};

fn embedding(seed: f32) -> IdentityEmbedding {
    let mut a = [0.0f32; EMBEDDING_DIM];
    for (i, v) in a.iter_mut().enumerate() {
        *v = seed + (i as f32) * 0.001;
    }
    IdentityEmbedding::from_raw(a)
}

fn salt() -> [u8; SITE_SALT_LEN] {
    [42u8; SITE_SALT_LEN]
}

// --- byte layout ----------------------------------------------------------

#[test]
fn embedding_canonical_length_is_dim_times_four() {
    let emb = embedding(0.5);
    let f = IdentityFeatures::from_embedding(&emb);
    assert_eq!(f.canonical_byte_len(), EMBEDDING_DIM * 4);
    assert_eq!(f.canonical_bytes().len(), EMBEDDING_DIM * 4);
}

#[test]
fn risk_factor_canonical_length_is_sixteen_bytes() {
    let f = IdentityFeatures::from_risk_factors(0.1, 0.2, 0.3, 0.4);
    assert_eq!(f.canonical_byte_len(), RISK_FACTOR_BYTES);
    assert_eq!(f.canonical_byte_len(), 16);
    assert_eq!(f.canonical_bytes().len(), 16);
}

#[test]
fn embedding_canonical_bytes_match_manual_flatten() {
    let emb = embedding(0.7);
    let f = IdentityFeatures::from_embedding(&emb);
    let actual = f.canonical_bytes();
    let expected: Vec<u8> = emb
        .as_slice()
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn risk_factor_canonical_bytes_match_explicit_le_layout() {
    let f = IdentityFeatures::from_risk_factors(0.1, 0.2, 0.3, 0.4);
    let actual = f.canonical_bytes();
    let mut expected = Vec::with_capacity(16);
    expected.extend_from_slice(&0.1f32.to_le_bytes());
    expected.extend_from_slice(&0.2f32.to_le_bytes());
    expected.extend_from_slice(&0.3f32.to_le_bytes());
    expected.extend_from_slice(&0.4f32.to_le_bytes());
    assert_eq!(actual, expected);
}

#[test]
fn write_canonical_bytes_appends_to_existing_buffer() {
    let f = IdentityFeatures::from_risk_factors(1.0, 2.0, 3.0, 4.0);
    let mut buf = vec![0xAA, 0xBB];
    f.write_canonical_bytes(&mut buf);
    assert_eq!(buf.len(), 2 + 16);
    assert_eq!(&buf[..2], &[0xAA, 0xBB]);
}

// --- hash integration ----------------------------------------------------

#[test]
fn compute_hash_matches_direct_hasher_invocation() {
    let h = SignatureHasher::new(salt());
    let emb = embedding(0.5);
    let f = IdentityFeatures::from_embedding(&emb);
    let via_features = f.compute_hash(&h, 100);
    let via_direct = h.compute(100, &f.canonical_bytes());
    assert_eq!(via_features, via_direct);
}

#[test]
fn embedding_and_risk_factors_produce_different_hashes() {
    let h = SignatureHasher::new(salt());
    let emb = embedding(0.5);
    let from_emb = IdentityFeatures::from_embedding(&emb).compute_hash(&h, 100);
    let from_rf = IdentityFeatures::from_risk_factors(0.5, 0.5, 0.5, 0.5).compute_hash(&h, 100);
    assert_ne!(
        from_emb, from_rf,
        "embedding and risk-factor encoders must produce distinct hashes",
    );
}

// --- backward compatibility regression (iter 16 wire format) -------------

/// Iter 16 used inline `emb.as_slice().iter().flat_map(|f| f.to_le_bytes())`
/// for the embedding path. Iter 18's IdentityFeatures must produce the
/// exact same hash for the same (salt, day, embedding) tuple — otherwise
/// existing nodes would silently flip their `rf_signature_hash` value on
/// upgrade.
#[test]
fn iter_16_wire_compat_embedding_path() {
    let h = SignatureHasher::new(salt());
    let emb = embedding(0.9);
    let day_epoch = 12345;

    // Iter 16 manual computation:
    let bytes_v16: Vec<u8> = emb
        .as_slice()
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let hash_v16 = h.compute(day_epoch, &bytes_v16);

    // Iter 18 IdentityFeatures path:
    let hash_v18 = IdentityFeatures::from_embedding(&emb).compute_hash(&h, day_epoch);

    assert_eq!(
        hash_v16, hash_v18,
        "iter 18 must produce iter-16 wire-compatible hashes",
    );
}

#[test]
fn iter_16_wire_compat_risk_factor_path() {
    let h = SignatureHasher::new(salt());
    let day_epoch = 12345;
    let (sep, stab, consist, conf) = (0.1f32, 0.2f32, 0.3f32, 0.4f32);

    // Iter 16 manual computation:
    let mut buf_v16 = [0u8; 16];
    buf_v16[0..4].copy_from_slice(&sep.to_le_bytes());
    buf_v16[4..8].copy_from_slice(&stab.to_le_bytes());
    buf_v16[8..12].copy_from_slice(&consist.to_le_bytes());
    buf_v16[12..16].copy_from_slice(&conf.to_le_bytes());
    let hash_v16 = h.compute(day_epoch, &buf_v16);

    // Iter 18 path:
    let hash_v18 =
        IdentityFeatures::from_risk_factors(sep, stab, consist, conf).compute_hash(&h, day_epoch);

    assert_eq!(hash_v16, hash_v18);
}
