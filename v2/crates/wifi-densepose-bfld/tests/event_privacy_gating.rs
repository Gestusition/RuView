//! Acceptance tests for ADR-121 §2.1 / ADR-122 §2.1 — `BfldEvent` privacy gating.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{BfldEvent, PrivacyClass};

fn sample_at(class: PrivacyClass) -> BfldEvent {
    BfldEvent::with_privacy_gating(
        "seed-01".to_string(),
        1_700_000_000_000_000_000,
        true,
        0.72,
        1,
        0.91,
        Some("living_room".to_string()),
        class,
        Some(0.84),
        Some([0xAB; 32]),
    )
}

#[test]
fn anonymous_event_retains_identity_risk_and_hash() {
    let e = sample_at(PrivacyClass::Anonymous);
    assert!(e.identity_risk_score.is_some());
    assert!(e.rf_signature_hash.is_some());
}

#[test]
fn restricted_event_strips_identity_fields() {
    let e = sample_at(PrivacyClass::Restricted);
    assert!(
        e.identity_risk_score.is_none(),
        "risk score must be None at class 3"
    );
    assert!(
        e.rf_signature_hash.is_none(),
        "rf hash must be None at class 3"
    );
    // Sensing fields still present.
    assert!(e.presence);
    assert_eq!(e.person_count, 1);
    assert_eq!(e.zone_id.as_deref(), Some("living_room"));
}

#[test]
fn apply_privacy_gating_is_idempotent() {
    let mut e = sample_at(PrivacyClass::Restricted);
    e.apply_privacy_gating();
    e.apply_privacy_gating();
    assert!(e.identity_risk_score.is_none());
}

#[test]
fn event_type_is_always_bfld_update() {
    for c in [
        PrivacyClass::Anonymous,
        PrivacyClass::Restricted,
        PrivacyClass::Derived,
    ] {
        assert_eq!(sample_at(c).event_type, "bfld_update");
    }
}

#[cfg(feature = "serde-json")]
mod json {
    use super::sample_at;
    use wifi_densepose_bfld::PrivacyClass;

    #[test]
    fn json_round_trip_emits_type_field_first_or_last_but_present() {
        let json = sample_at(PrivacyClass::Anonymous).to_json().unwrap();
        assert!(json.contains(r#""type":"bfld_update""#), "JSON: {json}");
        assert!(json.contains(r#""node_id":"seed-01""#));
        assert!(json.contains(r#""presence":true"#));
        assert!(json.contains(r#""privacy_class":"anonymous""#));
    }

    #[test]
    fn anonymous_json_includes_identity_fields() {
        let json = sample_at(PrivacyClass::Anonymous).to_json().unwrap();
        assert!(json.contains("identity_risk_score"));
        assert!(json.contains("rf_signature_hash"));
    }

    #[test]
    fn restricted_json_omits_identity_fields_entirely() {
        let json = sample_at(PrivacyClass::Restricted).to_json().unwrap();
        assert!(
            !json.contains("identity_risk_score"),
            "JSON must omit identity_risk_score at class 3, got: {json}",
        );
        assert!(
            !json.contains("rf_signature_hash"),
            "JSON must omit rf_signature_hash at class 3, got: {json}",
        );
        // Sensing fields still emitted.
        assert!(json.contains("presence"));
        assert!(json.contains("motion"));
        assert!(json.contains(r#""privacy_class":"restricted""#));
    }

    #[test]
    fn privacy_class_serializes_to_lowercase_name() {
        for (class, name) in [
            (PrivacyClass::Anonymous, "anonymous"),
            (PrivacyClass::Restricted, "restricted"),
        ] {
            let json = sample_at(class).to_json().unwrap();
            let needle = format!(r#""privacy_class":"{name}""#);
            assert!(json.contains(&needle), "missing {needle} in: {json}");
        }
    }

    #[test]
    fn zone_id_none_is_omitted_from_json() {
        let mut e = sample_at(PrivacyClass::Anonymous);
        e.zone_id = None;
        let json = e.to_json().unwrap();
        assert!(
            !json.contains("zone_id"),
            "None zone_id must be omitted: {json}"
        );
    }
}
