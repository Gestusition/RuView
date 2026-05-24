//! # BFLD — Beamforming Feedback Layer for Detection
//!
//! Privacy-gated WiFi sensing primitives derived from 802.11ac/ax Beamforming
//! Feedback Information (BFI). See [`docs/adr/ADR-118-bfld-beamforming-feedback-layer-for-detection.md`](../../../docs/adr/ADR-118-bfld-beamforming-feedback-layer-for-detection.md).
//!
//! ## Three structural invariants
//!
//! - **I1**: Raw BFI never exits the node.
//! - **I2**: Identity embedding is in-RAM-only.
//! - **I3**: Cross-site identity correlation is cryptographically impossible.
//!
//! Status: P1 in progress — frame format + sink marker traits. P2–P6 follow.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod embedding;
pub mod embedding_ring;
pub mod frame;
#[cfg(feature = "std")]
pub mod payload;
pub mod sink;

pub use embedding::{IdentityEmbedding, EMBEDDING_DIM};
pub use embedding_ring::{EmbeddingRing, RING_CAPACITY};
pub use frame::{BfldFrameHeader, BFLD_MAGIC, BFLD_VERSION, BFLD_HEADER_SIZE};
#[cfg(feature = "std")]
pub use frame::BfldFrame;
#[cfg(feature = "std")]
pub use payload::BfldPayload;
pub use sink::{check_class, LocalSink, MatterSink, NetworkSink, Sink};

/// Privacy classification carried in every `BfldFrame`. See ADR-120 §2.1.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivacyClass {
    /// Local-only research data including raw BFI matrix. Never networked.
    Raw = 0,
    /// Operator-acknowledged research mode over LAN. Downsampled angles +
    /// identity_embedding + identity_risk_score available. Required for
    /// Soul Signature deployments (ADR-120 §2.7).
    Derived = 1,
    /// Production default: aggregate sensing only, no identity-derived fields.
    Anonymous = 2,
    /// Care-home / regulated deployments: class 2 minus risk score and hash.
    Restricted = 3,
}

impl PrivacyClass {
    /// Returns `true` if frames of this class may cross a `NetworkSink`.
    /// Class 0 (`Raw`) is local-only by structural invariant I1.
    #[must_use]
    pub const fn allows_network(self) -> bool {
        !matches!(self, Self::Raw)
    }

    /// Returns `true` if frames of this class may cross the Matter boundary.
    /// Only classes 2 and 3 are Matter-eligible. See ADR-122 §2.4.
    #[must_use]
    pub const fn allows_matter(self) -> bool {
        matches!(self, Self::Anonymous | Self::Restricted)
    }

    /// Returns the byte value of this class (0..=3) for serialization.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for PrivacyClass {
    type Error = BfldError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::Derived),
            2 => Ok(Self::Anonymous),
            3 => Ok(Self::Restricted),
            other => Err(BfldError::InvalidPrivacyClass(other)),
        }
    }
}

/// Errors produced by BFLD operations.
#[derive(Debug, thiserror::Error)]
pub enum BfldError {
    /// Header magic did not match `BFLD_MAGIC`.
    #[error("invalid BFLD magic: expected 0x{BFLD_MAGIC:08X}, got 0x{0:08X}")]
    InvalidMagic(u32),

    /// Header version unsupported.
    #[error("unsupported BFLD version: {0}")]
    UnsupportedVersion(u16),

    /// Payload CRC32 mismatch — frame corrupted or tampered.
    #[error("payload CRC mismatch: expected 0x{expected:08X}, got 0x{actual:08X}")]
    Crc {
        /// CRC value the header declared.
        expected: u32,
        /// CRC value computed over the received payload.
        actual: u32,
    },

    /// Attempted to publish a class-0 (`Raw`) frame through a network sink.
    /// Enforces structural invariant I1.
    #[error("privacy violation: {reason}")]
    PrivacyViolation {
        /// `Sink::KIND` of the sink that rejected the frame.
        reason: &'static str,
    },

    /// Byte value did not map to any defined `PrivacyClass` (0..=3).
    #[error("invalid PrivacyClass byte: {0}")]
    InvalidPrivacyClass(u8),

    /// Buffer too short for header (86 bytes) or header + declared payload.
    #[error("truncated frame: got {got} bytes, need at least {need}")]
    TruncatedFrame {
        /// Bytes available in the input buffer.
        got: usize,
        /// Bytes the header indicates are required.
        need: usize,
    },

    /// Payload section length-prefix decoding failed or trailing bytes left over.
    #[error("malformed payload section at offset {offset}: {reason}")]
    MalformedSection {
        /// Byte offset within the payload where parsing failed.
        offset: usize,
        /// Human-readable reason for the failure.
        reason: &'static str,
    },
}
