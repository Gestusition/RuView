//! Validate the BFLD entry exists in the workspace-root CHANGELOG.md.
//! `cog-ha-matter`, `wifi-densepose-sensing-server`, and the pip wheel
//! ship under their own release cadence; the workspace CHANGELOG is the
//! one canonical record an operator scans when upgrading a local gateway.

#![cfg(feature = "std")]

const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

#[test]
fn changelog_documents_bfld_entry_under_unreleased() {
    // Find the position of the [Unreleased] header.
    let unreleased = CHANGELOG
        .find("## [Unreleased]")
        .expect("CHANGELOG must have an [Unreleased] section");
    // The first numbered version header marks the end of [Unreleased].
    let after_unreleased = CHANGELOG[unreleased..]
        .find("\n## [0")
        .or_else(|| CHANGELOG[unreleased..].find("\n## [1"))
        .map(|off| unreleased + off)
        .unwrap_or(CHANGELOG.len());
    let unreleased_block = &CHANGELOG[unreleased..after_unreleased];
    assert!(
        unreleased_block.contains("BFLD"),
        "[Unreleased] must mention BFLD",
    );
    assert!(unreleased_block.contains("wifi-densepose-bfld"));
    assert!(
        unreleased_block.contains("#787"),
        "[Unreleased] BFLD entry must link tracking issue #787",
    );
}

#[test]
fn changelog_bfld_entry_cites_companion_adrs() {
    for adr in [
        "ADR-118", "ADR-119", "ADR-120", "ADR-121", "ADR-122", "ADR-123",
    ] {
        assert!(
            CHANGELOG.contains(adr),
            "CHANGELOG BFLD entry must cite {adr}",
        );
    }
}

#[test]
fn changelog_bfld_entry_names_three_structural_invariants() {
    let needles = ["**I1**", "**I2**", "**I3**"];
    for n in needles {
        assert!(
            CHANGELOG.contains(n),
            "CHANGELOG must call out invariant {n}"
        );
    }
}

#[test]
fn changelog_bfld_entry_documents_a_runnable_example() {
    assert!(
        CHANGELOG.contains("cargo run -p wifi-densepose-bfld --example"),
        "CHANGELOG entry should give operators a copy-pasteable try-it command",
    );
}

#[test]
fn changelog_bfld_entry_references_research_bundle() {
    assert!(CHANGELOG.contains("docs/research/BFLD/"));
}
