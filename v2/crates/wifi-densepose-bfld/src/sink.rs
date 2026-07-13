//! Sink marker traits — structural enforcement of invariant I1.
//!
//! Every output destination (memory buffer, MQTT topic, Matter cluster) implements
//! exactly one of [`LocalSink`], [`NetworkSink`], or [`MatterSink`]. The associated
//! constant [`Sink::MIN_CLASS`] declares the lowest `PrivacyClass` value that sink
//! is willing to accept; the runtime gate [`check_class`] enforces this on every
//! publish.
//!
//! Mapping (ADR-120 §2.2, ADR-122 §2.4):
//!
//! | Sink trait    | `MIN_CLASS`          | Accepts classes |
//! |---------------|----------------------|-----------------|
//! | `LocalSink`   | `PrivacyClass::Raw`  | 0, 1, 2, 3      |
//! | `NetworkSink` | `PrivacyClass::Derived` | 1, 2, 3       |
//! | `MatterSink`  | `PrivacyClass::Anonymous` | 2, 3        |
//!
//! `MatterSink: NetworkSink` — every Matter sink is also a network sink.

use crate::{BfldError, PrivacyClass};

/// Base sink trait. Every sink type declares the minimum `PrivacyClass` it accepts.
pub trait Sink {
    /// Lowest privacy class (highest information density) this sink will publish.
    const MIN_CLASS: PrivacyClass;
    /// Human-readable sink kind, used in `BfldError::PrivacyViolation` messages.
    const KIND: &'static str;
}

/// Marker for sinks that stay on the originating node (memory, in-RAM channel,
/// local file with explicit operator opt-in). Accepts every class including `Raw`.
pub trait LocalSink: Sink {}

/// Marker for sinks that cross the node boundary (MQTT, HTTP, gRPC). Rejects
/// `Raw` frames by structural invariant I1.
pub trait NetworkSink: Sink {}

/// Marker for sinks that bridge into the Matter cluster surface. Rejects `Raw`
/// and `Derived`; the `cog-ha-matter` boundary filter consumes only classes 2/3.
pub trait MatterSink: NetworkSink {}

/// Runtime gate. Returns `Ok(())` if `class` is acceptable for `S`, otherwise
/// returns `BfldError::PrivacyViolation` with the offending sink kind.
///
/// Class numerical order *is* meaningful here: a sink that accepts `MIN_CLASS`
/// also accepts every higher-numbered class (less identity content). The check
/// is therefore a simple `>=` on the byte representation.
pub fn check_class<S: Sink>(class: PrivacyClass) -> Result<(), BfldError> {
    if class.as_u8() >= S::MIN_CLASS.as_u8() {
        Ok(())
    } else {
        Err(BfldError::PrivacyViolation { reason: S::KIND })
    }
}

// --- Default sink types ----------------------------------------------------
//
// Concrete sinks live in downstream crates (emitter.rs, mqtt.rs, the cog-ha-matter
// Matter bridge). These three "kind tags" are convenient zero-sized stand-ins for
// unit tests and for the privacy_gate compile-time tables.

/// Zero-sized tag: a local in-memory ring buffer or file sink.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalKind;

impl Sink for LocalKind {
    const MIN_CLASS: PrivacyClass = PrivacyClass::Raw;
    const KIND: &'static str = "LocalKind";
}
impl LocalSink for LocalKind {}

/// Zero-sized tag: a generic network sink (MQTT, HTTP, gRPC).
#[derive(Debug, Clone, Copy, Default)]
pub struct NetworkKind;

impl Sink for NetworkKind {
    const MIN_CLASS: PrivacyClass = PrivacyClass::Derived;
    const KIND: &'static str = "NetworkKind";
}
impl NetworkSink for NetworkKind {}

/// Zero-sized tag: the Matter cluster boundary in `cog-ha-matter`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MatterKind;

impl Sink for MatterKind {
    const MIN_CLASS: PrivacyClass = PrivacyClass::Anonymous;
    const KIND: &'static str = "MatterKind";
}
impl NetworkSink for MatterKind {}
impl MatterSink for MatterKind {}
