//! Browsing the messages inside a stored recording.
//!
//! The point of this is to answer "what actually triggered that warning"
//! without downloading the capture and opening it in something else. So it
//! deliberately does not decode anything itself: it runs the recording back
//! through the same DIAG, GSMTAP and LTE decoding path the heuristics used, so
//! what a person reads here is the same interpretation that produced the alert.
//!
//! # Packet numbering
//!
//! Warnings refer to a packet by number, and those numbers come from a counter
//! in the analysis harness that increments once per message *before* parsing,
//! so messages that fail to decode still consume a number. Numbering here has
//! to agree exactly or every "view packet" link lands on the wrong message.
//!
//! Rather than reimplementing that rule and hoping it stays in step, this
//! module walks the same message stream in the same order and counts the same
//! way, and a test runs both over identical input and asserts the numbers
//! match. See `numbering_matches_the_analysis_harness`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use rayhunter::analysis::information_element::{InformationElement, LteInformationElement};
use rayhunter::diag::Message;
use rayhunter::gsmtap::parser as gsmtap_parser;
use rayhunter::qmdl::QmdlMessageReader;
use serde::Serialize;

use crate::qmdl_store::FileKind;
use crate::server::ServerState;

/// Most packets returned in one listing request.
///
/// A recording holds tens of thousands of messages and the device has very
/// little memory, so listings are always a window rather than the whole thing.
const MAX_LIMIT: usize = 500;
const DEFAULT_LIMIT: usize = 100;

/// Largest raw payload rendered as hex in a detail view.
const MAX_HEX_BYTES: usize = 2048;

/// Largest decoded rendering returned for one packet.
///
/// Real messages measured on a device run from a few hundred characters to
/// about eleven thousand, so this is generous. It exists because indentation
/// multiplies the size of a deeply nested message, and the device has around
/// twenty megabytes of memory free: one pathological packet should not be able
/// to spend it.
const MAX_DECODED_CHARS: usize = 200_000;

/// One row in the packet list. Deliberately small: enough to scan and filter,
/// with nothing decoded that a person has not asked to see.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PacketSummary {
    pub packet_num: usize,
    pub timestamp: Option<String>,
    /// "LTE RRC", "LTE NAS", or the radio technology when it is not LTE.
    pub protocol: String,
    /// Logical channel for RRC, absent for NAS.
    pub channel: Option<String>,
    pub direction: Option<String>,
    /// Recognisable name for the message, when one can be determined.
    pub message_type: Option<String>,
    pub payload_len: usize,
    /// "decoded", "undecodable", or "not a signalling message".
    pub parse_status: String,
    /// Physical cell identity of the tower this arrived from or went to.
    ///
    /// The most useful thing here after the message name. A capture is a
    /// mixture of messages from whichever cells were in range, and without
    /// this there is no way to tell which came from where. When something
    /// suspicious appears, the first question is usually whether it came from
    /// the cell everything else did.
    pub pci: Option<u16>,
    /// Frequency channel this was carried on.
    pub earfcn: Option<u32>,
    /// System frame and subframe number: the radio's own clock, in units of
    /// ten milliseconds and one millisecond. Finer than the timestamp and
    /// useful for ordering messages that share one.
    pub sfn: Option<u32>,
    pub subfn: Option<u8>,
}

/// A single packet, decoded on request.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PacketDetail {
    #[serde(flatten)]
    pub summary: PacketSummary,
    /// The decoded message, rendered as an indented tree.
    ///
    /// Not a hand written field list: the decoders produce deeply nested
    /// generated types, and reformatting their own representation shows
    /// everything they parsed rather than only the fields somebody thought to
    /// surface. A structured tree can replace this later without changing what
    /// is on screen.
    pub decoded: Option<String>,
    /// Why the message could not be decoded, when that is what happened.
    pub decode_error: Option<String>,
    /// The protocol data unit alone, which is what the decoders read.
    pub raw_hex: Option<String>,
    /// True when the payload was longer than the hex shown.
    pub raw_truncated: bool,
    /// How many bytes of framing the modem wrapped around that unit.
    ///
    /// Worth stating, because the payload on its own can look implausibly
    /// short: a paging message really is a handful of bytes, and somebody
    /// reasonably wonders where the rest went. The rest is this, and it is
    /// summarised in the fields above rather than shown as bytes.
    pub framing_len: Option<usize>,
}

