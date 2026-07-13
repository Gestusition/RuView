//! Acceptance tests for ADR-122 §2.1 — HA auto-discovery payloads.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{render_discovery_payloads, PrivacyClass};

fn topics(class: PrivacyClass) -> Vec<String> {
    render_discovery_payloads("seed-01", class)
        .into_iter()
        .map(|m| m.topic)
        .collect()
}

#[test]
fn raw_and_derived_classes_produce_no_discovery_payloads() {
    for class in [PrivacyClass::Raw, PrivacyClass::Derived] {
        assert!(
            render_discovery_payloads("seed-01", class).is_empty(),
            "class {class:?} must not emit HA discovery",
        );
    }
}

#[test]
fn anonymous_class_produces_six_discovery_payloads() {
    let ts = topics(PrivacyClass::Anonymous);
    assert_eq!(ts.len(), 6);
}

#[test]
fn restricted_class_omits_identity_risk_discovery() {
    let ts = topics(PrivacyClass::Restricted);
    assert_eq!(ts.len(), 5, "Restricted: 5 entities, no identity_risk");
    assert!(
        !ts.iter().any(|t| t.contains("identity_risk")),
        "Restricted must not advertise identity_risk entity to HA",
    );
}

#[test]
fn discovery_topic_format_matches_ha_convention() {
    let ts = topics(PrivacyClass::Anonymous);
    assert!(ts.contains(&"homeassistant/binary_sensor/seed-01_bfld_presence/config".into()));
    assert!(ts.contains(&"homeassistant/sensor/seed-01_bfld_motion/config".into()));
    assert!(ts.contains(&"homeassistant/sensor/seed-01_bfld_person_count/config".into()));
    assert!(ts.contains(&"homeassistant/sensor/seed-01_bfld_zone_activity/config".into()));
    assert!(ts.contains(&"homeassistant/sensor/seed-01_bfld_confidence/config".into()));
    assert!(ts.contains(&"homeassistant/sensor/seed-01_bfld_identity_risk/config".into()));
}

#[test]
fn presence_payload_carries_occupancy_device_class() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    let pres = msgs
        .iter()
        .find(|m| m.topic.contains("presence"))
        .expect("presence config");
    assert!(pres.payload.contains("\"device_class\":\"occupancy\""));
}

#[test]
fn motion_payload_marked_as_diagnostic() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    let motion = msgs
        .iter()
        .find(|m| m.topic.contains("motion"))
        .expect("motion config");
    assert!(motion
        .payload
        .contains("\"entity_category\":\"diagnostic\""));
}

#[test]
fn person_count_payload_carries_unit_of_measurement() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    let pc = msgs
        .iter()
        .find(|m| m.topic.contains("person_count"))
        .expect("person_count config");
    assert!(pc.payload.contains("\"unit_of_measurement\":\"people\""));
}

#[test]
fn every_payload_contains_unique_id_and_state_topic_pointing_at_correct_state_topic() {
    let msgs = render_discovery_payloads("seed-99", PrivacyClass::Anonymous);
    for msg in &msgs {
        // unique_id is required for HA to dedupe entity creation.
        assert!(
            msg.payload.contains("\"unique_id\":\""),
            "missing unique_id in {msg:?}",
        );
        // state_topic must point back at the BFLD `ruview/<node>/bfld/<entity>/state` path.
        assert!(
            msg.payload
                .contains("\"state_topic\":\"ruview/seed-99/bfld/"),
            "state_topic wrong in {msg:?}",
        );
        // Device block ties all six entities to one HA device.
        assert!(msg.payload.contains("\"device\":{"));
        assert!(msg.payload.contains("\"identifiers\":\"seed-99\""));
        assert!(msg.payload.contains("\"manufacturer\":\"RuView\""));
    }
}

#[test]
fn unique_id_matches_topic_segment() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    for msg in &msgs {
        // topic is homeassistant/<type>/<unique_id>/config — the unique_id segment
        // must appear in the payload too.
        let parts: Vec<&str> = msg.topic.split('/').collect();
        assert_eq!(parts.len(), 4, "topic shape wrong: {}", msg.topic);
        assert_eq!(parts[0], "homeassistant");
        assert_eq!(parts[3], "config");
        let unique_id_from_topic = parts[2];
        let needle = format!("\"unique_id\":\"{unique_id_from_topic}\"");
        assert!(
            msg.payload.contains(&needle),
            "unique_id mismatch between topic and payload: {msg:?}",
        );
    }
}

#[test]
fn class_2_discovery_includes_identity_risk_explicitly() {
    let msgs = render_discovery_payloads("seed-01", PrivacyClass::Anonymous);
    let risk = msgs
        .iter()
        .find(|m| m.topic.contains("identity_risk"))
        .expect("identity_risk config must be present at class 2");
    assert!(risk.payload.contains("\"entity_category\":\"diagnostic\""));
}
