//! `BfldEvent` — privacy-gated output event. ADR-121 §2.1, ADR-122 §2.1.
//!
//! Field exposure per privacy_class (ADR-122 §2.1):
//!
//! | Field                  | Raw(0) | Derived(1) | Anonymous(2) | Restricted(3) |
//! |------------------------|--------|------------|--------------|---------------|
//! | presence               | y      | y          | y            | y             |
//! | motion                 | y      | y          | y            | y             |
//! | person_count           | y      | y          | y            | y             |
//! | confidence             | y      | y          | y            | y             |
//! | zone_id                | y      | y          | y            | y             |
//! | identity_risk_score    | y      | y          | **y**        | **n**         |
//! | rf_signature_hash      | y      | y          | **y**        | **n**         |
//!
//! Construction defers to [`BfldEvent::with_privacy_gating`] which applies
//! the policy by stripping disallowed fields to `None` based on the supplied
//! `privacy_class`. Direct field access remains possible (for unit tests),
//! but the JSON serializer always honors the gating because the dropped
//! fields are `None` and the `Serialize` derive uses `skip_serializing_if`.

#![cfg(feature = "std")]

use crate::PrivacyClass;

#[cfg(feature = "serde-json")]
use serde::Serialize;

/// Privacy-gated output event published by the BFLD pipeline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-json", derive(Serialize))]
pub struct BfldEvent {
    /// Always `"bfld_update"`. Tags the event type for downstream routers.
    #[cfg_attr(feature = "serde-json", serde(rename = "type"))]
    pub event_type: &'static str,

    /// Originating BFLD node identifier.
    pub node_id: String,

    /// Monotonic capture-clock timestamp in nanoseconds.
    pub timestamp_ns: u64,

    /// Whether an occupant is present in the sensing zone.
    pub presence: bool,

    /// Normalized motion magnitude in `[0.0, 1.0]`.
    pub motion: f32,

    /// Estimated number of occupants.
    pub person_count: u8,

    /// Sensing confidence in `[0.0, 1.0]`.
    pub confidence: f32,

    /// Optional zone identifier; absent if the deployment is single-zone.
    #[cfg_attr(feature = "serde-json", serde(skip_serializing_if = "Option::is_none"))]
    pub zone_id: Option<String>,

    /// Privacy classification byte for this event.
    #[cfg_attr(feature = "serde-json", serde(serialize_with = "ser_privacy_class"))]
    pub privacy_class: PrivacyClass,

    /// Identity-risk score, `[0.0, 1.0]`. Class 2 only; `None` at class 3.
    #[cfg_attr(feature = "serde-json", serde(skip_serializing_if = "Option::is_none"))]
    pub identity_risk_score: Option<f32>,

    /// 256-bit BLAKE3 keyed hash of the current cluster. Class 2 only; `None` at class 3.
    /// Serializes as the JSON string `"blake3:<64-hex>"` per the BFLD wire spec.
    #[cfg_attr(
        feature = "serde-json",
        serde(
            skip_serializing_if = "Option::is_none",
            serialize_with = "ser_rf_signature_hash"
        )
    )]
    pub rf_signature_hash: Option<[u8; 32]>,
}

impl BfldEvent {
    /// Build an event from sensing fields, applying the privacy_class policy
    /// to mask identity-derived fields. `identity_risk_score` and
    /// `rf_signature_hash` are nulled out at class `Restricted`.
    #[must_use]
    pub fn with_privacy_gating(
        node_id: String,
        timestamp_ns: u64,
        presence: bool,
        motion: f32,
        person_count: u8,
        confidence: f32,
        zone_id: Option<String>,
        privacy_class: PrivacyClass,
        identity_risk_score: Option<f32>,
        rf_signature_hash: Option<[u8; 32]>,
    ) -> Self {
        let mut e = Self {
            event_type: "bfld_update",
            node_id,
            timestamp_ns,
            presence,
            motion,
            person_count,
            confidence,
            zone_id,
            privacy_class,
            identity_risk_score,
            rf_signature_hash,
        };
        e.apply_privacy_gating();
        e
    }

    /// Idempotently mask fields disallowed at the current `privacy_class`.
    /// Called by [`Self::with_privacy_gating`]; exposed for callers that
    /// mutate the event in place before publication.
    pub fn apply_privacy_gating(&mut self) {
        if self.privacy_class.as_u8() >= PrivacyClass::Restricted.as_u8() {
            self.identity_risk_score = None;
            self.rf_signature_hash = None;
        }
    }

    /// Serialize to canonical JSON. Fields masked by privacy gating are omitted
    /// entirely (not emitted as `null`), so a privacy-gated event is
    /// observationally indistinguishable from one that never had the field set.
    #[cfg(feature = "serde-json")]
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[cfg(feature = "serde-json")]
fn ser_privacy_class<S: serde::Serializer>(class: &PrivacyClass, s: S) -> Result<S::Ok, S::Error> {
    let name = match class {
        PrivacyClass::Raw => "raw",
        PrivacyClass::Derived => "derived",
        PrivacyClass::Anonymous => "anonymous",
        PrivacyClass::Restricted => "restricted",
    };
    s.serialize_str(name)
}

/// Encode an `Option<[u8; 32]>` as the JSON string `"blake3:<64 lowercase hex chars>"`.
/// Used for `rf_signature_hash` so consumers don't have to decode a 32-element JSON
/// array of integers. Called only when the value is `Some(_)` because
/// `skip_serializing_if = "Option::is_none"` short-circuits the `None` case.
#[cfg(feature = "serde-json")]
fn ser_rf_signature_hash<S: serde::Serializer>(
    hash: &Option<[u8; 32]>,
    s: S,
) -> Result<S::Ok, S::Error> {
    // The unwrap is safe: skip_serializing_if guarantees we only run with Some.
    let bytes = hash
        .as_ref()
        .expect("ser_rf_signature_hash called with None");
    let mut out = String::with_capacity(7 + 64); // "blake3:" + 32*2 hex chars
    out.push_str("blake3:");
    for b in bytes {
        // Manual lowercase-hex push — avoids pulling in the `hex` crate for 32 bytes.
        out.push(nibble_to_hex(b >> 4));
        out.push(nibble_to_hex(b & 0x0F));
    }
    s.serialize_str(&out)
}

#[cfg(feature = "serde-json")]
const fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '?', // unreachable: input is masked with 0x0F
    }
}
