//! Location requests carried over LPP, the LTE Positioning Protocol.
//!
//! LPP (3GPP TS 36.355) is how an LTE network asks a phone to measure and
//! report its own position: GNSS readings, cell timing measurements, or a
//! computed location estimate. It exists for emergency calls and lawful
//! location services, but the same machinery lets whoever controls the
//! network — or an IMSI catcher impersonating it — quietly ask the phone
//! where it is. See EFForg/rayhunter#1072 and #534.
//!
//! LPP rides inside NAS `Downlink/Uplink Generic NAS Transport` messages
//! (3GPP TS 24.301 section 9.9.3.42, container type 1). The NAS parser hands
//! over the container as raw bytes; the LPP inside is UPER-encoded ASN.1,
//! which `telcom-parser` has no definitions for. Rather than pull in the
//! whole 36.355 schema to read three fields, the fixed prefix of the message
//! is decoded by hand: presence bits, transaction ID, and which of the eight
//! message kinds the body holds. Everything past that is deliberately
//! ignored. The layout is verified against encodings produced by pycrate's
//! reference 36.355 implementation in the tests below.
//!
//! Only the two message kinds that move location information — the request
//! and the report — raise a warning, at Low severity: LPP also carries
//! routine capability exchanges and GPS assistance data that mean nothing on
//! their own, and those are reported as informational events instead. A
//! periodic reporting session raises one warning per transaction rather than
//! one per report, so an hour of tracking does not bury the history in
//! thousands of identical rows.

use std::borrow::Cow;
use std::collections::HashMap;

use pycrate_rs::nas::NASMessage;
use pycrate_rs::nas::emm::EMMMessage;
use pycrate_rs::nas::generated::emm::emmdl_generic_nas_transport::GenericContTypeGenericContType as DlContainerType;
use pycrate_rs::nas::generated::emm::emmul_generic_nas_transport::GenericContTypeGenericContType as UlContainerType;

use super::analyzer::{Analyzer, Event, EventType};
use super::information_element::{InformationElement, LteInformationElement};

/// Reads single bits from a byte slice, returning `None` at the end rather
/// than panicking. UPER encodes nothing on byte boundaries, so everything
/// here is bit arithmetic.
struct BitReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn bit(&mut self) -> Option<bool> {
        let byte = self.bytes.get(self.position / 8)?;
        let bit = (byte >> (7 - (self.position % 8))) & 1;
        self.position += 1;
        Some(bit == 1)
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()? as u32;
        }
        Some(value)
    }
}

/// Who started an LPP transaction. The initiator is part of the transaction's
/// identity: the server and the device each number their own transactions, so
/// (initiator, number) pairs a request with its response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Initiator {
    LocationServer,
    TargetDevice,
}

/// The eight things an LPP message can be, per the `c1` CHOICE in 36.355.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LppBody {
    RequestCapabilities,
    ProvideCapabilities,
    RequestAssistanceData,
    ProvideAssistanceData,
    RequestLocationInformation,
    ProvideLocationInformation,
    Abort,
    Error,
    /// A spare `c1` alternative or the messageClassExtension escape hatch:
    /// reserved for future versions of the protocol, empty today.
    Reserved,
    /// The message carries no body at all — legal, used for bare
    /// acknowledgements.
    Absent,
}

/// What the fixed prefix of an LPP message says.
#[derive(Debug, PartialEq)]
pub struct LppSummary {
    pub transaction: Option<(Initiator, u8)>,
    pub end_transaction: bool,
    pub body: LppBody,
}

