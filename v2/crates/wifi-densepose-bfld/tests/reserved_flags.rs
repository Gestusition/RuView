//! ADR-119 §2.1 reserved-flag-bits forward-compat. The 16-bit `flags` field
//! currently uses bits 0 (HAS_CSI_DELTA), 1 (PRIVACY_MODE), and 3 (SELF_ONLY).
//! Bits 2 and 4..=15 are reserved. The parser must preserve any reserved bit
//! set by a future peer — otherwise round-tripping a frame through a node
//! running an older crate version silently drops information that a newer
//! peer might depend on.

use wifi_densepose_bfld::frame::flags;
use wifi_densepose_bfld::{BfldFrameHeader, BFLD_HEADER_SIZE};

fn header_with_flags(flags_value: u16) -> BfldFrameHeader {
    let mut h = BfldFrameHeader::empty();
    h.flags = flags_value;
    h
}

#[test]
fn known_flags_mask_covers_exactly_three_named_flags() {
    assert_eq!(
        flags::KNOWN_FLAGS_MASK,
        flags::HAS_CSI_DELTA | flags::PRIVACY_MODE | flags::SELF_ONLY,
    );
    // The three currently-named flags occupy bits 0, 1, 3 — three bits set.
    assert_eq!(flags::KNOWN_FLAGS_MASK.count_ones(), 3);
}

#[test]
fn reserved_and_known_masks_are_complementary() {
    assert_eq!(
        flags::KNOWN_FLAGS_MASK | flags::RESERVED_FLAGS_MASK,
        u16::MAX
    );
    assert_eq!(flags::KNOWN_FLAGS_MASK & flags::RESERVED_FLAGS_MASK, 0);
}

#[test]
fn known_flags_do_not_overlap_with_each_other() {
    // Each named flag uses exactly one bit and no two of them share a bit.
    let pairs = [
        (flags::HAS_CSI_DELTA, flags::PRIVACY_MODE),
        (flags::HAS_CSI_DELTA, flags::SELF_ONLY),
        (flags::PRIVACY_MODE, flags::SELF_ONLY),
    ];
    for (a, b) in pairs {
        assert_eq!(a & b, 0, "named flag overlap: 0x{a:04X} & 0x{b:04X}");
    }
}

#[test]
fn header_preserves_reserved_flag_bits_through_round_trip() {
    // Light bit 2 + bits 4..=15 — the full reserved space.
    let reserved_set = flags::RESERVED_FLAGS_MASK;
    let h = header_with_flags(reserved_set);
    let bytes = h.to_le_bytes();
    let parsed = BfldFrameHeader::from_le_bytes(&bytes).expect("parse");
    assert_eq!(
        { parsed.flags },
        reserved_set,
        "reserved bits must round-trip unchanged for forward-compat",
    );
    assert_eq!(bytes.len(), BFLD_HEADER_SIZE);
}

#[test]
fn header_preserves_mixed_known_and_reserved_bits() {
    let mixed = flags::HAS_CSI_DELTA | flags::PRIVACY_MODE | (1 << 7) | (1 << 14);
    let h = header_with_flags(mixed);
    let parsed = BfldFrameHeader::from_le_bytes(&h.to_le_bytes()).expect("parse");
    assert_eq!({ parsed.flags }, mixed);
    // Known flags still readable via the named constants.
    assert_ne!(({ parsed.flags }) & flags::HAS_CSI_DELTA, 0);
    assert_ne!(({ parsed.flags }) & flags::PRIVACY_MODE, 0);
}

#[test]
fn reserved_bits_do_not_collide_with_self_only_bit_3() {
    // SELF_ONLY uses bit 3 — bit 2 is the only unused bit in the 0..=3 range
    // and IS part of the reserved mask.
    assert_ne!(
        flags::SELF_ONLY & flags::RESERVED_FLAGS_MASK,
        flags::SELF_ONLY
    );
    assert_eq!(flags::RESERVED_FLAGS_MASK & (1 << 2), 1 << 2);
    assert_eq!(flags::RESERVED_FLAGS_MASK & (1 << 3), 0);
}

#[test]
fn all_zero_flags_round_trip_cleanly() {
    let h = header_with_flags(0);
    let parsed = BfldFrameHeader::from_le_bytes(&h.to_le_bytes()).expect("parse");
    assert_eq!({ parsed.flags }, 0);
}

#[test]
fn all_one_flags_round_trip_cleanly() {
    // Stress: every bit set. The parser has no business interpreting this
    // configuration but must preserve it.
    let h = header_with_flags(u16::MAX);
    let parsed = BfldFrameHeader::from_le_bytes(&h.to_le_bytes()).expect("parse");
    assert_eq!({ parsed.flags }, u16::MAX);
}
