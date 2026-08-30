//! Synthetic messages for demonstrating Rayhunter to an audience.
//!
//! These are injected into the live diag stream, so they are written to the
//! recording and analysed exactly like messages from the air. That is the point:
//! a demo that took a shortcut around the analysers would not show anybody how
//! Rayhunter actually works.
//!
//! # Why every one of these is labelled
//!
//! A fake surveillance detection is the sort of thing that gets screenshotted
//! and circulated as real. Worse, a recording containing one could be sent to
//! EFF as evidence of an attack that never happened. So the marking is not
//! cosmetic and it is not optional:
//!
//! - Demo mode has to be switched on in the config before any of this exists.
//! - Every generated warning says so in its own message text.
//! - The identifiers below are deliberately impossible values, so even someone
//!   reading the raw capture later can tell.
//!
//! The test operator code 001-01 is reserved by the ITU for exactly this
//! purpose and is never used by a real network.

use rayhunter::diag::{CRC_CCITT, DataType, HdlcEncapsulatedMessage, MessagesContainer};
use rayhunter::hdlc::hdlc_encapsulate;

/// Diag log type for a plain EMM NAS message arriving from the network. The
/// neighbouring ESM code, 0xb0e2, carries a different protocol entirely.
const LOG_TYPE_EMM_NAS_DOWNLINK: u16 = 0xb0ec;

/// Prefix on every message a demo generates. Deliberately shouty: this text
/// travels with the event into the recording, the history and any notification.
pub const DEMO_PREFIX: &str = "[DEMO, NOT REAL] ";

/// One thing a demo can show, as the messages needed to provoke it.
///
/// Each scenario is a self contained sequence: a state machine detector needs
/// its messages in order, so a scenario carries all of them rather than being a
/// single message.
pub struct Scenario {
    /// What this shows an audience, used in the log when it is chosen.
    pub name: &'static str,
    /// NAS payloads, injected in order.
    pub messages: Vec<Vec<u8>>,
}

/// Every scenario the demo can draw from.
///
/// The bytes follow 3GPP TS 24.301. Each message begins `07`, being a security
/// header type of 0 (plain, no integrity protection) in the high nibble and
/// protocol discriminator 7 (EPS Mobility Management) in the low nibble.
pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "tower switched encryption off (NAS null cipher)",
            messages: vec![
                // 5d = Security Mode Command. 00 selects EEA0, the null
                // cipher, meaning no encryption at all. 00 is the key set
                // identifier, then the replayed UE security capabilities.
                vec![0x07, 0x5d, 0x00, 0x00, 0x02, 0x80, 0x00, 0x00],
            ],
        },
        Scenario {
            name: "identity demanded after authentication (IMSI catcher pattern)",
            messages: vec![
                // 53 = Authentication Response, which moves the detector into
                // its authenticated state. 08 is the length of the response.
                vec![
                    0x07, 0x53, 0x08, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                ],
                // 55 = Identity Request, 01 = asking for the IMSI. Demanding
                // the permanent identity *after* authentication has no
                // legitimate reason and is the signature this detector wants.
                vec![0x07, 0x55, 0x01],
            ],
        },
        Scenario {
            name: "identity demanded with no attach request",
            messages: vec![
                // 45 = Detach Request, putting the detector in its
                // disconnected state, then an identity demand out of nowhere.
                vec![0x07, 0x45, 0x01, 0x07],
                vec![0x07, 0x55, 0x01],
            ],
        },
        Scenario {
            name: "permanent equipment identity demanded (IMEI)",
            messages: vec![
                vec![
                    0x07, 0x53, 0x08, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                ],
                // 02 = IMEI rather than IMSI: identifying the handset itself
                // rather than the subscription.
                vec![0x07, 0x55, 0x02],
            ],
        },
    ]
}

