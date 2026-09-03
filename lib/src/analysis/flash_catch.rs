//! FlashCatch: a fake tower that takes the phone's identity and then makes
//! the phone reject *it*, so the phone walks away on its own.
//!
//! Described by Paci, Bologna, Palamà and Bianchi in "FlashCatch: Minimizing
//! Disruption in IMSI Catcher Operations" (ACM WiSec 2025,
//! doi:10.1145/3734477.3734705). A conventional IMSI catcher holds a phone
//! after taking its identity, and the phone loses service until it gives up,
//! which a person can notice. FlashCatch avoids that. Posing as a tower of the
//! phone's own network, on a frequency that network really uses, it asks for
//! the permanent identity (IMSI) the moment the phone checks in with a
//! tracking area update or service request, and then sends authentication
//! challenges it has deliberately signed wrongly. The phone answers each with
//! AUTHENTICATION FAILURE, cause "MAC failure" (3GPP TS 24.301 §5.4.2.6),
//! and after the third it treats the tower as having failed authentication
//! (§5.4.2.7), bars the cell for five minutes (TS 36.304 §5.3.1), and goes
//! back to the real network with its keys and temporary identity intact. The
//! paper measured the identity leaving the phone within tens of milliseconds
//! of the check-in, and nothing a person could notice beyond a latency blip
//! of about a second.
//!
//! Rayhunter sees both halves. The identity request is the same message the
//! [identity detector](super::imsi_requested) watches for, but that detector
//! waits for the tower to reject or disconnect the phone afterwards, and here
//! the tower does neither: the *phone* does the rejecting. So this detector
//! keys on the part no other one reads, the run of AUTHENTICATION FAILURE
//! messages from the phone. A real network knows the SIM's key and passes
//! this check; a network that fails it twice in a row does not know the key.
//!
//! The standard counts three failures of any of causes #20 "MAC failure",
//! #21 "Synch failure" and #26 "non-EPS authentication unacceptable" towards
//! barring the cell. The first and the last need no key to provoke, so the
//! detector counts them together as forged challenges. A synch failure needs
//! a genuine challenge, replayed, and one on its own is ordinary
//! housekeeping, so those are counted apart and more leniently.
//!
//! The design follows the paper's description of the exchange and the
//! standard's handling of authentication failure; it has not been checked
//! against a recording of the attack itself.

use std::borrow::Cow;

use pycrate_rs::nas::NASMessage;
use pycrate_rs::nas::emm::EMMMessage;
use pycrate_rs::nas::generated::emm::emm_authentication_failure::EMMCauseEMMCause;
use pycrate_rs::nas::generated::emm::emm_identity_request::IDTypeV;

use super::analyzer::{Analyzer, Event, EventType};
use super::information_element::{InformationElement, LteInformationElement};

/// How many packets a check-in, an identity request or a rejection stays
/// relevant. The attack completes within a second, a few dozen log records;
/// the window is generous because the modem logs other traffic in between.
const WINDOW: usize = 200;

/// Consecutive forged-challenge rejections that make a warning. The standard
/// lets the phone bar the cell after three; two in a row is already something
/// a real network does not do.
const FORGED_CHALLENGES_TO_WARN: usize = 2;

/// Sequence-number failures that make a warning. One is ordinary housekeeping
/// (the phone and network resynchronise), so more are needed.
const SYNCH_FAILURES_TO_WARN: usize = 3;

pub struct FlashCatchAnalyzer {
    /// The packet where the tower last asked for the IMSI.
    imsi_requested_at: Option<usize>,
    /// The packet where the phone last checked in with a tracking area
    /// update or a service request.
    checked_in_at: Option<usize>,
    /// Packets where the phone rejected a challenge as forged (MAC failure
    /// or non-EPS authentication unacceptable), within the window, oldest
    /// first.
    forged_challenges: Vec<usize>,
    /// Packets where the phone reported the sequence number out of step.
    synch_failures: Vec<usize>,
    /// Whether the current run of failures has been reported, so a burst
    /// raises one warning rather than one per failure.
    reported: bool,
}

