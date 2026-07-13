//! `BfldFrame` wire-format primitives. See ADR-119.
//!
//! The header is `#[repr(C, packed)]` so the wire byte order is fixed across
//! x86_64, aarch64, and xtensa-esp32s3 — and so the witness-bundle pattern
//! (ADR-028) extends cleanly to BFLD frames.
//!
//! All multi-byte integers serialize as **little-endian**. The
//! `to_le_bytes`/`from_le_bytes` helpers encode/decode without `unsafe`, which
//! is forbidden in this crate; the encoded bytes are the canonical wire form.
//!
//! CRC-32/ISO-HDLC (the same polynomial Ethernet uses) protects the payload.
//! See [`crc32_of_payload`] for the canonical computation.

use static_assertions::const_assert_eq;

use crate::BfldError;

/// CRC-32/ISO-HDLC algorithm used to checksum payload bytes. Poly 0xEDB88320,
/// init 0xFFFFFFFF, xorout 0xFFFFFFFF, reflected — same as Ethernet / zlib.
pub const CRC32_ALG: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

/// Compute the canonical CRC32 over `payload`. The header CRC field is **not**
/// included in the digest (ADR-119 §2.2: "CRC32 covers all section bytes
/// including length prefixes, but not the header").
#[must_use]
pub fn crc32_of_payload(payload: &[u8]) -> u32 {
    CRC32_ALG.checksum(payload)
}

/// Magic value identifying a `BfldFrame`. Reads as "BFLD" in hex-dump tools.
pub const BFLD_MAGIC: u32 = 0xBF1D_0001;

/// Current `BfldFrame` major version. Bumps on any incompatible layout change.
pub const BFLD_VERSION: u16 = 1;

/// Size of the packed header in bytes. Asserted at compile time below.
///
/// Note: ADR-119 AC1 initially claimed 40 bytes — that was a counting error.
/// Actual packed layout sums to 86. Updated 2026-05-24 to match implementation.
pub const BFLD_HEADER_SIZE: usize = 86;

/// Flag bits in `BfldFrameHeader::flags`. See ADR-119 §2.1.
pub mod flags {
    /// Payload contains an optional CSI delta section.
    pub const HAS_CSI_DELTA: u16 = 1 << 0;
    /// `privacy_mode` is engaged: identity-derived fields suppressed.
    pub const PRIVACY_MODE: u16 = 1 << 1;
    /// ESP32-S3 self-only adapter (ADR-123 §2.5): no `identity_risk_score`.
    pub const SELF_ONLY: u16 = 1 << 3;

    /// Bitmask covering every named flag this version of the crate knows
    /// about. Useful for "did the wire form set any flags I don't recognize?"
    /// forward-compat checks.
    pub const KNOWN_FLAGS_MASK: u16 = HAS_CSI_DELTA | PRIVACY_MODE | SELF_ONLY;

    /// Complement of [`KNOWN_FLAGS_MASK`] — every bit position not currently
    /// assigned a meaning. Bits set in this mask MUST round-trip unchanged
    /// per ADR-119 §2.1 ("Reserved flag bits 2-15 lock in future-extension
    /// order; any new bit assignment is a version bump"). A future protocol
    /// revision may light these up; today's parser preserves them so a node
    /// running iter N can forward unknown bits to a peer running iter N+M
    /// without losing information.
    pub const RESERVED_FLAGS_MASK: u16 = !KNOWN_FLAGS_MASK;
}

/// On-the-wire BFLD frame header. 86 bytes, little-endian, packed.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct BfldFrameHeader {
    /// Must equal [`BFLD_MAGIC`].
    pub magic: u32,
    /// Layout version. Currently [`BFLD_VERSION`].
    pub version: u16,
    /// Flag bits — see [`flags`].
    pub flags: u16,
    /// Monotonic capture-clock timestamp in nanoseconds.
    pub timestamp_ns: u64,
    /// BLAKE3-keyed(site_salt, ap_mac)[0..16] — ADR-120 §2.3.
    pub ap_hash: [u8; 16],
    /// BLAKE3-keyed(site_salt ‖ day_epoch, sta_mac)[0..16] — daily-rotated.
    pub sta_hash: [u8; 16],
    /// Ephemeral session identifier, rotated on capture-session boundary.
    pub session_id: [u8; 16],
    /// 802.11 channel number.
    pub channel: u16,
    /// Channel bandwidth in MHz: 20 / 40 / 80 / 160.
    pub bandwidth_mhz: u16,
    /// Received signal strength in dBm.
    pub rssi_dbm: i16,
    /// Noise floor in dBm.
    pub noise_floor_dbm: i16,
    /// Number of OFDM subcarriers represented.
    pub n_subcarriers: u16,
    /// Number of transmit antennas.
    pub n_tx: u8,
    /// Number of receive antennas.
    pub n_rx: u8,
    /// 0=f32, 1=i16, 2=i8, 3=packed (4-bit nibbles).
    pub quantization: u8,
    /// `PrivacyClass` byte — see ADR-120 §2.1.
    pub privacy_class: u8,
    /// Length of the payload section in bytes.
    pub payload_len: u32,
    /// CRC-32/ISO-HDLC over payload bytes only.
    pub payload_crc32: u32,
}

