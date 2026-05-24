//! Acceptance tests for ADR-120 §2.5 `EmbeddingRing` lifecycle.

use wifi_densepose_bfld::{EmbeddingRing, IdentityEmbedding, EMBEDDING_DIM, RING_CAPACITY};

fn embedding_with_first(v: f32) -> IdentityEmbedding {
    let mut arr = [0.0f32; EMBEDDING_DIM];
    arr[0] = v;
    IdentityEmbedding::from_raw(arr)
}

#[test]
fn new_ring_is_empty() {
    let r = EmbeddingRing::new();
    assert_eq!(r.len(), 0);
    assert!(r.is_empty());
    assert!(!r.is_full());
    assert_eq!(r.capacity(), RING_CAPACITY);
    assert_eq!(r.iter().count(), 0);
}

#[test]
fn default_constructor_matches_new() {
    let r = EmbeddingRing::default();
    assert_eq!(r.len(), 0);
}

#[test]
fn push_below_capacity_returns_none() {
    let mut r = EmbeddingRing::new();
    for i in 0..5 {
        let evicted = r.push(embedding_with_first(i as f32));
        assert!(evicted.is_none(), "no eviction expected at i={i}");
    }
    assert_eq!(r.len(), 5);
}

#[test]
fn iter_yields_in_insertion_order() {
    let mut r = EmbeddingRing::new();
    for i in 0..5 {
        r.push(embedding_with_first(i as f32));
    }
    let firsts: Vec<f32> = r.iter().map(|e| e.as_slice()[0]).collect();
    assert_eq!(firsts, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn push_at_capacity_evicts_oldest_and_returns_it() {
    let mut r = EmbeddingRing::new();
    for i in 0..RING_CAPACITY {
        r.push(embedding_with_first(i as f32));
    }
    assert!(r.is_full());
    let evicted = r
        .push(embedding_with_first(999.0))
        .expect("must evict when full");
    // The evicted slot held the very first push (first = 0.0).
    assert_eq!(evicted.as_slice()[0], 0.0);
    assert_eq!(r.len(), RING_CAPACITY);
}

#[test]
fn push_beyond_capacity_keeps_last_n_entries() {
    let mut r = EmbeddingRing::new();
    // Push capacity + 10 entries; the first 10 must have been evicted.
    for i in 0..(RING_CAPACITY + 10) {
        r.push(embedding_with_first(i as f32));
    }
    let firsts: Vec<f32> = r.iter().map(|e| e.as_slice()[0]).collect();
    let expected: Vec<f32> = (10..(RING_CAPACITY + 10) as i32)
        .map(|i| i as f32)
        .collect();
    assert_eq!(firsts, expected);
}

#[test]
fn drain_empties_the_ring_and_returns_count() {
    let mut r = EmbeddingRing::new();
    for i in 0..7 {
        r.push(embedding_with_first(i as f32));
    }
    let drained = r.drain();
    assert_eq!(drained, 7);
    assert!(r.is_empty());
    assert_eq!(r.iter().count(), 0);
}

#[test]
fn drain_on_empty_ring_returns_zero() {
    let mut r = EmbeddingRing::new();
    assert_eq!(r.drain(), 0);
    assert!(r.is_empty());
}

#[test]
fn ring_can_be_refilled_after_drain() {
    let mut r = EmbeddingRing::new();
    r.push(embedding_with_first(1.0));
    r.push(embedding_with_first(2.0));
    r.drain();
    r.push(embedding_with_first(42.0));
    assert_eq!(r.len(), 1);
    assert_eq!(r.iter().next().unwrap().as_slice()[0], 42.0);
}
