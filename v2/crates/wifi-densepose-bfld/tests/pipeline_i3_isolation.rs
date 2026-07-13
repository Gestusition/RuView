//! End-to-end ADR-118 invariant I3 + ADR-120 §2.7 AC2 proof at the public
//! `BfldPipeline` surface — not just inside `SignatureHasher`. Validates that
//! the same physical person at:
//!
//! - **Different sites** produces uncorrelated `rf_signature_hash` values.
//! - **Different days** at the same site rotates the hash.
//! - **30 days apart** at the same site produces a different hash (the
//!   rotation isn't a one-bit difference; the whole digest changes).
//!
//! All assertions go through `BfldPipeline::process()` so the test exercises
//! the wired-up emitter + hasher + identity_features encoder path, not the
//! lower-level `SignatureHasher::compute` direct API.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, IdentityEmbedding, PrivacyClass, SensingInputs, SignatureHasher,
    EMBEDDING_DIM, SITE_SALT_LEN,
};

const SECONDS_PER_DAY: u64 = 86_400;
const NS_PER_SEC: u64 = 1_000_000_000;

fn salt(seed: u8) -> [u8; SITE_SALT_LEN] {
    let mut s = [0u8; SITE_SALT_LEN];
    for (i, b) in s.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8);
    }
    s
}

fn person_embedding() -> IdentityEmbedding {
    // A deterministic "person" — same vector across all sites and days in
    // the test so we're only varying salt + day_epoch.
    let mut a = [0.0f32; EMBEDDING_DIM];
    for (i, v) in a.iter_mut().enumerate() {
        *v = ((i as f32) * 0.0073).sin();
    }
    IdentityEmbedding::from_raw(a)
}

fn inputs_at(unix_secs: u64) -> SensingInputs {
    SensingInputs {
        timestamp_ns: unix_secs * NS_PER_SEC,
        presence: true,
        motion: 0.4,
        person_count: 1,
        sensing_confidence: 0.9,
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: None, // hasher derives
    }
}

fn pipeline_with_salt(node_id: &str, salt: [u8; SITE_SALT_LEN]) -> BfldPipeline {
    BfldPipeline::new(BfldConfig::new(node_id).with_signature_hasher(SignatureHasher::new(salt)))
}

fn hash_for(p: &mut BfldPipeline, unix_secs: u64) -> [u8; 32] {
    p.process(inputs_at(unix_secs), Some(person_embedding()))
        .expect("low-risk emit must succeed")
        .rf_signature_hash
        .expect("hasher-equipped pipeline must emit a hash")
}

fn hamming_distance(a: &[u8; 32], b: &[u8; 32]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

// --- cross-site (same person, same day, different salt) -----------------

#[test]
fn same_person_at_different_sites_same_day_produces_different_hashes() {
    let mut site_a = pipeline_with_salt("seed-a", salt(1));
    let mut site_b = pipeline_with_salt("seed-b", salt(2));
    let day_0_secs = 1_700_000_000;
    let h_a = hash_for(&mut site_a, day_0_secs);
    let h_b = hash_for(&mut site_b, day_0_secs);
    assert_ne!(h_a, h_b);
}

// --- same site, different days ------------------------------------------

#[test]
fn same_person_same_site_different_day_rotates_the_hash() {
    let mut site = pipeline_with_salt("seed-a", salt(1));
    let day_0 = 1_700_000_000;
    let day_1 = day_0 + SECONDS_PER_DAY;
    let h_0 = hash_for(&mut site, day_0);
    let h_1 = hash_for(&mut site, day_1);
    assert_ne!(
        h_0, h_1,
        "day rotation must change the hash at the pipeline surface"
    );
}

#[test]
fn thirty_day_gap_produces_thoroughly_different_hash() {
    let mut site = pipeline_with_salt("seed-a", salt(1));
    let day_0 = 1_700_000_000;
    let day_30 = day_0 + 30 * SECONDS_PER_DAY;
    let h_0 = hash_for(&mut site, day_0);
    let h_30 = hash_for(&mut site, day_30);
    let dist = hamming_distance(&h_0, &h_30);
    // Two independent BLAKE3 outputs differ by ~128 bits on average. Require
    // at least 80 bits to catch a regression where day_epoch is only weakly
    // mixed into the digest.
    assert!(
        dist >= 80,
        "30-day rotation Hamming distance too low: {dist}"
    );
}

// --- same person, same site, same day -> stable hash --------------------

#[test]
fn same_person_same_site_same_day_produces_stable_hash() {
    let mut a = pipeline_with_salt("seed-a", salt(1));
    let mut b = pipeline_with_salt("seed-a", salt(1));
    let day_0 = 1_700_000_000;
    assert_eq!(hash_for(&mut a, day_0), hash_for(&mut b, day_0));
}

// --- cross-site Hamming distance at the pipeline surface ----------------

#[test]
fn cross_site_hamming_distance_at_pipeline_surface_is_statistically_high() {
    let n_trials = 32usize;
    let mut total: u32 = 0;
    let day_0 = 1_700_000_000;
    for trial in 0..n_trials {
        let mut a = pipeline_with_salt("seed-a", salt(trial as u8));
        let mut b = pipeline_with_salt("seed-b", salt((trial as u8).wrapping_add(0xA5)));
        let dist = hamming_distance(&hash_for(&mut a, day_0), &hash_for(&mut b, day_0));
        total += dist;
    }
    let mean = total as f32 / n_trials as f32;
    assert!(
        mean >= 120.0,
        "pipeline-surface cross-site mean Hamming distance must be >= 120 (ADR-120 §2.7 AC2), got {mean}",
    );
}

// --- restricted class still rotates internally even though hash is stripped ---

#[test]
fn restricted_class_strips_hash_but_pipeline_state_advances() {
    // Class 3 strips rf_signature_hash from the event, but the underlying
    // pipeline state (ring, gate) still advances. This test pins that
    // contract so a future PR doesn't accidentally short-circuit the
    // pipeline at class 3 and miss legitimate sensing.
    let mut p = BfldPipeline::new(
        BfldConfig::new("seed-r")
            .with_privacy_class(PrivacyClass::Restricted)
            .with_signature_hasher(SignatureHasher::new(salt(7))),
    );
    let evt = p
        .process(inputs_at(1_700_000_000), Some(person_embedding()))
        .expect("low-risk emit");
    assert!(evt.rf_signature_hash.is_none());
    assert!(evt.identity_risk_score.is_none());
    assert!(evt.presence); // sensing fields still landed
}

// --- pipeline without hasher leaves hash as None or caller-supplied ----

#[test]
fn pipeline_without_signature_hasher_does_not_invent_a_hash() {
    let mut p = BfldPipeline::new(BfldConfig::new("seed-x"));
    let evt = p
        .process(inputs_at(1_700_000_000), Some(person_embedding()))
        .expect("low-risk emit");
    assert!(
        evt.rf_signature_hash.is_none(),
        "no hasher installed → no hash; got {:?}",
        evt.rf_signature_hash,
    );
}