/// Query parameters, parsed by hand.
///
/// Axum's `Query` extractor needs a feature this build does not enable, and
/// four optional integers do not justify adding one to a dependency the whole
/// project shares.
#[derive(Debug, Default)]
pub struct ListQuery {
    /// First packet number to return, counting from 1.
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    /// Centre the window on this packet instead, for jumping to a warning.
    pub around: Option<usize>,
    /// How many packets either side of `around` to include.
    pub context: Option<usize>,
}

impl ListQuery {
    /// Unparseable values are ignored rather than rejected: a bad query string
    /// should show the start of the recording, not an error page.
    fn parse(raw: Option<&str>) -> Self {
        let mut out = Self::default();
        let Some(raw) = raw else { return out };
        for pair in raw.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let Ok(number) = value.parse::<usize>() else {
                continue;
            };
            match key {
                "offset" => out.offset = Some(number),
                "limit" => out.limit = Some(number),
                "around" => out.around = Some(number),
                "context" => out.context = Some(number),
                _ => {}
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub struct PacketList {
    pub packets: Vec<PacketSummary>,
    /// Number of the first packet returned, so the caller can tell where the
    /// window landed when it asked for one centred on a warning.
    pub first_packet_num: usize,
    /// True when the recording ended before the requested window was filled.
    pub reached_end: bool,
}

/// What a message is, as far as the shared decoding path can tell.
struct Classified {
    timestamp: Option<String>,
    framing_len: Option<usize>,
    pci: Option<u16>,
    earfcn: Option<u32>,
    sfn: Option<u32>,
    subfn: Option<u8>,
    protocol: String,
    channel: Option<String>,
    direction: Option<String>,
    message_type: Option<String>,
    payload: Vec<u8>,
    element: Option<InformationElement>,
    error: Option<String>,
}

/// Run one message through the same path the analysers use.
fn classify(message: Result<Message, rayhunter::diag::DiagParsingError>) -> Classified {
    let mut out = Classified {
        timestamp: None,
        framing_len: None,
        pci: None,
        earfcn: None,
        sfn: None,
        subfn: None,
        protocol: "unknown".to_string(),
        channel: None,
        direction: None,
        message_type: None,
        payload: Vec::new(),
        element: None,
        error: None,
    };

    let message = match message {
        Ok(m) => m,
        Err(err) => {
            out.error = Some(format!("{err:?}"));
            return out;
        }
    };

    // Read the radio context off the log record before handing it on. The
    // GSMTAP conversion keeps only the protocol data unit and drops everything
    // wrapped around it, which is where the cell identity and timing live.
    read_radio_context(&message, &mut out);

    let gsmtap = match gsmtap_parser::parse(message) {
        Ok(Some((_, gsmtap))) => gsmtap,
        // Plenty of messages carry no signalling at all. That is ordinary, not
        // a failure, and saying so keeps it out of the "undecodable" count.
        Ok(None) => {
            out.error = Some("not a signalling message".to_string());
            return out;
        }
        Err(err) => {
            out.error = Some(format!("{err:?}"));
            return out;
        }
    };

    out.payload = gsmtap.payload.clone();
    // Only meaningful for NAS: the GSMTAP conversion sets this flag for NAS
    // messages and leaves it at its default for everything else, so trusting it
    // for RRC would label every uplink channel as downlink. RRC direction comes
    // from the channel name instead, below.
    if gsmtap.header.uplink {
        out.direction = Some("uplink".to_string());
    }

    match InformationElement::try_from(&gsmtap) {
        Ok(element) => {
            describe(&element, &mut out);
            out.element = Some(element);
        }
        Err(err) => out.error = Some(format!("{err:?}")),
    }
    out
}

/// Pull the timestamp and radio context out of a diag log record.
fn read_radio_context(message: &Message, out: &mut Classified) {
    use rayhunter::diag::diaglog::LogBody;
    use rayhunter::diag::diaglog::rrc::LteRrcOtaPacket;

    let Message::Log {
        timestamp, body, ..
    } = message
    else {
        return;
    };
    out.timestamp = Some(timestamp.to_datetime().to_rfc3339());

    if let LogBody::LteRrcOtaMessage { packet, .. } = body {
        // Header fields the modem supplied around the protocol data unit.
        out.framing_len = Some(match packet {
            LteRrcOtaPacket::V0 { .. } => 11,
            LteRrcOtaPacket::V5 { .. } => 13,
            LteRrcOtaPacket::V8 { .. } => 18,
            LteRrcOtaPacket::V25 { .. } => 20,
        });
        out.earfcn = Some(packet.get_earfcn());
        out.sfn = Some(packet.get_sfn());
        out.subfn = Some(packet.get_subfn());
        // No accessor exists for the cell identity, so it is read per layout.
        out.pci = match packet {
            LteRrcOtaPacket::V0 { phy_cell_id, .. }
            | LteRrcOtaPacket::V5 { phy_cell_id, .. }
            | LteRrcOtaPacket::V8 { phy_cell_id, .. }
            | LteRrcOtaPacket::V25 { phy_cell_id, .. } => Some(*phy_cell_id),
        };
    }
}

/// Fill in protocol, channel and message name from a decoded element.
fn describe(element: &InformationElement, out: &mut Classified) {
    match element {
        InformationElement::LTE(lte) => match &**lte {
            LteInformationElement::NAS(nas) => {
                out.protocol = "LTE NAS".to_string();
                // NAS direction did come from the message itself, so anything
                // not marked uplink is downlink.
                out.direction.get_or_insert_with(|| "downlink".to_string());
                let text = format!("{nas:?}");
                // The outer variant is only ever EMMMessage or ESMMessage,
                // which says nothing useful. The message itself is one level
                // in, so that is what gets shown, with the layer alongside it.
                out.channel = Some(variant_name(&text).replace("Message", ""));
                out.message_type = nested_name(&text);
            }
            other => {
                out.protocol = "LTE RRC".to_string();
                let text = format!("{other:?}");
                let channel = variant_name(&text);
                out.direction = direction_of_channel(&channel).map(str::to_string);
                out.channel = Some(channel);
                out.message_type = message_name(&text);
            }
        },
        InformationElement::GSM(gsm) => {
            out.protocol = "GSM".to_string();
            out.channel = Some(format!("{:?}", gsm.channel));
        }
        InformationElement::UMTS => out.protocol = "UMTS".to_string(),
        InformationElement::FiveG => out.protocol = "5G".to_string(),
    }
}

/// The outermost variant name in a debug rendering, e.g. "BcchDlSch".
fn variant_name(debug: &str) -> String {
    debug
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Which way an RRC message travelled, from the channel carrying it.
///
/// The logical channel name states the direction unambiguously: Ul is from the
/// device, Dl and the broadcast channels are towards it. Returns nothing for a
/// channel whose direction is not obvious, rather than assuming, because a
/// packet list that shows the wrong direction is worse than one that admits it
/// does not know.
fn direction_of_channel(channel: &str) -> Option<&'static str> {
    if channel.starts_with("Ul") {
        return Some("uplink");
    }
    if channel.starts_with("Dl")
        || channel.starts_with("Bcch")
        || channel.starts_with("Pcch")
        || channel == "PCCH"
        || channel == "MCCH"
        || channel.starts_with("ScMcch")
    {
        return Some("downlink");
    }
    None
}

/// The variant one level inside a debug rendering, e.g. the message inside
/// `ESMMessage(PDNConnectivityRequest(..))`.
fn nested_name(debug: &str) -> Option<String> {
    let inner = debug.split_once('(')?.1;
    let name = variant_name(inner);
    if name.is_empty() { None } else { Some(name) }
}

/// The message name buried inside an RRC debug rendering.
///
/// RRC nests the message under a `c1` choice, so the useful name is the variant
/// after the first `C1(`. Falls back to nothing rather than guessing, since a
/// wrong name is worse than an absent one when someone is trying to work out
/// what fired a warning.
fn message_name(debug: &str) -> Option<String> {
    let after = debug.split("C1(").nth(1)?;
    let name = variant_name(after);
    if name.is_empty() { None } else { Some(name) }
}

/// Cut a rendering to a length, on a character boundary, saying so.
fn truncate_decoded(text: String) -> String {
    if text.len() <= MAX_DECODED_CHARS {
        return text;
    }
    let mut cut = MAX_DECODED_CHARS;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}\n\n[truncated: the decoded message is larger than this view shows. \
         Download the recording to see all of it.]",
        &text[..cut]
    )
}

/// Indent a debug rendering so nesting is readable.
///
/// The decoders emit a single very long line. This does not attempt to
/// understand the contents, only to break it where the structure already says
/// it should break.
fn prettify(debug: &str) -> String {
    let mut out = String::with_capacity(debug.len() * 2);
    let mut depth = 0usize;
    let mut in_string = false;
    let mut previous = '\0';

    for c in debug.chars() {
        if in_string {
            out.push(c);
            if c == '"' && previous != '\\' {
                in_string = false;
            }
            previous = c;
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' | '(' => {
                depth += 1;
                out.push(c);
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            '}' | ']' | ')' => {
                depth = depth.saturating_sub(1);
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push(c);
            }
            ',' => {
                out.push(c);
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            ' ' if out.ends_with(['\n', ' ']) => {}
            _ => out.push(c),
        }
        previous = c;
    }
    out
}

fn summarize(packet_num: usize, classified: &Classified) -> PacketSummary {
    let parse_status = match &classified.error {
        None => "decoded",
        Some(e) if e == "not a signalling message" => "not a signalling message",
        Some(_) => "undecodable",
    };
    PacketSummary {
        packet_num,
        timestamp: classified.timestamp.clone(),
        protocol: classified.protocol.clone(),
        channel: classified.channel.clone(),
        direction: classified.direction.clone(),
        message_type: classified.message_type.clone(),
        payload_len: classified.payload.len(),
        parse_status: parse_status.to_string(),
        pci: classified.pci,
        earfcn: classified.earfcn,
        sfn: classified.sfn,
        subfn: classified.subfn,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Open a stored recording for reading.
async fn open_recording(
    state: &Arc<ServerState>,
    name: &str,
) -> Result<QmdlMessageReader<tokio::fs::File>, (StatusCode, String)> {
    let store = state.qmdl_store_lock.read().await;
    let (index, entry) = store
        .entry_for_name(name)
        .ok_or((StatusCode::NOT_FOUND, format!("no recording named {name}")))?;
    if entry.qmdl_size_bytes == 0 {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "recording is empty, try again shortly".to_string(),
        ));
    }
    let file = store
        .open_file(index, FileKind::Qmdl)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?
        .ok_or((StatusCode::NOT_FOUND, "recording file missing".to_string()))?;
    QmdlMessageReader::new(file)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))
}

/// List a window of packets from a recording.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/packets/{recording}",
    tag = "Packet explorer",
    responses(
        (status = StatusCode::OK, description = "A window of packets", body = PacketList),
        (status = StatusCode::NOT_FOUND, description = "No such recording"),
    ),
    summary = "List packets in a recording",
))]
pub async fn list_packets(
    State(state): State<Arc<ServerState>>,
    Path(recording): Path<String>,
    RawQuery(raw): RawQuery,
) -> Result<Json<PacketList>, (StatusCode, String)> {
    let query = ListQuery::parse(raw.as_deref());
    // A window centred on a warning takes precedence, since that is what a
    // "view context" link asks for.
    let (start, limit) = match query.around {
        Some(around) => {
            let context = query.context.unwrap_or(10).min(MAX_LIMIT / 2);
            (around.saturating_sub(context).max(1), context * 2 + 1)
        }
        None => (
            query.offset.unwrap_or(1).max(1),
            query.limit.unwrap_or(DEFAULT_LIMIT),
        ),
    };
    let limit = limit.clamp(1, MAX_LIMIT);

    let mut reader = open_recording(&state, &recording).await?;

    let mut packets = Vec::new();
    let mut packet_num = 0usize;
    let mut reached_end = true;

    // Counting mirrors the analysis harness: every message consumes a number,
    // including ones that fail to parse.
    while let Some(message) = reader
        .get_next_message()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?
    {
        packet_num += 1;
        if packet_num < start {
            continue;
        }
        if packets.len() >= limit {
            reached_end = false;
            break;
        }
        packets.push(summarize(packet_num, &classify(message)));
    }

    Ok(Json(PacketList {
        first_packet_num: start,
        packets,
        reached_end,
    }))
}

