//! Acceptance tests for the BFLD JSON wire spec `rf_signature_hash` format
//! (`"blake3:<64-hex>"`) and the end-to-end emitter → hasher → event → JSON path.

#![cfg(all(feature = "std", feature = "serde-json"))]

use wifi_densepose_bfld::{
    BfldEmitter, BfldEvent, IdentityEmbedding, PrivacyClass, SensingInputs, SignatureHasher,
    EMBEDDING_DIM, SITE_SALT_LEN,
};

fn manual_event(hash: Option<[u8; 32]>) -> BfldEvent {
    BfldEvent::with_privacy_gating(
        "seed-01".into(),
        1_700_000_000_000_000_000,
        true,
        0.5,
        1,
        0.9,
        None,
        PrivacyClass::Anonymous,
        Some(0.3),
        hash,
    )
}

#[test]
fn rf_signature_hash_serializes_as_blake3_prefixed_lowercase_hex() {
    let hash = [
        0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA,
        0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x0F, 0xED,
        0xCB, 0xA9,
    ];
    // Build expected hex programmatically — manual typing is error-prone.
    let mut expected_hex = String::from("blake3:");
    for b in &hash {
        expected_hex.push_str(&format!("{b:02x}"));
    }
    let json = manual_event(Some(hash)).to_json().unwrap();
    let needle = format!("\"rf_signature_hash\":\"{expected_hex}\"");
    assert!(
        json.contains(&needle),
        "JSON: {json}\nexpected substring: {needle}",
    );
}

#[test]
fn hex_string_is_always_64_chars_when_present() {
    let json = manual_event(Some([0x00; 32])).to_json().unwrap();
    // Find the substring after "blake3:" inside the rf_signature_hash field.
    let key = "\"rf_signature_hash\":\"blake3:";
    let start = json.find(key).expect("hash field present") + key.len();
    let end = json[start..].find('"').expect("closing quote") + start;
    let hex = &json[start..end];
    assert_eq!(
        hex.len(),
        64,
        "hash hex must be exactly 64 chars, got {}",
        hex.len()
    );
    assert!(
        hex.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "hash hex must be lowercase only, got {hex}",
    );
}

#[test]
fn hash_field_omitted_entirely_when_none() {
    let json = manual_event(None).to_json().unwrap();
    assert!(
        !json.contains("rf_signature_hash"),
        "None hash must be omitted entirely, got: {json}",
    );
}

// --- Cross-iter integration test ----------------------------------------

fn salt() -> [u8; SITE_SALT_LEN] {
    let mut s = [0u8; SITE_SALT_LEN];
    for (i, b) in s.iter_mut().enumerate() {
        *b = i as u8;
    }
    s
}

fn embedding() -> IdentityEmbedding {
    let mut a = [0.0f32; EMBEDDING_DIM];
    for (i, v) in a.iter_mut().enumerate() {
        *v = (i as f32) * 0.01;
    }
    IdentityEmbedding::from_raw(a)
}

fn inputs() -> SensingInputs {
    SensingInputs {
        timestamp_ns: 1_700_000_000_000_000_000,
        presence: true,
        motion: 0.42,
        person_count: 1,
        sensing_confidence: 0.91,
        sep: 0.2,
        stab: 0.2,
        consist: 0.2,
        risk_conf: 0.2,
        rf_signature_hash: None, // hasher will derive
    }
}

#[test]
fn end_to_end_emitter_hasher_to_json_emits_blake3_hex_hash() {
    let mut e = BfldEmitter::new("seed-01").with_signature_hasher(SignatureHasher::new(salt()));
    let event = e
        .emit(inputs(), Some(embedding()))
        .expect("low-risk emit must succeed");
    let json = event.to_json().expect("JSON serialization");
    assert!(
        json.contains("\"rf_signature_hash\":\"blake3:"),
        "end-to-end JSON missing derived hash: {json}",
    );
    assert!(json.contains("\"type\":\"bfld_update\""));
    assert!(json.contains("\"node_id\":\"seed-01\""));
    assert!(json.contains("\"privacy_class\":\"anonymous\""));
}

#[test]
fn end_to_end_restricted_class_omits_hash_even_with_hasher_set() {
    let mut e = BfldEmitter::new("seed-01")
        .with_privacy_class(PrivacyClass::Restricted)
        .with_signature_hasher(SignatureHasher::new(salt()));
    let event = e
        .emit(inputs(), Some(embedding()))
        .expect("low-risk emit must succeed");
    let json = event.to_json().expect("JSON serialization");
    assert!(
        !json.contains("rf_signature_hash"),
        "Restricted class must strip rf_signature_hash from JSON, got: {json}",
    );
    assert!(
        !json.contains("identity_risk_score"),
        "Restricted class must also strip identity_risk_score, got: {json}",
    );
}
