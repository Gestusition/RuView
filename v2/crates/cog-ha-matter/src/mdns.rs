//! `mdns` — pure builder for the cog's mDNS advertisement record.
//!
//! ADR-116 §2.2: the cog must advertise itself as `_ruview-ha._tcp`
//! so HA's discovery integration finds the service without manual
//! `broker host` config. This module produces the typed wire-format
//! shape — no socket I/O, no responder. The actual mDNS responder
//! (mdns-sd / zeroconf / pnet) lands next iter and consumes this
//! struct as its single input.
//!
//! Keeping the record-builder pure means:
//!
//!   * the responder library can be swapped without touching the
//!     content of the advertisement;
//!   * the build-time `--print-manifest` path can include the
//!     advertisement shape so integration tests can assert on
//!     it without booting tokio;
//!   * the TXT keys are locked by named unit tests — drift between
//!     the cog and the HA-side YAML auto-discovery (`hass-wifi-...`)
//!     fires a test instead of silently breaking a deployment.
//!
//! ## TXT record convention (RFC 6763)
//!
//! HA's mDNS discovery integration reads TXT records when binding a
//! manifest to a `homeassistant.<integration>` zeroconf hook. We
//! publish the minimum set that lets HA distinguish the integration from
//! a bare sensing-server and pick the right config flow:
//!
//! | Key | Value | Purpose |
//! |---|---|---|
//! | `cog_id` | `"ha-matter"` | Disambiguates from other RuView cogs |
//! | `cog_version` | `CARGO_PKG_VERSION` | HA Repairs surfaces upgrade nudges |
//! | `node_id` | identity node id | HA device registry key |
//! | `mqtt_port` | u16 string | Tells HA where to reach the cog's MQTT broker (embedded or external) |
//! | `privacy` | `"1"` / `"0"` | If `1`, HA's config flow gates biometric entities by default |
//! | `proto` | `"ruview-ha/1"` | Protocol version — bumps on breaking auto-discovery changes |
//!
//! No biometric data, no node coordinates, no SSID — TXT records
//! are broadcast in cleartext and harvested by passive scanners, so
//! treating them as PII-clean is part of the privacy posture.

use std::collections::HashMap;

use mdns_sd::ServiceInfo;

use crate::COG_ID;

/// Default mDNS instance name template. `{node_id}` is substituted
/// at build time. Visible in HA's UI when the integration card is
/// added — "RuView HA (kitchen)" beats a raw UUID.
const INSTANCE_TEMPLATE: &str = "RuView HA — {node_id}";

/// Wire-format twin of the mDNS service record this cog publishes.
/// Owned so the responder can move the whole thing into its task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsService {
    /// RFC 6763 service type. Locked to `_ruview-ha._tcp` by a named
    /// test — drift breaks HA's YAML auto-discovery binding.
    pub service_type: String,
    /// Human-readable instance name shown in HA's discovery UI.
    pub instance_name: String,
    /// Port the cog's control plane listens on (NOT the MQTT broker
    /// port — HA needs both, but the service record advertises the
    /// control plane; the MQTT port rides as a TXT record).
    pub control_port: u16,
    /// TXT records sorted by key for deterministic ordering. RFC
    /// 6763 §6.4 makes ordering implementation-defined, but locking
    /// it keeps the cog's wire shape byte-stable across rebuilds.
    pub txt_records: Vec<(String, String)>,
}