impl Default for FlashCatchAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl FlashCatchAnalyzer {
    pub fn new() -> Self {
        Self {
            imsi_requested_at: None,
            checked_in_at: None,
            forged_challenges: Vec::new(),
            synch_failures: Vec::new(),
            reported: false,
        }
    }

    /// Forget everything: the exchange ended, one way or another.
    fn reset(&mut self) {
        self.imsi_requested_at = None;
        self.checked_in_at = None;
        self.forged_challenges.clear();
        self.synch_failures.clear();
        self.reported = false;
    }

    /// Drop failures older than the window. When that empties the list, the
    /// earlier run is over and a new one may be reported.
    fn prune(&mut self, now: usize) {
        self.forged_challenges
            .retain(|&at| now.saturating_sub(at) <= WINDOW);
        self.synch_failures
            .retain(|&at| now.saturating_sub(at) <= WINDOW);
        if self.forged_challenges.is_empty() && self.synch_failures.is_empty() {
            self.reported = false;
        }
    }

    fn packet_list(packets: &[usize]) -> String {
        packets
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The phone checking in with a tower it already has keys for. A new
    /// exchange begins; what came before it is over.
    fn on_check_in(&mut self, packet_num: usize) {
        self.reset();
        self.checked_in_at = Some(packet_num);
    }

    fn on_forged_challenge(&mut self, packet_num: usize) -> Option<Event> {
        self.prune(packet_num);
        self.forged_challenges.push(packet_num);
        if self.reported || self.forged_challenges.len() < FORGED_CHALLENGES_TO_WARN {
            return None;
        }
        self.reported = true;
        let count = self.forged_challenges.len();
        let packets = Self::packet_list(&self.forged_challenges);
        if let Some(requested_at) = self
            .imsi_requested_at
            .filter(|&at| packet_num.saturating_sub(at) <= WINDOW)
        {
            return Some(Event {
                event_type: EventType::High,
                message: format!(
                    "The tower asked for the phone's permanent identity (IMSI) at packet \
                     {requested_at}, then failed its own authentication {count} times in a row \
                     (packets {packets}): the phone rejected each challenge as forged. This is \
                     the FlashCatch pattern. A fake tower takes the identity and then fails \
                     authentication on purpose; after the third rejection the phone bars the \
                     tower for five minutes and goes back to the real network with no visible \
                     interruption, keeping the temporary identity the tower can now tie to the \
                     IMSI."
                ),
            });
        }
        Some(Event {
            event_type: EventType::Medium,
            message: format!(
                "The phone rejected the tower's authentication challenge as forged {count} times \
                 in a row (packets {packets}). A real network knows the SIM's key and passes \
                 this check; a tower that keeps failing it does not know the key. This is the \
                 second half of the FlashCatch pattern, seen without the identity request that \
                 usually comes first."
            ),
        })
    }

    fn on_synch_failure(&mut self, packet_num: usize) -> Option<Event> {
        self.prune(packet_num);
        self.synch_failures.push(packet_num);
        if self.reported || self.synch_failures.len() < SYNCH_FAILURES_TO_WARN {
            return None;
        }
        self.reported = true;
        let packets = Self::packet_list(&self.synch_failures);
        Some(Event {
            event_type: EventType::Medium,
            message: format!(
                "The phone reported the tower's authentication sequence number out of step \
                 {} times in a row (packets {packets}). Once is normal housekeeping; a run of \
                 them suggests a tower replaying old authentication data it captured earlier.",
                self.synch_failures.len()
            ),
        })
    }

    fn on_imsi_request(&mut self, packet_num: usize) -> Option<Event> {
        self.imsi_requested_at = Some(packet_num);
        let checked_in_at = self
            .checked_in_at
            .filter(|&at| packet_num.saturating_sub(at) <= WINDOW)?;
        Some(Event {
            event_type: EventType::Informational,
            message: format!(
                "The tower asked for the permanent identity (IMSI) as soon as the phone checked \
                 in (tracking area update or service request at packet {checked_in_at}). Real \
                 networks do this when they have lost the phone's record; a fake tower does it \
                 to every phone. Noted only. This detector warns if forged authentication \
                 follows."
            ),
        })
    }
}

impl Analyzer for FlashCatchAnalyzer {
    fn get_name(&self) -> Cow<'_, str> {
        Cow::from("FlashCatch: identity taken, then forged authentication")
    }

