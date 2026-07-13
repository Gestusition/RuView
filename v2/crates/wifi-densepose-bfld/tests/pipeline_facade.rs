//! Acceptance tests for the `BfldPipeline` facade. ADR-118 §2.1.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, IdentityEmbedding, PrivacyClass, SensingInputs, SignatureHasher,
    EMBEDDING_DIM, SITE_SALT_LEN,
};

fn inputs() -> SensingInputs {
    SensingInputs {
        timestamp_ns: 1_700_000_000_000_000_000,
        presence: true,
        motion: 0.4,
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

// --- BfldConfig builder --------------------------------------------------

#[test]
fn config_defaults_to_anonymous_no_zone_no_hasher() {
    let c = BfldConfig::new("seed-01");
    assert_eq!(c.node_id, "seed-01");
    assert_eq!(c.privacy_class, PrivacyClass::Anonymous);
    assert!(c.default_zone_id.is_none());
    assert!(c.signature_hasher.is_none());
}

#[test]
fn config_builder_methods_chain() {
    let hasher = SignatureHasher::new([0u8; SITE_SALT_LEN]);
    let c = BfldConfig::new("seed-01")
        .with_zone("kitchen")
        .with_privacy_class(PrivacyClass::Derived)
        .with_signature_hasher(hasher);
    assert_eq!(c.default_zone_id.as_deref(), Some("kitchen"));
    assert_eq!(c.privacy_class, PrivacyClass::Derived);
    assert!(c.signature_hasher.is_some());
}

// --- BfldPipeline core ---------------------------------------------------

#[test]
fn fresh_pipeline_is_not_in_privacy_mode() {
    let p = BfldPipeline::new(BfldConfig::new("seed-01"));
    assert!(!p.is_privacy_mode_enabled());
    assert_eq!(p.current_privacy_class(), PrivacyClass::Anonymous);
}

#[test]
fn pipeline_process_returns_anonymous_event_under_low_risk() {
    let mut p = BfldPipeline::new(BfldConfig::new("seed-01"));
    let evt = p.process(inputs(), Some(embedding())).expect("low risk");
    assert_eq!(evt.privacy_class, PrivacyClass::Anonymous);
    assert!(evt.identity_risk_score.is_some());
}

// --- privacy_mode toggle -------------------------------------------------

#[test]
fn enable_privacy_mode_demotes_published_events_to_restricted() {
    let mut p = BfldPipeline::new(BfldConfig::new("seed-01"));
    p.enable_privacy_mode();
    assert!(p.is_privacy_mode_enabled());
    assert_eq!(p.current_privacy_class(), PrivacyClass::Restricted);
    let evt = p.process(inputs(), Some(embedding())).expect("low risk");
    assert_eq!(evt.privacy_class, PrivacyClass::Restricted);
    assert!(evt.identity_risk_score.is_none(), "score must be stripped");
    assert!(evt.rf_signature_hash.is_none(), "hash must be stripped");
}

#[test]
fn disable_privacy_mode_restores_baseline_class() {
    let mut p = BfldPipeline::new(BfldConfig::new("seed-01"));
    p.enable_privacy_mode();
    let demoted = p.process(inputs(), Some(embedding())).unwrap();
    assert_eq!(demoted.privacy_class, PrivacyClass::Restricted);

    p.disable_privacy_mode();
    assert!(!p.is_privacy_mode_enabled());
    assert_eq!(p.current_privacy_class(), PrivacyClass::Anonymous);
    let restored = p.process(inputs(), Some(embedding())).unwrap();
    assert_eq!(restored.privacy_class, PrivacyClass::Anonymous);
    assert!(restored.identity_risk_score.is_some());
}

#[test]
fn privacy_mode_overrides_derived_baseline_too() {
    // Operator running at Derived (class 1, research mode) can still flip the
    // emergency switch to Restricted without restarting the pipeline.
    let mut p =
        BfldPipeline::new(BfldConfig::new("seed-01").with_privacy_class(PrivacyClass::Derived));
    p.enable_privacy_mode();
    let evt = p.process(inputs(), Some(embedding())).unwrap();
    assert_eq!(evt.privacy_class, PrivacyClass::Restricted);
    assert!(evt.identity_risk_score.is_none());
}

// --- hasher wiring through the facade -----------------------------------

#[test]
fn pipeline_with_hasher_emits_derived_rf_signature_hash() {
    let hasher = SignatureHasher::new([7u8; SITE_SALT_LEN]);
    let mut p = BfldPipeline::new(BfldConfig::new("seed-01").with_signature_hasher(hasher));
    let evt = p.process(inputs(), Some(embedding())).unwrap();
    let hash = evt
        .rf_signature_hash
        .expect("hasher path must produce a hash");
    assert_ne!(hash, [0u8; 32], "derived hash must be non-trivial");
}

#[test]
fn zone_is_threaded_from_config_to_event() {
    let mut p = BfldPipeline::new(BfldConfig::new("seed-01").with_zone("kitchen"));
    let evt = p.process(inputs(), Some(embedding())).unwrap();
    assert_eq!(evt.zone_id.as_deref(), Some("kitchen"));
}
