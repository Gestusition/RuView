//! `runtime` — pure builders that turn the cog's small CLI surface
//! into the shapes ADR-115's `publisher::spawn` consumes.
//!
//! Kept side-effect-free so the tests don't need a tokio runtime, and
//! so the cog's mDNS responder / control plane (P4) can build the
//! same inputs from a different source (edge-host config, JSON
//! POST) without going through `clap`.
//!
//! Per the ADR-115 integration-test post-mortem (iter 45-48 of the
//! ADR-110 sprint), the MQTT `client_id` MUST be unique per process —
//! reusing a client_id causes the broker to disconnect the previous
//! session and the new publisher reconnects in a loop. We derive
//! `client_id` from the caller-supplied `node_id` for that reason.
//!
//! P3 of ADR-116: this module produces the input pair; the binary
//! wires the actual `tokio::spawn(publisher::run(...))` next iter.
//!
//! The publisher inputs are intentionally typed in *this* crate, so
//! the cog's tests and the `--print-manifest` path can exercise the
//! builder without pulling in the rumqttc event loop.

use std::sync::Arc;

use mdns_sd::ServiceDaemon;
use tokio::{sync::broadcast, task::JoinHandle};
use wifi_densepose_sensing_server::mqtt::{
    config::{MqttConfig, PublishRates, TlsConfig},
    publisher::{self, OwnedDiscoveryBuilder},
    state::VitalsSnapshot,
    DEFAULT_DISCOVERY_PREFIX, MANUFACTURER,
};

use crate::mdns::MdnsService;

/// Caller-supplied identity for the cog instance. Filled in by the
/// cog runtime from the mDNS hostname / local control plane in
/// production; threaded as a parameter so tests can build inputs
/// without touching the environment.
#[derive(Debug, Clone)]
pub struct CogIdentity {
    /// Stable node identifier — appears in MQTT topics, HA device
    /// registry, mDNS service name. Must be ASCII-safe; the cog
    /// runtime is responsible for sanitising user input.
    pub node_id: String,
    /// Human-readable name surfaced in the HA UI.
    pub friendly_name: String,
    /// SemVer of the cog binary. Surfaces as the HA device `sw_version`.
    pub sw_version: String,
}