    fn get_description(&self) -> Cow<'_, str> {
        Cow::from(
            "Watches for a tower that fails the phone's authentication check repeatedly, the \
             mark of a fake tower that took the phone's identity and then made the phone reject \
             it, so the phone walks away in under a second with no visible loss of service. High \
             when the permanent identity was requested just before; Medium for the repeated \
             failures alone.",
        )
    }

    fn get_version(&self) -> u32 {
        2
    }

    fn analyze_information_element(
        &mut self,
        ie: &InformationElement,
        packet_num: usize,
    ) -> Option<Event> {
        let InformationElement::LTE(lte) = ie else {
            return None;
        };
        let LteInformationElement::NAS(NASMessage::EMMMessage(emm)) = &**lte else {
            return None;
        };
        match emm {
            // The phone checking in. The paper's phone sends a tracking area
            // update when the fake tower advertises a different tracking area
            // and a service request when it copies the real one. The short
            // SERVICE REQUEST has no plain form for the parser to decode, so
            // only the extended and control-plane forms are seen here; the
            // warnings do not depend on it, only the informational note.
            EMMMessage::EMMTrackingAreaUpdateRequest(_)
            | EMMMessage::EMMExtServiceRequest(_)
            | EMMMessage::EMMCPServiceRequest(_) => {
                self.on_check_in(packet_num);
                None
            }
            // Starting from scratch is not checking in: the identity detector
            // covers an identity request during an attach.
            EMMMessage::EMMAttachRequest(_) => {
                self.reset();
                None
            }
            EMMMessage::EMMIdentityRequest(request) => match request.id_type.inner {
                IDTypeV::IMSI => self.on_imsi_request(packet_num),
                _ => None,
            },
            EMMMessage::EMMAuthenticationFailure(failure) => match failure.emm_cause.inner {
                EMMCauseEMMCause::MACFailure
                | EMMCauseEMMCause::NonEPSAuthenticationUnacceptable => {
                    self.on_forged_challenge(packet_num)
                }
                EMMCauseEMMCause::SynchFailure => self.on_synch_failure(packet_num),
                _ => None,
            },
            // The network passed the check, or the exchange ended well: no
            // run of failures to hold against it.
            EMMMessage::EMMAuthenticationResponse(_)
            | EMMMessage::EMMSecurityModeCommand(_)
            | EMMMessage::EMMAttachAccept(_)
            | EMMMessage::EMMTrackingAreaUpdateAccept(_)
            | EMMMessage::EMMServiceAccept(_) => {
                self.reset();
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Plain NAS messages, 3GPP TS 24.301. Each begins 0x07: security header
    // type 0 (no protection) in the high nibble, protocol discriminator 7
    // (EPS mobility management) in the low nibble, then the message type.
    // The modem logs the plain form of every EMM message, so this is what
    // the analyzer sees even when the phone integrity-protected its reply.

    /// IDENTITY REQUEST (0x55) for the IMSI (identity type 1, §9.9.3.17).
    const IDENTITY_REQUEST_IMSI: &[u8] = &[0x07, 0x55, 0x01];
    /// IDENTITY REQUEST for the IMEI (identity type 2).
    const IDENTITY_REQUEST_IMEI: &[u8] = &[0x07, 0x55, 0x02];
    /// AUTHENTICATION FAILURE (0x5C), cause 20 "MAC failure" (§9.9.3.9).
    const AUTH_FAILURE_MAC: &[u8] = &[0x07, 0x5C, 0x14];
    /// AUTHENTICATION FAILURE, cause 26 "non-EPS authentication
    /// unacceptable": the challenge was not even built for a 4G network.
    const AUTH_FAILURE_NON_EPS: &[u8] = &[0x07, 0x5C, 0x1A];
    /// AUTHENTICATION FAILURE, cause 21 "Synch failure", with the AUTS
    /// parameter (tag 0x30, 14 bytes) the phone sends for resynchronisation.
    const AUTH_FAILURE_SYNCH: &[u8] = &[
        0x07, 0x5C, 0x15, 0x30, 0x0E, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x33,
    ];
    /// AUTHENTICATION REQUEST (0x52): key set 0, a 16-byte RAND, then the
    /// 16-byte AUTN (length-prefixed) carrying the signature the phone checks.
    const AUTH_REQUEST: &[u8] = &[
        0x07, 0x52, 0x00, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11, 0x11, 0x10, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
    ];
    /// AUTHENTICATION RESPONSE (0x53) with an 8-byte RES.
    const AUTH_RESPONSE: &[u8] = &[
        0x07, 0x53, 0x08, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
    ];
    /// TRACKING AREA UPDATE REQUEST (0x48): update type 0 and key set 0, then
    /// the old GUTI (11 bytes: 0xF6 marks a GUTI, PLMN 001-01, MME group 1,
    /// MME code 1, M-TMSI 0xC0000001).
    const TAU_REQUEST: &[u8] = &[
        0x07, 0x48, 0x00, 0x0B, 0xF6, 0x00, 0xF1, 0x10, 0x00, 0x01, 0x01, 0xC0, 0x00, 0x00, 0x01,
    ];
    /// EXTENDED SERVICE REQUEST (0x4C): key set 0 and service type 0, then
    /// the M-TMSI (5 bytes: 0xF4 marks a TMSI, then 0xC0000001).
    const EXT_SERVICE_REQUEST: &[u8] = &[0x07, 0x4C, 0x00, 0x05, 0xF4, 0xC0, 0x00, 0x00, 0x01];
    /// SECURITY MODE COMMAND (0x5D), as the demo uses.
    const SECURITY_MODE_COMMAND: &[u8] = &[0x07, 0x5D, 0x00, 0x00, 0x02, 0x80, 0x00, 0x00];

    fn nas(bytes: &[u8]) -> InformationElement {
        let message = NASMessage::parse(bytes).expect("test vector should parse");
        InformationElement::LTE(Box::new(LteInformationElement::NAS(message)))
    }

    /// Feed messages at consecutive packet numbers starting at `first`,
    /// returning the event each produced.
    fn run(
        analyzer: &mut FlashCatchAnalyzer,
        first: usize,
        messages: &[&[u8]],
    ) -> Vec<Option<Event>> {
        messages
            .iter()
            .enumerate()
            .map(|(i, bytes)| analyzer.analyze_information_element(&nas(bytes), first + i))
            .collect()
    }

    fn severities(events: &[Option<Event>]) -> Vec<Option<EventType>> {
        events
            .iter()
            .map(|e| e.as_ref().map(|e| e.event_type))
            .collect()
    }

    #[test]
    fn test_vectors_are_the_messages_they_claim_to_be() {
        let expect = |bytes: &[u8], want: &str| {
            let parsed = NASMessage::parse(bytes).unwrap();
            let name = format!("{parsed:?}");
            assert!(name.starts_with(want), "{name} should be {want}");
        };
        expect(IDENTITY_REQUEST_IMSI, "EMMMessage(EMMIdentityRequest");
        expect(IDENTITY_REQUEST_IMEI, "EMMMessage(EMMIdentityRequest");
        expect(AUTH_FAILURE_MAC, "EMMMessage(EMMAuthenticationFailure");
        expect(AUTH_FAILURE_NON_EPS, "EMMMessage(EMMAuthenticationFailure");
        expect(AUTH_FAILURE_SYNCH, "EMMMessage(EMMAuthenticationFailure");
        expect(AUTH_REQUEST, "EMMMessage(EMMAuthenticationRequest");
        expect(AUTH_RESPONSE, "EMMMessage(EMMAuthenticationResponse");
        expect(TAU_REQUEST, "EMMMessage(EMMTrackingAreaUpdateRequest");
        expect(EXT_SERVICE_REQUEST, "EMMMessage(EMMExtServiceRequest");
        expect(SECURITY_MODE_COMMAND, "EMMMessage(EMMSecurityModeCommand");
    }

    #[test]
    fn the_flashcatch_exchange_is_high_and_reported_once() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            10,
            &[
                TAU_REQUEST,
                IDENTITY_REQUEST_IMSI,
                AUTH_REQUEST,
                AUTH_FAILURE_MAC,
                AUTH_REQUEST,
                AUTH_FAILURE_MAC,
                AUTH_REQUEST,
                AUTH_FAILURE_MAC,
            ],
        );
        assert_eq!(
            severities(&events),
            vec![
                None,
                Some(EventType::Informational),
                None,
                None,
                None,
                Some(EventType::High),
                None,
                None,
            ]
        );
        let high = events[5].as_ref().unwrap();
        assert!(high.message.contains("packet 11"), "{}", high.message);
        assert!(high.message.contains("packets 13, 15"), "{}", high.message);
        assert!(high.message.contains("FlashCatch"), "{}", high.message);
    }

    #[test]
    fn the_exchange_after_a_service_request_is_the_same() {
        // The paper's phone sends a service request instead of a tracking
        // area update when the fake tower copies the real tracking area.
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[
                EXT_SERVICE_REQUEST,
                IDENTITY_REQUEST_IMSI,
                AUTH_FAILURE_MAC,
                AUTH_FAILURE_MAC,
            ],
        );
        assert_eq!(
            severities(&events),
            vec![
                None,
                Some(EventType::Informational),
                None,
                Some(EventType::High)
            ]
        );
    }

    #[test]
    fn forged_challenges_without_an_identity_request_are_medium() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(&mut analyzer, 0, &[AUTH_FAILURE_MAC, AUTH_FAILURE_MAC]);
        assert_eq!(severities(&events), vec![None, Some(EventType::Medium)]);
    }

    #[test]
    fn a_non_eps_rejection_counts_as_a_forged_challenge() {
        // The standard counts causes 20 and 26 alike towards barring the
        // cell, and neither needs the key to provoke.
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[
                IDENTITY_REQUEST_IMSI,
                AUTH_FAILURE_NON_EPS,
                AUTH_FAILURE_MAC,
            ],
        );
        assert_eq!(severities(&events), vec![None, None, Some(EventType::High)]);

        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[AUTH_FAILURE_NON_EPS, AUTH_FAILURE_NON_EPS],
        );
        assert_eq!(severities(&events), vec![None, Some(EventType::Medium)]);
    }

    #[test]
    fn one_failure_is_quiet() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[IDENTITY_REQUEST_IMSI, AUTH_FAILURE_MAC, AUTH_RESPONSE],
        );
        assert_eq!(severities(&events), vec![None, None, None]);
    }

    #[test]
    fn a_passed_check_between_failures_clears_the_run() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[AUTH_FAILURE_MAC, AUTH_RESPONSE, AUTH_FAILURE_MAC],
        );
        assert_eq!(severities(&events), vec![None, None, None]);

        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            10,
            &[AUTH_FAILURE_MAC, SECURITY_MODE_COMMAND, AUTH_FAILURE_MAC],
        );
        assert_eq!(severities(&events), vec![None, None, None]);
    }

    #[test]
    fn a_stale_identity_request_does_not_escalate() {
        let mut analyzer = FlashCatchAnalyzer::new();
        assert!(
            analyzer
                .analyze_information_element(&nas(IDENTITY_REQUEST_IMSI), 0)
                .is_none()
        );
        let events = run(
            &mut analyzer,
            WINDOW + 50,
            &[AUTH_FAILURE_MAC, AUTH_FAILURE_MAC],
        );
        assert_eq!(severities(&events), vec![None, Some(EventType::Medium)]);
    }

    #[test]
    fn a_second_burst_after_the_window_is_reported_again() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(&mut analyzer, 0, &[AUTH_FAILURE_MAC, AUTH_FAILURE_MAC]);
        assert_eq!(severities(&events), vec![None, Some(EventType::Medium)]);
        // The same tower tries again much later, with no reconnect logged in
        // between: still worth saying.
        let events = run(
            &mut analyzer,
            WINDOW + 10,
            &[IDENTITY_REQUEST_IMSI, AUTH_FAILURE_MAC, AUTH_FAILURE_MAC],
        );
        assert_eq!(severities(&events), vec![None, None, Some(EventType::High)]);
    }

    #[test]
    fn an_imei_request_is_not_the_pattern() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[IDENTITY_REQUEST_IMEI, AUTH_FAILURE_MAC, AUTH_FAILURE_MAC],
        );
        assert_eq!(
            severities(&events),
            vec![None, None, Some(EventType::Medium)]
        );
    }

    #[test]
    fn a_new_exchange_forgets_the_old_one() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[
                IDENTITY_REQUEST_IMSI,
                AUTH_FAILURE_MAC,
                TAU_REQUEST,
                AUTH_FAILURE_MAC,
                AUTH_FAILURE_MAC,
            ],
        );
        // The identity request belonged to the earlier exchange, so the run
        // after the new tracking area update is Medium, not High.
        assert_eq!(
            severities(&events),
            vec![None, None, None, None, Some(EventType::Medium)]
        );
    }

    #[test]
    fn sequence_number_failures_need_three() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[AUTH_FAILURE_SYNCH, AUTH_FAILURE_SYNCH, AUTH_FAILURE_SYNCH],
        );
        assert_eq!(
            severities(&events),
            vec![None, None, Some(EventType::Medium)]
        );
    }

    #[test]
    fn an_identity_request_on_check_in_is_only_noted() {
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(&mut analyzer, 0, &[TAU_REQUEST, IDENTITY_REQUEST_IMSI]);
        assert_eq!(
            severities(&events),
            vec![None, Some(EventType::Informational)]
        );
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(
            &mut analyzer,
            0,
            &[EXT_SERVICE_REQUEST, IDENTITY_REQUEST_IMSI],
        );
        assert_eq!(
            severities(&events),
            vec![None, Some(EventType::Informational)]
        );
        // Without a check-in there is nothing to note: the identity detector
        // covers a bare identity request.
        let mut analyzer = FlashCatchAnalyzer::new();
        let events = run(&mut analyzer, 0, &[IDENTITY_REQUEST_IMSI]);
        assert_eq!(severities(&events), vec![None]);
    }

    #[test]
    fn other_traffic_is_ignored() {
        let mut analyzer = FlashCatchAnalyzer::new();
        // A session-management message (ESM INFORMATION REQUEST, protocol
        // discriminator 2) is not the detector's business and does not
        // disturb a run.
        const ESM_INFORMATION_REQUEST: &[u8] = &[0x02, 0x01, 0xD9];
        let events = run(
            &mut analyzer,
            0,
            &[AUTH_FAILURE_MAC, ESM_INFORMATION_REQUEST, AUTH_FAILURE_MAC],
        );
        assert_eq!(
            severities(&events),
            vec![None, None, Some(EventType::Medium)]
        );
    }

    #[test]
    fn truncated_messages_never_panic() {
        for bytes in [
            IDENTITY_REQUEST_IMSI,
            AUTH_REQUEST,
            AUTH_FAILURE_MAC,
            AUTH_FAILURE_NON_EPS,
            AUTH_FAILURE_SYNCH,
            AUTH_RESPONSE,
            TAU_REQUEST,
            EXT_SERVICE_REQUEST,
            SECURITY_MODE_COMMAND,
        ] {
            for len in 0..bytes.len() {
                let mut analyzer = FlashCatchAnalyzer::new();
                if let Ok(message) = NASMessage::parse(&bytes[..len]) {
                    let ie = InformationElement::LTE(Box::new(LteInformationElement::NAS(message)));
                    let _ = analyzer.analyze_information_element(&ie, 0);
                }
            }
        }
    }
}
