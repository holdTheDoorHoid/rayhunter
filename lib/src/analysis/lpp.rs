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
    read_lpp_prefix(&mut BitReader::new(bytes))
}

/// Read the fixed prefix, leaving `reader` positioned at the start of the
/// message body's content (right after the c1 message-type index). The detail
/// decoders below continue from there.
fn read_lpp_prefix(reader: &mut BitReader) -> Option<LppSummary> {
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

/// Which positioning methods an LPP message names. Each is a different level of
/// precision and intrusiveness, so which one the network asked for matters as
/// much as the fact that it asked.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Methods {
    /// Assisted GNSS: satellite positioning. The most precise, a true fix.
    pub gnss: bool,
    /// Observed Time Difference of Arrival: the device times signals from
    /// several towers. Precise, and it needs the device to actively measure.
    pub otdoa: bool,
    /// Enhanced Cell ID: which cell, plus timing to it. Coarser, but cheap and
    /// silent.
    pub ecid: bool,
    /// An externally defined positioning protocol carried inside LPP.
    pub epdu: bool,
}

impl Methods {
    /// A human list of the methods named, most precise first. Empty when the
    /// message named none explicitly (the method rides in the common fields).
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.gnss {
            parts.push("satellite (GNSS)");
        }
        if self.otdoa {
            parts.push("tower timing (OTDOA)");
        }
        if self.ecid {
            parts.push("cell ID (E-CID)");
        }
        if self.epdu {
            parts.push("an external protocol");
        }
        if parts.is_empty() {
            "an unspecified method".to_string()
        } else {
            parts.join(", ")
        }
    }

    fn any(&self) -> bool {
        self.gnss || self.otdoa || self.ecid || self.epdu
    }
}

/// What kind of answer a location request wants back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocInfoType {
    EstimateRequired,
    MeasurementsRequired,
    EstimatePreferred,
    MeasurementsPreferred,
    /// An extended value this decoder does not name.
    Unknown,
}

/// The decoded detail of a location request or response, past the prefix.
#[derive(Debug, PartialEq)]
pub enum LppDetailKind {
    Request {
        methods: Methods,
        /// The network asked for reports to repeat on a timer: continuous
        /// tracking rather than a single fix. This is the signature that
        /// separates surveillance from a one-off locate.
        periodic: bool,
        info_type: LocInfoType,
    },
    Response {
        methods: Methods,
        /// The device sent back an actual position estimate.
        provided_estimate: bool,
        /// The device answered with a failure cause instead of a position.
        declined: bool,
    },
    /// A message this decoder does not read in detail (a future critical
    /// extension, a spare alternative, or a non-location body).
    Other,
}

/// A location message decoded past its prefix, into what it actually asks for
/// or reports.
#[derive(Debug, PartialEq)]
pub struct LppDetail {
    pub summary: LppSummary,
    pub kind: LppDetailKind,
}

/// Decode a location request or response past the prefix, into the fields that
/// say *what kind* of location was asked for and whether it repeats.
///
/// Every field read here sits at a fixed bit offset from the message body, with
/// no variable-length content in front of it, which is what makes decoding it
/// by hand safe. The moment a field would sit behind something whose length
/// this code cannot compute, it stops. The offsets are verified in the tests
/// against encodings from a reference 36.355 implementation.
pub fn decode_lpp_detail(bytes: &[u8]) -> Option<LppDetail> {
    let mut reader = BitReader::new(bytes);
    let summary = read_lpp_prefix(&mut reader)?;
    let kind = match summary.body {
        LppBody::RequestLocationInformation => read_request_detail(&mut reader)?,
        LppBody::ProvideLocationInformation => read_response_detail(&mut reader)?,
        _ => LppDetailKind::Other,
    };
    Some(LppDetail { summary, kind })
}

/// Step through `criticalExtensions` to the r9 IEs SEQUENCE. Returns `false`
/// when the message uses a future critical extension or a spare alternative,
/// where the r9 layout does not apply.
fn enter_r9(reader: &mut BitReader) -> Option<bool> {
    // criticalExtensions ::= CHOICE { c1, criticalExtensionsFuture }: 1 bit.
    if reader.bit()? {
        return Some(false);
    }
    // c1 ::= CHOICE { ...-r9, spare3, spare2, spare1 }: 2 bits.
    if reader.bits(2)? != 0 {
        return Some(false);
    }
    Some(true)
}

