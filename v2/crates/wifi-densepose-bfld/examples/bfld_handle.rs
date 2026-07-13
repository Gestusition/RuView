//! Worker-thread BFLD example — the production-recommended pattern.
//!
//! Demonstrates the full operator lifecycle:
//!   1. publish_availability_online (retained) → HA marks device online
//!   2. publish_discovery (retained) → HA auto-creates 6 BFLD entities
//!   3. BfldPipelineHandle::spawn → worker owns gate + ring + hasher
//!   4. handle.send(input) per BFI frame → worker process + publish
//!   5. handle.shutdown() → clean worker join
//!   6. publish_availability_offline → HA marks device offline
//!
//! Run with:
//! ```sh
//! cargo run -p wifi-densepose-bfld --example bfld_handle
//! ```
//!
//! For a real broker, swap `CapturePublisher` for `RumqttPublisher::connect_with_lwt(...)`
//! (requires `--features mqtt`).

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wifi_densepose_bfld::{
    publish_availability_offline, publish_availability_online, publish_discovery, BfldConfig,
    BfldPipeline, BfldPipelineHandle, CapturePublisher, IdentityEmbedding, PipelineInput,
    PrivacyClass, SensingInputs, SignatureHasher, EMBEDDING_DIM, SITE_SALT_LEN,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let node_id = "seed-handle-demo";
    let site_salt: [u8; SITE_SALT_LEN] = [0xC0; SITE_SALT_LEN];

    // Shared publisher (CapturePublisher for demo; RumqttPublisher in prod).
    let publisher = Arc::new(Mutex::new(CapturePublisher::default()));

    // ----------------------------------------------------------------
    // Phase 1 — Bootstrap. Three messages land on the broker (or
    // capture log) BEFORE the worker starts: online + 6 discovery payloads.
    // In production these should be published with retain=true so HA picks
    // them up on reconnect.
    // ----------------------------------------------------------------
    publish_availability_online(&mut publisher.clone(), node_id)?;
    let discovery_count =
        publish_discovery(&mut publisher.clone(), node_id, PrivacyClass::Anonymous)?;
    println!("bootstrap: 1 availability + {discovery_count} discovery payloads");

    // ----------------------------------------------------------------
    // Phase 2 — Spawn the worker thread. From this point on, the
    // operator only calls handle.send(...) per frame; the worker owns
    // every piece of pipeline state.
    // ----------------------------------------------------------------
    let pipeline = BfldPipeline::new(
        BfldConfig::new(node_id).with_signature_hasher(SignatureHasher::new(site_salt)),
    );
    let handle = BfldPipelineHandle::spawn(pipeline, publisher.clone());

    // ----------------------------------------------------------------
    // Phase 3 — Drive 5 sensing frames. Each one becomes 5 MQTT state
    // messages (presence/motion/count/conf/identity_risk for Anonymous
    // class, no zone configured).
    // ----------------------------------------------------------------
    for i in 0..5u64 {
        let timestamp_ns = 1_700_000_000_000_000_000 + i * 200_000_000;
        let mut emb = [0.0f32; EMBEDDING_DIM];
        for (j, v) in emb.iter_mut().enumerate() {
            *v = (j as f32 + i as f32) * 0.005;
        }
        let input = PipelineInput {
            inputs: SensingInputs {
                timestamp_ns,
                presence: true,
                motion: 0.3 + (i as f32) * 0.1,
                person_count: 1,
                sensing_confidence: 0.9,
                sep: 0.2,
                stab: 0.2,
                consist: 0.2,
                risk_conf: 0.2,
                rf_signature_hash: None,
            },
            embedding: Some(IdentityEmbedding::from_raw(emb)),
        };
        handle.send(input)?;
    }

    // Give the worker time to drain the channel before shutdown.
    thread::sleep(Duration::from_millis(100));

    // ----------------------------------------------------------------
    // Phase 4 — Graceful shutdown. handle.shutdown() joins the worker;
    // publish_availability_offline then signals HA explicitly (the LWT
    // configured on RumqttPublisher::connect_with_lwt would handle the
    // crash case).
    // ----------------------------------------------------------------
    handle.shutdown();
    publish_availability_offline(&mut publisher.clone(), node_id)?;

    // Print a summary so the example produces visible output.
    let log = publisher.lock().expect("publisher mutex");
    println!("total messages published: {}", log.published.len());
    println!("first three topics:");
    for msg in log.published.iter().take(3) {
        println!("  {}", msg.topic);
    }
    println!("last three topics:");
    for msg in log
        .published
        .iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .iter()
        .rev()
    {
        println!("  {}", msg.topic);
    }
    Ok(())
}
