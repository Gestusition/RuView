//! Acceptance tests for ADR-122 §2.2 — `Publish` trait + `publish_event`.

#![cfg(feature = "std")]

use wifi_densepose_bfld::{
    publish_event, BfldEvent, CapturePublisher, PrivacyClass, Publish, TopicMessage,
};

fn sample_event(class: PrivacyClass, with_zone: bool) -> BfldEvent {
    BfldEvent::with_privacy_gating(
        "seed-99".into(),
        1_700_000_000_000_000_000,
        true,
        0.5,
        1,
        0.8,
        if with_zone {
            Some("kitchen".into())
        } else {
            None
        },
        class,
        Some(0.25),
        Some([0xCD; 32]),
    )
}

#[test]
fn capture_publisher_records_every_message() {
    let mut p = CapturePublisher::default();
    let count = publish_event(&mut p, &sample_event(PrivacyClass::Anonymous, true))
        .expect("publish must succeed");
    assert_eq!(
        count,
        p.published.len(),
        "return value must equal publish count"
    );
    assert_eq!(count, 6, "Anonymous + zone publishes 6 topics");
}

#[test]
fn publish_returns_zero_for_raw_and_derived_events() {
    for class in [PrivacyClass::Raw, PrivacyClass::Derived] {
        let mut p = CapturePublisher::default();
        let count = publish_event(&mut p, &sample_event(class, true)).unwrap();
        assert_eq!(count, 0, "class {class:?} must publish nothing");
        assert!(p.published.is_empty());
    }
}

#[test]
fn published_topics_match_render_events_ordering() {
    // The publish loop must iterate in the same order as render_events so
    // that downstream MQTT consumers see a stable per-event topic sequence.
    let event = sample_event(PrivacyClass::Anonymous, true);
    let mut p = CapturePublisher::default();
    publish_event(&mut p, &event).unwrap();
    let rendered = wifi_densepose_bfld::render_events(&event);
    assert_eq!(p.published, rendered);
}

#[test]
fn restricted_class_publishes_no_identity_risk_topic() {
    let mut p = CapturePublisher::default();
    publish_event(&mut p, &sample_event(PrivacyClass::Restricted, true)).unwrap();
    assert!(
        !p.published
            .iter()
            .any(|m| m.topic.contains("identity_risk")),
        "Restricted must not publish identity_risk, got: {:?}",
        p.published.iter().map(|m| &m.topic).collect::<Vec<_>>(),
    );
}

#[test]
fn anonymous_without_zone_publishes_five_messages() {
    let mut p = CapturePublisher::default();
    let count = publish_event(&mut p, &sample_event(PrivacyClass::Anonymous, false)).unwrap();
    assert_eq!(count, 5);
}

// --- error propagation --------------------------------------------------

struct FailingPublisher {
    fails_after: usize,
    published_so_far: usize,
}

impl Publish for FailingPublisher {
    type Error = &'static str;
    fn publish(&mut self, _msg: &TopicMessage) -> Result<(), Self::Error> {
        if self.published_so_far >= self.fails_after {
            return Err("broker offline");
        }
        self.published_so_far += 1;
        Ok(())
    }
}

#[test]
fn publisher_error_short_circuits_publish_event() {
    let mut p = FailingPublisher {
        fails_after: 2,
        published_so_far: 0,
    };
    let result = publish_event(&mut p, &sample_event(PrivacyClass::Anonymous, true));
    match result {
        Err("broker offline") => {}
        other => panic!("expected broker-offline error, got {other:?}"),
    }
    assert_eq!(
        p.published_so_far, 2,
        "exactly the first two messages should land before the error",
    );
}

// --- error type ergonomics ----------------------------------------------

#[test]
fn capture_publisher_error_type_is_infallible() {
    let mut p = CapturePublisher::default();
    let r: Result<usize, core::convert::Infallible> =
        publish_event(&mut p, &sample_event(PrivacyClass::Anonymous, false));
    assert!(r.is_ok());
}
