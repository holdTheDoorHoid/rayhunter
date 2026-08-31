//! Location requests carried over RRLP, the 2G/3G positioning protocol.
//!
//! RRLP (Radio Resource LCS Protocol, 3GPP TS 44.031) is the GSM-era ancestor
//! of LPP: the network asks the handset to measure and report its own position,
//! historically by GPS or by timing nearby towers. It is exactly the kind of
//! location request issue #534 asks Rayhunter to surface, and the 2G
//! counterpart to the LPP work in [`super::lpp`].
//!
//! On the air, RRLP does not travel on its own. The network wraps an RRLP APDU
//! inside a GSM Radio Resource **APPLICATION INFORMATION** message
//! (3GPP TS 44.018 section 9.1.53), which is what arrives here as a raw Layer 3
//! byte string. This analyzer reads that transport header to find the APDU,
//! then reads the front of the APDU to tell a location *request* from a
//! *response* or routine assistance data.
//!
//! Two layers, both decoded by hand from short fixed headers, both verified in
//! the tests against reference encoders: the GSM transport frame against
//! `pycrate_mobile`'s TS 44.018 implementation, and the RRLP APDU against
//! pycrate's TS 44.031 ASN.1. Nothing here parses the body of an RRLP message;
//! it reads only enough to say a positioning exchange happened and which way it
//! went.

use std::borrow::Cow;

use super::analyzer::{Analyzer, Event, EventType};
use super::information_element::InformationElement;

/// GSM Radio Resource management protocol discriminator (low nibble of the
/// first Layer 3 octet), 3GPP TS 24.007.
const PD_RR: u8 = 0x06;

/// RR message type for APPLICATION INFORMATION, the message that carries an
/// RRLP (or ETWS) APDU. 3GPP TS 44.018 Table 10.4.1; matches
/// `pycrate_mobile`'s `GSM48_MT_RR_APP_INFO` and osmocore.
const MT_APPLICATION_INFORMATION: u8 = 0x38;

/// APDU ID value meaning the payload is RRLP (rather than ETWS). 3GPP TS 44.018
/// section 10.5.2.48; it sits in the low nibble of the APDU ID / flags octet.
const APDU_ID_RRLP: u8 = 0x00;

/// Which RRLP message this is, from the component CHOICE at the front of the
/// APDU. Only the ones that matter for "was this device located" are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RrlpComponent {
    /// The network asking the device to measure and report its position.
    MeasurePositionRequest,
    /// The device's answer, carrying measurements or a position.
    MeasurePositionResponse,
    /// The network pushing data that speeds up a fix (ephemeris and the like).
    AssistanceData,
    /// A bare acknowledgement of assistance data.
    AssistanceDataAck,
    /// An RRLP protocol error.
    ProtocolError,
    /// A component this decoder does not name (an extension addition).
    Other,
}

/// One RRLP APDU, decoded only as far as its reference number and kind.
#[derive(Debug, PartialEq, Eq)]
pub struct RrlpApdu {
    pub reference_number: u8,
    pub component: RrlpComponent,
}

/// Decode the front of an RRLP APDU (3GPP TS 44.031).
///
/// ```text
/// PDU ::= SEQUENCE {
///     referenceNumber INTEGER (0..7),
///     component       RRLP-Component  -- an extensible CHOICE
/// }
/// ```
///
/// referenceNumber is three bits; the component CHOICE has an extension marker
/// (one bit) then a three-bit index over its five root alternatives. Returns
/// `None` only when there are not even enough bits for that.
pub fn decode_rrlp_apdu(bytes: &[u8]) -> Option<RrlpApdu> {
    let first = *bytes.first()?;
    // referenceNumber: top three bits.
    let reference_number = first >> 5;
    // component CHOICE extension bit: the fourth bit. A set extension bit means
    // one of the addition alternatives (posCapability*), which we do not name.
    let is_extension = (first >> 4) & 1 == 1;
    // component index: the next three bits.
    let index = (first >> 1) & 0b111;
    let component = if is_extension {
        RrlpComponent::Other
    } else {
        match index {
            0 => RrlpComponent::MeasurePositionRequest,
            1 => RrlpComponent::MeasurePositionResponse,
            2 => RrlpComponent::AssistanceData,
            3 => RrlpComponent::AssistanceDataAck,
            4 => RrlpComponent::ProtocolError,
            _ => RrlpComponent::Other,
        }
    };
    Some(RrlpApdu {
        reference_number,
        component,
    })
}

