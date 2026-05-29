//! # RuView Streaming Engine — integration layer
//!
//! This crate is the **composition root** that wires the ADR-135..146 building
//! blocks into one end-to-end *trust-traceable* pipeline cycle. Each block was
//! built and unit-tested independently; this crate proves they compose and that
//! the **trust throughline** holds end-to-end:
//!
//! > *Why believe the system when it says a person is present?* — every
//! > [`TrustedOutput`] names its **signal evidence** (ADR-137 `EvidenceRef`),
//! > its **model version** (ADR-136), its **calibration version** (ADR-135
//! > baseline id, ADR-136 `calibration_id`), and the **privacy decision**
//! > (ADR-141 mode → class) it was emitted under — and is anchored as a
//! > provenance-bearing node in the ADR-139 WorldGraph.
//!
//! One [`StreamingEngine::process_cycle`] performs, in order:
//! 1. **Fuse + score** the node frames (ADR-137 `fuse_scored`) → `QualityScore`
//!    with per-node weights, evidence, and tolerated contradiction flags.
//! 2. **Stamp calibration provenance** (ADR-135/136): the `CalibrationId` the
//!    calibration stage applied is recorded on the `QualityScore`.
//! 3. **Privacy control plane** (ADR-141): if the fusion recorded a tolerated
//!    contradiction, the active privacy class is **demoted one step** before
//!    emission (monotonic — information only ever removed).
//! 4. **Semantic state** (ADR-139/140): a `SemanticState` node is appended to
//!    the WorldGraph with mandatory provenance and a `DerivedFrom` edge to the
//!    room it was observed in.
//!
//! What is intentionally *not* here: the live 20 Hz I/O loop (sensing-server),
//! UWB hardware (ADR-144), and model training (ADR-146). This is the
//! composition + validation layer those will plug into.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use wifi_densepose_bfld::{PrivacyAction, PrivacyClass, PrivacyMode, PrivacyModeRegistry};
use wifi_densepose_geo::types::GeoRegistration;
use wifi_densepose_ruvector::viewpoint::coherence::ClockQualityScore;
use wifi_densepose_signal::ruvsense::fusion_quality::CalibrationId;
use wifi_densepose_signal::ruvsense::multistatic::{MultistaticConfig, MultistaticFuser};
use wifi_densepose_signal::ruvsense::{
    ArrayCoordinator, ArrayCoordinatorConfig, ArrayNodeInput, ChangePoint, DirectionalEvidence,
    EvolutionTracker, MultiBandCsiFrame, QualityScore, ReflectorObservation, RfSlam,
};
use wifi_densepose_worldgraph::{
    AnchorKind, EnuPoint, PrivacyRollup, SemanticProvenance, WorldEdge, WorldGraph, WorldGraphError,
    WorldId, WorldNode, ZoneBoundsEnu,
};

/// Errors from an engine cycle.
#[derive(Debug)]
pub enum EngineError {
    /// Multistatic fusion failed (no frames, timestamp spread, dimension mismatch).
    Fusion(wifi_densepose_signal::ruvsense::multistatic::MultistaticError),
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EngineError::Fusion(e) => write!(f, "fusion error: {e}"),
        }
    }
}
impl std::error::Error for EngineError {}
impl From<wifi_densepose_signal::ruvsense::multistatic::MultistaticError> for EngineError {
    fn from(e: wifi_densepose_signal::ruvsense::multistatic::MultistaticError) -> Self {
        EngineError::Fusion(e)
    }
}

/// Geometry of a sensing node, needed to run the ADR-138 array coordinator.
#[derive(Debug, Clone, Copy)]
struct NodeGeom {
    x: f32,
    y: f32,
    azimuth: f32,
}

/// The auditable result of one engine cycle — the trust chain made concrete.
#[derive(Debug, Clone)]
pub struct TrustedOutput {
    /// The `SemanticState` node id created in the WorldGraph.
    pub semantic_id: WorldId,
    /// The fusion quality record (evidence + contradictions + calibration).
    pub quality: QualityScore,
    /// The privacy class the output was emitted under (after any demotion).
    pub effective_class: PrivacyClass,
    /// Whether a tolerated contradiction forced a privacy demotion this cycle.
    pub demoted: bool,
    /// The mandatory provenance attached to the semantic node.
    pub provenance: SemanticProvenance,
    /// ADR-138 directional evidence, when node geometry is registered for every
    /// contributing node (else `None`).
    pub directional: Option<DirectionalEvidence>,
    /// ADR-142 cross-link change-point detected this cycle, if any (and the
    /// `Event` node it was recorded as in the WorldGraph).
    pub change_point: Option<(ChangePoint, WorldId)>,
}

