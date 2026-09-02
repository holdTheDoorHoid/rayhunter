//! The shipped signature pack is data, but it is data the detector's
//! credibility rests on. These tests hold it to the rules the schema cannot
//! express on its own.

use rayhunter_radio::observation::{ObservationPayload, WifiObservation};
use rayhunter_radio::signature::{Confidence, MatchCondition, Severity, SignatureDb};
use rayhunter_radio::{MacAddr, RadioTech};

const PACK: &str = include_str!("../signatures/builtin-surveillance-signatures.json");

fn pack() -> SignatureDb {
    SignatureDb::from_json(PACK).expect("builtin signature pack must parse")
}

fn mac(s: &str) -> MacAddr {
    MacAddr::parse(s).unwrap()
}

fn seen_from(addr: &str) -> ObservationPayload {
    let mut obs = WifiObservation::empty();
    obs.bssid = Some(mac(addr));
    obs.transmitter = Some(mac(addr));
    ObservationPayload::Wifi(obs)
}

#[test]
fn pack_parses_and_validates() {
    let db = pack();
    assert!(!db.signatures.is_empty());
    assert!(db.pack_version.is_some());
}

#[test]
fn every_signature_records_where_it_came_from() {
    for sig in &pack().signatures {
        assert!(
            !sig.evidence.is_empty(),
            "signature {} ships without provenance",
            sig.id
        );
        assert!(
            !sig.description.trim().is_empty(),
            "signature {} ships without a description",
            sig.id
        );
    }
}

/// Everything now ships enabled, at the user's request, so the safeguard moves
/// from "off by default" to "honestly labelled": anything whose provenance has
/// not been checked must still say so, and must carry a note explaining why.
#[test]
fn unverified_signatures_are_labelled_even_though_they_ship_enabled() {
    for sig in &pack().signatures {
        if sig.last_verified.is_none() {
            assert!(
                sig.notes.is_some(),
                "signature {} has never been verified and carries no note saying so",
                sig.id
            );
        }
    }
}

/// A rule that cannot fire on the only capture method this hardware has is
/// worse than one that is off: it looks like coverage. The pack may contain
/// them - they are ready for a platform that can capture probe requests - but
/// the UI is told which they are, so this asserts the derivation works rather
/// than that no such rules exist.
#[test]
fn rules_needing_monitor_mode_are_identifiable() {
    let pack = pack();
    let unreachable: Vec<&str> = pack
        .signatures
        .iter()
        .filter(|s| !s.reachable_via_bss_scan())
        .map(|s| s.id.as_str())
        .collect();

    // The two probe-request Flock rules cannot fire without monitor mode.
    assert!(
        unreachable.contains(&"research.flock.nitekry.wildcard-probe"),
        "got {unreachable:?}"
    );
    assert!(
        unreachable.contains(&"research.flock.ie-fingerprint"),
        "got {unreachable:?}"
    );

    // The plain vendor-prefix rules must remain reachable, or the whole panel
    // would show nothing.
    let reachable: Vec<&str> = pack
        .signatures
        .iter()
        .filter(|s| s.reachable_via_bss_scan())
        .map(|s| s.id.as_str())
        .collect();
    assert!(reachable.contains(&"camera.flock.oui"), "got {reachable:?}");
    assert!(reachable.contains(&"imsi.keyw.mas"), "got {reachable:?}");
}

/// A prefix rule on its own can never be more than corroborating evidence,
/// however trustworthy the vendor attribution is.
#[test]
fn single_prefix_signatures_are_never_more_than_informational() {
    for sig in &pack().signatures {
        let only_a_prefix = sig.conditions.len() == 1
            && matches!(sig.conditions[0], MatchCondition::MacPrefix { .. });
        if only_a_prefix {
            assert_eq!(
                sig.confidence,
                Confidence::Info,
                "signature {} claims more than informational confidence from a prefix alone",
                sig.id
            );
            assert_eq!(
                sig.severity,
                Severity::Informational,
                "signature {} raises severity from a prefix alone",
                sig.id
            );
        }
    }
}

