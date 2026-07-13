//! Structural validation for `.github/workflows/bfld-mqtt-integration.yml`.
//! Same pattern as iter-30's HA blueprint tests: embed via `include_str!`,
//! string-check the key fields. Avoids adding a serde_yaml dep just to lint
//! a CI workflow.

#![cfg(feature = "std")]

const WORKFLOW: &str = include_str!("../../../../.github/workflows/bfld-mqtt-integration.yml");

#[test]
fn workflow_declares_mosquitto_service_container() {
    assert!(
        WORKFLOW.contains("image: eclipse-mosquitto:2"),
        "workflow must declare eclipse-mosquitto:2 as a service container",
    );
    assert!(
        WORKFLOW.contains("- 1883:1883"),
        "workflow must expose port 1883 from the mosquitto service",
    );
}

#[test]
fn workflow_exports_broker_env_for_iter_24_and_29_tests() {
    assert!(
        WORKFLOW.contains("BFLD_MQTT_BROKER: tcp://localhost:1883"),
        "BFLD_MQTT_BROKER env var must point at the service container so the \
         iter-24 mosquitto_integration test exits skip mode",
    );
}

#[test]
fn workflow_runs_three_cargo_test_invocations() {
    // Regression guard for the default + no-default-features + mqtt matrix.
    // Each one catches a different class of bug:
    //   --no-default-features: catches std-feature leakage
    //   default:               catches the everyday surface
    //   --features mqtt:       catches the live-broker integration path
    assert!(WORKFLOW.contains("cargo test -p wifi-densepose-bfld --no-default-features"));
    assert!(WORKFLOW.contains("cargo test -p wifi-densepose-bfld"));
    assert!(WORKFLOW.contains("cargo test -p wifi-densepose-bfld --features mqtt"));
}

#[test]
fn workflow_waits_for_mosquitto_readiness_before_testing() {
    assert!(
        WORKFLOW.contains("nc -z localhost 1883"),
        "workflow must port-poll for mosquitto readiness — a service container \
         can take a few seconds to bind even with healthcheck",
    );
}

#[test]
fn workflow_uses_health_check_on_the_service() {
    assert!(
        WORKFLOW.contains("--health-cmd"),
        "service container should declare a health-check for stable startup",
    );
    assert!(
        WORKFLOW.contains("mosquitto_pub"),
        "health-check should attempt a real publish, not just process liveness",
    );
}

#[test]
fn workflow_only_triggers_on_bfld_paths() {
    assert!(
        WORKFLOW.contains("v2/crates/wifi-densepose-bfld/**"),
        "path filter must scope the workflow to BFLD changes, not run on every push",
    );
}

#[test]
fn workflow_pins_runner_to_ubuntu_latest_for_docker_service_support() {
    assert!(
        WORKFLOW.contains("runs-on: ubuntu-latest"),
        "GitHub Actions Docker service containers require linux; macOS and \
         Windows runners don't support `services:`.",
    );
}

#[test]
fn workflow_has_timeout_guard() {
    // The integration tests have 10-second recv timeouts but the matrix runs
    // three cargo invocations + cache + warmup; a top-level timeout-minutes
    // guards against a stuck broker or rumqttc handshake hanging the runner.
    assert!(
        WORKFLOW.contains("timeout-minutes:"),
        "workflow must declare a top-level timeout-minutes to bound runner cost",
    );
}
