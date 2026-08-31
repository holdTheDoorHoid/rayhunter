use crate::diag::Message;
use crate::diag::diaglog::{LogBody, Nas4GMessageDirection, Timestamp};
use crate::gsmtap::mac::mac_subpacket_to_gsmtap;
use crate::gsmtap::{
    GsmtapHeader, GsmtapMessage, GsmtapType, LteNasSubtype, LteRrcSubtype, UmSubtype,
    UmtsRrcSubtype,
};
use crate::log_codes;

use log::{debug, warn};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GsmtapParserError {
    #[error("Invalid LteRrcOtaMessage ext header version {0}")]
    InvalidLteRrcOtaExtHeaderVersion(u8),
    #[error("Invalid LteRrcOtaMessage header/PDU number combination: {0}/{1}")]
    InvalidLteRrcOtaHeaderPduNum(u8, u8),
    #[error("Invalid LteMacRachResponse packet: {0}")]
    InvalidLteMacRachResponse(String),
}

pub fn parse(msg: Message) -> Result<Option<(Timestamp, GsmtapMessage)>, GsmtapParserError> {
    if let Message::Log {
        timestamp, body, ..
    } = msg
    {
        match log_to_gsmtap(body)? {
            Some(msg) => Ok(Some((timestamp, msg))),
            None => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// The GSMTAP subtype for a GSM RR logical channel.
///
/// The numbering on the left is Qualcomm's, from `log_codes`; the values on
/// the right are GSMTAP's. They are not the same numbering, which is the
/// reason this exists rather than a cast.
fn gsm_rr_subtype(channel_type: u8) -> Option<UmSubtype> {
    Some(match channel_type as u32 {
        log_codes::DCCH => UmSubtype::Sdcch,
        log_codes::BCCH => UmSubtype::Bcch,
        log_codes::L2_RACH | log_codes::L2_RACH_WITH_NO_DELAY => UmSubtype::Rach,
        log_codes::CCCH => UmSubtype::Ccch,
        log_codes::SACCH => UmSubtype::Sdcch,
        log_codes::SDCCH => UmSubtype::Sdcch,
        log_codes::FACCH_F => UmSubtype::TchF,
        log_codes::FACCH_H => UmSubtype::TchH,
        _ => return None,
    })
}

fn log_to_gsmtap(value: LogBody) -> Result<Option<GsmtapMessage>, GsmtapParserError> {
    match value {
        LogBody::LteRrcOtaMessage {
            ext_header_version,
            packet,
        } => {
            let gsmtap_type = match ext_header_version {
                0x02 | 0x03 | 0x04 | 0x06 | 0x07 | 0x08 | 0x0d | 0x16 => match packet.get_pdu_num()
                {
                    1 => GsmtapType::LteRrc(LteRrcSubtype::BcchBch),
                    2 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSch),
                    3 => GsmtapType::LteRrc(LteRrcSubtype::MCCH),
                    4 => GsmtapType::LteRrc(LteRrcSubtype::PCCH),
                    5 => GsmtapType::LteRrc(LteRrcSubtype::DlCcch),
                    6 => GsmtapType::LteRrc(LteRrcSubtype::DlDcch),
                    7 => GsmtapType::LteRrc(LteRrcSubtype::UlCcch),
                    8 => GsmtapType::LteRrc(LteRrcSubtype::UlDcch),
                    pdu => {
                        return Err(GsmtapParserError::InvalidLteRrcOtaHeaderPduNum(
                            ext_header_version,
                            pdu,
                        ));
                    }
                },
                0x09 | 0x0c => match packet.get_pdu_num() {
                    8 => GsmtapType::LteRrc(LteRrcSubtype::BcchBch),
                    9 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSch),
                    10 => GsmtapType::LteRrc(LteRrcSubtype::MCCH),
                    11 => GsmtapType::LteRrc(LteRrcSubtype::PCCH),
                    12 => GsmtapType::LteRrc(LteRrcSubtype::DlCcch),
                    13 => GsmtapType::LteRrc(LteRrcSubtype::DlDcch),
                    14 => GsmtapType::LteRrc(LteRrcSubtype::UlCcch),
                    15 => GsmtapType::LteRrc(LteRrcSubtype::UlDcch),
                    pdu => {
                        return Err(GsmtapParserError::InvalidLteRrcOtaHeaderPduNum(
                            ext_header_version,
                            pdu,
                        ));
                    }
                },
                0x0e..=0x10 => match packet.get_pdu_num() {
                    1 => GsmtapType::LteRrc(LteRrcSubtype::BcchBch),
                    2 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSch),
                    4 => GsmtapType::LteRrc(LteRrcSubtype::MCCH),
                    5 => GsmtapType::LteRrc(LteRrcSubtype::PCCH),
                    6 => GsmtapType::LteRrc(LteRrcSubtype::DlCcch),
                    7 => GsmtapType::LteRrc(LteRrcSubtype::DlDcch),
                    8 => GsmtapType::LteRrc(LteRrcSubtype::UlCcch),
                    9 => GsmtapType::LteRrc(LteRrcSubtype::UlDcch),
                    pdu => {
                        return Err(GsmtapParserError::InvalidLteRrcOtaHeaderPduNum(
                            ext_header_version,
                            pdu,
                        ));
                    }
                },
                0x13 | 0x1a | 0x1b => match packet.get_pdu_num() {
                    1 => GsmtapType::LteRrc(LteRrcSubtype::BcchBch),
                    3 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSch),
                    6 => GsmtapType::LteRrc(LteRrcSubtype::MCCH),
                    7 => GsmtapType::LteRrc(LteRrcSubtype::PCCH),
                    8 => GsmtapType::LteRrc(LteRrcSubtype::DlCcch),
                    9 => GsmtapType::LteRrc(LteRrcSubtype::DlDcch),
                    10 => GsmtapType::LteRrc(LteRrcSubtype::UlCcch),
                    11 => GsmtapType::LteRrc(LteRrcSubtype::UlDcch),
                    45 => GsmtapType::LteRrc(LteRrcSubtype::BcchBchNb),
                    46 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSchNb),
                    47 => GsmtapType::LteRrc(LteRrcSubtype::PcchNb),
                    48 => GsmtapType::LteRrc(LteRrcSubtype::DlCcchNb),
                    49 => GsmtapType::LteRrc(LteRrcSubtype::DlDcchNb),
                    50 => GsmtapType::LteRrc(LteRrcSubtype::UlCcchNb),
                    52 => GsmtapType::LteRrc(LteRrcSubtype::UlDcchNb),
                    pdu => {
                        return Err(GsmtapParserError::InvalidLteRrcOtaHeaderPduNum(
                            ext_header_version,
                            pdu,
                        ));
                    }
                },
                0x14 | 0x18 | 0x19 => match packet.get_pdu_num() {
                    1 => GsmtapType::LteRrc(LteRrcSubtype::BcchBch),
                    2 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSch),
                    4 => GsmtapType::LteRrc(LteRrcSubtype::MCCH),
                    5 => GsmtapType::LteRrc(LteRrcSubtype::PCCH),
                    6 => GsmtapType::LteRrc(LteRrcSubtype::DlCcch),
                    7 => GsmtapType::LteRrc(LteRrcSubtype::DlDcch),
                    8 => GsmtapType::LteRrc(LteRrcSubtype::UlCcch),
                    9 => GsmtapType::LteRrc(LteRrcSubtype::UlDcch),
                    54 => GsmtapType::LteRrc(LteRrcSubtype::BcchBchNb),
                    55 => GsmtapType::LteRrc(LteRrcSubtype::BcchDlSchNb),
                    56 => GsmtapType::LteRrc(LteRrcSubtype::PcchNb),
                    57 => GsmtapType::LteRrc(LteRrcSubtype::DlCcchNb),
                    58 => GsmtapType::LteRrc(LteRrcSubtype::DlDcchNb),
                    59 => GsmtapType::LteRrc(LteRrcSubtype::UlCcchNb),
                    61 => GsmtapType::LteRrc(LteRrcSubtype::UlDcchNb),
                    pdu => {
                        return Err(GsmtapParserError::InvalidLteRrcOtaHeaderPduNum(
                            ext_header_version,
                            pdu,
                        ));
                    }
                },
                _ => {
                    return Err(GsmtapParserError::InvalidLteRrcOtaExtHeaderVersion(
                        ext_header_version,
                    ));
                }
            };
            let mut header = GsmtapHeader::new(gsmtap_type);
            header.arfcn = (packet.get_earfcn() as u16) & 0x3FFF;
            header.frame_number = packet.get_sfn();
            header.subslot = packet.get_subfn();
            Ok(Some(GsmtapMessage {
                header,
                payload: packet.take_payload(),
            }))
        }
        LogBody::Nas4GMessage { msg, direction, .. } => {
            // currently we only handle "plain" (i.e. non-secure) NAS messages
            let mut header = GsmtapHeader::new(GsmtapType::LteNas(LteNasSubtype::Plain));
            header.uplink = matches!(direction, Nas4GMessageDirection::Uplink);
            Ok(Some(GsmtapMessage {
                header,
                payload: msg,
            }))
        }
        LogBody::LteMacRachResponse { packet } => {
            if packet.subpackets.len() > 1 {
                warn!(
                    "expected 1 MAC subpacket for LogBody::LteMacRachResponse, but got {}! ignoring all but the first",
                    packet.subpackets.len()
                );
            }
            let Some(subpacket) = packet.subpackets.first() else {
                return Err(GsmtapParserError::InvalidLteMacRachResponse(
                    "no subpackets".to_string(),
                ));
            };
            mac_subpacket_to_gsmtap(&subpacket.body).map_err(|err| {
                GsmtapParserError::InvalidLteMacRachResponse(format!(
                    "unable to serialize GSMTAP payload: {err:?}"
                ))
            })
        }
        // 2G and 3G signalling. These were being collected into the QMDL and
        // then dropped here, so they never reached the PCAP and could not be
        // looked at in Wireshark at all. Rayhunter does not analyse them, but
        // "we cannot analyse it" is a poor reason to throw a capture away, and
        // in much of the world it is the traffic that matters.
        // See EFForg/rayhunter#1013.
        LogBody::GsmRrSignallingMessage {
            channel_type, msg, ..
        } => {
            // Layer 3 signalling on the Um air interface. The channel it
            // arrived on is what GSMTAP calls the subtype.
            let Some(subtype) = gsm_rr_subtype(channel_type) else {
                debug!("gsmtap_sink: unknown GSM RR channel type {channel_type:#04x}");
                return Ok(None);
            };
            Ok(Some(GsmtapMessage {
                header: GsmtapHeader::new(GsmtapType::Um(subtype)),
                payload: msg,
            }))
        }
        LogBody::GprsMacSignallingMessage {
            channel_type, msg, ..
        } => {
            // GPRS RLC/MAC control messages. GSMTAP carries these on the
            // packet channels rather than the circuit switched ones.
            let subtype = match channel_type as u32 {
                log_codes::PACCH_RRBP_CHANNEL
                | log_codes::UL_PACCH_CHANNEL
                | log_codes::DL_PACCH_CHANNEL => UmSubtype::Pacch,
                log_codes::PACKET_CHANNEL_REQUEST => UmSubtype::Rach,
                _ => UmSubtype::Pdch,
            };
            let mut header = GsmtapHeader::new(GsmtapType::Um(subtype));
            header.uplink = (channel_type as u32) == log_codes::UL_PACCH_CHANNEL
                || (channel_type as u32) == log_codes::PACKET_CHANNEL_REQUEST;
            Ok(Some(GsmtapMessage {
                header,
                payload: msg,
            }))
        }
        LogBody::WcdmaSignallingMessage {
            channel_type, msg, ..
        } => {
            // UMTS RRC. The channel numbering here is the one GSMTAP uses for
            // its own subtypes, so it passes straight through, but only the
            // values GSMTAP defines are accepted rather than trusting the byte.
            let Ok(subtype) = UmtsRrcSubtype::try_from(channel_type) else {
                debug!("gsmtap_sink: unknown WCDMA channel type {channel_type:#04x}");
                return Ok(None);
            };
            let mut header = GsmtapHeader::new(GsmtapType::UmtsRrc(subtype));
            header.uplink = matches!(
                subtype,
                UmtsRrcSubtype::UlDcch | UmtsRrcSubtype::UlCcch | UmtsRrcSubtype::UlShcch
            );
            Ok(Some(GsmtapMessage {
                header,
                payload: msg,
            }))
        }
        LogBody::UmtsNasOtaMessage { is_uplink, msg, .. } => {
            // GSMTAP has no UMTS NAS type of its own. SCAT and Wireshark both
            // treat these as GSM layer 3 on a dedicated channel, which is what
            // they are: the same 24.008 messages 2G carries.
            let mut header = GsmtapHeader::new(GsmtapType::Um(UmSubtype::Sdcch));
            header.uplink = is_uplink != 0;
            Ok(Some(GsmtapMessage {
                header,
                payload: msg,
            }))
        }
        _ => {
            debug!("gsmtap_sink: ignoring unhandled log type: {value:?}");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsmtap::GsmtapType;
    use deku::DekuContainerWrite;

    #[test]
    fn test_arfcn_exceeding_14_bits_does_not_panic() {
        let mut header = GsmtapHeader::new(GsmtapType::LteRrc(LteRrcSubtype::DlDcch));
        // EARFCN 54540 (band 46) exceeds 14-bit max of 16383
        let large_earfcn: u32 = 54540;
        header.arfcn = (large_earfcn as u16) & 0x3FFF;
        let msg = GsmtapMessage {
            header,
            payload: vec![0x00],
        };
        // This would panic before the fix with "bit size of input is larger than bit requested size"
        assert!(msg.to_bytes().is_ok());
    }
}

