//! End-to-end pipeline tests for `BfldEmitter`. ADR-118 §2.1.

#![cfg(feature = "std")]

use wifi_densepose_bfld::coherence_gate::DEBOUNCE_NS;
use wifi_densepose_bfld::{
    BfldEmitter, GateAction, IdentityEmbedding, PrivacyClass, SensingInputs, EMBEDDING_DIM,
};

fn inputs(ts_ns: u64, risk_factors: [f32; 4]) -> SensingInputs {
    let [sep, stab, consist, risk_conf] = risk_factors;
    SensingInputs {
        timestamp_ns: ts_ns,
        presence: true,
        motion: 0.5,
        person_count: 1,
        sensing_confidence: 0.9,
        sep,
        stab,
        consist,
        risk_conf,
        rf_signature_hash: Some([0xCD; 32]),
    }
}

fn dummy_embedding() -> IdentityEmbedding {
    IdentityEmbedding::from_raw([0.1; EMBEDDING_DIM])
}

#[test]
fn emitter_emits_event_under_low_risk() {
    let mut e = BfldEmitter::new("seed-01");
    let out = e
        .emit(inputs(0, [0.2, 0.2, 0.2, 0.2]), Some(dummy_embedding()))
        .expect("low risk should produce an event");
    assert_eq!(out.node_id, "seed-01");
    assert!(out.presence);
    assert!(out.identity_risk_score.is_some());
    assert_eq!(e.current_action(), GateAction::Accept);
}

#[test]
fn emitter_drops_event_under_sustained_high_risk() {
    let mut e = BfldEmitter::new("seed-01");
    // First call: score ~ 0.7 pending Reject. Event still emits this turn
    // because the gate hasn't promoted yet (current is still Accept).
    let first = e.emit(inputs(0, [1.0, 1.0, 1.0, 0.8]), Some(dummy_embedding()));
    assert!(first.is_some(), "first high-risk call still emits");
    // After debounce: current becomes Reject -> event dropped.
    let after = e.emit(
        inputs(DEBOUNCE_NS, [1.0, 1.0, 1.0, 0.8]),
        Some(dummy_embedding()),
    );
    assert!(after.is_none(), "post-debounce Reject drops the event");
    assert_eq!(e.current_action(), GateAction::Reject);
}

#[test]
fn emitter_drains_ring_on_recalibrate() {
    let mut e = BfldEmitter::new("seed-01");
    // Pump 5 embeddings under a slow rising score so the ring fills.
    for i in 0..5 {
        let _ = e.emit(
            inputs(i * 1_000_000, [0.3, 0.3, 0.3, 0.3]),
            Some(dummy_embedding()),
        );
    }
    assert_eq!(e.ring_len(), 5);

    // Now push a Recalibrate-grade score and run past debounce.
    e.emit(
        inputs(10_000_000, [1.0, 1.0, 1.0, 1.0]),
        Some(dummy_embedding()),
    );
    let _ = e.emit(
        inputs(10_000_000 + DEBOUNCE_NS, [1.0, 1.0, 1.0, 1.0]),
        Some(dummy_embedding()),
    );
    assert_eq!(e.current_action(), GateAction::Recalibrate);
    assert_eq!(e.ring_len(), 0, "Recalibrate must drain the embedding ring");
}

#[test]
fn restricted_class_strips_identity_fields_in_emitted_event() {
    let mut e = BfldEmitter::new("seed-01").with_privacy_class(PrivacyClass::Restricted);
    let out = e
        .emit(inputs(0, [0.2, 0.2, 0.2, 0.2]), Some(dummy_embedding()))
        .expect("Accept should emit");
    assert!(
        out.identity_risk_score.is_none(),
        "class 3 must strip identity_risk_score",
    );
    assert!(
        out.rf_signature_hash.is_none(),
        "class 3 must strip rf_signature_hash",
    );
}

#[test]
fn with_zone_sets_default_zone_id_on_event() {
    let mut e = BfldEmitter::new("seed-01").with_zone("kitchen");
    let out = e
        .emit(inputs(0, [0.1, 0.1, 0.1, 0.1]), Some(dummy_embedding()))
        .unwrap();
    assert_eq!(out.zone_id.as_deref(), Some("kitchen"));
}

#[test]
fn embedding_is_pushed_to_ring_even_when_event_dropped() {
    let mut e = BfldEmitter::new("seed-01");
    // Drive into Reject state.
    e.emit(inputs(0, [1.0, 1.0, 1.0, 0.8]), Some(dummy_embedding()));
    e.emit(
        inputs(DEBOUNCE_NS, [1.0, 1.0, 1.0, 0.8]),
        Some(dummy_embedding()),
    );
    assert_eq!(e.current_action(), GateAction::Reject);
    // Even though the gate dropped events, the embeddings landed in the ring.
    assert_eq!(e.ring_len(), 2);
}

#[test]
fn ring_unchanged_when_no_embedding_supplied() {
    let mut e = BfldEmitter::new("seed-01");
    let _ = e.emit(inputs(0, [0.1, 0.1, 0.1, 0.1]), None);
    assert_eq!(e.ring_len(), 0);
}
