//! Acceptance tests for `BfldFrameHeader` serialization (ADR-119 AC5/AC6).

use wifi_densepose_bfld::frame::flags;
use wifi_densepose_bfld::{BfldError, BfldFrameHeader, BFLD_HEADER_SIZE, BFLD_MAGIC};

fn sample_header() -> BfldFrameHeader {
    let mut h = BfldFrameHeader::empty();
    h.flags = flags::HAS_CSI_DELTA | flags::PRIVACY_MODE;
    h.timestamp_ns = 0x0123_4567_89AB_CDEF;
    h.ap_hash = [0xAA; 16];
    h.sta_hash = [0xBB; 16];
    h.session_id = [0xCC; 16];
    h.channel = 36;
    h.bandwidth_mhz = 80;
    h.rssi_dbm = -55;
    h.noise_floor_dbm = -95;
    h.n_subcarriers = 234;
    h.n_tx = 3;
    h.n_rx = 4;
    h.quantization = 1;
    h.privacy_class = 2;
    h.payload_len = 12_345;
    h.payload_crc32 = 0xDEAD_BEEF;
    h
}

#[test]
fn header_roundtrip_preserves_all_fields() {
    let original = sample_header();
    let bytes = original.to_le_bytes();
    let parsed = BfldFrameHeader::from_le_bytes(&bytes).expect("parse must succeed");

    assert_eq!({ parsed.magic }, BFLD_MAGIC);
    assert_eq!({ parsed.version }, 1);
    assert_eq!({ parsed.flags }, flags::HAS_CSI_DELTA | flags::PRIVACY_MODE);
    assert_eq!({ parsed.timestamp_ns }, 0x0123_4567_89AB_CDEF);
    assert_eq!(parsed.ap_hash, [0xAA; 16]);
    assert_eq!(parsed.sta_hash, [0xBB; 16]);
    assert_eq!(parsed.session_id, [0xCC; 16]);
    assert_eq!({ parsed.channel }, 36);
    assert_eq!({ parsed.bandwidth_mhz }, 80);
    assert_eq!({ parsed.rssi_dbm }, -55);
    assert_eq!({ parsed.noise_floor_dbm }, -95);
    assert_eq!({ parsed.n_subcarriers }, 234);
    assert_eq!(parsed.n_tx, 3);
    assert_eq!(parsed.n_rx, 4);
    assert_eq!(parsed.quantization, 1);
    assert_eq!(parsed.privacy_class, 2);
    assert_eq!({ parsed.payload_len }, 12_345);
    assert_eq!({ parsed.payload_crc32 }, 0xDEAD_BEEF);
}

#[test]
fn header_serialization_is_deterministic() {
    let h = sample_header();
    let a = h.to_le_bytes();
    let b = h.to_le_bytes();
    assert_eq!(
        a, b,
        "two serializations of the same header must be bit-identical"
    );
}

#[test]
fn header_magic_is_at_offset_zero_little_endian() {
    let bytes = sample_header().to_le_bytes();
    // BFLD_MAGIC = 0xBF1D_0001 → little-endian: 01 00 1D BF
    assert_eq!(&bytes[0..4], &[0x01, 0x00, 0x1D, 0xBF]);
}

#[test]
fn parsing_rejects_invalid_magic() {
    let mut bytes = sample_header().to_le_bytes();
    bytes[0] = 0xFF; // clobber magic
    match BfldFrameHeader::from_le_bytes(&bytes) {
        Err(BfldError::InvalidMagic(got)) => {
            assert_ne!(got, BFLD_MAGIC);
        }
        other => panic!("expected InvalidMagic, got {other:?}"),
    }
}

#[test]
fn parsing_rejects_unsupported_version() {
    let mut bytes = sample_header().to_le_bytes();
    bytes[4] = 99; // version field at offset 4 (LE u16)
    bytes[5] = 0;
    match BfldFrameHeader::from_le_bytes(&bytes) {
        Err(BfldError::UnsupportedVersion(v)) => assert_eq!(v, 99),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn wire_size_is_constant() {
    assert_eq!(sample_header().to_le_bytes().len(), BFLD_HEADER_SIZE);
}