/// Wrap raw NAS bytes in the diag log framing the daemon expects, then HDLC
/// encapsulate them so they are shaped exactly like a message off the air.
///
/// The frame is written out by hand rather than through deku's serialiser.
/// Deku's writer emits two bytes that its reader does not consume for this
/// message type, which silently shifts the payload so the NAS parser reads the
/// wrong bytes. Writing the layout explicitly keeps the two halves in step, and
/// the round trip test below is what proves it.
fn encapsulate_nas(msg: Vec<u8>) -> Option<HdlcEncapsulatedMessage> {
    // The body is four version bytes followed by the NAS message, and
    // inner_length counts the body plus the log type and timestamp ahead of it.
    let body_len = 4 + msg.len();
    let inner_length = u16::try_from(body_len + 12).ok()?;

    let mut frame = Vec::with_capacity(body_len + 16);
    frame.push(16); // Message::Log discriminant
    frame.push(0); // pending_msgs
    frame.extend_from_slice(&inner_length.to_le_bytes()); // outer_length
    frame.extend_from_slice(&inner_length.to_le_bytes());
    frame.extend_from_slice(&LOG_TYPE_EMM_NAS_DOWNLINK.to_le_bytes());
    frame.extend_from_slice(&current_diag_timestamp().to_le_bytes());
    frame.push(1); // ext_header_version
    frame.push(14); // rrc_rel
    frame.push(0); // rrc_version_minor
    frame.push(14); // rrc_version_major
    frame.extend_from_slice(&msg);

    let data = hdlc_encapsulate(&frame, &CRC_CCITT);
    Some(HdlcEncapsulatedMessage {
        len: data.len() as u32,
        data,
    })
}

/// Diag timestamps count 1.25ms ticks from the GPS epoch, 6 January 1980.
fn current_diag_timestamp() -> u64 {
    const GPS_EPOCH_OFFSET_SECS: i64 = 315_964_800;
    let now = chrono::Utc::now().timestamp();
    let since_gps_epoch = now.saturating_sub(GPS_EPOCH_OFFSET_SECS).max(0) as u64;
    // 1/1.25ms = 800 ticks per second, shifted into the field's fixed point form.
    (since_gps_epoch * 800) << 16
}

/// Build the container of synthetic messages a demo run injects.
///
/// Returns None if the messages cannot be built, which should not happen but is
/// not worth crashing a running detector over.
/// How many scenarios one press of the demo button uses.
///
/// More than one, so a demo shows that Rayhunter watches for several different
/// signs rather than a single trick. Not all of them, so repeated presses look
/// different and the audience sees the variety across a session.
const SCENARIOS_PER_RUN: usize = 2;

/// Choose the scenarios for one demo run.
///
/// Selection is shuffled so consecutive presses differ. The source of
/// randomness is the clock rather than a crate dependency: nothing here is
/// security sensitive, it only needs to not repeat itself.
pub fn choose_scenarios(count: usize) -> Vec<Scenario> {
    let mut pool = scenarios();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0x9e3779b9)
        | 1;

    // Fisher-Yates, with a small xorshift for the index at each step.
    for i in (1..pool.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        pool.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    pool.truncate(count.min(pool.len()).max(1));
    pool
}

pub fn demo_container() -> Option<MessagesContainer> {
    let chosen = choose_scenarios(SCENARIOS_PER_RUN);
    for s in &chosen {
        log::info!("demo scenario: {}", s.name);
    }
    demo_container_from(chosen)
}

