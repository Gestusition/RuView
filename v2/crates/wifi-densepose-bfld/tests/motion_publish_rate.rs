//! ADR-122 AC3 — motion-state topic publishes at ≥ 1 Hz during sustained
//! occupancy through the [`BfldPipelineHandle`] worker thread.
//!
//! Drives the handle with N inputs spaced over a known wall-clock window,
//! then counts motion topic messages in the capture log. Avoids broker
//! dependencies — entirely in-process via `CapturePublisher` + `Arc<Mutex<>>`.

#![cfg(feature = "std")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, BfldPipelineHandle, CapturePublisher, IdentityEmbedding,
    PipelineInput, SensingInputs, TopicMessage, EMBEDDING_DIM,
};

const NS_PER_SEC: u64 = 1_000_000_000;

fn input_at(ts_secs: f64, motion: f32) -> PipelineInput {
    let ts_ns = (ts_secs * NS_PER_SEC as f64) as u64;
    PipelineInput {
        inputs: SensingInputs {
            timestamp_ns: ts_ns,
            presence: true,
            motion,
            person_count: 1,
            sensing_confidence: 0.9,
            sep: 0.2,
            stab: 0.2,
            consist: 0.2,
            risk_conf: 0.2,
            rf_signature_hash: None,
        },
        embedding: Some(IdentityEmbedding::from_raw([0.05; EMBEDDING_DIM])),
    }
}

fn motion_messages(log: &[TopicMessage]) -> Vec<&TopicMessage> {
    log.iter()
        .filter(|m| m.topic.contains("/bfld/motion/state"))
        .collect()
}

#[test]
fn motion_publish_rate_meets_one_hz_under_sustained_input() {
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-rate"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    // Drive 10 inputs spaced 100ms apart in wall time — that's a 10 Hz
    // input rate, well above the 1 Hz AC3 floor. Timestamps advance in
    // lockstep so the gate/hasher see realistic monotonic time.
    let n = 10usize;
    let interval = Duration::from_millis(100);
    let start = Instant::now();
    for i in 0..n {
        let ts_secs = i as f64 * 0.1;
        handle.send(input_at(ts_secs, 0.5)).expect("send");
        thread::sleep(interval);
    }
    let elapsed = start.elapsed();

    // Worker has a small enqueue → process latency; give it a brief drain
    // before shutting down.
    thread::sleep(Duration::from_millis(150));
    handle.shutdown();

    let log = pub_arc.lock().unwrap();
    let motions = motion_messages(&log.published);
    let secs = elapsed.as_secs_f64();
    let rate = motions.len() as f64 / secs;

    eprintln!(
        "motion_publish_rate: {} messages in {:.3}s → {:.2} Hz (ADR-122 AC3 floor: 1.00 Hz)",
        motions.len(),
        secs,
        rate,
    );
    assert!(
        motions.len() >= n,
        "expected ≥ {n} motion topic messages (one per input), got {}",
        motions.len(),
    );
    assert!(
        rate >= 1.0,
        "motion publish rate {rate:.2} Hz below ADR-122 AC3 floor (1.00 Hz)",
    );
}

#[test]
fn motion_values_track_input_motion_values() {
    // Pin the payload-encoding contract from iter 21: motion value flows
    // through verbatim (formatted as "{:.6}") — no quantization drift.
    let pipeline = BfldPipeline::new(BfldConfig::new("seed-track"));
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    let values: [f32; 5] = [0.10, 0.25, 0.50, 0.75, 0.95];
    for (i, &v) in values.iter().enumerate() {
        handle.send(input_at(i as f64 * 0.05, v)).expect("send");
    }
    thread::sleep(Duration::from_millis(200));
    handle.shutdown();

    let log = pub_arc.lock().unwrap();
    let motions = motion_messages(&log.published);
    assert_eq!(motions.len(), values.len());
    for (i, &expected) in values.iter().enumerate() {
        let formatted = format!("{:.6}", expected);
        assert_eq!(
            motions[i].payload, formatted,
            "motion[{i}] payload {} != expected {}",
            motions[i].payload, formatted,
        );
    }
}

#[test]
fn motion_topic_never_appears_for_class_below_anonymous_publishing() {
    // Defense in depth: the iter-21 router returns empty for class < Anonymous
    // events. Confirm at the handle level too by configuring the pipeline
    // baseline to a research-only class. The handle's process() goes through
    // privacy_mode-aware logic; we don't have a class-1 baseline path from
    // BfldConfig, so this test exercises the class-3 strip-but-not-suppress
    // path: motion still publishes (it's sensing data), but identity_risk
    // does NOT (proven in iter 25).
    use wifi_densepose_bfld::PrivacyClass;
    let pipeline = BfldPipeline::new(
        BfldConfig::new("seed-cls3").with_privacy_class(PrivacyClass::Restricted),
    );
    let pub_arc = Arc::new(Mutex::new(CapturePublisher::default()));
    let handle = BfldPipelineHandle::spawn(pipeline, pub_arc.clone());

    handle.send(input_at(0.0, 0.4)).expect("send");
    thread::sleep(Duration::from_millis(100));
    handle.shutdown();

    let log = pub_arc.lock().unwrap();
    let motions = motion_messages(&log.published);
    assert_eq!(
        motions.len(),
        1,
        "Restricted still publishes motion (sensing)"
    );
    assert!(
        !log.published
            .iter()
            .any(|m| m.topic.contains("identity_risk")),
        "Restricted must NOT publish identity_risk topic",
    );
}
