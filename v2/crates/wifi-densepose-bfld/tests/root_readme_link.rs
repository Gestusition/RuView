//! Validate the workspace-root `README.md` Documentation table cites the
//! BFLD crate. crates.io won't show this, but new contributors browsing
//! `ruvnet/RuView` on GitHub will — the entry is the primary discovery
//! path for operators looking for "WiFi sensing privacy layer".

#![cfg(feature = "std")]

const ROOT_README: &str = include_str!("../../../../README.md");

#[test]
fn root_readme_links_to_bfld_crate_readme() {
    assert!(
        ROOT_README.contains("v2/crates/wifi-densepose-bfld/README.md"),
        "root README must link to the BFLD crate README from the Documentation table",
    );
}

#[test]
fn root_readme_mentions_bfld_acronym_and_full_name() {
    assert!(
        ROOT_README.contains("BFLD"),
        "root README must mention the BFLD acronym",
    );
    assert!(
        ROOT_README.contains("Beamforming Feedback Layer for Detection"),
        "root README must expand the BFLD acronym at least once",
    );
}

#[test]
fn root_readme_cites_all_six_bfld_adrs() {
    for adr in [
        "ADR-118", "ADR-119", "ADR-120", "ADR-121", "ADR-122", "ADR-123",
    ] {
        assert!(
            ROOT_README.contains(adr),
            "root README must cite {adr} so the discovery path is intact",
        );
    }
}

#[test]
fn root_readme_points_at_research_bundle() {
    assert!(
        ROOT_README.contains("docs/research/BFLD/"),
        "root README must point at the BFLD research dossier",
    );
}

#[test]
fn root_readme_documents_three_structural_invariants_in_summary() {
    // The doc-table summary is short, but it should still mention the
    // three I1/I2/I3 invariants since they're the single most operator-
    // visible property of BFLD.
    assert!(
        ROOT_README.contains("raw BFI never exits"),
        "root README must mention invariant I1 in the BFLD summary",
    );
    assert!(
        ROOT_README.contains("in-RAM-only") || ROOT_README.contains("in-RAM only"),
        "root README must mention invariant I2 in the BFLD summary",
    );
    assert!(
        ROOT_README.contains("cross-site"),
        "root README must mention invariant I3 in the BFLD summary",
    );
}
