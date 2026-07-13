//! Validate the cog-ha-matter HA blueprints structurally — they're shipped
//! YAML, so the test embeds each file at compile time via `include_str!` and
//! string-checks the required HA-blueprint fields. Avoids adding a serde_yaml
//! dep to BFLD for what is effectively a documentation-of-record asset.
//!
//! ADR-122 §2.6 specifies three blueprints; this test pins their structure.

#![cfg(feature = "std")]

const PRESENCE_LIGHTING: &str =
    include_str!("../../cog-ha-matter/blueprints/bfld/presence-lighting.yaml");
const MOTION_HVAC: &str = include_str!("../../cog-ha-matter/blueprints/bfld/motion-hvac.yaml");
const IDENTITY_RISK: &str =
    include_str!("../../cog-ha-matter/blueprints/bfld/identity-risk-anomaly.yaml");

fn assert_required_blueprint_fields(yaml: &str, name_substring: &str, label: &str) {
    assert!(
        yaml.contains("blueprint:"),
        "{label}: missing top-level `blueprint:` key",
    );
    assert!(yaml.contains("name:"), "{label}: missing `name`");
    assert!(
        yaml.contains(name_substring),
        "{label}: name does not mention {name_substring}",
    );
    assert!(
        yaml.contains("domain: automation"),
        "{label}: missing `domain: automation`",
    );
    assert!(yaml.contains("input:"), "{label}: missing `input:` block");
    assert!(yaml.contains("trigger:"), "{label}: missing `trigger:`");
    assert!(yaml.contains("action:"), "{label}: missing `action:`");
    assert!(yaml.contains("mode:"), "{label}: missing `mode:`");
}

#[test]
fn presence_lighting_blueprint_is_structurally_valid() {
    assert_required_blueprint_fields(PRESENCE_LIGHTING, "Presence", "presence-lighting");
    assert!(PRESENCE_LIGHTING.contains("bfld_presence"));
    assert!(PRESENCE_LIGHTING.contains("light.turn_on"));
    assert!(PRESENCE_LIGHTING.contains("light.turn_off"));
    assert!(
        PRESENCE_LIGHTING.contains("hold_seconds"),
        "must expose configurable hold time per ADR-122 §2.6",
    );
}

#[test]
fn motion_hvac_blueprint_is_structurally_valid() {
    assert_required_blueprint_fields(MOTION_HVAC, "HVAC", "motion-hvac");
    assert!(MOTION_HVAC.contains("bfld_motion"));
    assert!(MOTION_HVAC.contains("climate.set_temperature"));
    assert!(
        MOTION_HVAC.contains("motion_threshold"),
        "must expose configurable threshold per ADR-122 §2.6",
    );
    assert!(
        MOTION_HVAC.contains("delta_temperature_c"),
        "must expose configurable ΔT per ADR-122 §2.6",
    );
}

#[test]
fn identity_risk_blueprint_is_structurally_valid() {
    assert_required_blueprint_fields(IDENTITY_RISK, "Identity-Risk", "identity-risk-anomaly");
    assert!(IDENTITY_RISK.contains("bfld_identity_risk"));
    assert!(
        IDENTITY_RISK.contains("z_score_threshold"),
        "must expose rolling z-score threshold per ADR-122 §2.6",
    );
    assert!(
        IDENTITY_RISK.contains("statistics_entity"),
        "must require an HA Statistics helper entity for the 7-day baseline",
    );
}

#[test]
fn blueprints_carry_source_url_pointing_at_canonical_path() {
    for (label, yaml, fname) in [
        (
            "presence-lighting",
            PRESENCE_LIGHTING,
            "presence-lighting.yaml",
        ),
        ("motion-hvac", MOTION_HVAC, "motion-hvac.yaml"),
        (
            "identity-risk-anomaly",
            IDENTITY_RISK,
            "identity-risk-anomaly.yaml",
        ),
    ] {
        let needle = format!(
            "source_url: https://github.com/Gestusition/KdView/blob/main/v2/crates/cog-ha-matter/blueprints/bfld/{fname}"
        );
        assert!(
            yaml.contains(&needle),
            "{label}: source_url drift — expected {needle}",
        );
    }
}

#[test]
fn presence_blueprint_uses_mqtt_integration_filter() {
    // The presence blueprint targets BFLD entities published via MQTT auto-
    // discovery; the entity selector must filter to integration: mqtt so
    // operators don't accidentally bind a non-BFLD presence sensor.
    assert!(PRESENCE_LIGHTING.contains("integration: mqtt"));
}

#[test]
fn motion_blueprint_uses_mqtt_integration_filter() {
    assert!(MOTION_HVAC.contains("integration: mqtt"));
}

#[test]
fn identity_risk_blueprint_carries_privacy_class_caveat_in_description() {
    // The description should hint at the class 2-only availability so operators
    // running Restricted (class 3) deployments don't waste time installing the
    // blueprint.
    assert!(
        IDENTITY_RISK.contains("privacy_class") || IDENTITY_RISK.contains("Anonymous"),
        "identity-risk blueprint description should reference privacy_class gating",
    );
}
