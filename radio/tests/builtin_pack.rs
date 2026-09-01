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

/// The rule the user chose: broad coverage is fine, but anything whose
/// provenance has not been checked must be off until someone turns it on.
#[test]
fn unverified_signatures_ship_disabled() {
    for sig in &pack().signatures {
        if sig.last_verified.is_none() && sig.enabled {
            // The one deliberate exception is documented in its own notes.
            assert_eq!(
                sig.id, "imsi.keyw.mas",
                "signature {} is enabled but has never been verified",
                sig.id
            );
        }
    }
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

#[test]
fn disabled_research_signatures_do_not_fire() {
    let db = pack();
    // 82:6b:f2 is present in the pack but shipped disabled.
    assert!(
        db.match_observation(&seen_from("82:6b:f2:11:22:33"))
            .is_empty()
    );
    assert!(
        db.match_observation(&seen_from("70:c9:4e:11:22:33"))
            .is_empty()
    );
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
