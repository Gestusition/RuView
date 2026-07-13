//! Acceptance tests for ADR-120 §2.4 — `PrivacyGate::demote` monotonic class
//! transitions and payload-section zeroization.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    BfldError, BfldFrame, BfldFrameHeader, BfldPayload, PrivacyClass, PrivacyGate,
};

fn frame_at_class(class: PrivacyClass, with_csi: bool) -> BfldFrame {
    let payload = BfldPayload {
        compressed_angle_matrix: vec![0x11; 32],
        amplitude_proxy: vec![0x22; 16],
        phase_proxy: vec![0x33; 16],
        snr_vector: vec![0x44; 8],
        csi_delta: if with_csi { Some(vec![0x55; 24]) } else { None },
        vendor_extension: vec![0xAA],
    };
    let mut header = BfldFrameHeader::empty();
    header.privacy_class = class.as_u8();
    BfldFrame::from_payload(header, &payload)
}

#[test]
fn demote_to_same_class_is_identity() {
    let f = frame_at_class(PrivacyClass::Derived, false);
    let out = PrivacyGate::demote(f, PrivacyClass::Derived).expect("same-class demote OK");
    assert_eq!({ out.header.privacy_class }, PrivacyClass::Derived.as_u8());
}

#[test]
fn demote_derived_to_anonymous_strips_compressed_angle_matrix() {
    let f = frame_at_class(PrivacyClass::Derived, true);
    let out = PrivacyGate::demote(f, PrivacyClass::Anonymous).expect("demote");
    assert_eq!(
        { out.header.privacy_class },
        PrivacyClass::Anonymous.as_u8()
    );

    let payload = out.parse_payload().expect("payload still parses");
    assert!(
        payload.compressed_angle_matrix.is_empty(),
        "angle matrix must be stripped at class 2",
    );
    // CSI delta also dropped at Anonymous.
    assert!(payload.csi_delta.is_none(), "csi_delta dropped at class 2");
    // Sensing sections preserved.
    assert_eq!(payload.snr_vector.len(), 8);
    assert_eq!(payload.amplitude_proxy.len(), 16);
}

#[test]
fn demote_derived_to_restricted_strips_amplitude_and_phase_too() {
    let f = frame_at_class(PrivacyClass::Derived, true);
    let out = PrivacyGate::demote(f, PrivacyClass::Restricted).expect("demote");
    assert_eq!(
        { out.header.privacy_class },
        PrivacyClass::Restricted.as_u8()
    );

    let payload = out.parse_payload().expect("payload parses");
    assert!(payload.compressed_angle_matrix.is_empty());
    assert!(
        payload.amplitude_proxy.is_empty(),
        "amplitude stripped at class 3"
    );
    assert!(payload.phase_proxy.is_empty(), "phase stripped at class 3");
    // SNR + vendor still survive.
    assert_eq!(payload.snr_vector.len(), 8);
    assert_eq!(payload.vendor_extension.len(), 1);
}

#[test]
fn demote_anonymous_to_derived_is_rejected() {
    let f = frame_at_class(PrivacyClass::Anonymous, false);
    match PrivacyGate::demote(f, PrivacyClass::Derived) {
        Err(BfldError::InvalidDemote { from, to }) => {
            assert_eq!(from, PrivacyClass::Anonymous.as_u8());
            assert_eq!(to, PrivacyClass::Derived.as_u8());
        }
        other => panic!("expected InvalidDemote, got {other:?}"),
    }
}

#[test]
fn demote_to_raw_is_rejected_from_any_higher_class() {
    for src in [
        PrivacyClass::Derived,
        PrivacyClass::Anonymous,
        PrivacyClass::Restricted,
    ] {
        let f = frame_at_class(src, false);
        match PrivacyGate::demote(f, PrivacyClass::Raw) {
            Err(BfldError::InvalidDemote { .. }) => {}
            other => panic!("expected InvalidDemote from {src:?}, got {other:?}"),
        }
    }
}

#[test]
fn demote_preserves_frame_crc_consistency_through_wire_roundtrip() {
    // Demote produces a frame; that frame must round-trip through bytes
    // with no CRC error.
    let f = frame_at_class(PrivacyClass::Derived, true);
    let demoted = PrivacyGate::demote(f, PrivacyClass::Anonymous).expect("demote");
    let bytes = demoted.to_bytes();
    let parsed = BfldFrame::from_bytes(&bytes).expect("post-demote frame must round-trip");
    assert_eq!(
        { parsed.header.privacy_class },
        PrivacyClass::Anonymous.as_u8()
    );
}

#[test]
fn demote_clears_has_csi_delta_flag_bit() {
    use wifi_densepose_bfld::frame::flags;
    let f = frame_at_class(PrivacyClass::Derived, true);
    assert_ne!({ f.header.flags } & flags::HAS_CSI_DELTA, 0);

    let out = PrivacyGate::demote(f, PrivacyClass::Anonymous).expect("demote");
    assert_eq!(
        { out.header.flags } & flags::HAS_CSI_DELTA,
        0,
        "HAS_CSI_DELTA must clear when csi_delta is stripped",
    );
}
