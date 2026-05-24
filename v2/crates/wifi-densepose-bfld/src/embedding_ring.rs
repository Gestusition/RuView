//! `EmbeddingRing` — bounded FIFO of `IdentityEmbedding`s.
//!
//! Holds at most [`RING_CAPACITY`] (default 64) embeddings. When full, `push`
//! evicts and returns the oldest entry so its `Drop` runs and the f32 storage
//! is zeroized. `drain()` is the explicit "rotate site_salt" hook from the
//! coherence-gate `Recalibrate` action (ADR-121 §2.4): it clears every slot
//! at once. The ring is `no_std`-compatible; no heap allocation.

use crate::embedding::IdentityEmbedding;

/// Default ring capacity — matches ADR-120 §2.5 ("ring buffer of 64 entries").
pub const RING_CAPACITY: usize = 64;

/// Fixed-capacity FIFO of identity embeddings. Insertion-ordered; oldest
/// evicted first when full.
pub struct EmbeddingRing {
    slots: [Option<IdentityEmbedding>; RING_CAPACITY],
    /// Index of the oldest slot — the next eviction target.
    head: usize,
    /// Number of currently-occupied slots (0..=RING_CAPACITY).
    count: usize,
}

impl EmbeddingRing {
    /// Build an empty ring.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; RING_CAPACITY],
            head: 0,
            count: 0,
        }
    }

    /// Insert `emb`. If the ring is already full, evicts and returns the
    /// oldest entry (its `Drop` runs as the returned `Option` is dropped).
    pub fn push(&mut self, emb: IdentityEmbedding) -> Option<IdentityEmbedding> {
        if self.count < RING_CAPACITY {
            // Not full — write into the slot at head + count.
            let idx = (self.head + self.count) % RING_CAPACITY;
            self.slots[idx] = Some(emb);
            self.count += 1;
            None
        } else {
            // Full — overwrite the oldest slot, advance head.
            let evicted = self.slots[self.head].take();
            self.slots[self.head] = Some(emb);
            self.head = (self.head + 1) % RING_CAPACITY;
            evicted
        }
    }

    /// Number of occupied slots.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// `true` iff `len() == 0`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Maximum number of slots — always [`RING_CAPACITY`].
    #[must_use]
    pub const fn capacity(&self) -> usize {
        RING_CAPACITY
    }

    /// `true` iff `len() == capacity()`.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.count == RING_CAPACITY
    }

    /// Iterate occupied slots in **insertion order** (oldest first).
    pub fn iter(&self) -> impl Iterator<Item = &IdentityEmbedding> + '_ {
        (0..self.count).map(move |i| {
            let idx = (self.head + i) % RING_CAPACITY;
            self.slots[idx].as_ref().expect("occupied slot")
        })
    }

    /// Empty the ring. Every contained `IdentityEmbedding` is dropped, which
    /// zeroizes its storage. Returns the number of entries that were drained.
    pub fn drain(&mut self) -> usize {
        let drained = self.count;
        for slot in &mut self.slots {
            // Take() moves the embedding out; the temporary is dropped at the
            // end of this statement, running IdentityEmbedding::drop which
            // zeroes the f32 array.
            let _ = slot.take();
        }
        self.head = 0;
        self.count = 0;
        drained
    }
}

impl Default for EmbeddingRing {
    fn default() -> Self {
        Self::new()
    }
}