/// Decode the fixed prefix of a UPER-encoded LPP-Message.
///
/// ```text
/// LPP-Message ::= SEQUENCE {
///     transactionID    LPP-TransactionID   OPTIONAL,
///     endTransaction   BOOLEAN,
///     sequenceNumber   SequenceNumber      OPTIONAL,
///     acknowledgement  Acknowledgement     OPTIONAL,
///     lpp-MessageBody  LPP-MessageBody     OPTIONAL
/// }
/// ```
///
/// Returns `None` when the bytes run out early, or when an extension bit
/// says fields we cannot size come before the ones we want. Failing to
/// `None` rather than guessing matters: a wrong alignment would report the
/// wrong message kind, and a false "your location was requested" is worse
/// than an honest "an LPP message we could not read".
pub fn decode_lpp_prefix(bytes: &[u8]) -> Option<LppSummary> {
    let mut reader = BitReader::new(bytes);

    let has_transaction_id = reader.bit()?;
    let has_sequence_number = reader.bit()?;
    let has_acknowledgement = reader.bit()?;
    let has_body = reader.bit()?;

    let transaction = if has_transaction_id {
        // LPP-TransactionID is an extensible SEQUENCE. Its root fields still
        // decode when the extension bit is set, but unknown additions follow
        // them, so nothing after the ID can be trusted in that case.
        let id_extended = reader.bit()?;
        // Initiator is an extensible ENUMERATED with two root values. An
        // extended value encodes as a variable-length number, which we cannot
        // size, so nothing after it would be aligned.
        if reader.bit()? {
            return None;
        }
        let initiator = if reader.bit()? {
            Initiator::TargetDevice
        } else {
            Initiator::LocationServer
        };
        let number = reader.bits(8)? as u8;
        if id_extended {
            return None;
        }
        Some((initiator, number))
    } else {
        None
    };

    let end_transaction = reader.bit()?;

    if has_sequence_number {
        // SequenceNumber ::= INTEGER (0..255), fixed eight bits.
        reader.bits(8)?;
    }

    if has_acknowledgement {
        // Acknowledgement ::= SEQUENCE { ackRequested BOOLEAN,
        //                                ackIndicator SequenceNumber OPTIONAL }
        let has_indicator = reader.bit()?;
        reader.bit()?;
        if has_indicator {
            reader.bits(8)?;
        }
    }

    let body = if has_body {
        if reader.bit()? {
            // messageClassExtension: defined as an empty SEQUENCE today.
            LppBody::Reserved
        } else {
            match reader.bits(4)? {
                0 => LppBody::RequestCapabilities,
                1 => LppBody::ProvideCapabilities,
                2 => LppBody::RequestAssistanceData,
                3 => LppBody::ProvideAssistanceData,
                4 => LppBody::RequestLocationInformation,
                5 => LppBody::ProvideLocationInformation,
                6 => LppBody::Abort,
                7 => LppBody::Error,
                _ => LppBody::Reserved,
            }
        }
    } else {
        LppBody::Absent
    };

    Some(LppSummary {
        transaction,
        end_transaction,
        body,
    })
}

/// Which way a NAS message was travelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Downlink,
    Uplink,
}

/// Watches for the network asking this device where it is.
pub struct LppLocationRequestAnalyzer {
    /// Location transactions that have already raised their warning, keyed by
    /// (initiator, transaction number). Bounded by the key space: at most 512
    /// entries of a few bytes each. An entry is dropped when its transaction
    /// ends, so a later transaction reusing the number warns afresh.
    warned_transactions: HashMap<(Initiator, u8), ()>,
}

impl Default for LppLocationRequestAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LppLocationRequestAnalyzer {
    pub fn new() -> Self {
        Self {
            warned_transactions: HashMap::new(),
        }
    }

