//! `BfldError` Display format pinning. Operators grep log lines for these
//! strings; format drift between minor versions breaks monitoring queries.
//! Each variant gets a test that asserts the documented substrings appear.

#![cfg(feature = "std")]

use wifi_densepose_bfld::BfldError;

#[test]
fn invalid_magic_displays_both_expected_and_actual_in_hex() {
    let err = BfldError::InvalidMagic(0xDEAD_BEEF);
    let s = err.to_string();
    assert!(s.contains("invalid BFLD magic"), "got: {s}");
    assert!(s.contains("0xBF1D0001"), "expected magic missing: {s}");
    assert!(s.contains("0xDEADBEEF"), "actual magic missing: {s}");
}

#[test]
fn unsupported_version_displays_the_offending_version() {
    let err = BfldError::UnsupportedVersion(99);
    let s = err.to_string();
    assert!(s.contains("unsupported BFLD version"), "got: {s}");
    assert!(s.contains("99"), "version number missing: {s}");
}

#[test]
fn crc_mismatch_displays_both_values_in_hex() {
    let err = BfldError::Crc {
        expected: 0xCAFEBABE,
        actual: 0xDEADBEEF,
    };
    let s = err.to_string();
    assert!(s.contains("payload CRC mismatch"), "got: {s}");
    assert!(s.contains("0xCAFEBABE"), "expected missing: {s}");
    assert!(s.contains("0xDEADBEEF"), "actual missing: {s}");
}

#[test]
fn privacy_violation_displays_the_sink_reason() {
    let err = BfldError::PrivacyViolation {
        reason: "NetworkKind",
    };
    let s = err.to_string();
    assert!(s.contains("privacy violation"), "got: {s}");
    assert!(s.contains("NetworkKind"), "reason missing: {s}");
}

#[test]
fn invalid_privacy_class_displays_the_offending_byte() {
    let err = BfldError::InvalidPrivacyClass(7);
    let s = err.to_string();
    assert!(s.contains("invalid PrivacyClass byte"), "got: {s}");
    assert!(s.contains("7"), "byte value missing: {s}");
}

#[test]
fn truncated_frame_displays_got_and_need_byte_counts() {
    let err = BfldError::TruncatedFrame { got: 50, need: 86 };
    let s = err.to_string();
    assert!(s.contains("truncated frame"), "got: {s}");
    assert!(s.contains("50"), "got count missing: {s}");
    assert!(s.contains("86"), "need count missing: {s}");
}

#[test]
fn malformed_section_displays_offset_and_reason() {
    let err = BfldError::MalformedSection {
        offset: 1234,
        reason: "section body runs past buffer end",
    };
    let s = err.to_string();
    assert!(s.contains("malformed payload section"), "got: {s}");
    assert!(s.contains("1234"), "offset missing: {s}");
    assert!(s.contains("buffer end"), "reason missing: {s}");
}

#[test]
fn invalid_demote_displays_both_from_and_to_class_bytes() {
    let err = BfldError::InvalidDemote { from: 2, to: 1 };
    let s = err.to_string();
    assert!(s.contains("invalid demote"), "got: {s}");
    assert!(s.contains("from class 2"), "from missing: {s}");
    assert!(s.contains("to class 1"), "to missing: {s}");
}

// --- meta: error implements std::error::Error (for ? + dyn use) -------

#[test]
fn bfld_error_implements_std_error_trait() {
    fn assert_error_trait<E: std::error::Error>() {}
    assert_error_trait::<BfldError>();
}

#[test]
fn bfld_error_is_debug_so_panic_unwrap_messages_carry_diagnostics() {
    let err = BfldError::Crc {
        expected: 0xAA,
        actual: 0xBB,
    };
    let debug = format!("{err:?}");
    assert!(
        debug.contains("Crc"),
        "Debug must show variant name: {debug}"
    );
}

// --- catch-all: every variant has a non-empty Display -----------------

#[test]
fn every_variant_has_a_non_empty_display_string() {
    let cases: Vec<BfldError> = vec![
        BfldError::InvalidMagic(0),
        BfldError::UnsupportedVersion(0),
        BfldError::Crc {
            expected: 0,
            actual: 0,
        },
        BfldError::PrivacyViolation { reason: "X" },
        BfldError::InvalidPrivacyClass(0),
        BfldError::TruncatedFrame { got: 0, need: 0 },
        BfldError::MalformedSection {
            offset: 0,
            reason: "X",
        },
        BfldError::InvalidDemote { from: 0, to: 0 },
    ];
    for err in cases {
        let s = err.to_string();
        assert!(!s.is_empty(), "Display for {err:?} returned empty string");
        assert!(
            s.len() >= 5,
            "Display for {err:?} suspiciously short: {s:?}",
        );
    }
}
