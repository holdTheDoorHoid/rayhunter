//! The term "information element" is used by 3GPP to describe "structural
//! elements containing single or multiple fields" in 2G/3G/4G/5G. We use
//! the term to refer to a structured, fully parsed message in any telcom
//! standard.

use crate::gsmtap::{GsmtapMessage, GsmtapType, LteNasSubtype, LteRrcSubtype, UmSubtype};
use pycrate_rs::nas::NASMessage;
use telcom_parser::{decode, lte_rrc};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InformationElementError {
    #[error("Failed decoding RRC message")]
    RRCDecodingError(#[from] telcom_parser::ParsingError),
    #[error("Failed decoding NAS message")]
    NASDecodingError(#[from] pycrate_rs::nas::ParseError),
    #[error("Unsupported LTE RRC subtype {0:?}")]
    UnsupportedGsmtapType(GsmtapType),
}

#[derive(Debug)]
pub enum InformationElement {
    // Carries the raw Layer 3 signalling bytes (2G is TLV, not something we
    // parse into a tree the way we do LTE). Boxed to keep the enum small, and
    // because most 2G messages are never looked at past their type.
    GSM(Box<GsmInformationElement>),
    UMTS,
    // This element of the enum is substantially larger than the others,
    // so we box it to prevent the size of the enum (any variant) from blowing up.
    LTE(Box<LteInformationElement>),
    FiveG,
}

/// A 2G (GSM) Layer 3 signalling message, kept as its raw bytes.
///
/// Unlike the LTE variants there is no parsed tree here: the analysers that
/// care read the specific fields they need out of the bytes. The logical
/// channel it arrived on is kept because it is useful context and costs
/// nothing.
#[derive(Debug)]
pub struct GsmInformationElement {
    pub channel: UmSubtype,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum LteInformationElement {
    DlCcch(lte_rrc::DL_CCCH_Message),
    // This element of the enum is substantially larger than the others,
    // so we box it to prevent the size of the enum (any variant) from blowing up.
    DlDcch(Box<lte_rrc::DL_DCCH_Message>),
    UlCcch(lte_rrc::UL_CCCH_Message),
    UlDcch(lte_rrc::UL_DCCH_Message),
    BcchBch(lte_rrc::BCCH_BCH_Message),
    BcchDlSch(lte_rrc::BCCH_DL_SCH_Message),
    PCCH(lte_rrc::PCCH_Message),
    MCCH(lte_rrc::MCCH_Message),
    ScMcch(lte_rrc::SC_MCCH_Message_r13),
    BcchBchMbms(lte_rrc::BCCH_BCH_Message_MBMS),
    BcchDlSchBr(lte_rrc::BCCH_DL_SCH_Message_BR),
    BcchDlSchMbms(lte_rrc::BCCH_DL_SCH_Message_MBMS),
    SbcchSlBch(lte_rrc::SBCCH_SL_BCH_Message),
    SbcchSlBchV2x(lte_rrc::SBCCH_SL_BCH_Message_V2X_r14),

    NAS(NASMessage),

    /// A random access response, from the LTE MAC layer.
    ///
    /// The only place a device is told its timing advance, which is the one
    /// distance measurement it gets for nothing. Carried here so analysers can
    /// see it: until this existed, MAC never reached the analysis pipeline at
    /// all, which is why EFForg/rayhunter#756 could not be built.
    MacRar(MacRandomAccessResponse),
    // FIXME: unclear which message these "NB" types map to
    //DlCcchNb(),
    //DlDcchNb(),
    //UlCcchNb(),
    //UlDcchNb(),
    //BcchBchNb(),
    //BcchBchTddNb(),
    //BcchDlSchNb(),
    //PcchNb(),
    //ScMcchNb(),
}

/// What a random access response tells the device.
///
/// Read back out of the GSMTAP MAC-LTE frame Rayhunter builds for Wireshark,
/// rather than plumbed separately from the diag structs, so there is one
/// representation of this on the way through and the analysers see exactly
/// what a capture contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacRandomAccessResponse {
    /// Timing advance command, 11 bits, roughly 78 metres a step.
    ///
    /// Not every modem fills this in. The Orbic RC400L reports zero on every
    /// attach, so anything reading this must treat an all zero history as
    /// "not reported" rather than "no distance".
    pub timing_advance: u16,
    /// The temporary identity the network hands out in the response.
    pub tc_rnti: u16,
    /// Which preamble this responds to.
    pub preamble: u8,
}

/// Offsets within the GSMTAP MAC-LTE frame built by `gsmtap::mac`.
///
/// Three bytes of MAC-LTE context, the payload tag, the E/T/RAPID subheader,
/// then the six byte response body. Kept next to the parsing that uses them so
/// the two cannot drift apart silently.
mod mac_rar_offsets {
    /// E/T/RAPID subheader.
    pub const SUBHEADER: usize = 4;
    /// Start of the response body: R(1) TA(11) grant(20) TC-RNTI(16).
    pub const BODY: usize = 5;
    pub const BODY_LEN: usize = 6;
}

/// Read a random access response out of a GSMTAP MAC-LTE payload.
///
/// Returns None for any MAC frame that is not a random access response, which
/// is most of them, rather than treating an unexpected shape as an error worth
/// reporting.
fn parse_mac_rar(payload: &[u8]) -> Option<MacRandomAccessResponse> {
    use mac_rar_offsets::*;
    if payload.len() < BODY + BODY_LEN {
        return None;
    }
    let body = &payload[BODY..BODY + BODY_LEN];
    // One reserved bit, then eleven bits of timing advance.
    let timing_advance = (u16::from(body[0] & 0b0111_1111) << 4) | (u16::from(body[1]) >> 4);
    let tc_rnti = u16::from_be_bytes([body[4], body[5]]);
    let preamble = payload[SUBHEADER] & 0b0011_1111;
    Some(MacRandomAccessResponse {
        timing_advance,
        tc_rnti,
        preamble,
    })
}

impl TryFrom<&GsmtapMessage> for InformationElement {
    type Error = InformationElementError;

    fn try_from(gsmtap_msg: &GsmtapMessage) -> Result<Self, Self::Error> {
        match gsmtap_msg.header.gsmtap_type {
            GsmtapType::LteRrc(lte_rrc_subtype) => {
                use LteInformationElement as R;
                use LteRrcSubtype as L;
                let lte = match lte_rrc_subtype {
                    L::DlCcch => R::DlCcch(decode(&gsmtap_msg.payload)?),
                    L::DlDcch => R::DlDcch(Box::new(decode(&gsmtap_msg.payload)?)),
                    L::UlCcch => R::UlCcch(decode(&gsmtap_msg.payload)?),
                    L::UlDcch => R::UlDcch(decode(&gsmtap_msg.payload)?),
                    L::BcchBch => R::BcchBch(decode(&gsmtap_msg.payload)?),
                    L::BcchDlSch => R::BcchDlSch(decode(&gsmtap_msg.payload)?),
                    L::PCCH => R::PCCH(decode(&gsmtap_msg.payload)?),
                    L::MCCH => R::MCCH(decode(&gsmtap_msg.payload)?),
                    L::ScMcch => R::ScMcch(decode(&gsmtap_msg.payload)?),
                    L::BcchBchMbms => R::BcchBchMbms(decode(&gsmtap_msg.payload)?),
                    L::BcchDlSchBr => R::BcchDlSchBr(decode(&gsmtap_msg.payload)?),
                    L::BcchDlSchMbms => R::BcchDlSchMbms(decode(&gsmtap_msg.payload)?),
                    L::SbcchSlBch => R::SbcchSlBch(decode(&gsmtap_msg.payload)?),
                    L::SbcchSlBchV2x => R::SbcchSlBchV2x(decode(&gsmtap_msg.payload)?),
                    _ => {
                        return Err(InformationElementError::UnsupportedGsmtapType(
                            gsmtap_msg.header.gsmtap_type,
                        ));
                    }
                };
                Ok(InformationElement::LTE(Box::new(lte)))
            }
            GsmtapType::LteNas(LteNasSubtype::Plain) => {
                let msg = NASMessage::parse(&gsmtap_msg.payload)?;
                Ok(InformationElement::LTE(Box::new(
                    LteInformationElement::NAS(msg),
                )))
            }
            // A random access response, which is where timing advance lives.
            GsmtapType::LteMacFramed | GsmtapType::LteMac => {
                let Some(rar) = parse_mac_rar(&gsmtap_msg.payload) else {
                    return Err(InformationElementError::UnsupportedGsmtapType(
                        gsmtap_msg.header.gsmtap_type,
                    ));
                };
                Ok(InformationElement::LTE(Box::new(
                    LteInformationElement::MacRar(rar),
                )))
            }
            // 2G Layer 3 signalling, kept as raw bytes rather than parsed. It is
            // carried so analysers such as the RRLP location detector can read
            // it; the bytes are already written to the PCAP for Wireshark too.
            GsmtapType::Um(channel) => {
                Ok(InformationElement::GSM(Box::new(GsmInformationElement {
                    channel,
                    bytes: gsmtap_msg.payload.clone(),
                })))
            }
            _ => Err(InformationElementError::UnsupportedGsmtapType(
                gsmtap_msg.header.gsmtap_type,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::diaglog::mac::test::mac_rach_test_packets_from_scat;
    use crate::gsmtap::mac::mac_subpacket_to_gsmtap;

    /// The whole chain, on real captured bytes: diag packet, to the GSMTAP
    /// frame Rayhunter writes for Wireshark, back to what an analyser sees.
    ///
    /// Reading the fields back out of the frame rather than plumbing them
    /// separately means the offsets have to be right, and offsets derived by
    /// hand are exactly the thing that silently reads a neighbouring field
    /// instead. These expected values were decoded from the first SCAT test
    /// packet by hand: timing advance 4, TC-RNTI 0x1a23, preamble 27.
    #[test]
    fn a_random_access_response_survives_the_round_trip() {
        let packets = mac_rach_test_packets_from_scat();
        let subpacket = &packets[0].subpackets[0];
        let gsmtap = mac_subpacket_to_gsmtap(&subpacket.body)
            .expect("converts")
            .expect("is a random access response");

        let element = InformationElement::try_from(&gsmtap).expect("reaches the analysers");
        let InformationElement::LTE(lte) = element else {
            panic!("expected an LTE element");
        };
        let LteInformationElement::MacRar(rar) = *lte else {
            panic!("expected a random access response");
        };

        assert_eq!(rar.timing_advance, 4, "timing advance");
        assert_eq!(rar.tc_rnti, 0x1a23, "tc-rnti");
        assert_eq!(rar.preamble, 27, "preamble");
    }

    /// Timing advance is eleven bits, so it must survive values that do not
    /// fit in a byte. A wrong shift reads small values correctly and large
    /// ones as nonsense, which is the failure that hides.
    #[test]
    fn a_large_timing_advance_is_not_truncated() {
        use crate::gsmtap::{GsmtapHeader, GsmtapMessage};
        // Header, tag, subheader, then R(1) TA(11) grant(20) TC-RNTI(16).
        // TA = 0x7ff, the largest an eleven bit field holds.
        let mut payload = vec![0x01, 0x00, 0x02, 0x01, 0x00];
        payload.extend_from_slice(&[0x7f, 0xf0, 0x00, 0x00, 0xab, 0xcd]);
        let msg = GsmtapMessage {
            header: GsmtapHeader::new(GsmtapType::LteMacFramed),
            payload,
        };
        let element = InformationElement::try_from(&msg).expect("parses");
        let InformationElement::LTE(lte) = element else {
            panic!("expected LTE");
        };
        let LteInformationElement::MacRar(rar) = *lte else {
            panic!("expected a random access response");
        };
        assert_eq!(rar.timing_advance, 0x7ff);
        assert_eq!(rar.tc_rnti, 0xabcd);
    }

    /// A truncated frame must not be read as a response full of zeroes.
    #[test]
    fn a_short_frame_is_refused() {
        use crate::gsmtap::{GsmtapHeader, GsmtapMessage};
        let msg = GsmtapMessage {
            header: GsmtapHeader::new(GsmtapType::LteMacFramed),
            payload: vec![0x01, 0x00, 0x02, 0x01, 0x00, 0x00],
        };
        assert!(InformationElement::try_from(&msg).is_err());
    }
}