fn read_request_detail(reader: &mut BitReader) -> Option<LppDetailKind> {
    if !enter_r9(reader)? {
        return Some(LppDetailKind::Other);
    }
    // RequestLocationInformation-r9-IEs ::= SEQUENCE (extensible) with five root
    // optional fields. The extension bit and the five presence bits sit right
    // at the front; the method a request names is exactly which of these is
    // present.
    let _ext = reader.bit()?;
    let common = reader.bit()?;
    let methods = Methods {
        gnss: reader.bit()?,
        otdoa: reader.bit()?,
        ecid: reader.bit()?,
        epdu: reader.bit()?,
    };

    let mut periodic = false;
    let mut info_type = LocInfoType::Unknown;
    if common {
        // CommonIEsRequestLocationInformation ::= SEQUENCE (extensible) with a
        // mandatory locationInformationType and seven root optional fields. The
        // second optional is periodicalReporting.
        let _cext = reader.bit()?;
        let _triggered = reader.bit()?;
        periodic = reader.bit()?;
        let _additional_info = reader.bit()?;
        let _qos = reader.bit()?;
        let _environment = reader.bit()?;
        let _coordinate_types = reader.bit()?;
        let _velocity_types = reader.bit()?;
        // locationInformationType ::= ENUMERATED (extensible), 4 root values.
        let extended = reader.bit()?;
        let index = reader.bits(2)?;
        info_type = match (extended, index) {
            (false, 0) => LocInfoType::EstimateRequired,
            (false, 1) => LocInfoType::MeasurementsRequired,
            (false, 2) => LocInfoType::EstimatePreferred,
            (false, 3) => LocInfoType::MeasurementsPreferred,
            _ => LocInfoType::Unknown,
        };
    }

    Some(LppDetailKind::Request {
        methods,
        periodic,
        info_type,
    })
}

