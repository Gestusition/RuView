//! Validate `examples/bfld_handle.rs` operator quickstart. Re-runs the same
//! lifecycle inline so CI proves the worker-thread pattern works end-to-end.

#![cfg(feature = "std")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wifi_densepose_bfld::{
    publish_availability_offline, publish_availability_online, publish_discovery, BfldConfig,
    BfldPipeline, BfldPipelineHandle, CapturePublisher, IdentityEmbedding, PipelineInput,
    PrivacyClass, SensingInputs, SignatureHasher, EMBEDDING_DIM, SITE_SALT_LEN,
};

const HANDLE_EXAMPLE: &str = include_str!("../examples/bfld_handle.rs");

#[test]
fn handle_example_documents_full_lifecycle_phases() {
    // Doc drift guard: every operator-facing symbol must appear in the file.
    for needle in [
        "publish_availability_online",
        "publish_discovery",
        "BfldPipelineHandle::spawn",
        "handle.send",
        "handle.shutdown",
        "publish_availability_offline",
        "SignatureHasher",
        "PipelineInput",
    ] {
        assert!(
            HANDLE_EXAMPLE.contains(needle),
            "example must reference {needle}",
        );
    }
}

#[test]
fn handle_example_carries_run_instructions_and_prod_pointer() {
    assert!(
        HANDLE_EXAMPLE.contains("cargo run -p wifi-densepose-bfld --example bfld_handle"),
        "example must document its own run command",
    );
    assert!(
        HANDLE_EXAMPLE.contains("RumqttPublisher::connect_with_lwt"),
        "example must point operators at the production publisher path",
    );
}

#[test]
fn handle_example_lifecycle_produces_expected_message_counts() {
    // Re-execute the lifecycle inline. End state must show:
    //   1 (online) + 6 (discovery anonymous + zone-less) + 5×5 (state per
    //   send) + 1 (offline) = 33 messages.
    let node_id = "seed-handle-test";
    let site_salt: [u8; SITE_SALT_LEN] = [0xC0; SITE_SALT_LEN];

    let publisher = Arc::new(Mutex::new(CapturePublisher::default()));

    publish_availability_online(&mut publisher.clone(), node_id).expect("online");
    let discovery_count =
        publish_discovery(&mut publisher.clone(), node_id, PrivacyClass::Anonymous)
            .expect("discovery");
    assert_eq!(discovery_count, 6);

    let pipeline = BfldPipeline::new(
        BfldConfig::new(node_id).with_signature_hasher(SignatureHasher::new(site_salt)),
    );
    let handle = BfldPipelineHandle::spawn(pipeline, publisher.clone());

    for i in 0..5u64 {
        let timestamp_ns = 1_700_000_000_000_000_000 + i * 200_000_000;
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
            embedding: Some(IdentityEmbedding::from_raw([0.05; EMBEDDING_DIM])),
        };
        handle.send(input).expect("send");
    }
    thread::sleep(Duration::from_millis(120));
    handle.shutdown();

    publish_availability_offline(&mut publisher.clone(), node_id).expect("offline");

    let log = publisher.lock().expect("publisher mutex");
    let total = log.published.len();

    // Expected: 1 online + 6 discovery + 5 × 5 state + 1 offline = 33.
    assert_eq!(
        total,
        33,
        "expected 33 total messages from full lifecycle, got {total}; \
         topics: {:?}",
        log.published.iter().map(|m| &m.topic).collect::<Vec<_>>(),
    );

    // First message is the online availability.
    assert_eq!(log.published[0].payload, "online");
    // Last message is the offline availability.
    assert_eq!(log.published[total - 1].payload, "offline");
}

#[test]
fn handle_example_returns_box_dyn_error_for_main_signature() {
    assert!(HANDLE_EXAMPLE.contains("fn main() -> Result<(), Box<dyn std::error::Error>>"),);
}
