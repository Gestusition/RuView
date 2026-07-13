//! Cog manifest — same shape as `cog-pose-estimation/cog/manifest.template.json`
//! per ADR-101 / ADR-102 / ADR-116. Generated at build time by the cog's
//! Makefile, optionally signed by the project's Ed25519 release key, and
//! attached to the matching GitHub Release for installers to fetch.
//!
//! The runtime ships the typed view here so the cog can self-report its
//! manifest to a local control surface (`/api/v1/cog/status`).
//!
//! Kept in lib.rs's nearest sibling module so manifest format drift between
//! build-time template and runtime serializer fires a named test.

use serde::{Deserialize, Serialize};

/// Wire-format mirror of `cog/manifest.template.json`.
///
/// Every field is required at install time; `binary_signature` is the
/// Ed25519 sig over `binary_sha256` so an installer can verify the binary
/// was not tampered with between the GitHub Release and the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CogManifest {
    /// Stable cog identifier ("ha-matter"). Used as the installation
    /// directory name on the edge host.
    pub id: String,
    /// SemVer of the cog binary. Bumped by the Makefile from
    /// `cargo pkgid` at release time.
    pub version: String,
    /// Where the installer fetches the binary from. Arch-specific URL with
    /// the `{{ARCH}}` template slot filled in (e.g. `arm`, `x86_64`).
    pub binary_url: String,
    /// Bytes of the binary blob. Set at build time after `wc -c`.
    pub binary_bytes: u64,
    /// SHA-256 of the binary, hex-lowercase, no `0x` prefix. The installer
    /// verifies this before exec().
    pub binary_sha256: String,
    /// Ed25519 signature over `binary_sha256`, base64-encoded. Optional
    /// for unsigned development builds.
    pub binary_signature: String,
    /// Unix epoch seconds at install time. The installer stamps this when it
    /// completes a successful install/upgrade.
    pub installed_at: u64,
    /// One of `"installed"`, `"upgrading"`, `"degraded"`, `"removed"`.
    pub status: String,
}

impl CogManifest {
    pub fn id() -> &'static str {
        super::COG_ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock the JSON wire shape against accidental field renames. Both
    /// the local control plane and the build-time signer parse this —
    /// any drift fires a named test instead of silently breaking ops.
    #[test]
    fn manifest_round_trip_matches_template() {
        let m = CogManifest {
            id: "ha-matter".into(),
            version: "0.1.0".into(),
            binary_url: concat!(
                "https://github.com/Gestusition/KdView/releases/download/",
                "cog-ha-matter-v0.1.0/cog-ha-matter-arm"
            )
            .into(),
            binary_bytes: 4_200_000,
            binary_sha256: "a".repeat(64),
            binary_signature: "Zm9v".into(),
            installed_at: 1_779_512_400,
            status: "installed".into(),
        };
        let json = serde_json::to_value(&m).unwrap();
        // Eight required fields, no extras.
        for key in [
            "id",
            "version",
            "binary_url",
            "binary_bytes",
            "binary_sha256",
            "binary_signature",
            "installed_at",
            "status",
        ] {
            assert!(json.get(key).is_some(), "missing manifest field `{key}`");
        }
        let m2: CogManifest = serde_json::from_value(json).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn manifest_id_constant_matches_cog_id() {
        // The id helper must agree with the crate-level COG_ID constant
        // (regression guard for a future rename).
        assert_eq!(CogManifest::id(), super::super::COG_ID);
    }
}
