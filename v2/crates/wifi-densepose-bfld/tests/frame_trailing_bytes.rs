//! `BfldFrame::from_bytes` trailing-bytes contract. Pins the current
//! behavior: the parser reads exactly `header.payload_len` bytes after the
//! header and silently ignores anything past `BFLD_HEADER_SIZE +
//! header.payload_len`. This matches how the parser is used in iter-4
//! through iter-15: callers hand a sliced buffer that may include framing
//! noise (UDP MTU padding, ESP-NOW trailer alignment), and the parser
//! extracts only what the header declares.
//!
//! If a future iter decides to tighten this (reject trailing bytes as
//! `MalformedFrame`), updating this test makes the policy change deliberate
//! and traceable rather than silent.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{BfldFrame, BfldFrameHeader, BfldPayload, BFLD_HEADER_SIZE};

fn frame_with_typed_payload() -> BfldFrame {
    let payload = BfldPayload {
        compressed_angle_matrix: vec![0x11; 32],
        amplitude_proxy: vec![0x22; 16],
        phase_proxy: vec![0x33; 16],
        snr_vector: vec![0x44; 8],
        csi_delta: None,
        vendor_extension: vec![],
    };
    BfldFrame::from_payload(BfldFrameHeader::empty(), &payload)
}

#[test]
fn parser_accepts_buffer_with_one_trailing_byte() {
    let frame = frame_with_typed_payload();
    let mut bytes = frame.to_bytes();
    let canonical_len = bytes.len();
    bytes.push(0xFF);
    let parsed = BfldFrame::from_bytes(&bytes).expect("trailing byte must be tolerated");
    assert_eq!(
        parsed.payload.len(),
        { parsed.header.payload_len } as usize,
        "parsed payload size must equal header.payload_len, not buffer.len() - HEADER",
    );
    // Implicit: the trailing 0xFF byte is NOT in parsed.payload.
    assert_ne!(parsed.payload.last().copied(), Some(0xFF));
    let _ = canonical_len; // sanity anchor
}

#[test]
fn parser_accepts_many_trailing_bytes() {
    let frame = frame_with_typed_payload();
    let mut bytes = frame.to_bytes();
    bytes.extend_from_slice(&[0xCC; 256]);
    let parsed = BfldFrame::from_bytes(&bytes).expect("256 trailing bytes must be tolerated");
    assert_eq!(parsed.payload.len(), { parsed.header.payload_len } as usize);
}

#[test]
fn parsed_payload_round_trips_back_to_typed_payload_with_trailing_bytes_present() {
    // The trailing-bytes parser leniency must not corrupt the section parser
    // downstream. After from_bytes + parse_payload, the typed payload should
    // match the original BfldPayload byte-for-byte.
    let original_payload = BfldPayload {
        compressed_angle_matrix: vec![0x11; 32],
        amplitude_proxy: vec![0x22; 16],
        phase_proxy: vec![0x33; 16],
        snr_vector: vec![0x44; 8],
        csi_delta: None,
        vendor_extension: vec![],
    };
    let frame = BfldFrame::from_payload(BfldFrameHeader::empty(), &original_payload);
    let mut bytes = frame.to_bytes();
    bytes.extend_from_slice(&[0xEE; 64]);
    let parsed_frame = BfldFrame::from_bytes(&bytes).unwrap();
    let parsed_payload = parsed_frame.parse_payload().expect("typed payload parse");
    assert_eq!(parsed_payload, original_payload);
}

#[test]
fn header_only_buffer_at_exactly_header_size_with_zero_payload_len_succeeds() {
    let header = BfldFrameHeader::empty();
    let frame = BfldFrame::new(header, Vec::new());
    let bytes = frame.to_bytes();
    assert_eq!(
        bytes.len(),
        BFLD_HEADER_SIZE,
        "empty-payload frame is exactly header size"
    );
    let parsed = BfldFrame::from_bytes(&bytes).expect("parse");
    assert!(parsed.payload.is_empty());
}

#[test]
fn header_only_buffer_with_trailing_bytes_but_zero_payload_len_ignores_them() {
    let header = BfldFrameHeader::empty();
    let frame = BfldFrame::new(header, Vec::new());
    let mut bytes = frame.to_bytes();
    bytes.extend_from_slice(&[0xAA; 100]);
    let parsed = BfldFrame::from_bytes(&bytes).expect("parse");
    assert_eq!({ parsed.header.payload_len }, 0);
    assert!(
        parsed.payload.is_empty(),
        "trailing bytes must not leak into payload"
    );
}

#[test]
fn trailing_bytes_do_not_affect_crc_validation_when_payload_intact() {
    let frame = frame_with_typed_payload();
    let mut bytes = frame.to_bytes();
    let crc_before_extension = { frame.header.payload_crc32 };
    bytes.extend_from_slice(&[0xFF; 32]);
    let parsed = BfldFrame::from_bytes(&bytes).expect("CRC over payload-only must still match");
    assert_eq!({ parsed.header.payload_crc32 }, crc_before_extension);
}
