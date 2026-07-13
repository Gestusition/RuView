//! `PrivacyGate` — monotonic class transitions for `BfldFrame`. ADR-120 §2.4.
//!
//! The only way a higher-information frame becomes a lower-information frame
//! is through [`PrivacyGate::demote`]. This function:
//!
//! 1. Asserts the target class is **strictly higher in numerical value** (or
//!    equal) to the current class — going from Derived(1) to Anonymous(2) is
//!    a demote; going from Anonymous(2) back to Derived(1) is forbidden.
//! 2. Zeroes payload sections that are not permitted at the target class,
//!    using a `black_box`-guarded loop to defeat dead-store elimination.
//! 3. Re-syncs `header.privacy_class` and `header.payload_crc32`.
//! 4. Returns the new frame.
//!
//! There is no `promote` operation by design — once a section is zeroed, the
//! original bytes are unrecoverable.

#![cfg(feature = "std")]

use crate::frame::crc32_of_payload;
use crate::{BfldError, BfldFrame, BfldPayload, PrivacyClass};

/// Monotonic class transformer. See module docs.
pub struct PrivacyGate;

impl PrivacyGate {
    /// Apply a class demotion in-place: returns a new `BfldFrame` whose
    /// `privacy_class`, payload sections, and CRC match `target`.
    ///
    /// Returns [`BfldError::InvalidDemote`] when `target` would *increase*
    /// the information density (lower class number than the source).
    pub fn demote(mut frame: BfldFrame, target: PrivacyClass) -> Result<BfldFrame, BfldError> {
        let current = PrivacyClass::try_from(frame.header.privacy_class)?;
        if target.as_u8() < current.as_u8() {
            return Err(BfldError::InvalidDemote {
                from: current.as_u8(),
                to: target.as_u8(),
            });
        }

        // Strip payload sections not permitted at the target class. We only do
        // this when the payload parses cleanly; a malformed payload remains
        // untouched in the bytes (the class byte and CRC still get re-synced).
        if let Ok(mut payload) = frame.parse_payload() {
            if target.as_u8() >= PrivacyClass::Anonymous.as_u8() {
                // Anonymous: drop the compressed angle matrix (identity surface).
                zeroize_then_clear(&mut payload.compressed_angle_matrix);
                // Also drop optional sections that may carry identity-leaky
                // signal under high-separability conditions.
                if let Some(csi) = payload.csi_delta.as_mut() {
                    zeroize_then_clear(csi);
                }
            }
            if target.as_u8() >= PrivacyClass::Restricted.as_u8() {
                // Restricted: also drop amplitude + phase proxies.
                zeroize_then_clear(&mut payload.amplitude_proxy);
                zeroize_then_clear(&mut payload.phase_proxy);
            }
            // Note: csi_delta dropped above implies the flag bit should clear.
            // from_payload re-derives the flag from csi_delta.is_some(), so
            // taking the Option out below ensures the bit is cleared.
            if target.as_u8() >= PrivacyClass::Anonymous.as_u8() {
                payload.csi_delta = None;
            }
            frame = BfldFrame::from_payload(frame.header, &payload);
        }

        frame.header.privacy_class = target.as_u8();
        // from_payload already recomputed CRC, but recompute again so the
        // path that skipped payload parsing still produces a consistent frame.
        frame.header.payload_crc32 = crc32_of_payload(&frame.payload);
        Ok(frame)
    }
}

/// Overwrite `v` with zeros, then truncate. The `black_box` call defeats
/// dead-store elimination so the writes are observable.
fn zeroize_then_clear(v: &mut Vec<u8>) {
    for b in v.iter_mut() {
        *b = 0;
    }
    core::hint::black_box(v.as_ptr());
    v.clear();
}

// Convenience constructor: the gate is a unit type, but keeping a Default
// makes downstream injection sites (PrivacyGate.demote(...) vs static call)
// straightforward.
impl Default for PrivacyGate {
    fn default() -> Self {
        Self
    }
}

/// Discard the rest of an unused (#[allow(dead_code)]) — placeholder so
/// `BfldPayload` import isn't unused in builds that strip the implementation.
#[allow(dead_code)]
fn _unused_payload_marker(_: BfldPayload) {}