#[cfg(test)]
mod legacy_radio_tests {
    use super::*;

    /// Qualcomm's channel numbering and GSMTAP's subtype numbering are not the
    /// same, which is the whole reason the mapping exists. Passing the byte
    /// through would label a BCCH message as a CCCH one and quietly produce a
    /// capture that says the wrong thing.
    #[test]
    fn gsm_channels_map_to_their_gsmtap_subtypes() {
        assert_eq!(gsm_rr_subtype(log_codes::BCCH as u8), Some(UmSubtype::Bcch));
        assert_eq!(gsm_rr_subtype(log_codes::CCCH as u8), Some(UmSubtype::Ccch));
        assert_eq!(
            gsm_rr_subtype(log_codes::SDCCH as u8),
            Some(UmSubtype::Sdcch)
        );
        assert_eq!(
            gsm_rr_subtype(log_codes::FACCH_F as u8),
            Some(UmSubtype::TchF)
        );
        assert_eq!(
            gsm_rr_subtype(log_codes::FACCH_H as u8),
            Some(UmSubtype::TchH)
        );
    }

    /// Both random access encodings are the same channel as far as GSMTAP is
    /// concerned.
    #[test]
    fn both_rach_encodings_are_rach() {
        assert_eq!(
            gsm_rr_subtype(log_codes::L2_RACH as u8),
            Some(UmSubtype::Rach)
        );
        assert_eq!(
            gsm_rr_subtype(log_codes::L2_RACH_WITH_NO_DELAY as u8),
            Some(UmSubtype::Rach)
        );
    }