impl CogIdentity {
    /// Default identity used when the cog runs standalone (no external
    /// control plane). Uses the PID for uniqueness so two cog
    /// instances on the same host don't fight over the same MQTT
    /// session — same trick the ADR-115 publisher uses.
    pub fn default_for_build() -> Self {
        Self {
            node_id: format!("cog-ha-matter-{}", std::process::id()),
            friendly_name: "RuView HA integration".into(),
            sw_version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// The pair ADR-115's `publisher::spawn` needs. Owned so we can move
/// the whole thing into a `tokio::spawn` closure without lifetime
/// gymnastics.
#[derive(Debug, Clone)]
pub struct PublisherInputs {
    pub config: MqttConfig,
    pub discovery: OwnedDiscoveryBuilder,
}

/// Build the publisher inputs from the cog's small CLI surface.
///
/// Pure function — no I/O, no env reads. The caller wraps `config`
/// in an `Arc` before handing it to `publisher::spawn`.
pub fn build_publisher_inputs(
    mqtt_host: &str,
    mqtt_port: u16,
    privacy_mode: bool,
    identity: CogIdentity,
) -> PublisherInputs {
    let config = MqttConfig {
        host: mqtt_host.to_string(),
        port: mqtt_port,
        username: None,
        password: None,
        client_id: format!("{}-{}", super::COG_ID, identity.node_id),
        discovery_prefix: DEFAULT_DISCOVERY_PREFIX.to_string(),
        tls: TlsConfig::Off,
        refresh_secs: 60,
        rates: PublishRates::default(),
        publish_pose: false,
        privacy_mode,
    };

    let discovery = OwnedDiscoveryBuilder {
        discovery_prefix: DEFAULT_DISCOVERY_PREFIX.to_string(),
        node_id: identity.node_id,
        node_friendly_name: Some(identity.friendly_name),
        sw_version: identity.sw_version,
        model: format!("{MANUFACTURER} cog-ha-matter"),
        via_device: Some(super::COG_ID.to_string()),
    };

    PublisherInputs { config, discovery }
}

/// Default broadcast-channel capacity for the cog's VitalsSnapshot
/// stream. Matches the sensing-server's own default so the cog
/// doesn't bottleneck the publisher under bursty loads (multi-node
/// federation, mesh re-sync events).
pub const DEFAULT_STATE_CHANNEL_CAPACITY: usize = 256;

/// Spawn the ADR-115 MQTT publisher with the cog's typed inputs.
///
/// Thin wrapper around [`publisher::spawn`] that:
/// 1. wraps `inputs.config` in `Arc` (publisher requires shared
///    ownership across reconnects),
/// 2. moves `inputs.discovery` into the spawn (publisher clones it
///    per reconnect; `OwnedDiscoveryBuilder` is `Clone`),
/// 3. hands the broadcast receiver across without an intermediate.
///
/// Returning the `JoinHandle` lets `main.rs` await it on shutdown
/// (or `abort()` it from a control-plane handler).
pub fn spawn_publisher(
    inputs: PublisherInputs,
    state_rx: broadcast::Receiver<VitalsSnapshot>,
) -> JoinHandle<()> {
    let PublisherInputs { config, discovery } = inputs;
    publisher::spawn(Arc::new(config), discovery, state_rx)
}

/// Owned handle to a live mDNS responder. Holding it keeps the
/// service advertised; `shutdown` unregisters cleanly so HA's
/// discovery integration sees a goodbye packet instead of a
/// dropped advertisement.
///
/// `Drop` is best-effort: tries unregister + daemon shutdown but
/// swallows errors, since panicking in Drop would mask the real
/// failure that prompted the shutdown.
pub struct MdnsResponderHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsResponderHandle {
    /// Fully-qualified DNS-SD name (`<instance>.<type>.<domain>`).
    /// Exposed for tests + logging; the responder uses it to
    /// unregister.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// Unregister the service and shut down the daemon. Returns
    /// any error so the caller's shutdown sequence can surface it.
    pub fn shutdown(self) -> Result<(), mdns_sd::Error> {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown()?;
        Ok(())
    }
}

impl Drop for MdnsResponderHandle {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Start the mDNS responder for a cog and register its service.
///
/// Binds a multicast socket (`mdns_sd::ServiceDaemon::new`) and
/// publishes `service` under `hostname` (must end in `.local.`)
/// and `ipv4` (the LAN-routable address HA's discovery reaches
/// back on).
///
/// Live-I/O: binding multicast may fail in containerised CI or
/// on networks where 5353/udp is filtered — callers should treat
/// the error as recoverable (log + retry, or fall back to manual
/// HA configuration) rather than fatal to the cog.
pub fn start_mdns_responder(
    service: &MdnsService,
    hostname: &str,
    ipv4: &str,
) -> Result<MdnsResponderHandle, mdns_sd::Error> {
    let daemon = ServiceDaemon::new()?;
    let info = service.to_service_info(hostname, ipv4)?;
    let fullname = info.get_fullname().to_string();
    daemon.register(info)?;
    Ok(MdnsResponderHandle { daemon, fullname })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> CogIdentity {
        CogIdentity {
            node_id: "node-7".into(),
            friendly_name: "test-node".into(),
            sw_version: "0.0.1-test".into(),
        }
    }

    #[test]
    fn host_and_port_round_trip_into_mqtt_config() {
        let out = build_publisher_inputs("10.0.0.5", 8883, false, id());
        assert_eq!(out.config.host, "10.0.0.5");
        assert_eq!(out.config.port, 8883);
    }

    #[test]
    fn privacy_mode_propagates_to_mqtt_config() {
        let on = build_publisher_inputs("h", 1883, true, id());
        let off = build_publisher_inputs("h", 1883, false, id());
        assert!(on.config.privacy_mode);
        assert!(!off.config.privacy_mode);
    }

    #[test]
    fn discovery_prefix_defaults_to_homeassistant() {
        let out = build_publisher_inputs("h", 1883, false, id());
        assert_eq!(out.config.discovery_prefix, DEFAULT_DISCOVERY_PREFIX);
        assert_eq!(out.discovery.discovery_prefix, DEFAULT_DISCOVERY_PREFIX);
    }

    #[test]
    fn discovery_carries_identity_fields() {
        let out = build_publisher_inputs("h", 1883, false, id());
        assert_eq!(out.discovery.node_id, "node-7");
        assert_eq!(out.discovery.sw_version, "0.0.1-test");
        assert_eq!(
            out.discovery.node_friendly_name.as_deref(),
            Some("test-node")
        );
    }

    #[test]
    fn via_device_advertises_cog_id() {
        // ADR-101 / ADR-102: every cog must surface its `id` as the
        // HA device's `via_device` so the appliance shows up as the
        // bridge — fires a named test instead of silently breaking
        // the device-registry shape.
        let out = build_publisher_inputs("h", 1883, false, id());
        assert_eq!(
            out.discovery.via_device.as_deref(),
            Some(super::super::COG_ID)
        );
    }

    #[test]
    fn client_id_includes_node_id_for_session_uniqueness() {
        // Lesson from the ADR-115 integration-test post-mortem: two
        // publishers sharing a `client_id` fight over the broker
        // session and one reconnects forever. The cog must derive
        // `client_id` from `node_id` so multi-node deployments don't
        // collide.
        let out = build_publisher_inputs("h", 1883, false, id());
        assert!(out.config.client_id.contains("node-7"));
        assert!(out.config.client_id.starts_with(super::super::COG_ID));
    }

    #[test]
    fn tls_defaults_to_off_for_v1_lan_only() {
        // v1 ships LAN-only (no broker on the open internet); TLS
        // wiring lands in v0.8 alongside Matter Bridge per ADR-116
        // §4. Lock the default so a future refactor surfaces a
        // named test instead of silently enabling TLS.
        let out = build_publisher_inputs("h", 1883, false, id());
        assert!(matches!(out.config.tls, TlsConfig::Off));
    }

    #[tokio::test]
    async fn spawn_publisher_returns_live_handle_without_broker() {
        // No real broker on this port — rumqttc retries internally so
        // the spawned task stays alive. We just prove the wiring
        // compiles + the JoinHandle is not pre-finished. Aborting
        // immediately keeps the test under 100 ms.
        let inputs = build_publisher_inputs("127.0.0.1", 1, false, id());
        let (tx, rx) = broadcast::channel::<VitalsSnapshot>(DEFAULT_STATE_CHANNEL_CAPACITY);
        let handle = spawn_publisher(inputs, rx);
        // Task is still running (not pre-finished by config validation).
        assert!(!handle.is_finished());
        // Keep `tx` alive past the handle abort so the receiver side
        // doesn't panic on drop before the task notices the channel
        // closed.
        handle.abort();
        let _ = handle.await; // joined, may be Err(Cancelled) — OK.
        drop(tx);
    }

    #[test]
    fn default_state_channel_capacity_is_reasonable() {
        // Lock the default so a regression to e.g. 1 surfaces a named
        // test. Multi-node federation needs headroom for bursty
        // mesh re-sync events.
        assert!(DEFAULT_STATE_CHANNEL_CAPACITY >= 64);
    }

    #[test]
    fn mdns_responder_fullname_concatenates_instance_and_service_type() {
        // Live-I/O test: binds multicast on the loopback adapter.
        // Skips with a warning if the host's network stack refuses
        // the bind (containerised CI without --network host, etc.)
        // rather than failing the whole test suite.
        use crate::mdns::build_mdns_service;
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        let handle = match start_mdns_responder(&svc, "cog-ha-matter-test.local.", "127.0.0.1") {
            Ok(h) => h,
            Err(e) => {
                eprintln!("mdns multicast bind not available in this sandbox: {e} — skipping");
                return;
            }
        };
        // Fullname format is "<instance>.<service_type>." per RFC 6763.
        // mdns-sd may URL-escape special chars (— in instance name) so
        // we only assert on the service-type segment which is stable.
        let fullname = handle.fullname().to_string();
        assert!(!fullname.is_empty(), "fullname empty after register");
        assert!(
            fullname.contains("_ruview-ha._tcp"),
            "fullname `{fullname}` missing service type"
        );
        handle.shutdown().expect("clean shutdown");
    }

    #[test]
    fn default_identity_carries_pkg_version_and_pid() {
        let identity = CogIdentity::default_for_build();
        assert_eq!(identity.sw_version, env!("CARGO_PKG_VERSION"));
        assert!(identity.node_id.starts_with("cog-ha-matter-"));
        // Friendly name is non-empty so HA's device card has a label.
        assert!(!identity.friendly_name.is_empty());
    }
}