/// Pull an RRLP APDU out of a GSM RR APPLICATION INFORMATION message, if that
/// is what these Layer 3 bytes are.
///
/// ```text
/// APPLICATION INFORMATION (TS 44.018 9.1.53):
///   octet 1: skip indicator (high nibble) + protocol discriminator (low)  -> PD = RR
///   octet 2: message type                                                  -> 0x38
///   octet 3: APDU flags (high nibble) + APDU ID (low nibble)               -> ID 0 = RRLP
///   octet 4: APDU data length
///   octet 5..: APDU data (the RRLP APDU), `length` bytes
/// ```
///
/// Returns `None` for anything that is not RRLP-bearing APPLICATION
/// INFORMATION, so a wrong guess about the message type cannot by itself raise
/// a warning: the RRLP APDU behind the header still has to decode.
fn rrlp_apdu_from_application_information(l3: &[u8]) -> Option<&[u8]> {
    // Protocol discriminator is the low nibble of octet 1.
    if l3.first()? & 0x0f != PD_RR {
        return None;
    }
    if *l3.get(1)? != MT_APPLICATION_INFORMATION {
        return None;
    }
    if l3.get(2)? & 0x0f != APDU_ID_RRLP {
        return None;
    }
    let length = *l3.get(3)? as usize;
    let start = 4usize;
    let end = start.checked_add(length)?;
    l3.get(start..end)
}

/// Watches for a 2G location request or report carried over RRLP.
pub struct RrlpLocationAnalyzer {}

