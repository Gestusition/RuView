//! ADR-119 AC2: "Presence detection latency is ≤ 1s p95 from the first
//! non-empty BFI frame in a new occupancy event." This iter pins the
//! latency property at the `BfldPipeline::process()` surface — the call
//! between the iter-21 publisher and the iter-19 facade.
//!
//! Method: warm up the pipeline, then time N consecutive `process()` calls
//! over a fresh `BfldPipeline`. Compute p50 and p95 from the sorted latency
//! samples. AC2 caps p95 at 1 second; debug-build measurements come in well
//! under 1ms per call, so we assert against a **generous** 100ms floor that
//! still catches a catastrophic regression (e.g., accidental I/O in the
//! hot path) without flaking on a busy CI runner.

#![cfg(feature = "std")]

use std::time::{Duration, Instant};

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, IdentityEmbedding, SensingInputs, EMBEDDING_DIM,
};

const N_SAMPLES: usize = 500;
/// Generous CI floor — debug builds typically land < 1ms / call.
const DEBUG_P95_FLOOR: Duration = Duration::from_millis(100);
/// Documented ADR-119 AC2 target. CI doesn't assert against this directly
/// (release-build territory), but the constant is exported for operators
/// running `cargo test --release` to re-pin.
pub const ADR_119_AC2_P95_TARGET: Duration = Duration::from_secs(1);

fn inputs(ts_ns: u64) -> SensingInputs {
    SensingInputs {
        timestamp_ns: ts_ns,
        presence: true,
        motion: 0.3,
        person_count: 1,
        sensing_confidence: 0.9,
        sep: 0.1,
        stab: 0.1,
        consist: 0.1,
        risk_conf: 0.1,
        rf_signature_hash: None,
    }
}

fn embedding() -> IdentityEmbedding {
    IdentityEmbedding::from_raw([0.05; EMBEDDING_DIM])
}

fn percentile(sorted_samples: &[Duration], p: f64) -> Duration {
    debug_assert!(!sorted_samples.is_empty());
    let idx = ((sorted_samples.len() as f64) * p).floor() as usize;
    let idx = idx.min(sorted_samples.len() - 1);
    sorted_samples[idx]
}

#[test]
fn process_call_p95_latency_meets_debug_floor() {
    let mut pipeline = BfldPipeline::new(BfldConfig::new("seed-latency"));

    // Warm up branch predictor + cache.
    for i in 0..50 {
        let _ = pipeline.process(inputs(i * 1_000), Some(embedding()));
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(N_SAMPLES);
    for i in 0..N_SAMPLES {
        let ts_ns = (i as u64 + 50) * 1_000_000;
        let start = Instant::now();
        let _evt = pipeline.process(inputs(ts_ns), Some(embedding()));
        samples.push(start.elapsed());
    }

    samples.sort_unstable();
    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);
    let p99 = percentile(&samples, 0.99);

    eprintln!(
        "presence_latency: {N_SAMPLES} samples — p50={:.3}µs p95={:.3}µs p99={:.3}µs \
         (debug floor: {:?}, ADR-119 AC2 release target: {:?})",
        p50.as_secs_f64() * 1e6,
        p95.as_secs_f64() * 1e6,
        p99.as_secs_f64() * 1e6,
        DEBUG_P95_FLOOR,
        ADR_119_AC2_P95_TARGET,
    );

    assert!(
        p95 <= DEBUG_P95_FLOOR,
        "p95 latency {:?} exceeded debug floor {:?} — possible regression \
         (accidental I/O on the hot path, debug-build optimization regression)",
        p95,
        DEBUG_P95_FLOOR,
    );

    // ADR-119 AC2 documented target — debug build easily satisfies it
    // since DEBUG_P95_FLOOR is 100ms and AC2 is 1s.
    assert!(
        p95 <= ADR_119_AC2_P95_TARGET,
        "p95 latency {:?} exceeds ADR-119 AC2 ({:?})",
        p95,
        ADR_119_AC2_P95_TARGET,
    );
}

#[test]
fn first_call_after_pipeline_construction_is_not_pathologically_slow() {
    // Operators see "first event after node boot" as the user-visible
    // latency. Spinning up a fresh pipeline and measuring the very FIRST
    // call (no warmup) catches a constructor that does lazy work on first
    // process — would show up as a 100ms+ initial spike on a Pi 5.
    let mut pipeline = BfldPipeline::new(BfldConfig::new("seed-first"));
    let start = Instant::now();
    let _evt = pipeline.process(inputs(1_000_000), Some(embedding()));
    let first_call = start.elapsed();

    eprintln!(
        "first-call latency: {:.3}µs",
        first_call.as_secs_f64() * 1e6
    );
    // First call is allowed to be slower than steady-state but still
    // bounded — 250ms catches a real warm-up bug without flaking.
    assert!(
        first_call < Duration::from_millis(250),
        "first-call latency {:?} suggests lazy initialization in process() \
         path — operators see this as boot-time delay",
        first_call,
    );
}

#[test]
fn latency_does_not_grow_unbounded_over_long_runs() {
    // Catch monotonically growing per-call cost (memory leak, ring buffer
    // misbehavior, unbounded internal log). Compare first-100-sample mean
    // vs last-100-sample mean.
    let mut pipeline = BfldPipeline::new(BfldConfig::new("seed-grow"));
    let mut samples = Vec::with_capacity(N_SAMPLES);
    for i in 0..N_SAMPLES {
        let ts_ns = (i as u64) * 1_000_000;
        let start = Instant::now();
        let _ = pipeline.process(inputs(ts_ns), Some(embedding()));
        samples.push(start.elapsed());
    }
    let first_mean = samples[..100].iter().sum::<Duration>() / 100;
    let last_mean = samples[N_SAMPLES - 100..].iter().sum::<Duration>() / 100;
    eprintln!(
        "first-100 mean: {:.3}µs, last-100 mean: {:.3}µs",
        first_mean.as_secs_f64() * 1e6,
        last_mean.as_secs_f64() * 1e6,
    );
    // Allow 10× growth ratio to absorb noise + warmup effects; catches
    // genuine 100×+ regressions like an unbounded log.
    let ratio = last_mean.as_nanos() as f64 / first_mean.as_nanos().max(1) as f64;
    assert!(
        ratio < 10.0,
        "per-call latency growth ratio {ratio:.2}× suggests unbounded internal state",
    );
}
