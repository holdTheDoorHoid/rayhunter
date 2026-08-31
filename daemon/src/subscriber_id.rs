//! The identities the device sends about itself.
//!
//! IMSI, IMEI and the temporary identity the network assigns in their place.
//! These matter twice over: knowing your own is useful, and knowing *when* the
//! network asked for the permanent one is the whole point of the identity
//! request detector. A network that keeps asking for an IMSI rather than
//! accepting a temporary identity is behaving the way an IMSI catcher does.
//!
//! Decoded here rather than taken from the NAS parser because the parser does
//! not read them: `pycrate_rs` generates the identity field as an empty struct,
//! reads its bytes and discards them. The encoding is small and fully specified
//! in 3GPP TS 24.301 section 9.9.3.12, so it is decoded from the raw payload.

use serde::Serialize;

/// One identity, as sent on the air.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "apidocs", derive(utoipa::ToSchema))]
pub enum Identity {
    /// The permanent subscriber identity. The thing an IMSI catcher wants.
    Imsi(String),
    /// The permanent equipment identity, identifying the hardware.
    Imei(String),
    /// Equipment identity with software version.
    Imeisv(String),
    /// A temporary identity assigned by the network, as hex. Rotating these is
    /// what is supposed to stop a subscriber being followed between sessions.
    Tmsi(String),
}

/// Type of identity, from the low three bits of the first octet.
const TYPE_IMSI: u8 = 1;
const TYPE_IMEI: u8 = 2;
const TYPE_IMEISV: u8 = 3;
const TYPE_GUTI: u8 = 6;

/// Decode an EPS mobile identity information element.
///
/// The digit identities pack two decimal digits per octet, low nibble first,
/// with the very first digit sharing the type octet. Bit 4 of that octet says
/// whether the count is odd, which decides whether the final high nibble is a
/// digit or padding. Getting that bit wrong appends a spurious 15 to every
/// even length identity, which is why it is tested both ways.
pub fn decode_eps_mobile_identity(bytes: &[u8]) -> Option<Identity> {
    let first = *bytes.first()?;
    let id_type = first & 0b111;
    let odd = (first & 0b1000) != 0;

    match id_type {
        TYPE_IMSI | TYPE_IMEI | TYPE_IMEISV => {
            let mut digits = String::new();
            // The first digit rides in the high nibble of the type octet.
            digits.push(char::from_digit((first >> 4) as u32, 10)?);
            for (i, byte) in bytes[1..].iter().enumerate() {
                digits.push(char::from_digit((byte & 0x0f) as u32, 10)?);
                let is_last = i == bytes.len() - 2;
                // On an even length identity the final high nibble is filler,
                // conventionally 0xf, and must not be read as a digit.
                if is_last && !odd {
                    break;
                }
                digits.push(char::from_digit((byte >> 4) as u32, 10)?);
            }
            match id_type {
                TYPE_IMSI => Some(Identity::Imsi(digits)),
                TYPE_IMEI => Some(Identity::Imei(digits)),
                _ => Some(Identity::Imeisv(digits)),
            }
        }
        TYPE_GUTI => {
            // 1 octet of type, 3 of PLMN, 2 of MME group, 1 of MME code, then
            // the four byte M-TMSI, which is the part that identifies a
            // subscriber for the life of the assignment.
            if bytes.len() < 11 {
                return None;
            }
            let tmsi = u32::from_be_bytes([bytes[7], bytes[8], bytes[9], bytes[10]]);
            Some(Identity::Tmsi(format!("{tmsi:08x}")))
        }
        _ => None,
    }
}

/// Protocol discriminator for EPS mobility management, the half of NAS that
/// carries identities. Session management is 2 and carries none.
const PD_EMM: u8 = 7;

/// NAS message types that carry an identity we can find.
const IDENTITY_RESPONSE: u8 = 0x56;
const ATTACH_REQUEST: u8 = 0x41;

