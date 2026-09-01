//! Removing the device's own identifiers from a capture before sharing it.
//!
//! A recording is useful evidence *because* it is detailed, and that same
//! detail is what makes sharing it carelessly an exposure. The identifiers that
//! matter most are the ones this tool exists to protect: the IMSI and IMEI that
//! name a subscription and a handset, and the temporary identity that links a
//! session to them. See EFForg/rayhunter#940.
//!
//! **What this is honest about.** This finds identities in the NAS messages
//! where they sit at a known offset, which is where a device announces itself:
//! identity responses and attach requests. It is a real reduction in exposure,
//! not a guarantee. A ciphered message hides its contents from us as much as
//! from anyone reading the capture, and an identity encoded somewhere this does
//! not look for would survive.
//!
//! So a redacted export says what it removed and how many, and the counts are
//! reported rather than a claim of cleanliness. Somebody who believes a capture
//! is clean when it is not is worse off than somebody who knows it is not.
//!
//! **The original is never modified.** Redaction happens on the way out, into
//! a separate download. A recording is evidence, and evidence that got quietly
//! rewritten is not evidence any more.

use serde::{Deserialize, Serialize};

use crate::subscriber_id::{Identity, identity_from_nas_with_range};

/// What a redaction pass removed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RedactionReport {
    pub imsi: usize,
    pub imei: usize,
    pub imeisv: usize,
    pub tmsi: usize,
    /// Messages looked at, so a report of zero removals can be told apart from
    /// a pass that never ran.
    pub messages_scanned: usize,
}

impl RedactionReport {
    pub fn total(&self) -> usize {
        self.imsi + self.imei + self.imeisv + self.tmsi
    }

    /// Note one identity removed.
    pub fn record(&mut self, identity: &Identity) {
        match identity {
            Identity::Imsi(_) => self.imsi += 1,
            Identity::Imei(_) => self.imei += 1,
            Identity::Imeisv(_) => self.imeisv += 1,
            Identity::Tmsi(_) => self.tmsi += 1,
        }
    }
}

/// Overwrite the identity in a NAS payload, keeping the message parseable.
///
/// The digits are set to zero rather than the bytes being removed, because a
/// capture that still parses is one somebody can still read in Wireshark: the
/// point is to remove who this was, not to break the evidence of what happened.
/// An identity of all zeros is also obviously redacted rather than looking like
/// a real subscriber.
///
/// The first byte keeps its low nibble, which carries the identity type and the
/// odd/even indicator. Changing that would change the shape of the field and
/// leave a message that no longer decodes.
///
/// Returns what was removed, or `None` if there was nothing to remove.
pub fn redact_nas_identity(payload: &mut [u8]) -> Option<Identity> {
    let (identity, range) = identity_from_nas_with_range(payload)?;
    let value = payload.get_mut(range)?;
    let (first, rest) = value.split_first_mut()?;
    // Keep type and odd/even, zero the first digit.
    *first &= 0x0f;
    rest.fill(0);
    Some(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An identity response carrying a 15 digit IMSI, encoded the way a network
    /// does: type and odd/even with the first digit in the high nibble, then
    /// the rest as pairs.
    fn identity_response_with_imsi() -> Vec<u8> {
        // 310150123456789
        let digits = [3u8, 1, 0, 1, 5, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        // type 1 (IMSI), odd number of digits so the odd indicator is set.
        let mut value = vec![(digits[0] << 4) | 0b1000 | 1];
        for pair in digits[1..].chunks(2) {
            let low = pair[0];
            let high = pair.get(1).copied().unwrap_or(0x0f);
            value.push((high << 4) | low);
        }
        let mut payload = vec![0x07, 0x56, value.len() as u8];
        payload.extend_from_slice(&value);
        payload
    }

    #[test]
    fn an_imsi_is_found_before_it_is_removed() {
        let payload = identity_response_with_imsi();
        let (identity, _) = identity_from_nas_with_range(&payload).expect("decodes");
        assert_eq!(identity, Identity::Imsi("310150123456789".to_string()));
    }

    /// The point of the exercise: after redaction the identity is gone.
    #[test]
    fn the_imsi_does_not_survive_redaction() {
        let mut payload = identity_response_with_imsi();
        let removed = redact_nas_identity(&mut payload).expect("something was removed");
        assert_eq!(removed, Identity::Imsi("310150123456789".to_string()));

        let after = identity_from_nas_with_range(&payload).map(|(i, _)| i);
        match after {
            Some(Identity::Imsi(digits)) => {
                assert!(
                    digits.chars().all(|c| c == '0'),
                    "digits survived redaction: {digits}"
                );
            }
            other => panic!("expected an all-zero IMSI, got {other:?}"),
        }
    }

    /// The message has to still decode afterwards. A redaction that broke the
    /// structure would take the evidence of what happened with it.
    #[test]
    fn a_redacted_message_still_parses_as_an_identity() {
        let mut payload = identity_response_with_imsi();
        redact_nas_identity(&mut payload);
        assert!(
            identity_from_nas_with_range(&payload).is_some(),
            "the message stopped decoding after redaction"
        );
    }

    /// Only the identity is touched. Overwriting the header would change the
    /// message type, and the capture would say something different happened.
    #[test]
    fn nothing_outside_the_identity_is_touched() {
        let before = identity_response_with_imsi();
        let mut after = before.clone();
        redact_nas_identity(&mut after);
        assert_eq!(&after[..2], &before[..2], "message header changed");
        assert_eq!(after[2], before[2], "identity length changed");
        assert_eq!(after.len(), before.len(), "payload length changed");
    }

    /// A ciphered message hides its contents from us as much as from anyone
    /// else, so there is nothing to redact and nothing to claim.
    #[test]
    fn a_ciphered_message_is_left_alone() {
        let mut ciphered = vec![0x17, 0x56, 0x08, 0xde, 0xad, 0xbe, 0xef];
        let before = ciphered.clone();
        assert_eq!(redact_nas_identity(&mut ciphered), None);
        assert_eq!(ciphered, before);
    }

    #[test]
    fn a_message_with_no_identity_is_left_alone() {
        let mut other = vec![0x07, 0x5d, 0x00, 0x00, 0x02, 0x80, 0x00, 0x00];
        let before = other.clone();
        assert_eq!(redact_nas_identity(&mut other), None);
        assert_eq!(other, before);
    }

    /// The report has to distinguish "scanned and found nothing" from "never
    /// ran", or an empty result reads as a clean capture either way.
    #[test]
    fn the_report_counts_what_it_looked_at() {
        let mut report = RedactionReport::default();
        assert_eq!(report.messages_scanned, 0);
        assert_eq!(report.total(), 0);

        for mut payload in [
            identity_response_with_imsi(),
            vec![0x07, 0x5d, 0x00, 0x00],
            identity_response_with_imsi(),
        ] {
            report.messages_scanned += 1;
            if let Some(identity) = redact_nas_identity(&mut payload) {
                report.record(&identity);
            }
        }
        assert_eq!(report.messages_scanned, 3);
        assert_eq!(report.imsi, 2);
        assert_eq!(report.total(), 2);
    }

    /// Truncated and empty inputs must not panic. These arrive off the air.
    #[test]
    fn short_payloads_do_not_panic() {
        for len in 0..8 {
            let mut payload = identity_response_with_imsi();
            payload.truncate(len);
            let _ = redact_nas_identity(&mut payload);
        }
    }
}
