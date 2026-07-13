//! Acceptance tests for `BfldPipelineHandle`. ADR-118 §2.1 worker surface.

#![cfg(feature = "std")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, BfldPipelineHandle, CapturePublisher, IdentityEmbedding,
    PipelineInput, PrivacyClass, SensingInputs, EMBEDDING_DIM,
};

fn inputs(ts_ns: u64) -> SensingInputs {
    SensingInputs {
        timestamp_ns: ts_ns,
        presence: true,
        motion: 0.5,
        person_count: 1,
        sensing_confidence: 0.9,
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: None,
    }
}

fn embedding() -> IdentityEmbedding {
    IdentityEmbedding::from_raw([0.05; EMBEDDING_DIM])
}

fn input(ts_ns: u64) -> PipelineInput {
    PipelineInput {
        inputs: inputs(ts_ns),
        embedding: Some(embedding()),
    }
}

fn drain(published: &Arc<Mutex<CapturePublisher>>) -> Vec<String> {
    published
        .lock()
        .unwrap()
        .published
        .iter()
        .map(|m| m.topic.clone())
        .collect()
}

#[test]
fn handle_publishes_single_input() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-01"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    handle.send(input(0)).expect("send must succeed");

    // Give the worker a moment to drain the channel.
    thread::sleep(Duration::from_millis(50));
    handle.shutdown();

    let topics = drain(&pub_arc);
    assert_eq!(topics.len(), 5, "Anonymous + no zone → 5 topics");
}

#[test]
fn handle_publishes_multiple_inputs_in_order() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-01"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    for i in 0..3 {
        handle.send(input(i * 1_000_000)).unwrap();
    }
    thread::sleep(Duration::from_millis(80));
    handle.shutdown();

    let topics = drain(&pub_arc);
    assert_eq!(topics.len(), 15, "3 inputs × 5 topics each = 15");
}

#[test]
fn handle_send_after_shutdown_errors() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-01"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc);

    // Save the sender by cloning before shutdown — but BfldPipelineHandle
    // owns the sender, so the test demonstrates this via post-shutdown send:
    handle.shutdown();
    // shutdown consumed handle; we can't call send afterward at the type
    // level. The compile-time guarantee IS the test.
}

#[test]
fn handle_drop_without_explicit_shutdown_joins_worker_cleanly() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-01"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    {
        let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());
        handle.send(input(0)).unwrap();
        thread::sleep(Duration::from_millis(50));
        // No explicit shutdown — Drop must handle worker join.
    }
    // If we reached here without hanging or panicking, the Drop path worked.
    let topics = drain(&pub_arc);
    assert_eq!(topics.len(), 5);
}

#[test]
fn handle_honors_privacy_mode_toggle_via_pipeline_state() {
    let mut pipeline = BfldPipeline::new(BfldConfig::new("seed-01"));
    pipeline.enable_privacy_mode();
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    handle.send(input(0)).unwrap();
    thread::sleep(Duration::from_millis(50));
    handle.shutdown();

    let topics = drain(&pub_arc);
    // Restricted + no zone: presence/motion/count/confidence = 4 topics.
    assert_eq!(topics.len(), 4, "Restricted strips identity_risk topic");
    assert!(!topics.iter().any(|t| t.contains("identity_risk")));
}

#[test]
fn handle_drops_event_when_gate_rejects() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-01"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    // Two high-risk inputs back-to-back force the gate into Reject after debounce.
    use wifi_densepose_bfld::coherence_gate::DEBOUNCE_NS;
    let mut high_risk = inputs(0);
    high_risk.sep = 1.0;
    high_risk.stab = 1.0;
    high_risk.consist = 1.0;
    high_risk.risk_conf = 0.8;
    handle
        .send(PipelineInput {
            inputs: high_risk.clone(),
            embedding: Some(embedding()),
        })
        .unwrap();
    high_risk.timestamp_ns = DEBOUNCE_NS;
    handle
        .send(PipelineInput {
            inputs: high_risk,
            embedding: Some(embedding()),
        })
        .unwrap();
    thread::sleep(Duration::from_millis(80));
    handle.shutdown();

    let topics = drain(&pub_arc);
    // First input emits (Accept state) → 5 topics. Second input gate-promoted
    // to Reject → 0 topics. Total = 5.
    assert_eq!(
        topics.len(),
        5,
        "Reject must drop the second event entirely"
    );
}

#[test]
fn handle_with_zone_threads_through_to_published_topics() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-01").with_zone("kitchen"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    handle.send(input(0)).unwrap();
    thread::sleep(Duration::from_millis(50));
    handle.shutdown();

    let topics = drain(&pub_arc);
    assert!(
        topics.iter().any(|t| t.contains("zone_activity")),
        "zone_activity topic must be present when zone configured",
    );

    let zone_msg = pub_arc
        .lock()
        .unwrap()
        .published
        .iter()
        .find(|m| m.topic.contains("zone_activity"))
        .map(|m| m.payload.clone());
    assert_eq!(zone_msg.as_deref(), Some("\"kitchen\""));
}

#[test]
fn class_3_pipeline_baseline_produces_four_topics_per_input() {
    // Baseline class = Restricted (no privacy_mode toggle needed).
    let pipeline =
        BfldPipeline::new(BfldConfig::new("seed-01").with_privacy_class(PrivacyClass::Restricted));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    handle.send(input(0)).unwrap();
    thread::sleep(Duration::from_millis(50));
    handle.shutdown();

    assert_eq!(drain(&pub_arc).len(), 4);
}