const_assert_eq!(core::mem::size_of::<BfldFrameHeader>(), BFLD_HEADER_SIZE);

impl BfldFrameHeader {
    /// Build a header with `magic` and `version` already set correctly.
    /// All other fields default to zero — caller fills them in.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            magic: BFLD_MAGIC,
            version: BFLD_VERSION,
            ..Self::default()
        }
    }

    /// Serialize to canonical little-endian wire form (86 bytes).
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn to_le_bytes(&self) -> [u8; BFLD_HEADER_SIZE] {
        let mut buf = [0u8; BFLD_HEADER_SIZE];
        let mut o = 0usize;

        // Copy locally to dodge `#[repr(packed)]` unaligned-borrow warnings.
        let magic = self.magic;
        let version = self.version;
        let flags = self.flags;
        let timestamp_ns = self.timestamp_ns;
        let channel = self.channel;
        let bandwidth_mhz = self.bandwidth_mhz;
        let rssi_dbm = self.rssi_dbm;
        let noise_floor_dbm = self.noise_floor_dbm;
        let n_subcarriers = self.n_subcarriers;
        let payload_len = self.payload_len;
        let payload_crc32 = self.payload_crc32;

        buf[o..o + 4].copy_from_slice(&magic.to_le_bytes());
        o += 4;
        buf[o..o + 2].copy_from_slice(&version.to_le_bytes());
        o += 2;
        buf[o..o + 2].copy_from_slice(&flags.to_le_bytes());
        o += 2;
        buf[o..o + 8].copy_from_slice(&timestamp_ns.to_le_bytes());
        o += 8;
        buf[o..o + 16].copy_from_slice(&self.ap_hash);
        o += 16;
        buf[o..o + 16].copy_from_slice(&self.sta_hash);
        o += 16;
        buf[o..o + 16].copy_from_slice(&self.session_id);
        o += 16;
        buf[o..o + 2].copy_from_slice(&channel.to_le_bytes());
        o += 2;
        buf[o..o + 2].copy_from_slice(&bandwidth_mhz.to_le_bytes());
        o += 2;
        buf[o..o + 2].copy_from_slice(&rssi_dbm.to_le_bytes());
        o += 2;
        buf[o..o + 2].copy_from_slice(&noise_floor_dbm.to_le_bytes());
        o += 2;
        buf[o..o + 2].copy_from_slice(&n_subcarriers.to_le_bytes());
        o += 2;
        buf[o] = self.n_tx;
        o += 1;
        buf[o] = self.n_rx;
        o += 1;
        buf[o] = self.quantization;
        o += 1;
        buf[o] = self.privacy_class;
        o += 1;
        buf[o..o + 4].copy_from_slice(&payload_len.to_le_bytes());
        o += 4;
        buf[o..o + 4].copy_from_slice(&payload_crc32.to_le_bytes());
        o += 4;

        debug_assert_eq!(o, BFLD_HEADER_SIZE);
        buf
    }

    /// Parse from canonical little-endian wire form.
    ///
    /// Returns [`BfldError::InvalidMagic`] if the magic prefix is wrong, and
    /// [`BfldError::UnsupportedVersion`] for a version this build cannot decode.
    /// Field-level validation (CRC, payload_len bounds) is deliberately *not*
    /// performed here — that lives at the frame-level parser.
    pub fn from_le_bytes(bytes: &[u8; BFLD_HEADER_SIZE]) -> Result<Self, BfldError> {
        let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        if magic != BFLD_MAGIC {
            return Err(BfldError::InvalidMagic(magic));
        }
        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version != BFLD_VERSION {
            return Err(BfldError::UnsupportedVersion(version));
        }

        let mut h = Self {
            magic,
            version,
            flags: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            timestamp_ns: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            ap_hash: [0; 16],
            sta_hash: [0; 16],
            session_id: [0; 16],
            channel: u16::from_le_bytes(bytes[64..66].try_into().unwrap()),
            bandwidth_mhz: u16::from_le_bytes(bytes[66..68].try_into().unwrap()),
            rssi_dbm: i16::from_le_bytes(bytes[68..70].try_into().unwrap()),
            noise_floor_dbm: i16::from_le_bytes(bytes[70..72].try_into().unwrap()),
            n_subcarriers: u16::from_le_bytes(bytes[72..74].try_into().unwrap()),
            n_tx: bytes[74],
            n_rx: bytes[75],
            quantization: bytes[76],
            privacy_class: bytes[77],
            payload_len: u32::from_le_bytes(bytes[78..82].try_into().unwrap()),
            payload_crc32: u32::from_le_bytes(bytes[82..86].try_into().unwrap()),
        };
        h.ap_hash.copy_from_slice(&bytes[16..32]);
        h.sta_hash.copy_from_slice(&bytes[32..48]);
        h.session_id.copy_from_slice(&bytes[48..64]);
        Ok(h)
    }
}