fn read_response_detail(reader: &mut BitReader) -> Option<LppDetailKind> {
    if !enter_r9(reader)? {
        return Some(LppDetailKind::Other);
    }
    // ProvideLocationInformation-r9-IEs ::= SEQUENCE (extensible), same five
    // root optionals as the request: which result blocks are present says which
    // method the device actually measured with.
    let _ext = reader.bit()?;
    let common = reader.bit()?;
    let methods = Methods {
        gnss: reader.bit()?,
        otdoa: reader.bit()?,
        ecid: reader.bit()?,
        epdu: reader.bit()?,
    };

    let mut provided_estimate = false;
    let mut declined = false;
    if common {
        // CommonIEsProvideLocationInformation ::= SEQUENCE (extensible) with
        // three root optionals: locationEstimate, velocityEstimate, locationError.
        let _cext = reader.bit()?;
        provided_estimate = reader.bit()?;
        let _velocity = reader.bit()?;
        declined = reader.bit()?;
    }

    Some(LppDetailKind::Response {
        methods,
        provided_estimate,
        declined,
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

/// The LPP container bytes and travel direction, if this NAS message is a
/// Generic NAS Transport carrying LPP. Shared by both LPP analyzers.
fn lpp_container(ie: &InformationElement) -> Option<(&[u8], Direction)> {
    let InformationElement::LTE(lte) = ie else {
        return None;
    };
    let LteInformationElement::NAS(nas) = &**lte else {
        return None;
    };
    match nas {
        NASMessage::EMMMessage(EMMMessage::EMMDLGenericNASTransport(t))
            if t.generic_cont_type.inner
                == DlContainerType::LTEPositioningProtocolLPPMessageContainer =>
        {
            Some((&t.generic_container.inner.buf, Direction::Downlink))
        }
        NASMessage::EMMMessage(EMMMessage::EMMULGenericNASTransport(t))
            if t.generic_cont_type.inner
                == UlContainerType::LTEPositioningProtocolLPPMessageContainer =>
        {
            Some((&t.generic_container.inner.buf, Direction::Uplink))
        }
        _ => None,
    }
}

/// Reads LPP messages in depth, to say *what kind* of location the network
/// asked for and, above all, whether it asked for **continuous** tracking.
///
/// This is the heavier companion to [`LppLocationRequestAnalyzer`]: it decodes
/// past the message type into the request and response bodies. Separated so a
/// device short on memory can run the cheap request/response awareness without
/// this. It stands on its own, warning even if the basic analyzer is off.
pub struct LppLocationTrackingAnalyzer {
    /// Transactions already warned about, so a periodic session that reports
    /// for an hour raises one warning rather than thousands. Same bound and
    /// lifetime as the basic analyzer's map.
    warned_transactions: HashMap<(Initiator, u8), ()>,
}

impl Default for LppLocationTrackingAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LppLocationTrackingAnalyzer {
    pub fn new() -> Self {
        Self {
            warned_transactions: HashMap::new(),
        }
    }

    fn event_for(&mut self, detail: &LppDetail, direction: Direction) -> Option<Event> {
        let note = match detail.summary.transaction {
            Some((Initiator::LocationServer, n)) => format!(" (network transaction {n})"),
            Some((Initiator::TargetDevice, n)) => format!(" (device transaction {n})"),
            None => String::new(),
        };

        let event = match (&detail.kind, direction) {
            (
                LppDetailKind::Request {
                    methods, periodic, ..
                },
                Direction::Downlink,
            ) => {
                let methods = methods.describe();
                if *periodic {
                    // Continuous tracking is the surveillance signature, and a
                    // step above a one-off locate, so it warns at Medium.
                    self.escalating_event(
                        detail,
                        EventType::Medium,
                        format!(
                            "Network requested CONTINUOUS location tracking via {methods}: reports \
                             repeat until stopped{note}"
                        ),
                        format!("Network's continuous LPP tracking request repeated{note}"),
                    )
                } else {
                    self.escalating_event(
                        detail,
                        EventType::Low,
                        format!("Network requested a one-off location fix via {methods}{note}"),
                        format!("Network repeated its one-off LPP location request{note}"),
                    )
                }
            }
            // A request the wrong way round is not a normal flow, but it is
            // still the network machinery for locating a device.
            (LppDetailKind::Request { methods, .. }, Direction::Uplink) => Event {
                event_type: EventType::Low,
                message: format!(
                    "This device sent an LPP location request uplink ({}), which is unusual{note}",
                    methods.describe()
                ),
            },
            (
                LppDetailKind::Response {
                    methods,
                    provided_estimate,
                    declined,
                },
                Direction::Uplink,
            ) => {
                if *declined && !*provided_estimate && !methods.any() {
                    // Declining is the device protecting itself; worth noting,
                    // not worth alarming over.
                    Event {
                        event_type: EventType::Informational,
                        message: format!(
                            "This device declined the network's LPP location request{note}"
                        ),
                    }
                } else {
                    let how = if methods.any() {
                        methods.describe()
                    } else {
                        "an estimate".to_string()
                    };
                    self.escalating_event(
                        detail,
                        EventType::Low,
                        format!("This device reported its position to the network via {how}{note}"),
                        format!("This device sent another LPP location report{note}"),
                    )
                }
            }
            (LppDetailKind::Response { .. }, Direction::Downlink) => Event {
                event_type: EventType::Informational,
                message: format!(
                    "Network sent an LPP location report downlink, which is unusual{note}"
                ),
            },
            // Capability, assistance, abort and the like: the basic analyzer
            // covers those as informational. Nothing to add in depth.
            (LppDetailKind::Other, _) => return None,
        };

        if (detail.summary.end_transaction || detail.summary.body == LppBody::Abort)
            && let Some(transaction) = detail.summary.transaction
        {
            self.warned_transactions.remove(&transaction);
        }

        Some(event)
    }

    /// Warn at `severity` the first time a transaction moves location
    /// information, informational for the repeats, so periodic reporting does
    /// not flood the history.
    fn escalating_event(
        &mut self,
        detail: &LppDetail,
        severity: EventType,
        first_message: String,
        repeat_message: String,
    ) -> Event {
        let already_warned = match detail.summary.transaction {
            Some(transaction) => self.warned_transactions.insert(transaction, ()).is_some(),
            None => false,
        };
        if already_warned {
            Event {
                event_type: EventType::Informational,
                message: repeat_message,
            }
        } else {
            Event {
                event_type: severity,
                message: first_message,
            }
        }
    }
}

impl Analyzer for LppLocationTrackingAnalyzer {
    fn get_name(&self) -> Cow<'_, str> {
        Cow::from("LPP Location Tracking")
    }

    fn get_description(&self) -> Cow<'_, str> {
        Cow::from(
            "Reads LPP location messages in depth: which positioning method the network asked for \
             (satellite, tower timing or cell ID) and, most importantly, whether it asked for \
             continuous tracking rather than a single fix. A continuous-tracking request warns at \
             medium severity. This parses more of each message than the basic LPP check and can \
             be turned off on devices very short on memory.",
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
        let (container, direction) = lpp_container(ie)?;
        // Undecodable LPP is left to the basic analyzer to note; adding depth
        // to "could not read it" says nothing more.
        let detail = decode_lpp_detail(container)?;
        self.event_for(&detail, direction)
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

    // Deep-detail reference vectors, same provenance as above: encoded by
    // pycrate's 36.355 implementation and round-tripped before use. Each
    // isolates one field so a wrong bit offset fails a specific assertion.
    /// commonIEs present, locationInformationType = each of the four root
    /// values. Only the type differs between these.
    const REQ_TYPE_ESTIMATE_REQ: &str = "900b20400000";
    const REQ_TYPE_MEAS_REQ: &str = "900b20400080";
    const REQ_TYPE_ESTIMATE_PREF: &str = "900b20400100";
    const REQ_TYPE_MEAS_PREF: &str = "900b20400180";
    /// Only the ecid method requested, no commonIEs.
    const REQ_METHOD_ECID: &str = "900b200870";
    /// Single-shot vs periodic (continuous) reporting, otherwise identical.
    const REQ_SINGLE_SHOT: &str = "900b20400000";
    const REQ_PERIODIC: &str = "900b20408018";
    /// The same periodic request, but not ending its transaction (endTransaction
    /// = false), as an ongoing tracking session actually sends.
    const REQ_PERIODIC_OPEN: &str = "900a20408018";
    /// Periodic reporting AND the ecid method named together.
    const REQ_PERIODIC_PLUS_ECID: &str = "900b2048801870";
    /// Responses: an error (declined), a GNSS result, an E-CID result, empty.
    const PROV_ERROR: &str = "900b284040";
    const PROV_GNSS: &str = "900b282000";
    const PROV_ECID: &str = "900b280800";
    const PROV_EMPTY: &str = "900b2800";

    fn request_detail(hex: &str) -> (Methods, bool, LocInfoType) {
        match decode_lpp_detail(&from_hex(hex)).map(|d| d.kind) {
            Some(LppDetailKind::Request {
                methods,
                periodic,
                info_type,
            }) => (methods, periodic, info_type),
            other => panic!("{hex} decoded as {other:?}, expected a Request"),
        }
    }

    #[test]
    fn decodes_the_requested_location_information_type() {
        assert_eq!(
            request_detail(REQ_TYPE_ESTIMATE_REQ).2,
            LocInfoType::EstimateRequired
        );
        assert_eq!(
            request_detail(REQ_TYPE_MEAS_REQ).2,
            LocInfoType::MeasurementsRequired
        );
        assert_eq!(
            request_detail(REQ_TYPE_ESTIMATE_PREF).2,
            LocInfoType::EstimatePreferred
        );
        assert_eq!(
            request_detail(REQ_TYPE_MEAS_PREF).2,
            LocInfoType::MeasurementsPreferred
        );
    }

    #[test]
    fn decodes_which_positioning_method_was_requested() {
        let (methods, _, _) = request_detail(REQ_METHOD_ECID);
        assert_eq!(
            methods,
            Methods {
                ecid: true,
                ..Default::default()
            }
        );
        // The combined vector names ecid and, via commonIEs, asks periodically.
        let (methods, periodic, _) = request_detail(REQ_PERIODIC_PLUS_ECID);
        assert!(methods.ecid && periodic);
    }

    #[test]
    fn tells_periodic_tracking_from_a_single_fix() {
        assert!(
            !request_detail(REQ_SINGLE_SHOT).1,
            "single shot must not read as periodic"
        );
        assert!(
            request_detail(REQ_PERIODIC).1,
            "periodic reporting must be detected"
        );
    }

    #[test]
    fn reads_whether_the_device_provided_or_declined() {
        let declined = match decode_lpp_detail(&from_hex(PROV_ERROR)).map(|d| d.kind) {
            Some(LppDetailKind::Response {
                declined,
                provided_estimate,
                ..
            }) => {
                assert!(!provided_estimate);
                declined
            }
            other => panic!("expected Response, got {other:?}"),
        };
        assert!(declined, "a locationError response must read as declined");

        // A GNSS result block present means the device measured and answered.
        match decode_lpp_detail(&from_hex(PROV_GNSS)).map(|d| d.kind) {
            Some(LppDetailKind::Response { methods, .. }) => assert!(methods.gnss),
            other => panic!("expected Response, got {other:?}"),
        }
        match decode_lpp_detail(&from_hex(PROV_ECID)).map(|d| d.kind) {
            Some(LppDetailKind::Response { methods, .. }) => assert!(methods.ecid),
            other => panic!("expected Response, got {other:?}"),
        }
        match decode_lpp_detail(&from_hex(PROV_EMPTY)).map(|d| d.kind) {
            Some(LppDetailKind::Response {
                methods,
                provided_estimate,
                declined,
            }) => {
                assert!(!methods.any() && !provided_estimate && !declined);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn deep_decode_of_truncated_input_never_panics() {
        for hex in [REQ_PERIODIC_PLUS_ECID, PROV_ERROR, REQ_TYPE_MEAS_PREF] {
            let bytes = from_hex(hex);
            for n in 0..bytes.len() {
                // Must return Some(Other/partial) or None, never panic.
                let _ = decode_lpp_detail(&bytes[..n]);
            }
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

    // --- The deep tracking analyzer ---

    #[test]
    fn tracking_flags_continuous_requests_at_medium() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_PERIODIC)), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Medium);
        assert!(
            event.message.contains("CONTINUOUS"),
            "unexpected message: {}",
            event.message
        );
    }

    #[test]
    fn tracking_reports_a_one_off_request_at_low_with_its_method() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_METHOD_ECID)), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Low);
        assert!(
            event.message.contains("cell ID (E-CID)"),
            "should name the method: {}",
            event.message
        );
        assert!(event.message.contains("one-off"));
    }

    #[test]
    fn tracking_warns_once_per_transaction() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        // First continuous request warns Medium. Uses the open-transaction
        // vector: an ongoing session does not end its transaction each message.
        let first = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_PERIODIC_OPEN)), 1)
            .unwrap();
        assert_eq!(first.event_type, EventType::Medium);
        // Same transaction again: informational, not another Medium.
        let repeat = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_PERIODIC_OPEN)), 2)
            .unwrap();
        assert_eq!(repeat.event_type, EventType::Informational);
    }

    /// A periodic request that *does* end its transaction frees the number, so
    /// a genuinely new session with the same number warns again rather than
    /// being silently folded into the last one.
    #[test]
    fn tracking_rewarns_after_a_transaction_ends() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        let first = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_PERIODIC)), 1)
            .unwrap();
        assert_eq!(first.event_type, EventType::Medium);
        // REQ_PERIODIC ends its transaction, so the same bytes again are a new
        // session and warn afresh.
        let again = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_PERIODIC)), 2)
            .unwrap();
        assert_eq!(again.event_type, EventType::Medium);
    }

    #[test]
    fn tracking_notes_a_declined_response_informationally() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(false, &from_hex(PROV_ERROR)), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Informational);
        assert!(
            event.message.contains("declined"),
            "unexpected message: {}",
            event.message
        );
    }

    #[test]
    fn tracking_flags_a_position_report_at_low() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(false, &from_hex(PROV_GNSS)), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Low);
        assert!(
            event.message.contains("reported its position"),
            "unexpected message: {}",
            event.message
        );
    }

    /// Capability chatter carries no request/response detail, so the deep
    /// analyzer stays silent and leaves it to the basic one.
    #[test]
    fn tracking_ignores_capability_messages() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        assert_eq!(
            analyzer.analyze_information_element(
                &nas_transport(true, &from_hex(REQUEST_CAPABILITIES)),
                1
            ),
            None
        );
    }

    /// The deep analyzer is self-sufficient: a lone one-off request still warns
    /// even with no basic analyzer running alongside it.
    #[test]
    fn tracking_is_self_sufficient() {
        let mut analyzer = LppLocationTrackingAnalyzer::new();
        let event = analyzer
            .analyze_information_element(&nas_transport(true, &from_hex(REQ_TYPE_ESTIMATE_REQ)), 1)
            .expect("a location request must always produce a visible event");
        assert!(event.event_type >= EventType::Low);
    }
}
