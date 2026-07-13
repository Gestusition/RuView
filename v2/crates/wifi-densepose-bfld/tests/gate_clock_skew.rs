//! `CoherenceGate` clock-skew resilience. The gate's debounce uses
//! `timestamp_ns.saturating_sub(since)` so a backward time jump (NTP
//! rollback, system-clock adjustment, monotonic-source switch) yields a
//! zero-elapsed delta — the pending action stays pending, the current
//! action stays current. No spurious transitions either direction.
//!
//! This iter pins the property at the public CoherenceGate surface so a
//! future refactor that swaps `saturating_sub` for a plain `-` (which
//! would panic on underflow) fires loud.

use wifi_densepose_bfld::coherence_gate::DEBOUNCE_NS;
use wifi_densepose_bfld::{CoherenceGate, GateAction};

// Score that puts the gate into PredictOnly band after debounce.
fn predict_only_grade() -> f32 {
    0.6
}

// Score that puts the gate into Recalibrate band after debounce.
fn recalibrate_grade() -> f32 {
    0.95
}

fn low_risk() -> f32 {
    0.1
}

#[test]
fn backward_jump_after_pending_does_not_promote_prematurely() {
    let mut g = CoherenceGate::new();
    // Pending PredictOnly at t = DEBOUNCE_NS + 100 (so a forward DEBOUNCE_NS
    // elapsed time would have promoted, but we'll jump backward instead).
    g.evaluate(predict_only_grade(), DEBOUNCE_NS + 100);
    assert_eq!(g.current(), GateAction::Accept);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));

    // Backward jump to t = 0. saturating_sub(0, DEBOUNCE_NS+100) = 0 < DEBOUNCE_NS.
    // The pending stays in place; current stays Accept.
    let after_rollback = g.evaluate(predict_only_grade(), 0);
    assert_eq!(after_rollback, GateAction::Accept);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));
}

#[test]
fn forward_recovery_after_backward_jump_still_promotes_correctly() {
    let mut g = CoherenceGate::new();
    g.evaluate(predict_only_grade(), DEBOUNCE_NS + 100); // pending at t_old
    g.evaluate(predict_only_grade(), 0); // backward jump
                                         // Wall time advances past the ORIGINAL pending timestamp by DEBOUNCE_NS.
                                         // Since the "since" stamp wasn't reset on the backward jump (target
                                         // didn't change), the second evaluate at 0 didn't reset; the third at
                                         // 2*DEBOUNCE_NS + 100 should now satisfy (2*DEBOUNCE_NS + 100) -
                                         // (DEBOUNCE_NS + 100) >= DEBOUNCE_NS → promote.
    let after_recovery = g.evaluate(predict_only_grade(), 2 * DEBOUNCE_NS + 100);
    assert_eq!(after_recovery, GateAction::PredictOnly);
}

#[test]
fn identical_timestamps_across_repeated_polls_do_not_progress_state() {
    let mut g = CoherenceGate::new();
    let t = 1_000_000_000;
    // Three identical evaluations — saturating_sub(t, t) = 0 < DEBOUNCE_NS.
    // Gate never promotes regardless of how many times we poll.
    for _ in 0..5 {
        g.evaluate(predict_only_grade(), t);
    }
    assert_eq!(g.current(), GateAction::Accept);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));
}

#[test]
fn backward_jump_with_no_pending_is_a_noop() {
    let mut g = CoherenceGate::new();
    // No previous evaluation — pending is None. Backward jump from 1e9 to
    // 0 with a low-risk score must keep gate at Accept with no pending.
    g.evaluate(low_risk(), 1_000_000_000);
    assert_eq!(g.pending(), None);
    let after = g.evaluate(low_risk(), 0);
    assert_eq!(after, GateAction::Accept);
    assert_eq!(g.pending(), None);
}

#[test]
fn very_large_forward_jump_promotes_but_does_not_panic() {
    let mut g = CoherenceGate::new();
    g.evaluate(predict_only_grade(), 0);
    // Jump u64::MAX / 2 ns into the future — debounce trivially satisfied.
    let huge = u64::MAX / 2;
    let after = g.evaluate(predict_only_grade(), huge);
    assert_eq!(after, GateAction::PredictOnly);
}

#[test]
fn backward_then_forward_into_different_action_band_resets_pending_correctly() {
    let mut g = CoherenceGate::new();
    // Pending PredictOnly at t = 10 * DEBOUNCE_NS.
    g.evaluate(predict_only_grade(), 10 * DEBOUNCE_NS);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));

    // Backward jump but with a Recalibrate-grade score — gate should re-pend
    // Recalibrate at the NEW timestamp.
    g.evaluate(recalibrate_grade(), 5 * DEBOUNCE_NS);
    assert_eq!(g.pending(), Some(GateAction::Recalibrate));

    // The new pending is set at t=5*DEBOUNCE_NS. Advance another
    // DEBOUNCE_NS forward → promote to Recalibrate.
    let after = g.evaluate(recalibrate_grade(), 6 * DEBOUNCE_NS);
    assert_eq!(after, GateAction::Recalibrate);
}

#[test]
fn no_panic_on_zero_timestamp_with_predict_only_pending() {
    // Regression guard: a poorly-initialized monotonic clock could deliver
    // t=0 as the first sample. Gate must not panic even if `since` is 0
    // and `timestamp_ns` is 0.
    let mut g = CoherenceGate::new();
    g.evaluate(predict_only_grade(), 0);
    let after = g.evaluate(predict_only_grade(), 0);
    assert_eq!(after, GateAction::Accept);
}