// --- BfldFrame (header + payload) ------------------------------------------
//
// Gated on `std` because the payload is heap-allocated (`Vec<u8>`). ESP32-S3
// self-only mode (ADR-123 §2.5) will need a separate `BfldFrameRef<'_>` API
// that borrows a caller-provided buffer; that lands in a later iter.

/// Complete BFLD frame: header + payload bytes. The frame's wire form is
/// `header.to_le_bytes() ‖ payload`, with the header's `payload_len` and
/// `payload_crc32` fields kept consistent by `to_bytes`/`from_bytes`.
#[cfg(feature = "std")]
#[derive(Debug, Clone)]
pub struct BfldFrame {
    /// Header — `payload_len` and `payload_crc32` reflect the payload below.
    pub header: BfldFrameHeader,
    /// Raw payload bytes. The internal section layout (compressed_angle_matrix,
    /// amplitude_proxy, ...) lives in a later iter; for now the byte buffer is
    /// opaque to this struct.
    pub payload: Vec<u8>,
}

#[cfg(feature = "std")]
impl BfldFrame {
    /// Construct a frame, automatically syncing `header.payload_len` and
    /// `header.payload_crc32` to the supplied `payload`.
    #[must_use]
    pub fn new(mut header: BfldFrameHeader, payload: Vec<u8>) -> Self {
        let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        header.payload_len = len;
        header.payload_crc32 = crc32_of_payload(&payload);
        Self { header, payload }
    }

    /// Construct a frame from a typed `BfldPayload`. The header `flags`
    /// `HAS_CSI_DELTA` bit is auto-synced from `payload.csi_delta.is_some()`,
    /// then the payload is serialized via [`crate::payload::BfldPayload::to_bytes`]
    /// and the resulting bytes feed [`BfldFrame::new`]. The CRC therefore covers
    /// the **section-prefixed** wire bytes per ADR-119 §2.2.
    #[must_use]
    pub fn from_payload(
        mut header: BfldFrameHeader,
        payload: &crate::payload::BfldPayload,
    ) -> Self {
        let include_csi_delta = payload.csi_delta.is_some();
        if include_csi_delta {
            header.flags |= flags::HAS_CSI_DELTA;
        } else {
            header.flags &= !flags::HAS_CSI_DELTA;
        }
        let bytes = payload.to_bytes(include_csi_delta);
        Self::new(header, bytes)
    }

    /// Parse the opaque payload bytes back into a typed [`crate::payload::BfldPayload`].
    /// Consults `header.flags & HAS_CSI_DELTA` so the parser matches the
    /// originating encoder's framing.
    pub fn parse_payload(&self) -> Result<crate::payload::BfldPayload, BfldError> {
        let expect_csi_delta = (self.header.flags & flags::HAS_CSI_DELTA) != 0;
        crate::payload::BfldPayload::from_bytes(&self.payload, expect_csi_delta)
    }

    /// Serialize to wire form: 86 header bytes + `payload_len` payload bytes.
    /// Always recomputes `payload_crc32` so the returned bytes are internally
    /// consistent even if the caller mutated `header.payload_crc32` directly.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut header = self.header;
        header.payload_len = u32::try_from(self.payload.len()).unwrap_or(u32::MAX);
        header.payload_crc32 = crc32_of_payload(&self.payload);
        let header_bytes = header.to_le_bytes();
        let mut out = Vec::with_capacity(BFLD_HEADER_SIZE + self.payload.len());
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parse from wire form. Validates magic, version, payload length, and CRC.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BfldError> {
        if bytes.len() < BFLD_HEADER_SIZE {
            return Err(BfldError::TruncatedFrame {
                got: bytes.len(),
                need: BFLD_HEADER_SIZE,
            });
        }
        let header_bytes: &[u8; BFLD_HEADER_SIZE] = bytes[..BFLD_HEADER_SIZE].try_into().unwrap();
        let header = BfldFrameHeader::from_le_bytes(header_bytes)?;

        let payload_len = header.payload_len as usize;
        let expected_total = BFLD_HEADER_SIZE.saturating_add(payload_len);
        if bytes.len() < expected_total {
            return Err(BfldError::TruncatedFrame {
                got: bytes.len(),
                need: expected_total,
            });
        }
        let payload = bytes[BFLD_HEADER_SIZE..expected_total].to_vec();

        let actual = crc32_of_payload(&payload);
        let expected = header.payload_crc32;
        if actual != expected {
            return Err(BfldError::Crc { expected, actual });
        }
        Ok(Self { header, payload })
    }
}
