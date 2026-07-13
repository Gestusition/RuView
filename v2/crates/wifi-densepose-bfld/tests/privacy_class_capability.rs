//! `PrivacyClass::allows_network` and `allows_matter` const-helper truth
//! tables, plus a cross-consistency check against the `Sink` trait constants.
//! Iter 1 introduced these helpers; iter 3 introduced the `Sink::MIN_CLASS`
//! mechanism. The two APIs must agree.
//!
//! Why both APIs: `allows_network` / `allows_matter` are point-in-time
//! Boolean queries for ergonomics ("can I publish this frame?"); the `Sink`
//! marker-trait + `MIN_CLASS` const provides the structural enforcement at
//! compile-time. Drift between them is a silent correctness bug — this iter
//! pins the constraint that they always agree.

use wifi_densepose_bfld::sink::{LocalKind, MatterKind, NetworkKind, Sink};
use wifi_densepose_bfld::PrivacyClass;

const ALL_CLASSES: [PrivacyClass; 4] = [
    PrivacyClass::Raw,
    PrivacyClass::Derived,
    PrivacyClass::Anonymous,
    PrivacyClass::Restricted,
];

// --- direct truth tables ------------------------------------------------

#[test]
fn allows_network_truth_table() {
    assert!(!PrivacyClass::Raw.allows_network());
    assert!(PrivacyClass::Derived.allows_network());
    assert!(PrivacyClass::Anonymous.allows_network());
    assert!(PrivacyClass::Restricted.allows_network());
}

#[test]
fn allows_matter_truth_table() {
    assert!(!PrivacyClass::Raw.allows_matter());
    assert!(!PrivacyClass::Derived.allows_matter());
    assert!(PrivacyClass::Anonymous.allows_matter());
    assert!(PrivacyClass::Restricted.allows_matter());
}

// --- monotonicity property ---------------------------------------------

#[test]
fn allows_matter_implies_allows_network() {
    // Matter is a subset of Network — if a class is Matter-eligible, it
    // must also be Network-eligible. The reverse is not true (Derived is
    // Network-eligible but not Matter-eligible).
    for c in ALL_CLASSES {
        if c.allows_matter() {
            assert!(
                c.allows_network(),
                "{c:?}: allows_matter without allows_network is a contract violation",
            );
        }
    }
}

#[test]
fn allows_network_strictly_excludes_raw() {
    // Class 0 (Raw) is the only class that fails allows_network. Any future
    // refactor that lets Raw cross a NetworkSink violates ADR-118 invariant I1.
    for c in ALL_CLASSES {
        let expected = !matches!(c, PrivacyClass::Raw);
        assert_eq!(c.allows_network(), expected, "{c:?}: allows_network drift",);
    }
}

#[test]
fn allows_matter_strictly_requires_class_two_or_three() {
    for c in ALL_CLASSES {
        let expected = matches!(c, PrivacyClass::Anonymous | PrivacyClass::Restricted);
        assert_eq!(c.allows_matter(), expected, "{c:?}: allows_matter drift");
    }
}

// --- cross-consistency with Sink::MIN_CLASS ----------------------------

/// For a sink with `MIN_CLASS = K`, a class `C` should be accepted iff
/// `C.as_u8() >= K.as_u8()`. Iter 3 implemented exactly this in `check_class`.
/// The helpers above must agree.
fn check_consistency<S: Sink>(class: PrivacyClass, helper_says_allowed: bool) {
    let sink_min = S::MIN_CLASS.as_u8();
    let class_byte = class.as_u8();
    let sink_says_allowed = class_byte >= sink_min;
    assert_eq!(
        helper_says_allowed,
        sink_says_allowed,
        "{class:?} vs {} ({} >= {} should be {}, helper said {})",
        S::KIND,
        class_byte,
        sink_min,
        sink_says_allowed,
        helper_says_allowed,
    );
}

#[test]
fn local_sink_accepts_every_class_per_helper() {
    for c in ALL_CLASSES {
        // LocalSink has MIN_CLASS = Raw (byte 0) — accepts all.
        check_consistency::<LocalKind>(c, true);
    }
}

#[test]
fn network_sink_consistency_matches_allows_network() {
    for c in ALL_CLASSES {
        check_consistency::<NetworkKind>(c, c.allows_network());
    }
}

#[test]
fn matter_sink_consistency_matches_allows_matter() {
    for c in ALL_CLASSES {
        check_consistency::<MatterKind>(c, c.allows_matter());
    }
}

// --- byte-value pinning -----------------------------------------------

#[test]
fn as_u8_returns_documented_byte_values() {
    assert_eq!(PrivacyClass::Raw.as_u8(), 0);
    assert_eq!(PrivacyClass::Derived.as_u8(), 1);
    assert_eq!(PrivacyClass::Anonymous.as_u8(), 2);
    assert_eq!(PrivacyClass::Restricted.as_u8(), 3);
}

#[test]
fn class_byte_ordering_matches_information_density() {
    // Higher numerical class = less information density. Sanity check.
    let raw = PrivacyClass::Raw.as_u8();
    let derived = PrivacyClass::Derived.as_u8();
    let anonymous = PrivacyClass::Anonymous.as_u8();
    let restricted = PrivacyClass::Restricted.as_u8();
    assert!(raw < derived);
    assert!(derived < anonymous);
    assert!(anonymous < restricted);
}
