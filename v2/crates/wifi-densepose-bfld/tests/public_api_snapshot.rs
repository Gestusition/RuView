//! Public API surface snapshot. Compile-time witness that every `pub use`
//! re-export from `lib.rs` survives refactors. A future PR that removes
//! one of these breaks the build with a specific named-symbol error,
//! which is a much louder signal than a silent SemVer-breaking removal.
//!
//! Two feature configurations are exercised:
//! - Always available (no_std-compatible core)
//! - `feature = "std"` items behind a cfg guard
//!
//! `feature = "mqtt"` items have their own snapshot test below.

// --- always-available exports (work under `--no-default-features`) ----

use wifi_densepose_bfld::frame::{flags, BFLD_HEADER_SIZE, BFLD_MAGIC, BFLD_VERSION};
use wifi_densepose_bfld::sink::{
    check_class, LocalKind, LocalSink, MatterKind, MatterSink, NetworkKind, NetworkSink, Sink,
};
use wifi_densepose_bfld::{
    BfldError, BfldFrameHeader, CoherenceGate, EmbeddingRing, GateAction, IdentityEmbedding,
    MatchOutcome, NullOracle, PrivacyClass, SignatureHasher, SoulMatchOracle, EMBEDDING_DIM,
    RF_SIGNATURE_LEN, RING_CAPACITY, SITE_SALT_LEN,
};

#[test]
fn always_available_types_are_re_exported() {
    // Type-existence witnesses. Each line will fail to compile if the
    // corresponding `pub use` is removed from lib.rs.
    let _: PrivacyClass = PrivacyClass::Anonymous;
    let _: GateAction = GateAction::Accept;
    let _: MatchOutcome = MatchOutcome::NotEnrolled;
    let _: BfldFrameHeader = BfldFrameHeader::empty();
    let _: CoherenceGate = CoherenceGate::new();
    let _: NullOracle = NullOracle;
    let _: EmbeddingRing = EmbeddingRing::new();
    let _: SignatureHasher = SignatureHasher::new([0u8; SITE_SALT_LEN]);
    let _: IdentityEmbedding = IdentityEmbedding::from_raw([0.0; EMBEDDING_DIM]);

    // Compile-time const witnesses.
    let _: u32 = BFLD_MAGIC;
    let _: u16 = BFLD_VERSION;
    let _: usize = BFLD_HEADER_SIZE;
    let _: usize = EMBEDDING_DIM;
    let _: usize = RING_CAPACITY;
    let _: usize = RF_SIGNATURE_LEN;
    let _: usize = SITE_SALT_LEN;
    let _: u16 = flags::HAS_CSI_DELTA;
    let _: u16 = flags::PRIVACY_MODE;
    let _: u16 = flags::SELF_ONLY;
    let _: u16 = flags::KNOWN_FLAGS_MASK;
    let _: u16 = flags::RESERVED_FLAGS_MASK;
}

#[test]
fn sink_trait_hierarchy_re_exported() {
    fn assert_sink<S: Sink>() {}
    fn assert_local<S: LocalSink>() {}
    fn assert_network<S: NetworkSink>() {}
    fn assert_matter<S: MatterSink>() {}
    assert_sink::<LocalKind>();
    assert_local::<LocalKind>();
    assert_sink::<NetworkKind>();
    assert_network::<NetworkKind>();
    assert_sink::<MatterKind>();
    assert_network::<MatterKind>();
    assert_matter::<MatterKind>();

    // check_class is reachable.
    let _ = check_class::<NetworkKind>(PrivacyClass::Anonymous);
}

#[test]
fn soul_match_oracle_trait_re_exported() {
    fn assert_oracle<O: SoulMatchOracle>() {}
    assert_oracle::<NullOracle>();
}

#[test]
fn bfld_error_re_exported_with_all_named_variants() {
    let _ = BfldError::InvalidMagic(0);
    let _ = BfldError::UnsupportedVersion(0);
    let _ = BfldError::Crc {
        expected: 0,
        actual: 0,
    };
    let _ = BfldError::PrivacyViolation { reason: "X" };
    let _ = BfldError::InvalidPrivacyClass(0);
    let _ = BfldError::TruncatedFrame { got: 0, need: 0 };
    let _ = BfldError::MalformedSection {
        offset: 0,
        reason: "X",
    };
    let _ = BfldError::InvalidDemote { from: 0, to: 0 };
}

// --- `std` feature exports --------------------------------------------