/// Composition root for the RuView streaming engine.
pub struct StreamingEngine {
    fuser: MultistaticFuser,
    coherence_accept: f32,
    privacy: PrivacyModeRegistry,
    world: WorldGraph,
    model_version: u16,
    cycle: u64,
    // ADR-138: array coordinator + per-node geometry (by frame node_id).
    array: ArrayCoordinator,
    node_geom: BTreeMap<u8, NodeGeom>,
    // ADR-142: per-link evolution tracker (sized lazily to the node count).
    evolution: Option<EvolutionTracker>,
    // ADR-143: persistent reflector discovery (v2 mode).
    slam: RfSlam,
    // ADR-139 live loop: stable track_id -> PersonTrack WorldId.
    person_tracks: BTreeMap<u64, WorldId>,
}

impl StreamingEngine {
    /// Build an engine with a starting privacy mode and model version. The
    /// WorldGraph is registered to the installation origin.
    #[must_use]
    pub fn new(mode: PrivacyMode, model_version: u16, registration: GeoRegistration) -> Self {
        Self {
            fuser: MultistaticFuser::with_config(MultistaticConfig::default()),
            coherence_accept: 0.85,
            privacy: PrivacyModeRegistry::new(mode),
            world: WorldGraph::new(registration),
            model_version,
            cycle: 0,
            array: ArrayCoordinator::new(ArrayCoordinatorConfig::default()),
            node_geom: BTreeMap::new(),
            evolution: None,
            slam: RfSlam::with_discovery(0.5, 5, 0.6),
            person_tracks: BTreeMap::new(),
        }
    }

    /// ADR-139 live loop: create or update a `PersonTrack` node by stable
    /// `track_id`, locate it in `room`, and wire an `Observes` edge from
    /// `sensor` (so the privacy rollup can suppress it under identity-strict
    /// modes). Returns the (stable) WorldGraph id.
    pub fn update_person_track(
        &mut self,
        track_id: u64,
        x: f32,
        y: f32,
        room: WorldId,
        sensor: WorldId,
    ) -> WorldId {
        let existing = self.person_tracks.get(&track_id).copied();
        let node = WorldNode::PersonTrack {
            id: existing.unwrap_or(WorldId::UNASSIGNED),
            track_id,
            last_position: EnuPoint { east_m: f64::from(x), north_m: f64::from(y), up_m: 0.0 },
            reid_embedding_ref: None,
        };
        let id = self.world.upsert_node(node);
        if existing.is_none() {
            self.person_tracks.insert(track_id, id);
            let _ = self.world.add_edge(id, room, WorldEdge::LocatedIn { since_unix_ms: 0 });
            let _ = self.world.add_edge(
                sensor,
                id,
                WorldEdge::Observes { quality: 1.0, last_seen_unix_ms: 0 },
            );
        }
        id
    }

    /// ADR-139 §2.4 / ADR-141: materialise `PrivacyLimitedBy` edges for the
    /// active privacy mode. Under an identity-suppressing mode, `person_track`
    /// observations are denied; the rollup names what was suppressed.
    pub fn apply_active_privacy_mode(&mut self) -> PrivacyRollup {
        let mode = self.privacy.active_mode();
        let suppress_identity = self.privacy.is_action_enforced(PrivacyAction::SuppressIdentity);
        self.world.apply_privacy_mode(
            &format!("{mode:?}"),
            "SuppressIdentity",
            move |_sensor_kind, node_kind| !(suppress_identity && node_kind == "person_track"),
        )
    }

    /// Persist the WorldGraph as deterministic JSON (the RVF payload). Contains
    /// only graph nodes/edges — **never** raw RF frames.
    ///
    /// # Errors
    /// [`WorldGraphError`] on serialisation failure.
    pub fn snapshot_json(&self) -> Result<Vec<u8>, WorldGraphError> {
        self.world.to_json()
    }

