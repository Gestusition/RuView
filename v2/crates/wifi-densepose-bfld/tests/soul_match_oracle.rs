//! Acceptance tests for ADR-121 §2.6 — `SoulMatchOracle` Recalibrate exemption.

use wifi_densepose_bfld::coherence_gate::DEBOUNCE_NS;
use wifi_densepose_bfld::{CoherenceGate, GateAction, MatchOutcome, NullOracle, SoulMatchOracle};

/// Oracle that always claims an enrolled match.
struct AlwaysMatch;
impl SoulMatchOracle for AlwaysMatch {
    fn matches_enrolled(&self) -> MatchOutcome {
        MatchOutcome::Match {
            person_id: 0x4242_4242,
        }
    }
}

/// Oracle that reports suppressed (class-3 deployment).
struct AlwaysSuppressed;
impl SoulMatchOracle for AlwaysSuppressed {
    fn matches_enrolled(&self) -> MatchOutcome {
        MatchOutcome::Suppressed
    }
}

#[test]
fn null_oracle_matches_default_evaluate_behavior() {
    let mut a = CoherenceGate::new();
    let mut b = CoherenceGate::new();
    let oracle = NullOracle;
    for (i, score) in [0.1, 0.4, 0.6, 0.8, 0.95].iter().enumerate() {
        let ts = (i as u64) * 2 * DEBOUNCE_NS;
        assert_eq!(
            a.evaluate(*score, ts),
            b.evaluate_with_oracle(*score, ts, &oracle)
        );
    }
}

#[test]
fn match_outcome_downgrades_recalibrate_to_predict_only() {
    let mut g = CoherenceGate::new();
    let oracle = AlwaysMatch;
    // Score = 0.95 would normally pend Recalibrate. With AlwaysMatch oracle,
    // it pends PredictOnly instead.
    g.evaluate_with_oracle(0.95, 0, &oracle);
    assert_eq!(g.pending(), Some(GateAction::PredictOnly));
}

#[test]
fn match_exemption_promotes_predict_only_after_debounce_not_recalibrate() {
    let mut g = CoherenceGate::new();
    let oracle = AlwaysMatch;
    g.evaluate_with_oracle(0.95, 0, &oracle);
    let out = g.evaluate_with_oracle(0.95, DEBOUNCE_NS, &oracle);
    assert_eq!(out, GateAction::PredictOnly);
    assert_ne!(
        out,
        GateAction::Recalibrate,
        "Match must prevent Recalibrate"
    );
}

#[test]
fn match_outcome_does_not_affect_lower_actions() {
    let mut g = CoherenceGate::new();
    let oracle = AlwaysMatch;
    // Score in the Reject band — oracle exemption does NOT apply (only to Recalibrate).
    g.evaluate_with_oracle(0.8, 0, &oracle);
    assert_eq!(g.pending(), Some(GateAction::Reject));

    // Run to debounce — current must become Reject, not PredictOnly.
    let out = g.evaluate_with_oracle(0.8, DEBOUNCE_NS, &oracle);
    assert_eq!(out, GateAction::Reject);
}

#[test]
fn suppressed_outcome_does_not_exempt_recalibrate() {
    let mut g = CoherenceGate::new();
    let oracle = AlwaysSuppressed;
    g.evaluate_with_oracle(0.95, 0, &oracle);
    // Suppressed is functionally equivalent to NotEnrolled — Recalibrate stays pending.
    assert_eq!(g.pending(), Some(GateAction::Recalibrate));
}

#[test]
fn not_enrolled_outcome_does_not_exempt_recalibrate() {
    let mut g = CoherenceGate::new();
    let oracle = NullOracle; // always NotEnrolled
    g.evaluate_with_oracle(0.95, 0, &oracle);
    assert_eq!(g.pending(), Some(GateAction::Recalibrate));
}

#[test]
fn match_outcome_carries_person_id() {
    let outcome = AlwaysMatch.matches_enrolled();
    match outcome {
        MatchOutcome::Match { person_id } => assert_eq!(person_id, 0x4242_4242),
        other => panic!("expected Match, got {other:?}"),
    }
}

#[test]
fn null_oracle_default_constructor_works() {
    let oracle = NullOracle;
    assert_eq!(oracle.matches_enrolled(), MatchOutcome::NotEnrolled);
}
