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

/// Diag log type for an LTE RRC message received over the air.
const LOG_TYPE_LTE_RRC_OTA: u16 = 0xb0c0;

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
    /// The messages, injected in order.
    pub messages: Vec<DemoMessage>,
    /// Whether this scenario raises a high severity warning on its own.
    ///
    /// A demo exists to turn the device red in front of an audience, so every
    /// run has to contain at least one of these. Kept as a flag rather than
    /// worked out at run time because choosing the scenarios must not mean
    /// running the analysers first; `the_high_severity_flags_are_true` is what
    /// stops it drifting from what the detectors actually do.
    pub raises_high: bool,
}

/// A single synthetic message, and which protocol layer it belongs to.
#[derive(Clone)]
pub enum DemoMessage {
    /// A NAS message, carried in an EMM log record.
    Nas(Vec<u8>),
    /// An RRC message on a logical channel. `pdu_num` 2 is the broadcast
    /// channel and 7 is the dedicated downlink one.
    Rrc { payload: Vec<u8>, pdu_num: u8 },
    /// A 2G (GSM) Layer 3 signalling message, carried in a GSM RR log record.
    Gsm(Vec<u8>),
    /// A random access response, carrying the timing advance a tower reported.
    MacRach { timing_advance: u16 },
}

/// Every scenario the demo can draw from.
///
/// The bytes follow 3GPP TS 24.301. Each message begins `07`, being a security
/// header type of 0 (plain, no integrity protection) in the high nibble and
/// protocol discriminator 7 (EPS Mobility Management) in the low nibble.
pub fn scenarios() -> Vec<Scenario> {
    // NAS bytes follow 3GPP TS 24.301. Each begins `07`: security header type
    // 0 (plain) in the high nibble, protocol discriminator 7 (EPS Mobility
    // Management) in the low nibble.
    //
    // RRC payloads are UPER encoded, derived from the ASN.1 choice indices in
    // telcom-parser rather than captured, so they contain no real network's
    // identifiers.
    vec![
        Scenario {
            name: "tower switched encryption off (NAS null cipher)",
            messages: vec![DemoMessage::Nas(vec![
                // 5d = Security Mode Command. 00 selects EEA0, the null
                // cipher, meaning no encryption at all.
                0x07, 0x5d, 0x00, 0x00, 0x02, 0x80, 0x00, 0x00,
            ])],
            raises_high: true,
        },
        Scenario {
            name: "tower took the identity, then failed authentication (FlashCatch)",
            messages: {
                // 48 = Tracking Area Update Request, the phone checking in:
                // update type 0, key set 0, then its old GUTI (f6) on the test
                // network 001-01. 55 = Identity Request, 01 asks for the IMSI.
                // 52 = Authentication Request: key set 0, a 16-byte RAND, then
                // a 16-byte AUTN whose signature the phone cannot verify.
                // 5c 14 = Authentication Failure, cause 20, "MAC failure": the
                // phone rejecting the challenge as forged. Three rounds, as in
                // the attack.
                let tau_request = vec![
                    0x07, 0x48, 0x00, 0x0b, 0xf6, 0x00, 0xf1, 0x10, 0x00, 0x01, 0x01, 0xc0, 0x00,
                    0x00, 0x01,
                ];
                let identity_request = vec![0x07, 0x55, 0x01];
                let mut auth_request = vec![0x07, 0x52, 0x00];
                auth_request.extend([0x11; 16]);
                auth_request.push(0x10);
                auth_request.extend([0x22; 16]);
                let auth_failure = vec![0x07, 0x5c, 0x14];
                let mut messages = vec![
                    DemoMessage::Nas(tau_request),
                    DemoMessage::Nas(identity_request),
                ];
                for _ in 0..3 {
                    messages.push(DemoMessage::Nas(auth_request.clone()));
                    messages.push(DemoMessage::Nas(auth_failure.clone()));
                }
                messages
            },
            raises_high: true,
        },
        Scenario {
            name: "tower switched encryption off (RRC null cipher)",
            messages: vec![DemoMessage::Rrc {
                // DL-DCCH securityModeCommand selecting EEA0. Bit by bit:
                // 0 picks the c1 branch, 0110 picks securityModeCommand
                // (index 6), then the transaction id, the r8 critical
                // extension, and finally the ciphering algorithm as 000,
                // which is EEA0, no encryption.
                payload: vec![0x30, 0x00, 0x10],
                pdu_num: 7,
            }],
            raises_high: true,
        },
        Scenario {
            name: "pushed down onto a 2G network (connection release redirect)",
            messages: vec![DemoMessage::Rrc {
                // DL-DCCH rrcConnectionRelease carrying redirectedCarrierInfo
                // set to geran, which is a tower handing the phone off to 2G.
                // 0 picks c1, 0101 picks rrcConnectionRelease (index 5), the
                // optional preamble 100 marks redirectedCarrierInfo present,
                // and 001 inside it selects geran over the other targets.
                payload: vec![0x28, 0x22, 0x20, 0x00, 0x00],
                pdu_num: 7,
            }],
            raises_high: true,
        },
        Scenario {
            name: "2G advertised as a better choice than nearby 4G (SIB downgrade)",
            messages: vec![
                // Three broadcasts in order, because this detector compares
                // priorities and only decides when it sees a SIB1.
                //
                // SIB3 sets the LTE reselection priority to 1.
                DemoMessage::Rrc {
                    payload: vec![0x00, 0x04, 0x00, 0x08, 0x00, 0x00],
                    pdu_num: 2,
                },
                // SIB7 advertises a 2G carrier at priority 7, the highest
                // there is, so 2G outranks the LTE neighbours.
                DemoMessage::Rrc {
                    payload: vec![0x00, 0x14, 0x80, 0x00, 0x00, 0x17, 0x00, 0x00, 0x00],
                    pdu_num: 2,
                },
                // The SIB1 that makes the detector compare the two. It also
                // carries only one further scheduling entry, which is what the
                // Incomplete SIB detector looks for. Its network identity is
                // 001-01, the code the ITU reserves for testing, so it can
                // never collide with a real operator.
                DemoMessage::Rrc {
                    payload: vec![
                        0x40, 0x40, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                        0x00, 0x00, 0x00,
                    ],
                    pdu_num: 2,
                },
            ],
            raises_high: true,
        },
        Scenario {
            name: "identity demanded after authentication (IMSI catcher pattern)",
            messages: vec![
                // 53 = Authentication Response, moving the detector into its
                // authenticated state. 08 is the length of the response.
                DemoMessage::Nas(vec![
                    0x07, 0x53, 0x08, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                ]),
                // 55 = Identity Request, 01 = the IMSI. Demanding the
                // permanent identity after authentication has no legitimate
                // reason and is the signature this detector wants.
                DemoMessage::Nas(vec![0x07, 0x55, 0x01]),
            ],
            raises_high: true,
        },
        Scenario {
            name: "identity demanded with no attach request",
            messages: vec![
                // 45 = Detach Request, putting the detector in its
                // disconnected state, then a demand out of nowhere.
                DemoMessage::Nas(vec![0x07, 0x45, 0x01, 0x07]),
                DemoMessage::Nas(vec![0x07, 0x55, 0x01]),
            ],
            raises_high: true,
        },
        Scenario {
            name: "network set up continuous location tracking (LPP)",
            messages: vec![DemoMessage::Nas(vec![
                // 68 = Downlink Generic NAS Transport, container type 01 (LPP),
                // then a two byte length and the LPP message itself: a
                // requestLocationInformation for transaction 5 that asks for
                // PERIODIC reporting by cell ID (E-CID) — the continuous-
                // tracking signature the depth analyzer raises to medium, and a
                // named method so the demo shows the method breakdown too. The
                // seven LPP bytes are the reference periodic+ecid vector from
                // lib/src/analysis/lpp.rs. The endTransaction bit is set (0x0b),
                // so each press is a fresh, self-contained transaction rather
                // than folding into the last.
                0x07, 0x68, 0x01, 0x00, 0x07, 0x90, 0x0b, 0x20, 0x48, 0x80, 0x18, 0x70,
            ])],
            raises_high: false,
        },
        Scenario {
            name: "2G network asked the device for its location (RRLP)",
            messages: vec![DemoMessage::Gsm(vec![
                // A GSM RR APPLICATION INFORMATION message (message type 0x38,
                // protocol discriminator 6) carrying an RRLP measure-position
                // request. Bytes are the reference frame from
                // lib/src/analysis/rrlp.rs, encoded by pycrate_mobile (44.018)
                // around a pycrate RRLP APDU (44.031): APDU ID 0 (RRLP), a
                // four byte APDU whose component is msrPositionReq.
                0x06, 0x38, 0x00, 0x04, 0x60, 0x00, 0xbc, 0x68,
            ])],
            raises_high: false,
        },
        Scenario {
            name: "a cell answered from a different distance (timing advance)",
            messages: vec![
                // The SIB1 first, so the random access responses below can be
                // attributed to a cell. Without knowing which cell answered, a
                // change in distance means nothing: two towers at different
                // distances are not one tower that moved.
                DemoMessage::Rrc {
                    payload: vec![
                        0x40, 0x40, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                        0x00, 0x00, 0x00,
                    ],
                    pdu_num: 2,
                },
                // Two agreeing responses establish what this cell reports.
                DemoMessage::MacRach { timing_advance: 10 },
                DemoMessage::MacRach { timing_advance: 10 },
                // Then the same cell answers from about six kilometres further
                // away, which a tower cannot do.
                DemoMessage::MacRach { timing_advance: 90 },
            ],
            raises_high: false,
        },
        Scenario {
            name: "permanent equipment identity demanded (IMEI)",
            messages: vec![
                DemoMessage::Nas(vec![
                    0x07, 0x53, 0x08, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                ]),
                // 02 = IMEI rather than IMSI: identifying the handset itself
                // rather than the subscription.
                DemoMessage::Nas(vec![0x07, 0x55, 0x02]),
            ],
            raises_high: true,
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

/// Wrap a raw RRC payload the same way, for the messages that arrive over the
/// air interface rather than as NAS. `pdu_num` selects the logical channel:
/// 2 is BCCH-DL-SCH (the broadcasts) and 6 is DL-DCCH (dedicated downlink).
#[cfg(test)]
pub fn rrc_container(payload: &[u8], pdu_num: u8) -> Option<MessagesContainer> {
    let msg = encapsulate_rrc(payload, pdu_num)?;
    Some(MessagesContainer {
        data_type: DataType::UserSpace,
        num_messages: 1,
        messages: vec![msg],
    })
}

fn encapsulate_rrc(payload: &[u8], pdu_num: u8) -> Option<HdlcEncapsulatedMessage> {
    // LteRrcOtaPacket::V8, selected by ext_header_version 20. Field widths
    // matter: earfcn and sib_mask are both 32 bit, and getting either wrong
    // shifts everything after it so the payload is never seen.
    let len = u16::try_from(payload.len()).ok()?;
    let inner_length = u16::try_from(31 + payload.len()).ok()?;

    let mut frame = Vec::with_capacity(payload.len() + 32);
    frame.push(16); // Message::Log
    frame.push(0); // pending_msgs
    frame.extend_from_slice(&inner_length.to_le_bytes());
    frame.extend_from_slice(&inner_length.to_le_bytes());
    frame.extend_from_slice(&LOG_TYPE_LTE_RRC_OTA.to_le_bytes());
    frame.extend_from_slice(&current_diag_timestamp().to_le_bytes());
    frame.push(20); // ext_header_version, selecting the V8 layout
    frame.push(14); // rrc_rel_maj
    frame.push(48); // rrc_rel_min
    frame.push(0); // bearer_id
    frame.extend_from_slice(&160u16.to_le_bytes()); // phy_cell_id
    frame.extend_from_slice(&2050u32.to_le_bytes()); // earfcn
    frame.extend_from_slice(&4057u16.to_le_bytes()); // sfn_subfn
    frame.push(pdu_num);
    frame.extend_from_slice(&0u32.to_le_bytes()); // sib_mask
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(payload);

    let data = hdlc_encapsulate(&frame, &CRC_CCITT);
    Some(HdlcEncapsulatedMessage {
        len: data.len() as u32,
        data,
    })
}

/// Diag log type for a 2G GSM Radio Resource signalling message.
const LOG_TYPE_GSM_RR_SIGNALLING: u16 = 0x512f;

/// SDCCH channel type, as Qualcomm's diag numbers it (`log_codes::SDCCH`). The
/// gsmtap layer maps it to the SDCCH logical channel.
const GSM_CHANNEL_SDCCH: u8 = 0x05;

/// Wrap raw GSM Layer 3 bytes in a GSM RR signalling log record, the way a 2G
/// message arrives. The body is channel type, a diag message-type byte the
/// parser ignores, a length, then the Layer 3 message itself.
const LOG_TYPE_LTE_MAC_RACH_RESPONSE: u16 = 0xb062;

/// A random access response reporting a given timing advance.
///
/// Built by patching one field of a real captured packet rather than composing
/// the structure by hand. The layout is version dependent and fiddly, and the
/// base packet is the same one the parser is tested against, so the demo
/// exercises exactly the shape a device really produces.
///
/// The two timing advance bytes sit at offset 21 within the packet: four bytes
/// of packet header, four of subpacket header, four of attempt header, four of
/// msg1, then msg2's backoff (2) and result (1) and tc-rnti (2).
fn mac_rach_packet_with_ta(timing_advance: u16) -> Vec<u8> {
    const TA_OFFSET: usize = 21;
    let mut packet: Vec<u8> = vec![
        0x01, 0x01, 0xa0, 0x69, 0x06, 0x02, 0x24, 0x00, 0x01, 0x00, 0x01, 0x07, 0x1b, 0xff, 0x98,
        0xff, 0x00, 0x00, 0x01, 0x23, 0x1a, 0x04, 0x00, 0x18, 0x1c, 0x01, 0x00, 0x07, 0x00, 0x06,
        0x00, 0x46, 0x5c, 0x80, 0xbd, 0x06, 0x48, 0x00, 0x00, 0x00,
    ];
    packet[TA_OFFSET..TA_OFFSET + 2].copy_from_slice(&timing_advance.to_le_bytes());
    packet
}

/// Wrap a random access response in the diag log framing, as for the others.
fn encapsulate_mac_rach(timing_advance: u16) -> Option<HdlcEncapsulatedMessage> {
    let body = mac_rach_packet_with_ta(timing_advance);
    let inner_length = u16::try_from(body.len() + 12).ok()?;

    let mut frame = Vec::with_capacity(body.len() + 16);
    frame.push(16); // Message::Log discriminant
    frame.push(0); // pending_msgs
    frame.extend_from_slice(&inner_length.to_le_bytes()); // outer_length
    frame.extend_from_slice(&inner_length.to_le_bytes());
    frame.extend_from_slice(&LOG_TYPE_LTE_MAC_RACH_RESPONSE.to_le_bytes());
    frame.extend_from_slice(&current_diag_timestamp().to_le_bytes());
    frame.extend_from_slice(&body);

    let data = hdlc_encapsulate(&frame, &CRC_CCITT);
    Some(HdlcEncapsulatedMessage {
        len: data.len() as u32,
        data,
    })
}

fn encapsulate_gsm(msg: &[u8]) -> Option<HdlcEncapsulatedMessage> {
    let length = u8::try_from(msg.len()).ok()?;
    // Body: channel_type + message_type + length + msg.
    let body_len = 3 + msg.len();
    // inner_length counts the body plus the log type and timestamp ahead of it,
    // the same twelve-byte offset the NAS and RRC records use.
    let inner_length = u16::try_from(body_len + 12).ok()?;

    let mut frame = Vec::with_capacity(body_len + 16);
    frame.push(16); // Message::Log discriminant
    frame.push(0); // pending_msgs
    frame.extend_from_slice(&inner_length.to_le_bytes()); // outer_length
    frame.extend_from_slice(&inner_length.to_le_bytes());
    frame.extend_from_slice(&LOG_TYPE_GSM_RR_SIGNALLING.to_le_bytes());
    frame.extend_from_slice(&current_diag_timestamp().to_le_bytes());
    frame.push(GSM_CHANNEL_SDCCH); // channel_type
    frame.push(0); // message_type, unused by the parser for this record
    frame.push(length);
    frame.extend_from_slice(msg);

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

    // A demo that does not turn the device red has not demonstrated anything,
    // so one scenario that raises a high severity warning is moved to the
    // front and always survives the truncation below. Without this the
    // selection is simply random, and a run drawn entirely from the quieter
    // detectors shows an audience nothing.
    if let Some(high) = pool.iter().position(|s| s.raises_high) {
        pool.swap(0, high);
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
        .filter_map(|m| match m {
            DemoMessage::Nas(bytes) => encapsulate_nas(bytes),
            DemoMessage::Rrc { payload, pdu_num } => encapsulate_rrc(&payload, pdu_num),
            DemoMessage::Gsm(bytes) => encapsulate_gsm(&bytes),
            DemoMessage::MacRach { timing_advance } => encapsulate_mac_rach(timing_advance),
        })
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

    /// Every scenario must raise a real warning on its own, not merely make
    /// a detector fire.
    ///
    /// This is stricter than it looks. Rows carrying only informational events
    /// are treated as empty and never written to the recording, so a scenario
    /// that produced only notes would fire its detector, pass a weaker check,
    /// and still show an audience an unchanged screen. Two scenarios did
    /// exactly that before this assertion was tightened. Low is enough: a low
    /// warning is written, counted and shown, and a scenario for a detector
    /// whose honest severity is Low should demonstrate that severity rather
    /// than be excluded for it.
    #[test]
    fn every_scenario_raises_a_warning_by_itself() {
        for scenario in scenarios() {
            let name = scenario.name;
            let mut harness = Harness::new_with_config(
                &AnalyzerConfig::default(),
                &rayhunter::DeviceMetadata::default(),
            );
            let container = demo_container_from(vec![scenario]).expect("container should build");
            let rows = harness.analyze_qmdl_messages(container);
            let highest = rows
                .iter()
                .flat_map(|row| row.events.iter().flatten())
                .map(|e| e.event_type)
                .max();

            assert!(
                highest.is_some_and(|h| h >= EventType::Low),
                "scenario {name:?} produced {highest:?}; a scenario that cannot raise at least \
                 a low warning shows an audience an unchanged screen"
            );
        }
    }

    /// Every scenario must still warn when drawn a second time by the same
    /// harness.
    ///
    /// The per-scenario test above uses a fresh harness, but the daemon's
    /// harness lives as long as the recording, and a detector may
    /// deliberately not repeat itself: the LPP detector warns once per
    /// transaction. A scenario that only fires on a fresh harness demos
    /// exactly once per recording and then looks broken on every later
    /// press. Caught on real hardware, not by the fresh-harness test.
    #[test]
    fn every_scenario_warns_again_on_a_repeat_press() {
        for scenario in scenarios() {
            let name = scenario.name;
            let mut harness = Harness::new_with_config(
                &AnalyzerConfig::default(),
                &rayhunter::DeviceMetadata::default(),
            );
            let mut highest_per_press = Vec::new();
            for press in [&scenario, &scenario] {
                let container = demo_container_from(vec![Scenario {
                    name: press.name,
                    messages: press.messages.clone(),
                    raises_high: press.raises_high,
                }])
                .expect("container should build");
                let highest = harness
                    .analyze_qmdl_messages(container)
                    .iter()
                    .flat_map(|row| row.events.iter().flatten())
                    .map(|e| e.event_type)
                    .max();
                highest_per_press.push(highest);
            }
            for (press, highest) in highest_per_press.iter().enumerate() {
                assert!(
                    highest.is_some_and(|h| h >= EventType::Low),
                    "scenario {name:?} produced {highest:?} on press {}; a scenario must \
                     warn every time it is drawn, or repeat presses appear broken",
                    press + 1
                );
            }
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
        let mut harness = Harness::new_with_config(
            &AnalyzerConfig::default(),
            &rayhunter::DeviceMetadata::default(),
        );
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

    /// `raises_high` has to match what the detectors actually do, or the
    /// guarantee that every demo turns the device red is worthless. A flag
    /// maintained by hand drifts the moment a detector's severity changes, so
    /// this checks it against real analysis rather than trusting it.
    #[test]
    fn the_high_severity_flags_are_true() {
        let mut mismatches: Vec<String> = Vec::new();
        for scenario in scenarios() {
            let mut harness = Harness::new_with_config(
                &AnalyzerConfig::default(),
                &rayhunter::DeviceMetadata::default(),
            );
            let container = demo_container_from(vec![Scenario {
                name: scenario.name,
                messages: scenario.messages.clone(),
                raises_high: scenario.raises_high,
            }])
            .expect("container should build");
            let highest = harness
                .analyze_qmdl_messages(container)
                .iter()
                .flat_map(|row| row.events.iter().flatten())
                .map(|e| e.event_type)
                .max();
            let actually_high = highest == Some(EventType::High);
            if actually_high != scenario.raises_high {
                mismatches.push(format!(
                    "{}: raises_high is {} but the detectors produced {highest:?}",
                    scenario.name, scenario.raises_high
                ));
            }
        }
        assert!(mismatches.is_empty(), "{mismatches:#?}");
    }

    /// Every message a demo produces has to be identifiable as fake, including
    /// by somebody reading the recording later who was not at the demo.
    #[test]
    fn the_demo_prefix_is_unmistakable() {
        assert!(DEMO_PREFIX.contains("DEMO"));
        assert!(DEMO_PREFIX.contains("NOT REAL"));
    }
}

#[cfg(test)]
mod coverage {
    use super::*;
    use rayhunter::analysis::analyzer::{AnalyzerConfig, Harness};

    /// Detectors the demo cannot show. Empty: every enabled detector is now
    /// reachable. Kept so a future detector arriving undemonstrable is a
    /// deliberate entry here rather than a silent gap.
    const KNOWN_UNCOVERED: &[&str] = &[];

    /// Every enabled detector is either demonstrable or explicitly listed as
    /// not yet demonstrable. A new detector arriving with neither fails here.
    #[test]
    fn demo_coverage_is_accounted_for() {
        let names: Vec<String> = Harness::new_with_config(
            &AnalyzerConfig::default(),
            &rayhunter::DeviceMetadata::default(),
        )
        .get_metadata()
        .analyzers
        .iter()
        .map(|a| a.name.clone())
        .collect();

        let mut fired: std::collections::HashSet<String> = Default::default();
        for scenario in scenarios() {
            let mut harness = Harness::new_with_config(
                &AnalyzerConfig::default(),
                &rayhunter::DeviceMetadata::default(),
            );
            let container = demo_container_from(vec![scenario]).unwrap();
            for row in harness.analyze_qmdl_messages(container) {
                for (i, ev) in row.events.iter().enumerate() {
                    if ev.is_some() {
                        fired.insert(names[i].clone());
                    }
                }
            }
        }

        let unaccounted: Vec<&String> = names
            .iter()
            .filter(|n| !fired.contains(*n) && !KNOWN_UNCOVERED.contains(&n.as_str()))
            .collect();
        assert!(
            unaccounted.is_empty(),
            "these detectors have no demo scenario and are not listed as known gaps: {unaccounted:?}"
        );

        let now_working: Vec<&&str> = KNOWN_UNCOVERED
            .iter()
            .filter(|n| fired.contains(**n))
            .collect();
        assert!(
            now_working.is_empty(),
            "these are listed as uncovered but now fire; remove them from KNOWN_UNCOVERED: {now_working:?}"
        );
    }

    #[test]
    #[ignore]
    fn report_which_analyzers_the_demo_can_trigger() {
        let names: Vec<String> = Harness::new_with_config(
            &AnalyzerConfig::default(),
            &rayhunter::DeviceMetadata::default(),
        )
        .get_metadata()
        .analyzers
        .iter()
        .map(|a| a.name.clone())
        .collect();

        let mut fired: std::collections::HashSet<String> = Default::default();
        for scenario in scenarios() {
            let mut harness = Harness::new_with_config(
                &AnalyzerConfig::default(),
                &rayhunter::DeviceMetadata::default(),
            );
            let container = demo_container_from(vec![scenario]).unwrap();
            for row in harness.analyze_qmdl_messages(container) {
                for (i, ev) in row.events.iter().enumerate() {
                    if ev.is_some() {
                        fired.insert(names[i].clone());
                    }
                }
            }
        }
        println!("COVERED:");
        for n in &names {
            if fired.contains(n) {
                println!("   yes  {n}");
            }
        }
        println!("NOT COVERED:");
        for n in &names {
            if !fired.contains(n) {
                println!("   no   {n}");
            }
        }
    }
}

#[cfg(test)]
mod derived_payloads {
    use rayhunter::analysis::analyzer::{AnalyzerConfig, Harness};

    fn fires(payload: &[u8], pdu_num: u8) -> Option<Vec<String>> {
        let container = super::rrc_container(payload, pdu_num)?;
        let mut harness = Harness::new_with_config(
            &AnalyzerConfig::default(),
            &rayhunter::DeviceMetadata::default(),
        );
        let names: Vec<String> = harness
            .get_metadata()
            .analyzers
            .iter()
            .map(|a| a.name.clone())
            .collect();
        let mut hits = Vec::new();
        for row in harness.analyze_qmdl_messages(container) {
            for (i, e) in row.events.iter().enumerate() {
                if let Some(e) = e {
                    hits.push(format!("{}: {}", names[i], e.message));
                }
            }
        }
        if hits.is_empty() { None } else { Some(hits) }
    }

    /// Sanity: does the RRC framing parse at all? Uses the known good captured
    /// payload from the diag library's own tests.
    #[test]
    fn framing_sanity() {
        let payload = [0x40u8, 0x1, 0xee, 0xad, 0xd5, 0x4d, 0xd0];
        for pdu in [2u8, 5, 6] {
            let c = super::rrc_container(&payload, pdu).unwrap();
            let parsed = c.messages();
            let mut harness = Harness::new_with_config(
                &AnalyzerConfig::default(),
                &rayhunter::DeviceMetadata::default(),
            );
            let rows = harness.analyze_qmdl_messages(super::rrc_container(&payload, pdu).unwrap());
            println!(
                "pdu {pdu}: parsed_ok={} skipped={:?}",
                parsed.iter().all(|m| m.is_ok()),
                rows.iter()
                    .filter_map(|r| r.skipped_message_reason.clone())
                    .collect::<Vec<_>>()
            );
        }
    }

    /// SIB3 then SIB7 then SIB1: an LTE priority to compare against, a higher
    /// 2G priority, and the SIB1 that makes the detector compare them. That
    /// third piece is what turns a note into an actual warning.
    #[test]
    fn try_sib3_sib7_sib1_sequence() {
        let sib3 = vec![0x00u8, 0x04, 0x00, 0x08, 0x00, 0x00];
        let sib7 = vec![0x00u8, 0x14, 0x80, 0x00, 0x00, 0x17, 0x00, 0x00, 0x00];
        let sib1 = vec![
            0x40u8, 0x40, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let mut harness = Harness::new_with_config(
            &AnalyzerConfig::default(),
            &rayhunter::DeviceMetadata::default(),
        );
        let names: Vec<String> = harness
            .get_metadata()
            .analyzers
            .iter()
            .map(|a| a.name.clone())
            .collect();
        for payload in [&sib3, &sib7, &sib1] {
            let container = super::rrc_container(payload, 2).unwrap();
            for row in harness.analyze_qmdl_messages(container) {
                for (i, ev) in row.events.iter().enumerate() {
                    if let Some(ev) = ev {
                        println!("  {} [{:?}] {}", names[i], ev.event_type, ev.message);
                    }
                }
            }
        }
    }

    /// The derived SIB1. PDU 2 is the broadcast channel.
    #[test]
    fn try_derived_sib1() {
        let sib1 = vec![
            0x40u8, 0x40, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        match fires(&sib1, 2) {
            Some(hits) => println!("  sib1 -> {hits:?}"),
            None => println!("  sib1 -> nothing"),
        }
    }

    /// The derived 2G redirect payload, plus a few neighbours in case one of
    /// the enumerated widths is off by a bit.
    #[test]
    fn try_derived_connection_release() {
        for candidate in [
            vec![0x28u8, 0x22, 0x20, 0x00, 0x00],
            vec![0x28, 0x20, 0x20, 0x00, 0x00],
            vec![0x28, 0x24, 0x20, 0x00, 0x00],
            vec![0x28, 0x26, 0x20, 0x00, 0x00],
            vec![0x28, 0x22, 0x20, 0x00, 0x00, 0x00],
            vec![0x28, 0x22, 0x10, 0x00, 0x00],
        ] {
            match fires(&candidate, 7) {
                Some(hits) => println!("  {candidate:02x?} -> {hits:?}"),
                None => println!("  {candidate:02x?} -> nothing"),
            }
        }
    }

    /// Targeted check of bit layouts derived from the ASN.1 rather than
    /// searched for. PDU 7 is the dedicated downlink channel; 6 is the common
    /// one, which was the earlier mistake.
    #[test]
    fn try_derived_security_mode_command() {
        for candidate in [
            vec![0x30u8, 0x00, 0x10],
            vec![0x30, 0x00, 0x08],
            vec![0x30, 0x00, 0x00],
            vec![0x30, 0x00, 0x20],
            vec![0x30, 0x08, 0x00],
            vec![0x38, 0x00, 0x00],
        ] {
            match fires(&candidate, 7) {
                Some(hits) => println!("  {candidate:02x?} -> {hits:?}"),
                None => println!("  {candidate:02x?} -> nothing"),
            }
        }
    }
}