    /// Register a contributing node's geometry (ADR-138). When every frame's
    /// `node_id` in a cycle has a registered geometry, the cycle runs the array
    /// coordinator and folds its contradictions into the privacy decision.
    pub fn register_node_geometry(&mut self, node_id: u8, x: f32, y: f32, azimuth: f32) {
        self.node_geom.insert(node_id, NodeGeom { x, y, azimuth });
    }

    /// Ingest CIR-derived reflector sightings (ADR-143) and persist any newly
    /// stable static anchors into the WorldGraph as `ObjectAnchor` nodes.
    /// Returns the WorldGraph ids written this call.
    pub fn ingest_reflectors(&mut self, observations: &[ReflectorObservation]) -> Vec<WorldId> {
        for obs in observations {
            self.slam.observe(obs);
        }
        let mut written = Vec::new();
        for (pos, class) in self.slam.static_anchors(0.05, 1.0) {
            let kind = match class {
                wifi_densepose_signal::ruvsense::ReflectorClass::Wall => AnchorKind::Reflector,
                wifi_densepose_signal::ruvsense::ReflectorClass::Furniture => AnchorKind::Furniture,
                wifi_densepose_signal::ruvsense::ReflectorClass::Mobile => continue,
            };
            let id = self.world.upsert_node(WorldNode::ObjectAnchor {
                id: WorldId::UNASSIGNED,
                position: EnuPoint { east_m: pos[0], north_m: pos[1], up_m: pos[2] },
                anchor_kind: kind,
                confidence: 0.9,
            });
            written.push(id);
        }
        written
    }

    /// Register a room and return its WorldGraph id (the observation scope).
    pub fn add_room(&mut self, area_id: &str, name: &str) -> WorldId {
        self.world.upsert_node(WorldNode::Room {
            id: WorldId::UNASSIGNED,
            area_id: Some(area_id.to_string()),
            name: name.to_string(),
            bounds_enu: ZoneBoundsEnu::Rectangle { min_e: 0.0, min_n: 0.0, max_e: 5.0, max_n: 4.0 },
            floor: 0,
        })
    }

    /// Register a sensor node and an `observes` edge to a room.
    pub fn add_sensor(&mut self, device_id: &str, room: WorldId) -> WorldId {
        let id = self.world.upsert_node(WorldNode::Sensor {
            id: WorldId::UNASSIGNED,
            device_id: device_id.to_string(),
            position: EnuPoint { east_m: 0.0, north_m: 0.0, up_m: 0.0 },
            modality: wifi_densepose_worldgraph::SensorModality::WifiCsi,
        });
        let _ = self.world.add_edge(
            id,
            room,
            WorldEdge::Observes { quality: 1.0, last_seen_unix_ms: 0 },
        );
        id
    }

    /// Switch the active privacy mode (records a hash-chained attestation).
    pub fn set_privacy_mode(&mut self, mode: PrivacyMode) {
        self.privacy.set_mode(mode);
    }

    /// Borrow the WorldGraph (for queries / persistence).
    #[must_use]
    pub fn world(&self) -> &WorldGraph {
        &self.world
    }

    /// Borrow the privacy registry (for attestation audit).
    #[must_use]
    pub fn privacy(&self) -> &PrivacyModeRegistry {
        &self.privacy
    }

    /// Cycles processed so far.
    #[must_use]
    pub fn cycle_count(&self) -> u64 {
        self.cycle
    }