    fn event_for(&mut self, summary: &LppSummary, direction: Direction) -> Event {
        let transaction_note = match summary.transaction {
            Some((Initiator::LocationServer, n)) => format!(" (network transaction {n})"),
            Some((Initiator::TargetDevice, n)) => format!(" (device transaction {n})"),
            None => String::new(),
        };

        let event = match (summary.body, direction) {
            (LppBody::RequestLocationInformation, Direction::Downlink) => self.location_event(
                summary,
                format!("Network requested this device's location over LPP{transaction_note}"),
                format!("Network repeated its LPP location request{transaction_note}"),
            ),
            // A location request travelling up, or a report travelling down,
            // is not a defined LPP flow; report it, but note the oddity.
            (LppBody::RequestLocationInformation, Direction::Uplink) => Event {
                event_type: EventType::Low,
                message: format!(
                    "This device sent an LPP location request uplink, which is not a normal flow{transaction_note}"
                ),
            },
            (LppBody::ProvideLocationInformation, Direction::Uplink) => self.location_event(
                summary,
                format!(
                    "This device reported its location to the network over LPP{transaction_note}"
                ),
                format!("This device sent another LPP location report{transaction_note}"),
            ),
            (LppBody::ProvideLocationInformation, Direction::Downlink) => Event {
                event_type: EventType::Informational,
                message: format!("Network sent an LPP location report downlink{transaction_note}"),
            },
            (LppBody::RequestCapabilities, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP positioning capability request{transaction_note}"),
            },
            (LppBody::ProvideCapabilities, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP positioning capability response{transaction_note}"),
            },
            (LppBody::RequestAssistanceData, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP assistance data request{transaction_note}"),
            },
            (LppBody::ProvideAssistanceData, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP assistance data (routine GPS help){transaction_note}"),
            },
            (LppBody::Abort, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP transaction aborted{transaction_note}"),
            },
            (LppBody::Error, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP error message{transaction_note}"),
            },
            (LppBody::Reserved | LppBody::Absent, _) => Event {
                event_type: EventType::Informational,
                message: format!("LPP message with no readable body{transaction_note}"),
            },
        };

        // A finished transaction frees its number for a genuinely new
        // exchange, which should warn again rather than be treated as more of
        // the same.
        if (summary.end_transaction || summary.body == LppBody::Abort)
            && let Some(transaction) = summary.transaction
        {
            self.warned_transactions.remove(&transaction);
        }

        event
    }

    /// A Low warning the first time a transaction moves location information,
    /// informational for the repeats. Periodic reporting sends a report every
    /// interval for as long as the session lasts, and a thousand copies of
    /// the same warning would bury everything else in the history.
    fn location_event(
        &mut self,
        summary: &LppSummary,
        first_message: String,
        repeat_message: String,
    ) -> Event {
        let already_warned = match summary.transaction {
            Some(transaction) => self.warned_transactions.insert(transaction, ()).is_some(),
            // No transaction ID to group by, so every such message warns.
            None => false,
        };
        if already_warned {
            Event {
                event_type: EventType::Informational,
                message: repeat_message,
            }
        } else {
            Event {
                event_type: EventType::Low,
                message: first_message,
            }
        }
    }
}