impl Analyzer for RrlpLocationAnalyzer {
    fn get_name(&self) -> Cow<'_, str> {
        Cow::from("RRLP Location Request")
    }

    fn get_description(&self) -> Cow<'_, str> {
        Cow::from(
            "Watches 2G (GSM) signalling for the network asking this device to measure and report \
             its own position over RRLP, the older cousin of LPP, and for the device's answers. A \
             request or a position report warns at low severity; assistance data and \
             acknowledgements are informational. Legitimate on emergency calls and some \
             location-based services.",
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
        let InformationElement::GSM(gsm) = ie else {
            return None;
        };
        let apdu_bytes = rrlp_apdu_from_application_information(&gsm.bytes)?;
        let apdu = decode_rrlp_apdu(apdu_bytes)?;

        let event = match apdu.component {
            RrlpComponent::MeasurePositionRequest => Event {
                event_type: EventType::Low,
                message: "Network requested this device's location over RRLP (2G positioning)"
                    .to_string(),
            },
            RrlpComponent::MeasurePositionResponse => Event {
                event_type: EventType::Low,
                message: "This device reported its position to the network over RRLP (2G)"
                    .to_string(),
            },
            RrlpComponent::AssistanceData => Event {
                event_type: EventType::Informational,
                message: "RRLP assistance data (2G positioning help)".to_string(),
            },
            RrlpComponent::AssistanceDataAck => Event {
                event_type: EventType::Informational,
                message: "RRLP assistance data acknowledged (2G)".to_string(),
            },
            RrlpComponent::ProtocolError => Event {
                event_type: EventType::Informational,
                message: "RRLP protocol error (2G positioning)".to_string(),
            },
            RrlpComponent::Other => Event {
                event_type: EventType::Informational,
                message: "RRLP positioning message (2G)".to_string(),
            },
        };
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::information_element::GsmInformationElement;
    use crate::gsmtap::UmSubtype;

    fn from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    // RRLP APDUs, encoded by pycrate's TS 44.031 ASN.1 and round-tripped.
    const APDU_ACK_REF0: &str = "06";
    const APDU_ACK_REF5: &str = "a6";
    const APDU_MSR_REQ: &str = "6000bc68";
    const APDU_MSR_RSP: &str = "6200";
    const APDU_ASSIST: &str = "8400";
    const APDU_ERROR: &str = "e800";

    // Full GSM RR APPLICATION INFORMATION frames carrying those APDUs, encoded
    // by pycrate_mobile's TS 44.018 implementation.
    const APP_MSR_REQ: &str = "063800046000bc68";
    const APP_MSR_RSP: &str = "063800026200";
    const APP_ASSIST: &str = "063800028400";
    const APP_ACK: &str = "0638000106";
    const APP_ERROR: &str = "06380002e800";
    /// APPLICATION INFORMATION carrying a non-RRLP (ETWS, APDU ID 1) payload.
    const APP_ETWS: &str = "063801020011";

    #[test]
    fn decodes_the_apdu_reference_vectors() {
        let cases: &[(&str, u8, RrlpComponent)] = &[
            (APDU_ACK_REF0, 0, RrlpComponent::AssistanceDataAck),
            (APDU_ACK_REF5, 5, RrlpComponent::AssistanceDataAck),
            (APDU_MSR_REQ, 3, RrlpComponent::MeasurePositionRequest),
            (APDU_MSR_RSP, 3, RrlpComponent::MeasurePositionResponse),
            (APDU_ASSIST, 4, RrlpComponent::AssistanceData),
            (APDU_ERROR, 7, RrlpComponent::ProtocolError),
        ];
        for (hex, ref_num, component) in cases {
            let apdu = decode_rrlp_apdu(&from_hex(hex)).unwrap_or_else(|| panic!("decode {hex}"));
            assert_eq!(apdu.reference_number, *ref_num, "ref number in {hex}");
            assert_eq!(apdu.component, *component, "component in {hex}");
        }
    }

    #[test]
    fn extracts_the_apdu_from_the_transport_frame() {
        assert_eq!(
            rrlp_apdu_from_application_information(&from_hex(APP_MSR_REQ)),
            Some(from_hex(APDU_MSR_REQ).as_slice())
        );
        assert_eq!(
            rrlp_apdu_from_application_information(&from_hex(APP_ERROR)),
            Some(from_hex(APDU_ERROR).as_slice())
        );
    }

    /// A non-RRLP APDU (ETWS) in the same message type must not be treated as
    /// positioning.
    #[test]
    fn ignores_a_non_rrlp_application_information() {
        assert_eq!(
            rrlp_apdu_from_application_information(&from_hex(APP_ETWS)),
            None
        );
    }

    fn gsm(hex: &str) -> InformationElement {
        InformationElement::GSM(Box::new(GsmInformationElement {
            channel: UmSubtype::Sdcch,
            bytes: from_hex(hex),
        }))
    }

    #[test]
    fn a_2g_location_request_warns() {
        let mut analyzer = RrlpLocationAnalyzer {};
        let event = analyzer
            .analyze_information_element(&gsm(APP_MSR_REQ), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Low);
        assert!(
            event
                .message
                .contains("requested this device's location over RRLP"),
            "unexpected message: {}",
            event.message
        );
    }

    #[test]
    fn a_2g_location_report_warns() {
        let mut analyzer = RrlpLocationAnalyzer {};
        let event = analyzer
            .analyze_information_element(&gsm(APP_MSR_RSP), 1)
            .expect("must produce an event");
        assert_eq!(event.event_type, EventType::Low);
        assert!(event.message.contains("reported its position"));
    }

    #[test]
    fn assistance_and_acks_are_informational() {
        let mut analyzer = RrlpLocationAnalyzer {};
        for hex in [APP_ASSIST, APP_ACK] {
            let event = analyzer
                .analyze_information_element(&gsm(hex), 1)
                .expect("must produce an event");
            assert_eq!(event.event_type, EventType::Informational, "for {hex}");
        }
    }

    #[test]
    fn non_rrlp_gsm_is_ignored() {
        let mut analyzer = RrlpLocationAnalyzer {};
        // A non-RRLP APPLICATION INFORMATION (ETWS).
        assert_eq!(
            analyzer.analyze_information_element(&gsm(APP_ETWS), 1),
            None
        );
        // An ordinary GSM message that is not APPLICATION INFORMATION at all:
        // a System Information Type 5 on SACCH (message type 0x35, PD RR).
        assert_eq!(analyzer.analyze_information_element(&gsm("0635"), 1), None);
    }

    /// Truncated frames and APDUs must fail cleanly, never panic. This is 2G
    /// signalling, some of which is attacker-shaped, so robustness matters.
    #[test]
    fn truncated_input_never_panics() {
        let mut analyzer = RrlpLocationAnalyzer {};
        for hex in [APP_MSR_REQ, APP_ERROR, APP_ETWS] {
            let bytes = from_hex(hex);
            for n in 0..bytes.len() {
                let _ = analyzer.analyze_information_element(&gsm(&hex[..n * 2]), 1);
                let _ = rrlp_apdu_from_application_information(&bytes[..n]);
                let _ = decode_rrlp_apdu(&bytes[..n]);
            }
        }
        // A frame that claims more APDU bytes than it carries.
        assert_eq!(
            rrlp_apdu_from_application_information(&from_hex("06380010aa")),
            None
        );
    }
}