/// High confidence has to be earned by more than one independent signal.
#[test]
fn high_confidence_requires_a_composite_signature() {
    for sig in &pack().signatures {
        if sig.confidence == Confidence::High {
            assert!(
                sig.conditions.len() >= 2,
                "signature {} claims high confidence from a single condition",
                sig.id
            );
        }
    }
}

#[test]
fn keyw_matches_only_its_own_nibble_block() {
    let db = pack();

    let inside = db.match_observation(&seen_from("70:b3:d5:7c:b4:01"));
    assert_eq!(inside.len(), 1);
    assert_eq!(inside[0].signature_id, "imsi.keyw.mas");
    assert_eq!(inside[0].confidence, Confidence::Info);

    // Same IEEE Registration Authority parent block, a different assignee.
    assert!(
        db.match_observation(&seen_from("70:b3:d5:7c:a4:01"))
            .is_empty()
    );
    assert!(
        db.match_observation(&seen_from("70:b3:d5:11:22:33"))
            .is_empty()
    );
}

#[test]
fn vendors_with_confusable_names_are_not_matched() {
    let db = pack();
    // Axon Enterprise (the body-camera vendor) must match.
    let real = db.match_observation(&seen_from("00:25:df:11:22:33"));
    assert_eq!(real.len(), 1);
    assert_eq!(real[0].vendor, "Axon Enterprise, Inc.");

    // "Axon Networks Inc." and "Axonne Inc." are unrelated companies.
    assert!(
        db.match_observation(&seen_from("00:58:28:11:22:33"))
            .is_empty()
    );
    assert!(
        db.match_observation(&seen_from("00:c0:d4:11:22:33"))
            .is_empty()
    );
    assert!(
        db.match_observation(&seen_from("fc:85:96:11:22:33"))
            .is_empty()
    );
}

#[test]
fn drone_vendors_are_labelled_as_drones_not_surveillance() {
    let db = pack();
    for addr in [
        "48:1c:b9:11:22:33",
        "38:1d:14:11:22:33",
        "90:3a:e6:11:22:33",
    ] {
        let hits = db.match_observation(&seen_from(addr));
        assert_eq!(hits.len(), 1, "expected exactly one match for {addr}");
        assert_eq!(
            hits[0].product.as_deref(),
            Some("drone"),
            "{addr} should be categorised as a drone"
        );
        assert_eq!(hits[0].severity, Severity::Informational);
    }
}

/// The research prefixes now ship enabled, so they should match. This is the
/// user's deliberate choice of broad coverage; the safeguard is that they are
/// labelled unverified and capped at informational confidence.
#[test]
fn enabled_research_prefixes_now_match_but_stay_weak() {
    let db = pack();
    let hits = db.match_observation(&seen_from("82:6b:f2:11:22:33"));
    assert_eq!(
        hits.len(),
        1,
        "the locally-administered Flock prefix should match"
    );
    assert_eq!(hits[0].confidence, Confidence::Info);
    assert_eq!(hits[0].severity, Severity::Informational);
}

#[test]
fn an_ordinary_access_point_matches_nothing() {
    let db = pack();
    // The Verizon-issued router observed during hardware testing.
    assert!(
        db.match_observation(&seen_from("3c:52:a1:fe:c9:8b"))
            .is_empty()
    );
    // The Orbic's own hotspot.
    assert!(
        db.match_observation(&seen_from("58:32:77:28:7b:a6"))
            .is_empty()
    );
}

#[test]
fn every_signature_targets_a_declared_technology() {
    for sig in &pack().signatures {
        // The RC400L cannot supply BLE, so a BLE signature in the builtin pack
        // would be dead weight that only looks like coverage.
        assert_eq!(
            sig.technology,
            RadioTech::Wifi,
            "signature {} targets {:?}, which no supported capture source provides yet",
            sig.id,
            sig.technology
        );
    }
}