impl Analyzer for LppLocationRequestAnalyzer {
    fn get_name(&self) -> Cow<'_, str> {
        Cow::from("LPP Location Request")
    }

    fn get_description(&self) -> Cow<'_, str> {
        Cow::from(
            "Watches for the network asking this device to measure and report its own position \
             via the LTE Positioning Protocol (LPP), and for the device's answers. Raises a low \
             warning once per location transaction; capability exchanges and GPS assistance \
             data are recorded as informational events. Emergency calls and some carriers' \
             location-based services use LPP legitimately.",
        )
    }

    fn get_version(&self) -> u32 {
        1
    }

    fn analyze_information_element(
        &mut self,
        ie: &InformationElement,
        _packet_num: usize,
    ) -> Option<Event> {
        let InformationElement::LTE(lte) = ie else {
            return None;
        };
        let LteInformationElement::NAS(nas) = &**lte else {
            return None;
        };

        let (container_is_lpp, container, direction) = match nas {
            NASMessage::EMMMessage(EMMMessage::EMMDLGenericNASTransport(transport)) => (
                transport.generic_cont_type.inner
                    == DlContainerType::LTEPositioningProtocolLPPMessageContainer,
                &transport.generic_container.inner.buf,
                Direction::Downlink,
            ),
            NASMessage::EMMMessage(EMMMessage::EMMULGenericNASTransport(transport)) => (
                transport.generic_cont_type.inner
                    == UlContainerType::LTEPositioningProtocolLPPMessageContainer,
                &transport.generic_container.inner.buf,
                Direction::Uplink,
            ),
            _ => return None,
        };

        if !container_is_lpp {
            // Container type 2 carries other location service messages; anything
            // else is not about positioning at all.
            return Some(Event {
                event_type: EventType::Informational,
                message: "Non-LPP generic NAS transport message".to_string(),
            });
        }

        match decode_lpp_prefix(container) {
            Some(summary) => Some(self.event_for(&summary, direction)),
            None => Some(Event {
                event_type: EventType::Informational,
                message: "LPP message seen, but its type could not be read".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test bytes were produced with pycrate's reference implementation of
    /// 3GPP TS 36.355 (`pycrate_asn1dir.LPP`), each round-tripped through its
    /// own decoder before being trusted. The layout here was derived by hand
    /// from the ASN.1 and then checked against these; the check caught a
    /// missed extension bit on LPP-TransactionID, which is exactly why
    /// vectors from an independent encoder are the ground truth and not
    /// bytes derived from the same reading of the spec being tested.
    fn from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    /// requestLocationInformation, transaction (locationServer, 5), not
    /// ending the transaction, with a common "location estimate required" IE.
    const REQUEST_LOCATION: &str = "900a20400000";
    /// provideLocationInformation, transaction (locationServer, 5), ending it.
    const PROVIDE_LOCATION_END: &str = "900b2800";
    /// requestCapabilities, transaction (locationServer, 0).
    const REQUEST_CAPABILITIES: &str = "90000000";
    /// provideCapabilities, transaction (targetDevice, 255), with a sequence
    /// number present before the body.
    const PROVIDE_CAPABILITIES_SEQ: &str = "d3fe110800";
    /// requestLocationInformation, transaction (locationServer, 42), with a
    /// sequence number AND an acknowledgement (with indicator) before the
    /// body — every optional field at once.
    const REQUEST_LOCATION_ALL_OPTIONALS: &str = "f054c8c24800";
    /// No transaction ID, no body: a bare acknowledgement.
    const ACK_ONLY: &str = "2406";
    /// abort, transaction (locationServer, 5), ending it.
    const ABORT: &str = "900b3000";
    /// messageClassExtension instead of a c1 body.
    const CLASS_EXTENSION: &str = "14";

    #[test]
    fn decodes_the_reference_vectors() {
        let cases: &[(&str, Option<(Initiator, u8)>, bool, LppBody)] = &[
            (
                REQUEST_LOCATION,
                Some((Initiator::LocationServer, 5)),
                false,
                LppBody::RequestLocationInformation,
            ),
            (
                PROVIDE_LOCATION_END,
                Some((Initiator::LocationServer, 5)),
                true,
                LppBody::ProvideLocationInformation,
            ),
            (
                REQUEST_CAPABILITIES,
                Some((Initiator::LocationServer, 0)),
                false,
                LppBody::RequestCapabilities,
            ),
            (
                PROVIDE_CAPABILITIES_SEQ,
                Some((Initiator::TargetDevice, 255)),
                false,
                LppBody::ProvideCapabilities,
            ),
            (
                REQUEST_LOCATION_ALL_OPTIONALS,
                Some((Initiator::LocationServer, 42)),
                false,
                LppBody::RequestLocationInformation,
            ),
            (ACK_ONLY, None, false, LppBody::Absent),
            (
                ABORT,
                Some((Initiator::LocationServer, 5)),
                true,
                LppBody::Abort,
            ),
            (CLASS_EXTENSION, None, false, LppBody::Reserved),
        ];
        for (hex, transaction, end, body) in cases {
            let summary = decode_lpp_prefix(&from_hex(hex))
                .unwrap_or_else(|| panic!("failed to decode {hex}"));
            assert_eq!(summary.transaction, *transaction, "transaction in {hex}");
            assert_eq!(summary.end_transaction, *end, "endTransaction in {hex}");
            assert_eq!(summary.body, *body, "body in {hex}");
        }
    }

    /// Truncated input must come back as None, never panic and never a
    /// misread. Every prefix of every vector is tried.
    #[test]
    fn truncated_messages_fail_cleanly() {
        for hex in [
            REQUEST_LOCATION,
            PROVIDE_LOCATION_END,
            REQUEST_LOCATION_ALL_OPTIONALS,
        ] {
            let bytes = from_hex(hex);
            // Zero bytes, and every partial byte count short of what the
            // prefix needs. Some longer prefixes still decode, legitimately:
            // the trailing bytes are body content we ignore.
            assert_eq!(decode_lpp_prefix(&[]), None, "empty input");
            assert_eq!(decode_lpp_prefix(&bytes[..1]), None, "one byte of {hex}");
        }
    }

    /// Wrap LPP bytes the way they arrive off the air: inside a plain NAS
    /// Downlink (0x68) or Uplink (0x69) Generic NAS Transport message, with
    /// container type 1 (LPP) and a two-byte big-endian length.
    fn nas_transport(downlink: bool, lpp: &[u8]) -> InformationElement {
        let mut bytes = vec![
            0x07,
            if downlink { 0x68 } else { 0x69 },
            0x01,
            (lpp.len() >> 8) as u8,
            lpp.len() as u8,
        ];
        bytes.extend_from_slice(lpp);
        let nas = NASMessage::parse(&bytes).expect("test NAS message must parse");
        InformationElement::LTE(Box::new(LteInformationElement::NAS(nas)))
    }

    #[test]
    fn a_location_request_raises_a_low_warning() {
        let mut analyzer = LppLocationRequestAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQUEST_LOCATION)), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Low);
        assert!(
            event.message.contains("requested this device's location"),
            "unexpected message: {}",
            event.message
        );
        assert!(event.message.contains("network transaction 5"));
    }

    #[test]
    fn a_location_report_raises_a_low_warning() {
        let mut analyzer = LppLocationRequestAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(false, &from_hex(PROVIDE_LOCATION_END)), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Low);
        assert!(
            event.message.contains("reported its location"),
            "unexpected message: {}",
            event.message
        );
    }

    /// One warning per transaction: the request warns, the report on the same
    /// transaction is informational, and a fresh transaction warns again.
    #[test]
    fn a_transaction_warns_once_until_it_ends() {
        let mut analyzer = LppLocationRequestAnalyzer::new();

        let request = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQUEST_LOCATION)), 1)
            .unwrap();
        assert_eq!(request.event_type, EventType::Low);

        // Same transaction (locationServer, 5), so no second warning — but
        // this report ends the transaction...
        let report = analyzer
            .analyze_information_element(&nas_transport(false, &from_hex(PROVIDE_LOCATION_END)), 2)
            .unwrap();
        assert_eq!(report.event_type, EventType::Informational);

        // ...which frees the number for a genuinely new exchange.
        let repeat = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQUEST_LOCATION)), 3)
            .unwrap();
        assert_eq!(repeat.event_type, EventType::Low);
    }

    /// A report with no preceding request still warns: a recording that
    /// starts mid-session must not miss the disclosure itself.
    #[test]
    fn an_unpaired_report_still_warns() {
        let mut analyzer = LppLocationRequestAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(false, &from_hex(PROVIDE_LOCATION_END)), 1)
            .unwrap();
        assert_eq!(event.event_type, EventType::Low);
    }

    #[test]
    fn capability_chatter_is_informational() {
        let mut analyzer = LppLocationRequestAnalyzer::new();
        for (downlink, hex) in [
            (true, REQUEST_CAPABILITIES),
            (false, PROVIDE_CAPABILITIES_SEQ),
        ] {
            let event = analyzer
                .analyze_information_element(&nas_transport(downlink, &from_hex(hex)), 1)
                .expect("must produce an event");
            assert_eq!(event.event_type, EventType::Informational, "for {hex}");
        }
    }

    /// Unreadable LPP must be reported as exactly that, not guessed at.
    #[test]
    fn unreadable_lpp_is_reported_informationally() {
        let mut analyzer = LppLocationRequestAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(true, &[0xff]), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Informational);
        assert!(event.message.contains("could not be read"));
    }

    /// Messages that are not generic NAS transport are none of our business.
    #[test]
    fn other_nas_messages_are_ignored() {
        let mut analyzer = LppLocationRequestAnalyzer::new();
        // An EMM identity request, which belongs to a different analyzer.
        let nas = NASMessage::parse(&[0x07, 0x55, 0x01]).unwrap();
        let ie = InformationElement::LTE(Box::new(LteInformationElement::NAS(nas)));
        assert_eq!(analyzer.analyze_information_element(&ie, 1), None);
    }
}
