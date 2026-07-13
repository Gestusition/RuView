//! Pipeline event-stream determinism. Operators capturing BFI for offline
//! analysis need the guarantee that **two pipelines with identical config +
//! salt + input streams produce byte-identical event JSON sequences**.
//! Without this, replay-driven regression testing across BFLD versions is
//! impossible.
//!
//! This is the cross-pipeline counterpart to iter 31's I3 isolation test
//! (which proves hash *differences* across sites/days); here we prove hash
//! *and full-event* equality across two pipeline instances with matching
//! configuration.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    BfldConfig, BfldEvent, BfldPipeline, IdentityEmbedding, PrivacyClass, SensingInputs,
    SignatureHasher, EMBEDDING_DIM, SITE_SALT_LEN,
};

const NS_PER_SEC: u64 = 1_000_000_000;

fn salt() -> [u8; SITE_SALT_LEN] {
    let mut s = [0u8; SITE_SALT_LEN];
    for (i, b) in s.iter_mut().enumerate() {
        *b = i as u8;
    }
    s
}

fn person_embedding(seed: f32) -> IdentityEmbedding {
    let mut a = [0.0f32; EMBEDDING_DIM];
    for (i, v) in a.iter_mut().enumerate() {
        *v = (seed + i as f32) * 0.0073;
    }
    IdentityEmbedding::from_raw(a)
}

fn inputs_at(unix_secs: u64, motion: f32) -> SensingInputs {
    SensingInputs {
        timestamp_ns: unix_secs * NS_PER_SEC,
        presence: true,
        motion,
        person_count: 1,
        sensing_confidence: 0.91,
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: None,
    }
}

fn fresh_pipeline() -> BfldPipeline {
    BfldPipeline::new(
        BfldConfig::new("seed-det").with_signature_hasher(SignatureHasher::new(salt())),
    )
}

fn drive(p: &mut BfldPipeline, n: usize) -> Vec<BfldEvent> {
    (0..n)
        .map(|i| {
            let secs = 1_700_000_000 + i as u64;
            let motion = 0.1 + (i as f32) * 0.1;
            p.process(inputs_at(secs, motion), Some(person_embedding(i as f32)))
                .expect("low-risk emit")
        })
        .collect()
}

#[test]
fn two_pipelines_with_identical_config_produce_identical_event_streams() {
    let mut a = fresh_pipeline();
    let mut b = fresh_pipeline();
    let n = 5;
    let events_a = drive(&mut a, n);
    let events_b = drive(&mut b, n);
    assert_eq!(events_a.len(), n);
    assert_eq!(events_b.len(), n);
    for (i, (ea, eb)) in events_a.iter().zip(events_b.iter()).enumerate() {
        assert_eq!(ea.timestamp_ns, eb.timestamp_ns, "event[{i}] ts differs");
        assert_eq!(ea.presence, eb.presence, "event[{i}] presence differs");
        assert_eq!(ea.motion, eb.motion, "event[{i}] motion differs");
        assert_eq!(ea.person_count, eb.person_count);
        assert_eq!(ea.confidence, eb.confidence);
        assert_eq!(ea.zone_id, eb.zone_id);
        assert_eq!(ea.privacy_class, eb.privacy_class);
        assert_eq!(ea.identity_risk_score, eb.identity_risk_score);
        assert_eq!(ea.rf_signature_hash, eb.rf_signature_hash);
    }
}

#[cfg(feature = "serde-json")]
#[test]
fn two_pipelines_produce_byte_identical_event_json_streams() {
    let mut a = fresh_pipeline();
    let mut b = fresh_pipeline();
    let n = 5;
    let json_a: Vec<String> = drive(&mut a, n)
        .iter()
        .map(|e| e.to_json().unwrap())
        .collect();
    let json_b: Vec<String> = drive(&mut b, n)
        .iter()
        .map(|e| e.to_json().unwrap())
        .collect();
    assert_eq!(json_a, json_b, "event JSON streams must be byte-identical");
    // Sanity: each JSON includes the derived hash field, so the equality is
    // covering the salt/day/embedding → hash path too.
    assert!(json_a.iter().all(|j| j.contains("rf_signature_hash")));
}

#[test]
fn replaying_same_input_sequence_after_pipeline_reset_reproduces_events() {
    // Same instance, two passes: build → drive → record → drop → rebuild →
    // drive → record → compare. Catches any accidental hidden state that
    // wouldn't be carried in BfldConfig but would still influence output.
    let n = 5;
    let pass_a = drive(&mut fresh_pipeline(), n);
    let pass_b = drive(&mut fresh_pipeline(), n);
    for (i, (ea, eb)) in pass_a.iter().zip(pass_b.iter()).enumerate() {
        assert_eq!(
            ea.rf_signature_hash, eb.rf_signature_hash,
            "rf_signature_hash differs at event[{i}] across pipeline rebuilds",
        );
    }
}

#[test]
fn different_input_sequences_diverge_after_the_first_difference() {
    let mut a = fresh_pipeline();
    let mut b = fresh_pipeline();
    // First two inputs identical:
    let ea0 = a
        .process(inputs_at(1_700_000_000, 0.1), Some(person_embedding(0.0)))
        .unwrap();
    let eb0 = b
        .process(inputs_at(1_700_000_000, 0.1), Some(person_embedding(0.0)))
        .unwrap();
    assert_eq!(ea0.rf_signature_hash, eb0.rf_signature_hash);
    // Third input differs in embedding:
    let ea1 = a
        .process(inputs_at(1_700_000_001, 0.2), Some(person_embedding(1.0)))
        .unwrap();
    let eb1 = b
        .process(inputs_at(1_700_000_001, 0.2), Some(person_embedding(99.0)))
        .unwrap();
    assert_ne!(
        ea1.rf_signature_hash, eb1.rf_signature_hash,
        "different embeddings must produce different hashes",
    );
}

#[test]
fn class_3_pipelines_produce_identical_stripped_event_streams() {
    // Determinism property must hold across privacy classes too — operators
    // running Restricted deployments should be able to replay captures and
    // see the same (stripped) event sequences.
    let make = || {
        BfldPipeline::new(
            BfldConfig::new("seed-r3")
                .with_privacy_class(PrivacyClass::Restricted)
                .with_signature_hasher(SignatureHasher::new(salt())),
        )
    };
    let mut a = make();
    let mut b = make();
    let n = 3;
    let events_a = drive(&mut a, n);
    let events_b = drive(&mut b, n);
    for (i, (ea, eb)) in events_a.iter().zip(events_b.iter()).enumerate() {
        assert!(ea.identity_risk_score.is_none(), "event[{i}] class-3 strip");
        assert!(ea.rf_signature_hash.is_none(), "event[{i}] class-3 strip");
        assert_eq!(
            ea.motion, eb.motion,
            "event[{i}] motion still deterministic"
        );
        assert_eq!(ea.presence, eb.presence);
    }
}