/// Pull an identity out of a raw NAS message, if it carries one.
///
/// Only the messages where the identity sits at a known offset are read. The
/// alternative is walking the whole information element chain, which means
/// reimplementing the NAS parser to reach one field. A missed identity is not
/// harmful here; a wrongly decoded one would be.
pub fn identity_from_nas(payload: &[u8]) -> Option<Identity> {
    // Octet 0 carries the protocol discriminator in its low nibble and, for
    // mobility management, the security header type in its high nibble.
    let header = *payload.first()?;

    // Only EPS mobility management carries identities. Session management uses
    // the same first octet for an EPS bearer identity, so a bearer numbered
    // zero would otherwise look like a plain mobility message and its procedure
    // transaction identity would be read as a message type. Seen in a real
    // capture: eighteen session management messages and not one mobility
    // message that was not ciphered.
    if (header & 0x0f) != PD_EMM {
        return None;
    }

    // A non-zero security header means the message is ciphered and there is
    // nothing readable behind it.
    if (header >> 4) != 0 {
        return None;
    }
    let message_type = *payload.get(1)?;

    let ie_start = match message_type {
        // Identity Response: the identity is the only mandatory field, as a
        // length prefixed value straight after the message type.
        IDENTITY_RESPONSE => 2,
        // Attach Request: one octet of NAS key set identifier and EPS attach
        // type comes first, then the identity, again length prefixed.
        ATTACH_REQUEST => 3,
        _ => return None,
    };

    let length = *payload.get(ie_start)? as usize;
    let value = payload.get(ie_start + 1..ie_start + 1 + length)?;
    decode_eps_mobile_identity(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real IMSIs are 15 digits, which is odd, so the odd path is the common
    /// one. Built here the way a network encodes it: type and first digit in
    /// octet zero, then two digits per octet with the low nibble first.
    #[test]
    fn decodes_a_fifteen_digit_imsi() {
        // IMSI 310150123456789
        let bytes = [
            0x39, // digit 3, odd=1, type=IMSI
            0x01, // digits 1, 0
            0x51, // 1, 5
            0x10, // 0, 1
            0x32, // 2, 3
            0x54, // 4, 5
            0x76, // 6, 7
            0x98, // 8, 9
        ];
        assert_eq!(
            decode_eps_mobile_identity(&bytes),
            Some(Identity::Imsi("310150123456789".to_string()))
        );
    }

    /// An even length identity ends with a filler nibble that must not be read
    /// as a digit. Getting this wrong appends a spurious digit to every one.
    #[test]
    fn does_not_read_the_filler_nibble_as_a_digit() {
        // IMEI 12345678901234 (14 digits, even)
        let bytes = [0x12, 0x32, 0x54, 0x76, 0x98, 0x10, 0x32, 0xf4];
        let decoded = decode_eps_mobile_identity(&bytes);
        match decoded {
            Some(Identity::Imei(digits)) => {
                assert_eq!(digits.len(), 14, "got {digits}");
                assert!(!digits.ends_with('5'), "filler nibble leaked: {digits}");
            }
            other => panic!("expected an IMEI, got {other:?}"),
        }
    }

    #[test]
    fn recognises_each_identity_type() {
        assert!(matches!(
            decode_eps_mobile_identity(&[0x19, 0x03]),
            Some(Identity::Imsi(_))
        ));
        assert!(matches!(
            decode_eps_mobile_identity(&[0x1a, 0x03]),
            Some(Identity::Imei(_))
        ));
        assert!(matches!(
            decode_eps_mobile_identity(&[0x1b, 0x03]),
            Some(Identity::Imeisv(_))
        ));
    }

    /// The M-TMSI is the part of a GUTI that follows a subscriber, so it is the
    /// part worth showing. It sits at a fixed offset after the PLMN and MME
    /// identifiers.
    #[test]
    fn extracts_the_tmsi_from_a_guti() {
        let bytes = [
            0xf6, // type GUTI
            0x13, 0x01, 0x05, // PLMN
            0x00, 0x01, // MME group
            0x02, // MME code
            0xde, 0xad, 0xbe, 0xef, // M-TMSI
        ];
        assert_eq!(
            decode_eps_mobile_identity(&bytes),
            Some(Identity::Tmsi("deadbeef".to_string()))
        );
    }

    #[test]
    fn refuses_anything_it_cannot_read() {
        assert_eq!(decode_eps_mobile_identity(&[]), None);
        // Type 0 and 7 are not identity types.
        assert_eq!(decode_eps_mobile_identity(&[0x10, 0x00]), None);
        assert_eq!(decode_eps_mobile_identity(&[0x17, 0x00]), None);
        // A GUTI too short to contain an M-TMSI.
        assert_eq!(decode_eps_mobile_identity(&[0xf6, 0x13, 0x01]), None);
    }

    /// A ciphered message has nothing readable in it, and guessing at the
    /// ciphertext would produce confident nonsense.
    #[test]
    fn ciphered_messages_are_left_alone() {
        let ciphered = [
            0x27, 0x56, 0x08, 0x39, 0x01, 0x51, 0x10, 0x32, 0x54, 0x76, 0x98,
        ];
        assert_eq!(identity_from_nas(&ciphered), None);
    }

    #[test]
    fn finds_the_identity_in_an_identity_response() {
        let msg = [
            0x07, // plain NAS, EPS mobility management
            IDENTITY_RESPONSE,
            0x08, // length
            0x39,
            0x01,
            0x51,
            0x10,
            0x32,
            0x54,
            0x76,
            0x98,
        ];
        assert_eq!(
            identity_from_nas(&msg),
            Some(Identity::Imsi("310150123456789".to_string()))
        );
    }

    #[test]
    fn finds_the_identity_in_an_attach_request() {
        let msg = [
            0x07,
            ATTACH_REQUEST,
            0x02, // NAS key set identifier and attach type
            0x08, // length
            0x39,
            0x01,
            0x51,
            0x10,
            0x32,
            0x54,
            0x76,
            0x98,
        ];
        assert_eq!(
            identity_from_nas(&msg),
            Some(Identity::Imsi("310150123456789".to_string()))
        );
    }

    /// Session management messages must not be read as mobility ones. Their
    /// first octet holds an EPS bearer identity where mobility holds a security
    /// header, so a bearer numbered zero looks plain, and the byte after it is
    /// a procedure transaction identity rather than a message type. If that
    /// transaction number happened to be 0x56, an identity would be decoded out
    /// of unrelated bytes.
    #[test]
    fn session_management_messages_are_not_read_as_identities() {
        // PD 2 (session management), bearer 0, transaction number 0x56.
        let esm = [
            0x02,
            IDENTITY_RESPONSE,
            0x08,
            0x39,
            0x01,
            0x51,
            0x10,
            0x32,
            0x54,
            0x76,
            0x98,
        ];
        assert_eq!(identity_from_nas(&esm), None);
    }

    #[test]
    fn other_message_types_are_ignored() {
        let security_mode_command = [0x07, 0x5d, 0x08, 0x39, 0x01];
        assert_eq!(identity_from_nas(&security_mode_command), None);
        assert_eq!(identity_from_nas(&[0x07]), None);
        assert_eq!(identity_from_nas(&[]), None);
    }

    /// A truncated message must not read past its end.
    #[test]
    fn a_truncated_message_is_refused_rather_than_read_past() {
        let truncated = [0x07, IDENTITY_RESPONSE, 0x08, 0x39, 0x01];
        assert_eq!(identity_from_nas(&truncated), None);
    }
}
