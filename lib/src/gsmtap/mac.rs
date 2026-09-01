//! The structs/enum values defined here are derived from a number of sources:
//! * SCAT's construction of MAC GSMTAP packets: https://github.com/fgsect/scat/blob/9763cb5b1dcd5ee980f5b0ead9a8d520c8c51a51/src/scat/parsers/qualcomm/diagltelogparser.py#L562-L640
//! * https://www.sharetechnote.com/html/MAC_LTE.html#MAC_PDU_Structure_RAR
//! * 3GPP's TS 36.321, mostly sections 6.1.4, 6.1.5, and 6.1.6

use deku::prelude::*;

use crate::{
    diag::diaglog::mac::SubpacketBody,
    gsmtap::{GsmtapHeader, GsmtapMessage, GsmtapType},
};
use deku::{DekuContainerWrite, DekuError};

#[derive(DekuRead, DekuWrite)]
pub struct Header {
    pub radio_type: RadioType,
    pub direction: Direction,
    pub rnti_type: RntiType,
}

#[derive(DekuRead, DekuWrite)]
#[deku(id_type = "u8")]
pub enum RadioType {
    #[deku(id = "1")]
    Fdd,
    #[deku(id = "2")]
    Tdd,
}

#[derive(DekuRead, DekuWrite)]
#[deku(id_type = "u8")]
pub enum Direction {
    #[deku(id = "0")]
    Uplink,
    #[deku(id = "1")]
    Downlink,
}

#[derive(DekuRead, DekuWrite)]
#[deku(id_type = "u8")]
pub enum RntiType {
    #[deku(id = "0")]
    No,
    #[deku(id = "1")]
    P,
    #[deku(id = "2")]
    Ra,
    #[deku(id = "3")]
    C,
    #[deku(id = "4")]
    Si,
    #[deku(id = "5")]
    Sps,
    #[deku(id = "6")]
    M,
    #[deku(id = "7")]
    Sl,
    #[deku(id = "9")]
    Sc,
    #[deku(id = "10")]
    G,
}

#[derive(DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct ETRAPIDSubheader {
    #[deku(bits = 1)]
    pub extended: bool,
    #[deku(bits = 1)]
    pub type_field: bool,
    #[deku(bits = 6)]
    pub rapid: u8,
}

#[derive(DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct RACHResponse {
    #[deku(pad_bits_before = "1", bits = 11)]
    pub tac: u16,
    #[deku(bits = 20)]
    pub ul_grant: u32,
    pub tc_rnti: u16,
}

/// Wireshark's MAC-LTE tags, from `packet-mac-lte.h`.
///
/// Only the two needed here. The payload tag must be last, since everything
/// after it is the PDU itself.
const MAC_LTE_PAYLOAD_TAG: u8 = 0x01;
const MAC_LTE_FRAME_SUBFRAME_TAG: u8 = 0x04;

/// The mapping as a plain byte, so a test can assert the value that actually
/// reaches the capture rather than the enum on the way there.
#[cfg(test)]
pub fn wireshark_rnti_type_for_test(qualcomm: u8) -> u8 {
    use deku::DekuContainerWrite;
    wireshark_rnti_type(qualcomm).to_bytes().unwrap()[0]
}

/// Translate Qualcomm's RNTI type into the one Wireshark expects.
///
/// The two do not agree: Qualcomm's 0 is a C-RNTI, which is 3 to Wireshark, so
/// passing the value through unchanged would label every ordinary transmission
/// as something else. Anything unrecognised becomes NO_RNTI rather than a
/// guess, since a confidently wrong label is worse than an absent one.
fn wireshark_rnti_type(qualcomm: u8) -> RntiType {
    match qualcomm {
        0 | 4 => RntiType::C,
        2 => RntiType::P,
        3 => RntiType::Ra,
        5 => RntiType::Si,
        _ => RntiType::No,
    }
}

/// Build one GSMTAP MAC-LTE frame for a transport block.
///
/// The frame carries the MAC header exactly as it was on the air, ahead of it
/// the context Wireshark's dissector needs to make sense of it: which radio,
/// which direction, what kind of identity, and which frame and subframe.
fn transport_block_gsmtap(
    downlink: bool,
    rnti_type: u8,
    sfn_subfn: u16,
    mac_header: &[u8],
) -> Result<GsmtapMessage, DekuError> {
    let mut payload = Vec::with_capacity(mac_header.len() + 7);
    payload.extend(
        Header {
            radio_type: RadioType::Fdd,
            direction: if downlink {
                Direction::Downlink
            } else {
                Direction::Uplink
            },
            rnti_type: wireshark_rnti_type(rnti_type),
        }
        .to_bytes()?,
    );
    payload.push(MAC_LTE_FRAME_SUBFRAME_TAG);
    // Network order, with the frame number in the twelve high bits and the
    // subframe in the four low ones, which is how the diag record already
    // packs them.
    payload.extend_from_slice(&sfn_subfn.to_be_bytes());
    payload.push(MAC_LTE_PAYLOAD_TAG);
    payload.extend_from_slice(mac_header);

    Ok(GsmtapMessage {
        header: GsmtapHeader::new(GsmtapType::LteMacFramed),
        payload,
    })
}