    /// Run one full trust-traceable cycle (see crate docs for the steps).
    ///
    /// `calibration` is the [`CalibrationId`] the calibration stage applied to
    /// these frames (ADR-135 `BaselineCalibration::calibration_id()`); `room` is
    /// the observation scope (an existing WorldGraph Room id).
    ///
    /// # Errors
    /// [`EngineError::Fusion`] if multistatic fusion rejects the input.
    pub fn process_cycle(
        &mut self,
        node_frames: &[MultiBandCsiFrame],
        calibration: CalibrationId,
        room: WorldId,
        now_ms: i64,
    ) -> Result<TrustedOutput, EngineError> {
        // 1. Array coordination (ADR-138) — only when geometry is known for
        //    every contributing node. Its contradictions feed the privacy gate.
        let directional = self.coordinate_array(node_frames);
        let array_contradiction =
            directional.as_ref().is_some_and(|d| !d.contradictions.is_empty());

        // 2. Fuse + score (ADR-137).
        let (fused, mut quality) = self.fuser.fuse_scored(node_frames, self.coherence_accept)?;

        // 3. Stamp calibration provenance (ADR-135 → ADR-136 → ADR-137).
        quality.calibration_id = Some(calibration);

        // 4. Evolution change-point (ADR-142) over per-node mean amplitude.
        let change_point = self.track_evolution(node_frames, now_ms, room);

        // 5. Privacy control plane (ADR-141): demote on a fusion-level OR an
        //    array-level contradiction (monotonic — information only removed).
        let base_class = self.privacy.active_class();
        let demoted = quality.forces_privacy_demotion() || array_contradiction;
        let effective_class = if demoted { demote_one(base_class) } else { base_class };

        // 6. Semantic state with mandatory provenance (ADR-139/140).
        let provenance = SemanticProvenance {
            evidence: quality.evidence_refs.iter().map(|e| format!("{e:?}")).collect(),
            model_version: format!("rfenc-v{}", self.model_version),
            calibration_version: format!("cal:{:016x}", calibration.0),
            privacy_decision: format!("{:?}/{:?}", self.privacy.active_mode(), effective_class),
        };
        let statement = format!(
            "occupancy coherence={:.2} nodes={} demoted={}",
            quality.base_coherence, fused.active_nodes, demoted
        );
        let semantic_id = self.world.add_semantic_state(
            statement,
            quality.penalized_coherence(),
            now_ms,
            provenance.clone(),
            &[room],
        );

        self.cycle += 1;
        Ok(TrustedOutput {
            semantic_id,
            quality,
            effective_class,
            demoted,
            provenance,
            directional,
            change_point,
        })
    }

    /// ADR-138: build per-node array inputs and coordinate, iff every frame's
    /// `node_id` has a registered geometry. Returns `None` otherwise.
    fn coordinate_array(&self, node_frames: &[MultiBandCsiFrame]) -> Option<DirectionalEvidence> {
        if node_frames.is_empty() {
            return None;
        }
        let mut inputs = Vec::with_capacity(node_frames.len());
        for f in node_frames {
            let g = self.node_geom.get(&f.node_id)?; // bail if any node lacks geometry
            inputs.push(ArrayNodeInput {
                node_id: u32::from(f.node_id),
                position: (g.x, g.y),
                azimuth: g.azimuth,
                coherence: f.coherence,
                clock: ClockQualityScore { offset_stdev_us: 50.0, age_us: 1_000, valid: true },
                amplitude: f.channel_frames.first().map(|cf| cf.amplitude.clone()),
            });
        }
        Some(self.array.coordinate(&inputs))
    }

    /// ADR-142: fold per-node mean amplitude into the evolution tracker and,
    /// on a cross-link change-point, record an `Event` node in the WorldGraph.
    fn track_evolution(
        &mut self,
        node_frames: &[MultiBandCsiFrame],
        now_ms: i64,
        room: WorldId,
    ) -> Option<(ChangePoint, WorldId)> {
        let values: Vec<f64> = node_frames
            .iter()
            .filter_map(|f| f.channel_frames.first())
            .map(|cf| {
                if cf.amplitude.is_empty() {
                    0.0
                } else {
                    cf.amplitude.iter().map(|&a| f64::from(a)).sum::<f64>() / cf.amplitude.len() as f64
                }
            })
            .collect();
        if values.is_empty() {
            return None;
        }
        let n = values.len();
        let tracker = self
            .evolution
            .get_or_insert_with(|| EvolutionTracker::new(n, 2.0, (n / 2).max(2)));
        // Node count must be stable for the tracker to remain meaningful.
        if tracker.n_links() != n {
            return None;
        }
        let cp = tracker.observe_window(&values)?;
        let event = self.world.upsert_node(WorldNode::Event {
            id: WorldId::UNASSIGNED,
            event_type: "baseline_topology_change".to_string(),
            at_unix_ms: now_ms,
            located_in: Some(room),
        });
        let _ = self.world.add_edge(event, room, WorldEdge::LocatedIn { since_unix_ms: now_ms });
        Some((cp, event))
    }
}