impl MdnsService {
    /// Look up a TXT key without iterating the caller. `None` if the
    /// key isn't published — the responder treats absence as
    /// "feature off" rather than "unknown".
    pub fn txt(&self, key: &str) -> Option<&str> {
        self.txt_records
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Convert into the `mdns_sd::ServiceInfo` the responder daemon
    /// consumes. Pure transform — no socket binding, no daemon
    /// registration. The caller wires the resulting `ServiceInfo`
    /// into `ServiceDaemon::register` (next iter).
    ///
    /// `hostname` should end in `.local.` per RFC 6762 — e.g.
    /// `"ruview-ha-1.local."`. `ipv4` is the LAN-routable
    /// address HA's discovery will reach back on.
    pub fn to_service_info(
        &self,
        hostname: &str,
        ipv4: &str,
    ) -> Result<ServiceInfo, mdns_sd::Error> {
        let mut props: HashMap<String, String> = HashMap::with_capacity(self.txt_records.len());
        for (k, v) in &self.txt_records {
            props.insert(k.clone(), v.clone());
        }
        ServiceInfo::new(
            &self.service_type,
            &self.instance_name,
            hostname,
            ipv4,
            self.control_port,
            Some(props),
        )
    }
}

/// Build the cog's mDNS advertisement record from the cog's typed
/// identity + ports. Pure: no I/O, no env reads.
pub fn build_mdns_service(
    identity: &crate::runtime::CogIdentity,
    control_port: u16,
    mqtt_port: u16,
    privacy_mode: bool,
) -> MdnsService {
    let mut txt_records = vec![
        ("cog_id".to_string(), COG_ID.to_string()),
        ("cog_version".to_string(), identity.sw_version.clone()),
        ("node_id".to_string(), identity.node_id.clone()),
        ("mqtt_port".to_string(), mqtt_port.to_string()),
        (
            "privacy".to_string(),
            if privacy_mode { "1" } else { "0" }.to_string(),
        ),
        ("proto".to_string(), "ruview-ha/1".to_string()),
    ];
    // Deterministic ordering — see field docstring.
    txt_records.sort();

    MdnsService {
        service_type: crate::MDNS_SERVICE_TYPE.to_string(),
        instance_name: INSTANCE_TEMPLATE.replace("{node_id}", &identity.node_id),
        control_port,
        txt_records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::CogIdentity;

    fn id() -> CogIdentity {
        CogIdentity {
            node_id: "kitchen-node".into(),
            friendly_name: "Kitchen Node".into(),
            sw_version: "0.3.0".into(),
        }
    }

    #[test]
    fn service_type_locked_to_ruview_ha_tcp() {
        // Drift here breaks HA's YAML auto-discovery binding. Lock
        // it so a future rename surfaces a named test instead of a
        // silent broken deployment.
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        assert_eq!(svc.service_type, "_ruview-ha._tcp");
        assert_eq!(svc.service_type, crate::MDNS_SERVICE_TYPE);
    }

    #[test]
    fn instance_name_carries_node_id() {
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        assert!(svc.instance_name.contains("kitchen-node"));
    }

    #[test]
    fn control_port_field_holds_control_not_mqtt_port() {
        // Easy to swap by accident. Lock the binding so a refactor
        // doesn't silently advertise the MQTT broker as the control
        // plane.
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        assert_eq!(svc.control_port, 9180);
        assert_eq!(svc.txt("mqtt_port"), Some("1883"));
    }

    #[test]
    fn privacy_flag_is_one_or_zero() {
        let on = build_mdns_service(&id(), 9180, 1883, true);
        let off = build_mdns_service(&id(), 9180, 1883, false);
        assert_eq!(on.txt("privacy"), Some("1"));
        assert_eq!(off.txt("privacy"), Some("0"));
    }

    #[test]
    fn proto_version_bumps_surface_in_txt() {
        // Locked so a future breaking-change in the cog ↔ HA YAML
        // contract surfaces here. Bumping it is a deliberate act.
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        assert_eq!(svc.txt("proto"), Some("ruview-ha/1"));
    }

    #[test]
    fn cog_id_in_txt_matches_crate_constant() {
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        assert_eq!(svc.txt("cog_id"), Some(crate::COG_ID));
    }

    #[test]
    fn txt_records_are_sorted_for_byte_stable_advertisement() {
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        let keys: Vec<&str> = svc.txt_records.iter().map(|(k, _)| k.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn txt_carries_no_biometric_or_pii_keys() {
        // TXT records broadcast in cleartext; passive scanners
        // harvest them. Lock the publishable surface so a future
        // "let's add hr_bpm to TXT for convenience" patch fires a
        // named test instead of leaking biometrics.
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        let forbidden = [
            "hr_bpm", "br_bpm", "pose_x", "pose_y", "keypoint", "ssid", "lat", "lon", "mac", "rssi",
        ];
        for key in forbidden {
            assert!(
                svc.txt(key).is_none(),
                "TXT key `{key}` leaks PII / biometric data — must not be advertised"
            );
        }
    }

    #[test]
    fn to_service_info_carries_service_type_and_port() {
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        let info = svc
            .to_service_info("ruview-ha-1.local.", "192.168.1.50")
            .expect("valid service info");
        // mdns-sd may rewrite the type with a trailing dot; allow
        // both forms.
        let ty = info.get_type();
        assert!(
            ty == "_ruview-ha._tcp" || ty == "_ruview-ha._tcp.",
            "unexpected service type: {ty}"
        );
        assert_eq!(info.get_port(), 9180);
    }

    #[test]
    fn to_service_info_propagates_txt_records() {
        let svc = build_mdns_service(&id(), 9180, 1883, true);
        let info = svc
            .to_service_info("ruview-ha-1.local.", "192.168.1.50")
            .expect("valid service info");
        // Every locked TXT key must reach the wire-format payload.
        assert_eq!(info.get_property_val_str("cog_id"), Some(crate::COG_ID));
        assert_eq!(info.get_property_val_str("mqtt_port"), Some("1883"));
        assert_eq!(info.get_property_val_str("privacy"), Some("1"));
        assert_eq!(info.get_property_val_str("proto"), Some("ruview-ha/1"));
        assert!(info.get_property_val_str("node_id").is_some());
        assert!(info.get_property_val_str("cog_version").is_some());
    }

    #[test]
    fn to_service_info_does_not_silently_drop_caller_hostname() {
        // mdns-sd 0.11 accepts bare hostnames (no `.local.`); the
        // responsibility for the trailing dot lives in our wrapper.
        // Lock that the caller's hostname survives the conversion
        // verbatim — a future bump that starts mutating the value
        // surfaces a named test instead of a silent change.
        let svc = build_mdns_service(&id(), 9180, 1883, false);
        let info = svc
            .to_service_info("ruview-ha-1.local.", "192.168.1.50")
            .unwrap();
        assert!(info.get_hostname().contains("ruview-ha-1"));
    }

    #[test]
    fn txt_keys_match_locked_surface() {
        // The HA-side YAML auto-discovery binds on these exact keys.
        // Adding a key is fine; removing or renaming one breaks
        // every deployed node. This test catches both directions.
        let svc = build_mdns_service(&id(), 9180, 1883, true);
        let required = [
            "cog_id",
            "cog_version",
            "node_id",
            "mqtt_port",
            "privacy",
            "proto",
        ];
        for key in required {
            assert!(svc.txt(key).is_some(), "TXT key `{key}` missing");
        }
    }
}