/// Every transport block in a downlink or uplink subpacket, as GSMTAP frames.
///
/// One subpacket usually holds several blocks, so this returns a frame per
/// block rather than one per record. Measured on an Orbic: downlink records
/// average two and reach ten, uplink reach twenty three. Keeping only the
/// first would throw most of the capture away.
pub fn mac_transport_to_gsmtap(subpacket: &SubpacketBody) -> Result<Vec<GsmtapMessage>, DekuError> {
    match subpacket {
        SubpacketBody::DlTransportBlock(blocks) => blocks
            .samples
            .iter()
            .map(|b| transport_block_gsmtap(true, b.rnti_type(), b.sfn_subfn(), b.mac_header()))
            .collect(),
        SubpacketBody::UlTransportBlock(blocks) => blocks
            .samples
            .iter()
            .map(|b| transport_block_gsmtap(false, b.rnti_type(), b.sfn_subfn(), b.mac_header()))
            .collect(),
        _ => Ok(Vec::new()),
    }
}

pub fn mac_subpacket_to_gsmtap(
    subpacket: &SubpacketBody,
) -> Result<Option<GsmtapMessage>, DekuError> {
    match subpacket {
        SubpacketBody::RachAttempt(attempt) => {
            let (Some(msg1), Some(msg2), Some(msg3)) =
                (attempt.get_msg1(), attempt.get_msg2(), attempt.get_msg3())
            else {
                return Ok(None);
            };
            let mut payload = Vec::new();
            payload.extend(
                Header {
                    radio_type: RadioType::Fdd,
                    direction: Direction::Downlink,
                    rnti_type: RntiType::Ra,
                }
                .to_bytes()?,
            );
            payload.push(0x01); // MAC Payload Tag
            payload.extend(
                ETRAPIDSubheader {
                    extended: false,
                    type_field: true,
                    rapid: msg1.get_preamble_index() & 0b111111,
                }
                .to_bytes()?,
            );
            payload.extend(
                RACHResponse {
                    tac: msg2.ta & 0b11111111111,
                    ul_grant: msg3.get_grant(),
                    tc_rnti: msg2.tc_rnti,
                }
                .to_bytes()?,
            );
            Ok(Some(GsmtapMessage {
                header: GsmtapHeader::new(GsmtapType::LteMacFramed),
                payload,
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::diag::diaglog::mac::Packet;
    use crate::diag::diaglog::mac::test::mac_rach_test_packets_from_scat;
    use crate::test_util::unhexlify;

    use super::*;

    fn assert_mac_gsmtap(packet: &Packet, expected_hexstr: Option<&str>) {
        assert_eq!(packet.subpackets.len(), 1);
        let subpacket = &packet.subpackets[0];
        let result = mac_subpacket_to_gsmtap(&subpacket.body).unwrap();
        match (result, expected_hexstr) {
            (Some(msg), Some(hexstr)) => {
                let (_, data) = unhexlify(hexstr);
                // SCAT's test cases use GSMTAP v3, but we're on V2, so skip
                // their GSMTAP header and just compare the payloads
                let expected_payload = &data.into_inner().into_inner()[34..];
                assert_eq!(&msg.payload, expected_payload);
            }
            (Some(msg), None) => panic!("expected no GSMTAP message, got {msg:?}"),
            (None, Some(_)) => panic!("expected GSMTAP message, got None"),
            _ => {}
        }
    }

    #[test]
    fn test_mac_rach() {
        // test data from SCAT unit tests: https://github.com/fgsect/scat/blob/9763cb5b1dcd5ee980f5b0ead9a8d520c8c51a51/tests/test_diagltelogparser.py#L129
        let test_packets = mac_rach_test_packets_from_scat();
        assert_mac_gsmtap(
            &test_packets[0],
            Some(
                "03000009040000000000000c0000000012d53d80000000000002000400000000fffe010102015b00411c181a23",
            ),
        );
        assert_mac_gsmtap(
            &test_packets[1],
            Some(
                "03000009040000000000000c0000000012d53d80000000000002000400000000fffe010102015800b0a2b461c6",
            ),
        );
        assert_mac_gsmtap(&test_packets[2], None);
        assert_mac_gsmtap(
            &test_packets[3],
            Some(
                "03000009040000000000000c0000000012d53d80000000000002000400000ea5fffe010102014a0070e218481c",
            ),
        );
        assert_mac_gsmtap(&test_packets[4], None);
        assert_mac_gsmtap(
            &test_packets[5],
            Some(
                "03000009040000000000000c0000000012d53d80000000000002000400000d16fffe0101020153005146b45aad",
            ),
        );
    }
}
