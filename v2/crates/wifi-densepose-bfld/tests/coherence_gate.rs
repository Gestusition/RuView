//! Acceptance tests for ADR-121 §2.5 — `CoherenceGate` hysteresis + debounce.

use wifi_densepose_bfld::coherence_gate::{DEBOUNCE_NS, HYSTERESIS};
use wifi_densepose_bfld::{CoherenceGate, GateAction};

#[test]
fn fresh_gate_starts_in_accept_with_no_pending() {
    let g = CoherenceGate::new();
    assert_eq!(g.current(), GateAction::Accept);
    assert_eq!(g.pending(), None);
}

#[test]
fn low_score_stays_in_accept_with_no_pending() {
    let mut g = CoherenceGate::new();
    let out = g.evaluate(0.3, 0);
    assert_eq!(out, GateAction::Accept);
    assert_eq!(g.pending(), None);
}

#[test]
fn score_just_past_boundary_but_within_hysteresis_does_not_pend() {
    // current = Accept, upper edge = 0.5, hysteresis = 0.05 → need >= 0.55 to start pending.
    let mut g = CoherenceGate::new();
    let out = g.evaluate(0.52, 0);
    assert_eq!(out, GateAction::Accept);
    assert_eq!(
        g.pending(),
        None,
        "0.52 must not start a pending transition"
    );
}

#[test]
fn score_clearly_past_hysteresis_starts_pending() {
    let mut g = CoherenceGate::new();
    let out = g.evaluate(0.6, 0);
    assert_eq!(
        out,
        GateAction::Accept,
        "still Accept until debounce elapses"
    );
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));
}

#[test]
fn pending_action_promotes_after_full_debounce() {
    let mut g = CoherenceGate::new();
    g.evaluate(0.6, 0);
    assert_eq!(g.current(), GateAction::Accept);
    let out = g.evaluate(0.6, DEBOUNCE_NS);
    assert_eq!(out, GateAction::PredictOnly);
    assert_eq!(g.pending(), None);
}

#[test]
fn pending_action_does_not_promote_before_debounce() {
    let mut g = CoherenceGate::new();
    g.evaluate(0.6, 0);
    let out = g.evaluate(0.6, DEBOUNCE_NS - 1);
    assert_eq!(out, GateAction::Accept);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));
}

#[test]
fn returning_to_current_band_cancels_pending() {
    let mut g = CoherenceGate::new();
    g.evaluate(0.6, 0); // pending PredictOnly
    let out = g.evaluate(0.4, 1_000_000_000); // 1s later, back in Accept band
    assert_eq!(out, GateAction::Accept);
    assert_eq!(
        g.pending(),
        None,
        "returning to current band cancels pending"
    );
}

#[test]
fn changing_pending_target_resets_the_debounce_clock() {
    let mut g = CoherenceGate::new();
    g.evaluate(0.6, 0); // pending PredictOnly at t=0
    g.evaluate(0.95, 1_000_000_000); // pending Recalibrate at t=1s (clock reset)
                                     // At t=1s + DEBOUNCE_NS - 1, still not promoted (Recalibrate pending since 1s)
    let out = g.evaluate(0.95, 1_000_000_000 + DEBOUNCE_NS - 1);
    assert_eq!(out, GateAction::Accept);
    // At t=1s + DEBOUNCE_NS, promoted to Recalibrate
    let out = g.evaluate(0.95, 1_000_000_000 + DEBOUNCE_NS);
    assert_eq!(out, GateAction::Recalibrate);
}

#[test]
fn downward_transitions_also_require_hysteresis() {
    let mut g = CoherenceGate::new();
    // Force gate into PredictOnly state.
    g.evaluate(0.6, 0);
    g.evaluate(0.6, DEBOUNCE_NS);
    assert_eq!(g.current(), GateAction::PredictOnly);

    // 0.48 is below 0.5 but only by 0.02 — within hysteresis envelope.
    let out = g.evaluate(0.48, 2 * DEBOUNCE_NS);
    assert_eq!(out, GateAction::PredictOnly);
    assert_eq!(g.pending(), None, "0.48 is within downward hysteresis");

    // 0.44 is below 0.5 - 0.05 = 0.45 → starts pending Accept.
    g.evaluate(0.44, 3 * DEBOUNCE_NS);
    assert_eq!(g.pending(), Some(GateAction::Accept));
}

#[test]
fn spike_to_one_then_back_to_zero_never_promotes_to_recalibrate() {
    let mut g = CoherenceGate::new();
    g.evaluate(1.0, 0); // pending Recalibrate at t=0
                        // 1 second later score is back to 0 — cancel pending.
    let out = g.evaluate(0.0, 1_000_000_000);
    assert_eq!(out, GateAction::Accept);
    assert_eq!(g.pending(), None);
    // Even waiting longer, the gate stays in Accept.
    let out = g.evaluate(0.0, 100 * DEBOUNCE_NS);
    assert_eq!(out, GateAction::Accept);
}

#[test]
fn boundary_value_with_hysteresis_does_not_promote() {
    // Edge: current=Accept, score = upper_edge + HYSTERESIS - epsilon (just below).
    let mut g = CoherenceGate::new();
    let out = g.evaluate(0.5 + HYSTERESIS - 0.0001, 0);
    assert_eq!(out, GateAction::Accept);
    assert_eq!(g.pending(), None);
}

#[test]
fn boundary_value_at_hysteresis_exact_does_pend() {
    let mut g = CoherenceGate::new();
    let out = g.evaluate(0.5 + HYSTERESIS, 0);
    assert_eq!(out, GateAction::Accept);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));
}

#[test]
fn nan_score_stays_in_current_action_with_no_pending() {
    let mut g = CoherenceGate::new();
    let out = g.evaluate(f32::NAN, 0);
    // NaN maps to Accept via from_score; gate stays in Accept and clears pending.
    assert_eq!(out, GateAction::Accept);
    assert_eq!(g.pending(), None);
}