#[cfg(feature = "std")]
mod std_surface {
    use wifi_densepose_bfld::{
        availability_topic, identity_risk_score, offline_message, online_message,
        publish_availability_offline, publish_availability_online, publish_discovery,
        publish_event, render_discovery_payloads, render_events, BfldConfig, BfldEmitter,
        BfldEvent, BfldFrame, BfldPayload, BfldPipeline, BfldPipelineHandle, CapturePublisher,
        IdentityFeatures, PipelineInput, PrivacyClass, PrivacyGate, Publish, SensingInputs,
        TopicMessage, PAYLOAD_AVAILABLE, PAYLOAD_NOT_AVAILABLE, RISK_FACTOR_BYTES,
    };

    #[test]
    fn std_only_types_are_re_exported() {
        let _: BfldConfig = BfldConfig::new("seed-snap");
        let _: BfldPipeline = BfldPipeline::new(BfldConfig::new("seed-snap"));
        let _: BfldEmitter = BfldEmitter::new("seed-snap");
        let _: PrivacyGate = PrivacyGate;
        let _: CapturePublisher = CapturePublisher::default();

        // Free-function exports
        let _: u32 = wifi_densepose_bfld::BFLD_MAGIC;
        let _ = identity_risk_score(0.0, 0.0, 0.0, 0.0);
        let _: String = availability_topic("seed-snap");
        let _: TopicMessage = online_message("seed-snap");
        let _: TopicMessage = offline_message("seed-snap");
        let _: &'static str = PAYLOAD_AVAILABLE;
        let _: &'static str = PAYLOAD_NOT_AVAILABLE;
        let _: usize = RISK_FACTOR_BYTES;

        // Type-erased witnesses for the publish + render helpers.
        let mut cap = CapturePublisher::default();
        let _ = publish_availability_online(&mut cap, "seed-snap");
        let _ = publish_availability_offline(&mut cap, "seed-snap");
        let _ = publish_discovery(&mut cap, "seed-snap", PrivacyClass::Anonymous);
        let _: Vec<TopicMessage> = render_discovery_payloads("seed-snap", PrivacyClass::Anonymous);

        // Event + frame + payload constructible.
        let event = BfldEvent::with_privacy_gating(
            "seed-snap".into(),
            0,
            false,
            0.0,
            0,
            0.0,
            None,
            PrivacyClass::Anonymous,
            None,
            None,
        );
        let _ = render_events(&event);
        let _ = publish_event(&mut cap, &event);

        let _: BfldFrame =
            BfldFrame::new(wifi_densepose_bfld::BfldFrameHeader::empty(), Vec::new());
        let _: BfldPayload = BfldPayload::default();
        let _: IdentityFeatures<'_> = IdentityFeatures::from_risk_factors(0.0, 0.0, 0.0, 0.0);

        // Publish-trait usage path.
        fn _accepts_publisher<P: Publish>(_: &mut P) {}

        // Sensing-inputs surface.
        let _: SensingInputs = SensingInputs {
            timestamp_ns: 0,
            presence: false,
            motion: 0.0,
            person_count: 0,
            sensing_confidence: 0.0,
            sep: 0.0,
            stab: 0.0,
            consist: 0.0,
            risk_conf: 0.0,
            rf_signature_hash: None,
        };

        // PipelineInput + Handle types reachable from lib.rs.
        let _ = PipelineInput {
            inputs: SensingInputs {
                timestamp_ns: 0,
                presence: false,
                motion: 0.0,
                person_count: 0,
                sensing_confidence: 0.0,
                sep: 0.0,
                stab: 0.0,
                consist: 0.0,
                risk_conf: 0.0,
                rf_signature_hash: None,
            },
            embedding: None,
        };
        // BfldPipelineHandle type witness (don't actually spawn — costs a thread).
        fn _accepts_handle(_: BfldPipelineHandle) {}
    }
}

// --- `mqtt` feature exports -------------------------------------------

#[cfg(feature = "mqtt")]
mod mqtt_surface {
    use wifi_densepose_bfld::{with_lwt, RumqttPublisher};

    #[test]
    fn mqtt_publisher_types_are_re_exported() {
        fn _accepts_pub(_: RumqttPublisher) {}
        fn _accepts_with_lwt_signature(
            opts: rumqttc::MqttOptions,
            node: &str,
        ) -> rumqttc::MqttOptions {
            with_lwt(opts, node)
        }
        let _ = _accepts_with_lwt_signature;
    }
}
