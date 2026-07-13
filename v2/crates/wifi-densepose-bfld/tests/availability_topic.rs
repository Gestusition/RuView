//! Acceptance tests for ADR-122 §2.2 availability topic + LWT integration.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    availability_topic, offline_message, online_message, publish_availability_offline,
    publish_availability_online, render_discovery_payloads, CapturePublisher, PrivacyClass,
    PAYLOAD_AVAILABLE, PAYLOAD_NOT_AVAILABLE,
};

#[test]
fn availability_topic_format_matches_documented_path() {
    assert_eq!(
        availability_topic("seed-01"),
        "ruview/seed-01/bfld/availability",
    );
}

#[test]
fn online_message_is_retained_friendly_payload() {
    let msg = online_message("seed-99");
    assert_eq!(msg.topic, "ruview/seed-99/bfld/availability");
    assert_eq!(msg.payload, "online");
    assert_eq!(msg.payload, PAYLOAD_AVAILABLE);
}

#[test]
fn offline_message_is_retained_friendly_payload() {
    let msg = offline_message("seed-99");
    assert_eq!(msg.payload, "offline");
    assert_eq!(msg.payload, PAYLOAD_NOT_AVAILABLE);
}

#[test]
fn publish_online_lands_one_message() {
    let mut p = CapturePublisher::default();
    publish_availability_online(&mut p, "seed-01").unwrap();
    assert_eq!(p.published.len(), 1);
    assert_eq!(p.published[0].payload, "online");
}

#[test]
fn publish_offline_lands_one_message() {
    let mut p = CapturePublisher::default();
    publish_availability_offline(&mut p, "seed-01").unwrap();
    assert_eq!(p.published.len(), 1);
    assert_eq!(p.published[0].payload, "offline");
}

// --- discovery payload integration --------------------------------------

#[test]
fn discovery_payload_includes_availability_topic_field() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    for msg in &msgs {
        assert!(
            msg.payload
                .contains("\"availability_topic\":\"ruview/seed-01/bfld/availability\""),
            "discovery payload must reference availability_topic, got: {}",
            msg.payload,
        );
    }
}

#[test]
fn discovery_payload_includes_payload_available_and_not_available_strings() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    for msg in &msgs {
        assert!(
            msg.payload.contains("\"payload_available\":\"online\""),
            "discovery payload missing payload_available, got: {}",
            msg.payload,
        );
        assert!(
            msg.payload
                .contains("\"payload_not_available\":\"offline\""),
            "discovery payload missing payload_not_available, got: {}",
            msg.payload,
        );
    }
}

#[test]
fn restricted_class_discovery_still_carries_availability_fields() {
    // Availability isn't an identity field — class 3 retains it.
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Restricted);
    assert_eq!(msgs.len(), 5);
    for msg in &msgs {
        assert!(msg.payload.contains("\"availability_topic\":"));
    }
}

// --- bootstrap composition ----------------------------------------------

#[test]
fn bootstrap_sequence_online_then_discovery_lands_in_order() {
    let mut p = CapturePublisher::default();
    publish_availability_online(&mut p, "seed-01").expect("online");
    let count = wifi_densepose_bfld::publish_discovery(&mut p, "seed-01", PrivacyClass::Anonymous)
        .expect("discovery");
    assert_eq!(count, 6);
    assert_eq!(p.published.len(), 1 + 6);
    assert_eq!(p.published[0].payload, "online");
    for msg in p.published.iter().skip(1) {
        assert!(msg.topic.starts_with("homeassistant/"));
    }
}

#[test]
fn graceful_shutdown_sequence_publishes_offline_message_last() {
    let mut p = CapturePublisher::default();
    publish_availability_online(&mut p, "seed-01").unwrap();
    publish_availability_offline(&mut p, "seed-01").unwrap();
    assert_eq!(p.published.len(), 2);
    assert_eq!(p.published[0].payload, "online");
    assert_eq!(p.published[1].payload, "offline");
}
