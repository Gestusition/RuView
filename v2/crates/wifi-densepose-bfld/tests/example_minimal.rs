//! Validates the `examples/bfld_minimal.rs` operator-quickstart contract.
//! The example file embeds via include_str! for documentation-drift checks,
//! then a separate test re-executes the same end-to-end flow inline so we
//! get a CI-runnable proof that the operator workflow produces valid JSON.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, IdentityEmbedding, SensingInputs, SignatureHasher, EMBEDDING_DIM,
    SITE_SALT_LEN,
};

const MINIMAL_EXAMPLE: &str = include_str!("../examples/bfld_minimal.rs");

#[test]
fn minimal_example_documents_the_operator_quickstart_flow() {
    // The example must call out the canonical operator-facing types so
    // anyone reading it sees the right entry points.
    assert!(MINIMAL_EXAMPLE.contains("BfldPipeline"));
    assert!(MINIMAL_EXAMPLE.contains("SignatureHasher"));
    assert!(MINIMAL_EXAMPLE.contains("SensingInputs"));
    assert!(MINIMAL_EXAMPLE.contains("IdentityEmbedding"));
    assert!(MINIMAL_EXAMPLE.contains("BfldConfig"));
    assert!(
        MINIMAL_EXAMPLE.contains(".process("),
        "example must invoke pipeline.process(...) — method-chain style OK",
    );
    assert!(MINIMAL_EXAMPLE.contains("to_json"));
}

#[test]
fn minimal_example_carries_run_instructions_in_doc_comments() {
    assert!(
        MINIMAL_EXAMPLE.contains("cargo run -p wifi-densepose-bfld --example bfld_minimal"),
        "example must document its own run command",
    );
}

#[test]
fn minimal_example_flow_produces_valid_json_with_documented_fields() {
    // Re-execute the same logic the example does so CI proves the flow
    // works end-to-end without needing `cargo run --example`.
    let site_salt: [u8; SITE_SALT_LEN] = [0xAB; SITE_SALT_LEN];
    let mut pipeline = BfldPipeline::new(
        BfldConfig::new("seed-example").with_signature_hasher(SignatureHasher::new(site_salt)),
    );
    let inputs = SensingInputs {
        timestamp_ns: 1_700_000_000_000_000_000,
        presence: true,
        motion: 0.42,
        person_count: 1,
        sensing_confidence: 0.91,
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: None,
    };
    let mut emb_values = [0.0f32; EMBEDDING_DIM];
    for (i, v) in emb_values.iter_mut().enumerate() {
        *v = (i as f32) * 0.0073;
    }
    let embedding = IdentityEmbedding::from_raw(emb_values);

    let event = pipeline
        .process(inputs, Some(embedding))
        .expect("low-risk emit must succeed");
    let json = event.to_json().expect("JSON serialization must succeed");

    // The published JSON should carry every documented anonymous-class field.
    for needle in [
        "\"type\":\"bfld_update\"",
        "\"node_id\":\"seed-example\"",
        "\"presence\":true",
        "\"motion\":",
        "\"person_count\":1",
        "\"confidence\":",
        "\"privacy_class\":\"anonymous\"",
        "\"identity_risk_score\":",
        "\"rf_signature_hash\":\"blake3:",
    ] {
        assert!(
            json.contains(needle),
            "example JSON missing expected snippet `{needle}`\nfull JSON: {json}",
        );
    }
}

#[test]
fn example_returns_box_dyn_error_for_main_signature() {
    // `main() -> Result<(), Box<dyn std::error::Error>>` is the standard
    // Rust-example pattern. Confirm the file uses it so future copy-paste
    // doesn't drop error propagation.
    assert!(MINIMAL_EXAMPLE.contains("fn main() -> Result<(), Box<dyn std::error::Error>>"),);
}
