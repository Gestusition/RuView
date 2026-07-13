//! Minimal end-to-end BFLD pipeline example. Demonstrates the operator-facing
//! flow: construct a `BfldPipeline` with a `SignatureHasher`, feed one
//! `SensingInputs` + `IdentityEmbedding`, and print the resulting privacy-
//! gated `BfldEvent` as JSON.
//!
//! Run with:
//! ```sh
//! cargo run -p wifi-densepose-bfld --example bfld_minimal
//! ```
//!
//! Expected output: one JSON line on stdout matching the BfldEvent schema
//! (presence, motion, person_count, identity_risk_score, rf_signature_hash,
//! privacy_class = "anonymous").

use wifi_densepose_bfld::{
    BfldConfig, BfldPipeline, IdentityEmbedding, SensingInputs, SignatureHasher, EMBEDDING_DIM,
    SITE_SALT_LEN,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Per-site secret (in production: loaded from TPM / KMS / secret file).
    let site_salt: [u8; SITE_SALT_LEN] = [
        0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x07, 0x18, 0x29, 0x3A, 0x4B, 0x5C, 0x6D, 0x7E, 0x8F,
        0x90, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF, 0x00,
    ];

    // 2. Build the pipeline. Default class = Anonymous, no zone, hasher
    //    installed so rf_signature_hash gets derived from the embedding.
    let mut pipeline = BfldPipeline::new(
        BfldConfig::new("seed-example").with_signature_hasher(SignatureHasher::new(site_salt)),
    );

    // 3. One per-frame sensing observation. In production these come from
    //    the BFI extractor + RuvSense feature engine.
    let inputs = SensingInputs {
        timestamp_ns: 1_700_000_000_000_000_000,
        presence: true,
        motion: 0.42,
        person_count: 1,
        sensing_confidence: 0.91,
        // Low risk — gate stays in Accept; event is published.
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: None, // hasher will derive
    };

    // 4. Embedding from the AETHER encoder (ADR-024). For the example we
    //    fill with a deterministic ramp; production uses real model output.
    let mut emb_values = [0.0f32; EMBEDDING_DIM];
    for (i, v) in emb_values.iter_mut().enumerate() {
        *v = (i as f32) * 0.0073;
    }
    let embedding = IdentityEmbedding::from_raw(emb_values);

    // 5. Drive the pipeline. Returns Some(BfldEvent) when the gate permits;
    //    None on Reject / Recalibrate.
    let event = pipeline
        .process(inputs, Some(embedding))
        .ok_or("gate dropped the event — should not happen at this risk level")?;

    // 6. Publish JSON. Real deployments would feed this to MQTT via the
    //    iter-22 publish_event(&publisher, &event) helper.
    let json = event.to_json()?;
    println!("{json}");
    Ok(())
}