/// Demote a privacy class by one step (more restrictive), clamped at `Restricted`.
/// Monotonic: information is only ever removed (ADR-120/141).
fn demote_one(c: PrivacyClass) -> PrivacyClass {
    let next = (c.as_u8() + 1).min(PrivacyClass::Restricted.as_u8());
    PrivacyClass::try_from(next).unwrap_or(PrivacyClass::Restricted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wifi_densepose_signal::hardware_norm::{CanonicalCsiFrame, HardwareType};

    fn node_frame(node_id: u8, ts_us: u64, n_sub: usize) -> MultiBandCsiFrame {
        MultiBandCsiFrame {
            node_id,
            timestamp_us: ts_us,
            channel_frames: vec![CanonicalCsiFrame {
                amplitude: (0..n_sub).map(|i| 1.0 + 0.1 * i as f32).collect(),
                phase: (0..n_sub).map(|i| i as f32 * 0.05).collect(),
                hardware_type: HardwareType::Esp32S3,
            }],
            frequencies_mhz: vec![2412],
            coherence: 0.9,
        }
    }

    fn engine() -> (StreamingEngine, WorldId) {
        let mut e = StreamingEngine::new(PrivacyMode::PrivateHome, 1, GeoRegistration::default());
        let room = e.add_room("living_room", "Living Room");
        e.add_sensor("esp32-com9", room);
        (e, room)
    }

    /// End-to-end trust invariant: a clean cycle produces a SemanticState whose
    /// provenance names evidence + model + calibration + privacy decision, and
    /// the calibration id flows from input → QualityScore → provenance.
    #[test]
    fn cycle_carries_full_provenance() {
        let (mut e, room) = engine();
        let cal = CalibrationId(0xABCD_1234);
        let frames = [node_frame(0, 1000, 56), node_frame(1, 1001, 56)];
        let out = e.process_cycle(&frames, cal, room, 10_000).unwrap();

        // Calibration flows all the way through.
        assert_eq!(out.quality.calibration_id, Some(cal));
        assert_eq!(out.provenance.calibration_version, "cal:00000000abcd1234");
        // Model + privacy provenance present.
        assert_eq!(out.provenance.model_version, "rfenc-v1");
        assert!(out.provenance.privacy_decision.starts_with("PrivateHome/"));
        // Evidence refs recorded.
        assert!(!out.provenance.evidence.is_empty());
        // Clean cycle (tight timestamps) → no demotion, stays Anonymous (PrivateHome).
        assert!(!out.demoted);
        assert_eq!(out.effective_class, PrivacyClass::Anonymous);

        // The SemanticState is in the graph with a DerivedFrom edge to the room.
        assert!(e.world().node(out.semantic_id).is_some());
        assert!(e
            .world()
            .neighbors(out.semantic_id)
            .iter()
            .any(|(to, edge)| *to == room && matches!(edge, WorldEdge::DerivedFrom { .. })));
    }

    /// A tolerated contradiction (loose timestamp spread, within the hard guard)
    /// demotes the privacy class one step — proving ADR-137 → ADR-141 wiring.
    #[test]
    fn contradiction_demotes_privacy() {
        let (mut e, room) = engine();
        let cal = CalibrationId(7);
        // 2 ms spread: within the 5 ms hard guard but above the 1 ms soft guard.
        let frames = [node_frame(0, 1000, 56), node_frame(1, 3000, 56)];
        let out = e.process_cycle(&frames, cal, room, 20_000).unwrap();

        assert!(out.demoted, "loose alignment must demote");
        // PrivateHome base = Anonymous(2) → demoted to Restricted(3).
        assert_eq!(out.effective_class, PrivacyClass::Restricted);
        assert!(out.provenance.privacy_decision.contains("Restricted"));
        // Penalized coherence is below the base coherence.
        assert!(out.quality.penalized_coherence() <= out.quality.base_coherence);
    }

    /// Determinism: identical input twice → identical provenance + class
    /// (the ADR-136 witness-replay spirit, end-to-end through the engine).
    #[test]
    fn cycle_is_deterministic() {
        let cal = CalibrationId(42);
        let frames = [node_frame(0, 1000, 56), node_frame(1, 1001, 56)];

        let (mut e1, r1) = engine();
        let o1 = e1.process_cycle(&frames, cal, r1, 5_000).unwrap();
        let (mut e2, r2) = engine();
        let o2 = e2.process_cycle(&frames, cal, r2, 5_000).unwrap();

        assert_eq!(o1.provenance.calibration_version, o2.provenance.calibration_version);
        assert_eq!(o1.provenance.evidence, o2.provenance.evidence);
        assert_eq!(o1.effective_class, o2.effective_class);
        assert_eq!(o1.quality.per_node_weights, o2.quality.per_node_weights);
    }

    fn node_frame_scaled(node_id: u8, ts_us: u64, n_sub: usize, scale: f32) -> MultiBandCsiFrame {
        MultiBandCsiFrame {
            node_id,
            timestamp_us: ts_us,
            channel_frames: vec![CanonicalCsiFrame {
                amplitude: (0..n_sub).map(|i| scale * (1.0 + 0.1 * i as f32)).collect(),
                phase: (0..n_sub).map(|i| i as f32 * 0.05).collect(),
                hardware_type: HardwareType::Esp32S3,
            }],
            frequencies_mhz: vec![2412],
            coherence: 0.9,
        }
    }

    /// ADR-138 composed: with node geometry registered, the cycle produces
    /// directional evidence (admitted nodes + weights).
    #[test]
    fn array_coordinator_runs_when_geometry_registered() {
        use std::f32::consts::PI;
        let (mut e, room) = engine();
        e.register_node_geometry(0, 1.0, 0.0, 0.0);
        e.register_node_geometry(1, -1.0, 0.0, PI); // opposite → good diversity
        let out = e
            .process_cycle(&[node_frame(0, 1000, 56), node_frame(1, 1001, 56)], CalibrationId(1), room, 1)
            .unwrap();
        let d = out.directional.expect("geometry registered → directional evidence");
        assert_eq!(d.n_admitted, 2);
        assert!((d.weights.iter().map(|(_, w)| *w).sum::<f32>() - 1.0).abs() < 1e-3);
        // Well-separated, coherent nodes → no array contradiction → no demotion.
        assert!(!out.demoted);
    }

    /// ADR-138 composed: poor geometry (near-colinear nodes) raises a
    /// GeometryInsufficient contradiction that demotes privacy.
    #[test]
    fn array_geometry_insufficient_demotes() {
        let (mut e, room) = engine();
        e.register_node_geometry(0, 1.0, 0.0, 0.0);
        e.register_node_geometry(1, 1.0, 0.01, 0.01); // nearly colinear → low GDI
        let out = e
            .process_cycle(&[node_frame(0, 1000, 56), node_frame(1, 1001, 56)], CalibrationId(1), room, 1)
            .unwrap();
        let d = out.directional.unwrap();
        assert!(!d.contradictions.is_empty(), "insufficient geometry flagged");
        assert!(out.demoted && out.effective_class == PrivacyClass::Restricted);
    }

    /// ADR-142 composed: a sustained baseline then a simultaneous amplitude
    /// shift on both links yields a change-point + an Event node in the graph.
    #[test]
    fn evolution_change_point_recorded_as_event() {
        let (mut e, room) = engine();
        let cal = CalibrationId(1);
        // Jittered baseline so each link has non-zero std (constant std=0 is undefined).
        for i in 0..30u64 {
            let s = if i % 2 == 0 { 0.99 } else { 1.01 };
            let out = e
                .process_cycle(&[node_frame_scaled(0, 1000, 56, s), node_frame_scaled(1, 1001, 56, s)], cal, room, i as i64)
                .unwrap();
            assert!(out.change_point.is_none(), "baseline must not trip a change-point");
        }
        // Large simultaneous excursion on both links → change-point.
        let out = e
            .process_cycle(&[node_frame_scaled(0, 1000, 56, 1.6), node_frame_scaled(1, 1001, 56, 1.6)], cal, room, 99)
            .unwrap();
        let (_, event_id) = out.change_point.expect("simultaneous shift → change-point");
        assert!(matches!(
            e.world().node(event_id),
            Some(WorldNode::Event { event_type, .. }) if event_type == "baseline_topology_change"
        ));
    }

    /// ADR-143 composed: ingesting stable reflector sightings writes an
    /// ObjectAnchor node into the WorldGraph.
    #[test]
    fn reflector_ingestion_writes_object_anchors() {
        use wifi_densepose_signal::ruvsense::ReflectorObservation;
        let (mut e, _room) = engine();
        let day_ns = 86_400_000_000_000u64;
        // 8 tight, coherent sightings spanning ~a day → a stable Wall anchor.
        let obs: Vec<ReflectorObservation> = (0..8u64)
            .map(|i| {
                let j = if i % 2 == 0 { 0.005 } else { -0.005 };
                ReflectorObservation { position: [3.0 + j, 1.0, 0.0], delay_ns: 12.0, coherence: 0.9, at_ns: i * (day_ns / 8) }
            })
            .collect();
        let written = e.ingest_reflectors(&obs);
        assert!(!written.is_empty(), "stable reflector → ObjectAnchor written");
        assert!(matches!(
            e.world().node(written[0]),
            Some(WorldNode::ObjectAnchor { .. })
        ));
    }

    /// ADR-139 live-loop acceptance (the architecture-proving path):
    /// `live_frame -> fusion_event -> worldgraph_update -> privacy_rollup ->
    ///  persist -> reload -> same_contents`, with NO raw RF frame persisted.
    #[test]
    fn live_frame_to_reload_same_contents() {
        let mut e =
            StreamingEngine::new(PrivacyMode::StrictNoIdentity, 1, GeoRegistration::default());
        let room = e.add_room("living_room", "Living Room");
        let sensor = e.add_sensor("esp32-com9", room);

        // live_frame -> fusion_event -> worldgraph_update (SemanticState).
        let out = e
            .process_cycle(&[node_frame(0, 1000, 56), node_frame(1, 1001, 56)], CalibrationId(9), room, 100)
            .unwrap();
        // person track feeding.
        let pt = e.update_person_track(7, 2.0, 2.0, room, sensor);

        // privacy_rollup: StrictNoIdentity suppresses the person_track.
        let rollup = e.apply_active_privacy_mode();
        assert!(rollup.suppressed_nodes.contains(&pt), "person track suppressed");
        assert!(rollup.denied_pairs.iter().any(|(_s, n)| *n == pt));

        // persist.
        let bytes = e.snapshot_json().unwrap();
        // No raw RF frame persisted — the snapshot is graph nodes/edges only.
        let json = String::from_utf8(bytes.clone()).unwrap();
        assert!(!json.contains("\"amplitude\"") && !json.contains("\"data\""), "no raw RF in snapshot");

        // reload.
        let reloaded = WorldGraph::from_json(&bytes).unwrap();

        // same_contents: node count, area resolution, the SemanticState + track,
        // and an identical room-contents query before vs after reload.
        assert_eq!(reloaded.node_count(), e.world().node_count());
        assert_eq!(reloaded.room_for_area("living_room"), e.world().room_for_area("living_room"));
        assert!(reloaded.node(out.semantic_id).is_some());
        assert!(reloaded.node(pt).is_some());
        let mut before = e.world().contents_of(room);
        before.sort_by_key(|w| w.0);
        let mut after = reloaded.contents_of(room);
        after.sort_by_key(|w| w.0);
        assert_eq!(before, after, "same room-contents query after reload");
        // Deterministic persistence: re-serialising the reload is byte-identical.
        assert_eq!(reloaded.to_json().unwrap(), bytes);
    }

    /// The privacy mode switch is recorded in a verifiable attestation chain
    /// (ADR-141), and a stricter mode raises the emitted class.
    #[test]
    fn privacy_mode_switch_is_attested_and_effective() {
        let (mut e, room) = engine();
        e.set_privacy_mode(PrivacyMode::StrictNoIdentity);
        assert!(e.privacy().verify_chain());
        let out = e
            .process_cycle(&[node_frame(0, 1000, 56), node_frame(1, 1001, 56)], CalibrationId(1), room, 1)
            .unwrap();
        // StrictNoIdentity base = Restricted, even with no contradiction.
        assert_eq!(out.effective_class, PrivacyClass::Restricted);
    }
}
