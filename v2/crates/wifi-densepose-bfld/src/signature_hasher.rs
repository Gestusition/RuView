//! `SignatureHasher` — BLAKE3 keyed-hash for `rf_signature_hash`. ADR-120 §2.3.
//!
//! Computes a per-site, per-day, identity-features digest that **structurally
//! prevents** cross-site identity correlation (BFLD invariant I3):
//!
//! ```text
//! rf_signature_hash = BLAKE3-keyed(site_salt, day_epoch || features)
//! ```
//!
//! - **Site isolation**: `site_salt` is a 256-bit secret unique to each node
//!   and never transmitted. Two nodes observing the same physical person
//!   produce uncorrelated hashes — there is no key an operator (or an
//!   attacker who compromises one node) can use to bridge sites.
//! - **Daily rotation**: `day_epoch = floor(unix_time_utc / 86_400)` flips at
//!   UTC midnight, so the same person's hash changes once per day.
//!
//! See ADR-120 §2.7 AC2 for the cross-site Hamming-distance acceptance
//! criterion. `tests/signature_hasher.rs` exercises it directly.

use blake3::Hasher;

/// Number of seconds in a UTC day; the daily-rotation modulus.
pub const SECONDS_PER_DAY: u64 = 86_400;

/// Length of the keyed `site_salt`, fixed by BLAKE3 keyed mode at 32 bytes.
pub const SITE_SALT_LEN: usize = 32;

/// Output length — always 32 bytes (BLAKE3 default).
pub const RF_SIGNATURE_LEN: usize = 32;

/// Per-node hasher carrying the secret `site_salt`. Construct once at boot
/// from the persistent secret store (TPM, KMS, or strict-mode file).
#[derive(Debug, Clone)]
pub struct SignatureHasher {
    site_salt: [u8; SITE_SALT_LEN],
}

impl SignatureHasher {
    /// Build a hasher from an existing `site_salt`. The salt is **never
    /// transmitted** from this point on; callers must keep it in secure storage.
    #[must_use]
    pub const fn new(site_salt: [u8; SITE_SALT_LEN]) -> Self {
        Self { site_salt }
    }

    /// Compute the daily epoch from a UTC unix-seconds timestamp.
    #[must_use]
    pub const fn day_epoch_from_unix_secs(unix_secs: u64) -> u32 {
        (unix_secs / SECONDS_PER_DAY) as u32
    }

    /// Compute the `rf_signature_hash` for the supplied (day, features) pair.
    /// `features` is the canonical-bytes representation of the current
    /// identity-features tuple — the caller is responsible for deterministic
    /// serialization (e.g., `bincode` with sorted keys, or a hand-rolled
    /// fixed-order byte layout).
    #[must_use]
    pub fn compute(&self, day_epoch: u32, features: &[u8]) -> [u8; RF_SIGNATURE_LEN] {
        let mut hasher = Hasher::new_keyed(&self.site_salt);
        hasher.update(&day_epoch.to_le_bytes());
        hasher.update(features);
        *hasher.finalize().as_bytes()
    }

    /// Convenience: compute from a unix-seconds timestamp instead of an
    /// explicit `day_epoch`.
    #[must_use]
    pub fn compute_at(&self, unix_secs: u64, features: &[u8]) -> [u8; RF_SIGNATURE_LEN] {
        self.compute(Self::day_epoch_from_unix_secs(unix_secs), features)
    }
}
