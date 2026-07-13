//! `IdentityFeatures` — typed canonical-bytes encoder for `SignatureHasher`.
//!
//! Wraps the two possible feature sources (a borrowed [`IdentityEmbedding`] or
//! the four-tuple of risk factors) behind a single API so callers don't need
//! to know which one ultimately feeds the BLAKE3 keyed hash. Replaces the
//! ad-hoc `canonical_risk_bytes` + inline embedding-flatten paths that lived
//! in `emitter.rs` through iter 17.
//!
//! Borrowing semantics:
//! - `IdentityFeatures::Embedding(&IdentityEmbedding)` is the **preferred**
//!   source — it carries the AETHER cluster identity directly.
//! - `IdentityFeatures::RiskFactors { .. }` is the fallback used when the
//!   per-frame embedding is unavailable.
//!
//! Both variants emit canonical little-endian f32 bytes. Embedding produces
//! `EMBEDDING_DIM * 4` bytes (512 by default); risk factors produce
//! [`RISK_FACTOR_BYTES`] bytes (16).

#![cfg(feature = "std")]

use crate::signature_hasher::{SignatureHasher, RF_SIGNATURE_LEN};
use crate::{IdentityEmbedding, EMBEDDING_DIM};

/// Wire-form length for the `RiskFactors` variant (4 × f32 little-endian).
pub const RISK_FACTOR_BYTES: usize = 16;

/// Borrowed feature source for the signature hasher.
#[derive(Debug)]
pub enum IdentityFeatures<'a> {
    /// Preferred: a borrowed identity embedding. The embedding stays in-RAM
    /// (invariant I2) — this enum holds only a reference.
    Embedding(&'a IdentityEmbedding),
    /// Fallback: the four risk-score factors. Less identity-stable than the
    /// embedding, but always available even when the encoder is offline.
    RiskFactors {
        /// `identity_separability_score`.
        sep: f32,
        /// `temporal_stability`.
        stab: f32,
        /// `cross_perspective_consistency`.
        consist: f32,
        /// Risk-score sample confidence factor.
        conf: f32,
    },
}

impl<'a> IdentityFeatures<'a> {
    /// Build from a borrowed embedding (preferred path).
    #[must_use]
    pub const fn from_embedding(emb: &'a IdentityEmbedding) -> Self {
        Self::Embedding(emb)
    }

    /// Build from the risk-factor four-tuple (fallback path).
    #[must_use]
    pub const fn from_risk_factors(sep: f32, stab: f32, consist: f32, conf: f32) -> Self {
        Self::RiskFactors {
            sep,
            stab,
            consist,
            conf,
        }
    }

    /// Predicted wire length without allocating.
    #[must_use]
    pub const fn canonical_byte_len(&self) -> usize {
        match self {
            Self::Embedding(_) => EMBEDDING_DIM * 4,
            Self::RiskFactors { .. } => RISK_FACTOR_BYTES,
        }
    }

    /// Append canonical little-endian bytes to `out`. Useful for callers that
    /// already own a buffer (avoids the `canonical_bytes` allocation).
    pub fn write_canonical_bytes(&self, out: &mut Vec<u8>) {
        out.reserve(self.canonical_byte_len());
        match self {
            Self::Embedding(emb) => {
                for f in emb.as_slice() {
                    out.extend_from_slice(&f.to_le_bytes());
                }
            }
            Self::RiskFactors {
                sep,
                stab,
                consist,
                conf,
            } => {
                out.extend_from_slice(&sep.to_le_bytes());
                out.extend_from_slice(&stab.to_le_bytes());
                out.extend_from_slice(&consist.to_le_bytes());
                out.extend_from_slice(&conf.to_le_bytes());
            }
        }
    }

    /// Allocating convenience wrapper around [`Self::write_canonical_bytes`].
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.canonical_byte_len());
        self.write_canonical_bytes(&mut v);
        v
    }

    /// Drive `hasher` with this feature source at the given `day_epoch`. The
    /// returned hash is what the emitter publishes as `rf_signature_hash`.
    #[must_use]
    pub fn compute_hash(&self, hasher: &SignatureHasher, day_epoch: u32) -> [u8; RF_SIGNATURE_LEN] {
        hasher.compute(day_epoch, &self.canonical_bytes())
    }
}