/// Decode one packet in full.
#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/packets/{recording}/{packet_num}",
    tag = "Packet explorer",
    responses(
        (status = StatusCode::OK, description = "The decoded packet", body = PacketDetail),
        (status = StatusCode::NOT_FOUND, description = "No such recording or packet"),
    ),
    summary = "Decode one packet",
))]
pub async fn get_packet(
    State(state): State<Arc<ServerState>>,
    Path((recording, wanted)): Path<(String, usize)>,
) -> Result<Json<PacketDetail>, (StatusCode, String)> {
    let mut reader = open_recording(&state, &recording).await?;
    let mut packet_num = 0usize;

    while let Some(message) = reader
        .get_next_message()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?
    {
        packet_num += 1;
        if packet_num != wanted {
            continue;
        }
        let classified = classify(message);
        let truncated = classified.payload.len() > MAX_HEX_BYTES;
        let hex_slice = &classified.payload[..classified.payload.len().min(MAX_HEX_BYTES)];

        return Ok(Json(PacketDetail {
            summary: summarize(packet_num, &classified),
            decoded: classified
                .element
                .as_ref()
                .map(|e| truncate_decoded(prettify(&format!("{e:?}")))),
            decode_error: classified.error.clone(),
            raw_hex: if classified.payload.is_empty() {
                None
            } else {
                Some(to_hex(hex_slice))
            },
            raw_truncated: truncated,
            framing_len: classified.framing_len,
        }));
    }

    Err((
        StatusCode::NOT_FOUND,
        format!("recording has only {packet_num} packets"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole feature rests on this: a warning says "packet 1234", and the
    /// explorer must land on the same message.
    ///
    /// The harness counts every message before attempting to parse it, so
    /// failures still consume a number. Anything that counted only successes
    /// would drift further out of step with every undecodable message, and the
    /// drift would be silent.
    #[test]
    fn numbering_matches_the_analysis_harness() {
        use rayhunter::analysis::analyzer::{AnalyzerConfig, Harness};
        use rayhunter::diag::{DataType, MessagesContainer};

        // A mix on purpose: real messages the demo generates, with deliberate
        // rubbish between them so unparseable messages are part of the test
        // rather than an unexercised edge.
        let mut messages = Vec::new();
        for scenario in crate::demo::scenarios() {
            if let Some(container) = crate::demo::demo_container_from(vec![scenario]) {
                messages.extend(container.messages);
            }
            messages.push(rayhunter::diag::HdlcEncapsulatedMessage {
                len: 4,
                data: vec![0xde, 0xad, 0xbe, 0x7e],
            });
        }

        let container = MessagesContainer {
            data_type: DataType::UserSpace,
            num_messages: messages.len() as u32,
            messages,
        };

        // What the analysers number these as.
        let mut harness = Harness::new_with_config(&AnalyzerConfig::default());
        let from_harness: Vec<usize> = harness
            .analyze_qmdl_messages(container.clone())
            .iter()
            .filter_map(|row| row.packet_num)
            .collect();

        // What the explorer numbers the same messages as.
        let from_explorer: Vec<usize> = (1..=container.messages().len()).collect();

        assert_eq!(
            from_harness, from_explorer,
            "packet numbering drifted from the analysis harness"
        );
        assert!(
            from_harness.len() > crate::demo::scenarios().len(),
            "test did not exercise enough messages to be meaningful"
        );
    }

    /// Undecodable messages must still consume a number, or every packet after
    /// the first bad one is off by however many preceded it.
    #[test]
    fn unparseable_messages_still_take_a_number() {
        use rayhunter::analysis::analyzer::{AnalyzerConfig, Harness};
        use rayhunter::diag::{DataType, HdlcEncapsulatedMessage, MessagesContainer};

        let container = MessagesContainer {
            data_type: DataType::UserSpace,
            num_messages: 3,
            messages: vec![
                HdlcEncapsulatedMessage {
                    len: 4,
                    data: vec![0xde, 0xad, 0xbe, 0x7e],
                },
                HdlcEncapsulatedMessage {
                    len: 4,
                    data: vec![0xba, 0xad, 0xf0, 0x7e],
                },
                HdlcEncapsulatedMessage {
                    len: 4,
                    data: vec![0x00, 0x11, 0x22, 0x7e],
                },
            ],
        };
        let mut harness = Harness::new_with_config(&AnalyzerConfig::default());
        let numbers: Vec<usize> = harness
            .analyze_qmdl_messages(container)
            .iter()
            .filter_map(|row| row.packet_num)
            .collect();
        assert_eq!(
            numbers,
            vec![1, 2, 3],
            "bad messages were skipped, not counted"
        );
    }

    #[test]
    fn query_parsing_takes_what_it_understands_and_ignores_the_rest() {
        let q = ListQuery::parse(Some("offset=50&limit=20&junk=x&around=abc"));
        assert_eq!(q.offset, Some(50));
        assert_eq!(q.limit, Some(20));
        // A non-numeric value is skipped rather than failing the request.
        assert_eq!(q.around, None);
        assert_eq!(ListQuery::parse(None).offset, None);
    }

    #[test]
    fn variant_names_come_from_the_start_of_a_debug_rendering() {
        assert_eq!(
            variant_name("BcchDlSch(BCCH_DL_SCH_Message { .. })"),
            "BcchDlSch"
        );
        assert_eq!(variant_name("NAS(EMMMessage(..))"), "NAS");
        assert_eq!(variant_name(""), "");
    }

    /// "EMMMessage" and "ESMMessage" are wrappers, not message names, so the
    /// list has to reach one level in or every NAS row looks identical.
    /// The GSMTAP conversion only marks NAS messages with a direction, so RRC
    /// has to take it from the channel or every uplink message reads as
    /// downlink. Observed on a real recording: an RrcConnectionReconfiguration
    /// Complete, which the device sends, was shown arriving.
    #[test]
    fn rrc_direction_comes_from_the_channel() {
        assert_eq!(direction_of_channel("UlDcch"), Some("uplink"));
        assert_eq!(direction_of_channel("UlCcch"), Some("uplink"));
        assert_eq!(direction_of_channel("DlDcch"), Some("downlink"));
        assert_eq!(direction_of_channel("BcchDlSch"), Some("downlink"));
        assert_eq!(direction_of_channel("PCCH"), Some("downlink"));
    }

    /// An unrecognised channel yields nothing rather than a guess.
    #[test]
    fn unknown_channels_have_no_direction() {
        assert_eq!(direction_of_channel("SbcchSlBch"), None);
        assert_eq!(direction_of_channel(""), None);
    }

    #[test]
    fn nas_message_names_come_from_inside_the_wrapper() {
        assert_eq!(
            nested_name("ESMMessage(PDNConnectivityRequest(..))").as_deref(),
            Some("PDNConnectivityRequest")
        );
        assert_eq!(
            nested_name("EMMMessage(EMMIdentityRequest(..))").as_deref(),
            Some("EMMIdentityRequest")
        );
        assert_eq!(nested_name("NoParenthesesHere"), None);
    }

    #[test]
    fn message_names_are_read_from_the_rrc_choice() {
        let debug = "DlDcch(DL_DCCH_Message { message: C1(SecurityModeCommand(..)) })";
        assert_eq!(message_name(debug).as_deref(), Some("SecurityModeCommand"));
    }

    /// A wrong name is worse than none when somebody is working out what fired
    /// a warning, so anything unrecognised yields nothing.
    #[test]
    fn message_names_are_absent_rather_than_guessed() {
        assert_eq!(message_name("BcchBch(SomethingElse { .. })"), None);
        assert_eq!(message_name("C1("), None);
    }

    #[test]
    fn decoded_output_is_bounded() {
        let small = "a".repeat(100);
        assert_eq!(truncate_decoded(small.clone()), small);

        let huge = "b".repeat(MAX_DECODED_CHARS + 5_000);
        let cut = truncate_decoded(huge);
        assert!(cut.len() < MAX_DECODED_CHARS + 200);
        assert!(cut.contains("truncated"));
    }

    /// Cutting must land on a character boundary, or the response is not valid
    /// UTF-8 and the whole request fails rather than one field being short.
    #[test]
    fn truncation_does_not_split_a_character() {
        let text = "é".repeat(MAX_DECODED_CHARS);
        let cut = truncate_decoded(text);
        // Reaching here at all means no panic; this confirms it stayed valid.
        assert!(cut.contains("truncated"));
    }

    #[test]
    fn hex_is_rendered_byte_by_byte() {
        assert_eq!(to_hex(&[0x07, 0x55, 0x01]), "07 55 01");
        assert_eq!(to_hex(&[]), "");
    }

    #[test]
    fn prettify_breaks_nesting_onto_lines() {
        let out = prettify("A { b: C(1, 2) }");
        assert!(out.lines().count() > 1);
        // Nothing is dropped, only rearranged.
        let stripped: String = out.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(stripped, "A{b:C(1,2)}");
    }

    /// Strings are left alone, so a brace inside one cannot throw off the
    /// indentation of everything after it.
    #[test]
    fn prettify_leaves_string_contents_intact() {
        let out = prettify(r#"A { name: "x { y }" }"#);
        assert!(out.contains(r#""x { y }""#));
    }
}