    /// An unrecognised channel is dropped rather than guessed at. A mislabelled
    /// packet in a capture is worse than an absent one, because somebody will
    /// read it and believe it.
    #[test]
    fn an_unknown_channel_is_refused() {
        assert_eq!(gsm_rr_subtype(0x7f), None);
        assert_eq!(gsm_rr_subtype(0xff), None);
    }

    /// 2G signalling now reaches the PCAP instead of being dropped. This is
    /// the behaviour EFForg/rayhunter#1013 asked for: Rayhunter cannot analyse
    /// these, but that is a poor reason to throw the capture away.
    #[test]
    fn gsm_signalling_becomes_a_gsmtap_message() {
        let body = LogBody::GsmRrSignallingMessage {
            channel_type: log_codes::BCCH as u8,
            message_type: 0,
            length: 3,
            msg: vec![1, 2, 3],
        };
        let out = log_to_gsmtap(body).expect("should convert");
        let msg = out.expect("should produce a message");
        assert_eq!(msg.header.gsmtap_type, GsmtapType::Um(UmSubtype::Bcch));
        assert_eq!(msg.payload, vec![1, 2, 3]);
    }

    #[test]
    fn wcdma_signalling_becomes_a_gsmtap_message_and_keeps_its_direction() {
        let body = LogBody::WcdmaSignallingMessage {
            channel_type: UmtsRrcSubtype::UlDcch as u8,
            radio_bearer: 0,
            length: 2,
            msg: vec![9, 9],
        };
        let msg = log_to_gsmtap(body)
            .unwrap()
            .expect("should produce a message");
        assert_eq!(
            msg.header.gsmtap_type,
            GsmtapType::UmtsRrc(UmtsRrcSubtype::UlDcch)
        );
        assert!(msg.header.uplink, "uplink channel must be marked uplink");

        let downlink = LogBody::WcdmaSignallingMessage {
            channel_type: UmtsRrcSubtype::DlDcch as u8,
            radio_bearer: 0,
            length: 1,
            msg: vec![1],
        };
        let msg = log_to_gsmtap(downlink).unwrap().unwrap();
        assert!(!msg.header.uplink);
    }

    #[test]
    fn umts_nas_keeps_its_direction() {
        let up = LogBody::UmtsNasOtaMessage {
            is_uplink: 1,
            length: 1,
            msg: vec![7],
        };
        assert!(log_to_gsmtap(up).unwrap().unwrap().header.uplink);

        let down = LogBody::UmtsNasOtaMessage {
            is_uplink: 0,
            length: 1,
            msg: vec![7],
        };
        assert!(!log_to_gsmtap(down).unwrap().unwrap().header.uplink);
    }

    /// An unrecognised WCDMA channel is dropped rather than cast blindly, since
    /// the subtype byte comes off the air.
    #[test]
    fn an_unknown_wcdma_channel_is_refused() {
        let body = LogBody::WcdmaSignallingMessage {
            channel_type: 0xfe,
            radio_bearer: 0,
            length: 1,
            msg: vec![0],
        };
        assert!(log_to_gsmtap(body).unwrap().is_none());
    }
}