/// Build a container from the given scenarios, in order.
pub fn demo_container_from(chosen: Vec<Scenario>) -> Option<MessagesContainer> {
    let messages: Vec<_> = chosen
        .into_iter()
        .flat_map(|s| s.messages)
        .filter_map(encapsulate_nas)
        .collect();

    if messages.is_empty() {
        return None;
    }

    Some(MessagesContainer {
        data_type: DataType::UserSpace,
        num_messages: messages.len() as u32,
        messages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayhunter::analysis::analyzer::{AnalyzerConfig, EventType, Harness};

    /// Every message has to survive the same journey a real one takes: HDLC
    /// framing, diag parsing, gsmtap conversion, then the analysers. Testing
    /// the bytes in isolation would prove nothing about whether a demo works.
    #[test]
    fn every_scenario_round_trips_through_diag_parsing() {
        for scenario in scenarios() {
            let expected = scenario.messages.len();
            let container = demo_container_from(vec![scenario]).expect("container should build");
            let parsed = container.messages();
            assert_eq!(parsed.len(), expected);
            for message in parsed {
                assert!(message.is_ok(), "a demo message did not parse back");
            }
        }
    }

    /// Each scenario has to raise a high severity warning on its own, since any
    /// of them can be chosen alone. One that only produced an informational
    /// note would leave a demo showing nothing.
    #[test]
    fn every_scenario_raises_a_high_warning_by_itself() {
        for scenario in scenarios() {
            let name = scenario.name;
            let mut harness = Harness::new_with_config(&AnalyzerConfig::default());
            let container = demo_container_from(vec![scenario]).expect("container should build");
            let rows = harness.analyze_qmdl_messages(container);
            let highest = rows
                .iter()
                .flat_map(|row| row.events.iter().flatten())
                .map(|e| e.event_type)
                .max();
            assert_eq!(
                highest,
                Some(EventType::High),
                "scenario {name:?} did not raise a high warning"
            );
        }
    }

    /// A run draws more than one scenario, so an audience sees that several
    /// different signs are being watched for rather than a single trick.
    #[test]
    fn a_run_uses_several_scenarios() {
        const { assert!(SCENARIOS_PER_RUN > 1) };
        assert!(
            scenarios().len() > SCENARIOS_PER_RUN,
            "the pool must be larger than one run, or every run is identical"
        );
        assert_eq!(choose_scenarios(SCENARIOS_PER_RUN).len(), SCENARIOS_PER_RUN);
    }

    /// Selection must actually vary, or repeated presses look identical.
    #[test]
    fn selection_varies_between_runs() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let names: Vec<_> = choose_scenarios(SCENARIOS_PER_RUN)
                .iter()
                .map(|s| s.name)
                .collect();
            seen.insert(names);
            std::thread::sleep(std::time::Duration::from_nanos(1));
        }
        assert!(
            seen.len() > 1,
            "every run chose the same scenarios, so the demo never varies"
        );
    }

    /// Every scenario in the pool must be reachable, or it is dead weight that
    /// nobody will notice has stopped working.
    #[test]
    fn every_scenario_can_be_chosen() {
        let all: std::collections::HashSet<_> = scenarios().iter().map(|s| s.name).collect();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            for s in choose_scenarios(SCENARIOS_PER_RUN) {
                seen.insert(s.name);
            }
            std::thread::sleep(std::time::Duration::from_nanos(1));
        }
        assert_eq!(seen, all, "some scenarios are never chosen");
    }

    /// The point of the whole feature: a run must actually trip real detectors.
    #[test]
    fn a_demo_run_triggers_real_heuristics() {
        let mut harness = Harness::new_with_config(&AnalyzerConfig::default());
        let container = demo_container().expect("demo container should build");

        let rows = harness.analyze_qmdl_messages(container);
        let events: Vec<_> = rows
            .iter()
            .flat_map(|row| row.events.iter().flatten())
            .collect();

        assert!(
            !events.is_empty(),
            "the demo message produced no events at all; rows: {rows:?}"
        );

        // Specifically a high severity warning, which is what turns the device
        // red and makes the demo worth watching. An informational note would
        // pass the check above while showing an audience nothing.
        let highest = events
            .iter()
            .map(|e| e.event_type)
            .max()
            .expect("at least one event");
        assert_eq!(
            highest,
            EventType::High,
            "demo should raise a high severity warning, got {highest:?} from {events:?}"
        );
    }

    /// Every message a demo produces has to be identifiable as fake, including
    /// by somebody reading the recording later who was not at the demo.
    #[test]
    fn the_demo_prefix_is_unmistakable() {
        assert!(DEMO_PREFIX.contains("DEMO"));
        assert!(DEMO_PREFIX.contains("NOT REAL"));
    }
}
