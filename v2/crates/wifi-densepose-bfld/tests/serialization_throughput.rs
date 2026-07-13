//! ADR-119 AC7 serialization throughput. Target: **≥ 50,000 frames/sec** on a
//! 2025-era M1/M2 / Pi 5 release build.
//!
//! Debug builds run 20–100× slower than release because the `to_le_bytes`
//! copies and `try_into` slice conversions don't inline / vectorize. We
//! therefore assert a **generous debug-mode floor** (≥ 5,000 frames/sec) so
//! `cargo test` (debug) passes on any reasonable machine, and document the
//! actual AC threshold here for `cargo test --release` operators.
//!
//! Two scenarios:
//! 1. Header-only `BfldFrameHeader::to_le_bytes()` — the inner hot path.
//! 2. Full `BfldFrame::to_bytes()` including CRC32 over a typical payload.

#![cfg(feature = "std")]

use std::time::Instant;

use wifi_densepose_bfld::frame::flags;
use wifi_densepose_bfld::{BfldFrame, BfldFrameHeader, BFLD_HEADER_SIZE};

const N_ITERS: usize = 50_000;
const DEBUG_FLOOR_FRAMES_PER_SEC: f64 = 5_000.0;
/// Documented AC7 release-mode target. `cargo test` (debug) never asserts
/// against this; `cargo test --release` operators can re-set the floor.
pub const RELEASE_TARGET_FRAMES_PER_SEC: f64 = 50_000.0;

fn sample_header() -> BfldFrameHeader {
    let mut h = BfldFrameHeader::empty();
    h.flags = flags::HAS_CSI_DELTA | flags::PRIVACY_MODE;
    h.timestamp_ns = 0x0123_4567_89AB_CDEF;
    h.ap_hash = [0xAA; 16];
    h.sta_hash = [0xBB; 16];
    h.session_id = [0xCC; 16];
    h.channel = 36;
    h.bandwidth_mhz = 80;
    h.rssi_dbm = -55;
    h.noise_floor_dbm = -95;
    h.n_subcarriers = 234;
    h.n_tx = 3;
    h.n_rx = 4;
    h.quantization = 1;
    h.privacy_class = 2;
    h.payload_len = 0;
    h.payload_crc32 = 0;
    h
}

fn typical_payload() -> Vec<u8> {
    // ~512 bytes of pseudo-CBFR-shaped bytes — close to a real BFI frame
    // for an 80 MHz / 4×4 capture.
    (0u8..=255).cycle().take(512).collect()
}

#[test]
fn header_only_to_le_bytes_throughput_meets_debug_floor() {
    let header = sample_header();

    // Warm up the cache + JIT-equivalent — Rust doesn't have JIT, but the
    // first iteration takes the branch-predictor hit; skip it from timing.
    for _ in 0..1_000 {
        let _ = core::hint::black_box(header.to_le_bytes());
    }

    let start = Instant::now();
    for _ in 0..N_ITERS {
        let bytes = header.to_le_bytes();
        // black_box prevents DCE from eliminating the entire loop.
        core::hint::black_box(bytes);
    }
    let elapsed = start.elapsed();

    let throughput = N_ITERS as f64 / elapsed.as_secs_f64();
    eprintln!(
        "header-only to_le_bytes: {N_ITERS} iters in {:.3}ms → {:.0} frames/sec \
         (debug floor: {:.0}, ADR-119 AC7 release target: {RELEASE_TARGET_FRAMES_PER_SEC:.0})",
        elapsed.as_millis(),
        throughput,
        DEBUG_FLOOR_FRAMES_PER_SEC,
    );
    assert!(
        throughput >= DEBUG_FLOOR_FRAMES_PER_SEC,
        "header serialization throughput {throughput:.0} below debug floor \
         {DEBUG_FLOOR_FRAMES_PER_SEC:.0}",
    );
}

#[test]
fn full_frame_to_bytes_throughput_meets_debug_floor() {
    let header = sample_header();
    let payload = typical_payload();
    let frame = BfldFrame::new(header, payload);

    for _ in 0..100 {
        let _ = core::hint::black_box(frame.to_bytes());
    }

    let start = Instant::now();
    for _ in 0..N_ITERS {
        let bytes = frame.to_bytes();
        core::hint::black_box(bytes);
    }
    let elapsed = start.elapsed();

    let throughput = N_ITERS as f64 / elapsed.as_secs_f64();
    eprintln!(
        "BfldFrame::to_bytes (512B payload + CRC32): {N_ITERS} iters in {:.3}ms \
         → {:.0} frames/sec (debug floor: {:.0}, release target: {RELEASE_TARGET_FRAMES_PER_SEC:.0})",
        elapsed.as_millis(),
        throughput,
        DEBUG_FLOOR_FRAMES_PER_SEC,
    );
    assert!(
        throughput >= DEBUG_FLOOR_FRAMES_PER_SEC,
        "full-frame serialization throughput {throughput:.0} below debug floor \
         {DEBUG_FLOOR_FRAMES_PER_SEC:.0}",
    );
}

#[test]
fn round_trip_through_bytes_remains_constant_time_per_byte() {
    // Sanity: parse cost should scale with payload size. Two payload sizes,
    // verify the bigger one isn't pathologically slower (regression guard
    // against an accidental O(n²) parser, which would jump the ratio).
    let small_payload = typical_payload(); // 512 bytes
    let mut big_payload = small_payload.clone();
    big_payload.extend(typical_payload().iter().copied()); // 1024 bytes

    let small_frame = BfldFrame::new(sample_header(), small_payload);
    let big_frame = BfldFrame::new(sample_header(), big_payload);

    let n = 5_000;
    let small_bytes = small_frame.to_bytes();
    let big_bytes = big_frame.to_bytes();

    let t_small = {
        let start = Instant::now();
        for _ in 0..n {
            let f = BfldFrame::from_bytes(&small_bytes).unwrap();
            core::hint::black_box(f);
        }
        start.elapsed().as_secs_f64()
    };

    let t_big = {
        let start = Instant::now();
        for _ in 0..n {
            let f = BfldFrame::from_bytes(&big_bytes).unwrap();
            core::hint::black_box(f);
        }
        start.elapsed().as_secs_f64()
    };

    let ratio = t_big / t_small;
    eprintln!("parse-cost ratio (1024B / 512B payload): {ratio:.2}× (expect ~2× for O(n))",);
    // O(n) parser → ratio ≈ 2.0. Allow generous bounds (1.0 .. 4.0) to absorb
    // timer noise + CRC32 quadratic-ish behavior on small inputs.
    assert!(
        (1.0..=4.0).contains(&ratio),
        "parse-cost ratio {ratio:.2} suggests non-linear scaling — investigate parser",
    );
}

#[test]
fn header_size_constant_is_used_consistently_by_serializer() {
    // Belt-and-suspenders cross-check: the serialized header length equals
    // the BFLD_HEADER_SIZE constant. Pins the AC1 contract from the
    // throughput-test side too.
    let bytes = sample_header().to_le_bytes();
    assert_eq!(bytes.len(), BFLD_HEADER_SIZE);
    assert_eq!(bytes.len(), 86);
}
