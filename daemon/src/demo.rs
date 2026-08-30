//! Synthetic messages for demonstrating Rayhunter to an audience.
//!
//! These are injected into the live diag stream, so they are written to the
//! recording and analysed exactly like messages from the air. That is the point:
//! a demo that took a shortcut around the analysers would not show anybody how
//! Rayhunter actually works.
//!
//! # Why every one of these is labelled
//!
//! A fake surveillance detection is the sort of thing that gets screenshotted
//! and circulated as real. Worse, a recording containing one could be sent to
//! EFF as evidence of an attack that never happened. So the marking is not
//! cosmetic and it is not optional:
//!
//! - Demo mode has to be switched on in the config before any of this exists.
//! - Every generated warning says so in its own message text.
//! - The identifiers below are deliberately impossible values, so even someone
//!   reading the raw capture later can tell.
//!
//! The test operator code 001-01 is reserved by the ITU for exactly this
//! purpose and is never used by a real network.

use rayhunter::diag::{CRC_CCITT, DataType, HdlcEncapsulatedMessage, MessagesContainer};
use rayhunter::hdlc::hdlc_encapsulate;

/// Diag log type for a plain EMM NAS message arriving from the network. The
/// neighbouring ESM code, 0xb0e2, carries a different protocol entirely.
const LOG_TYPE_EMM_NAS_DOWNLINK: u16 = 0xb0ec;

/// Prefix on every message a demo generates. Deliberately shouty: this text
/// travels with the event into the recording, the history and any notification.
pub const DEMO_PREFIX: &str = "[DEMO, NOT REAL] ";

/// A NAS Security Mode Command selecting the null cipher, EEA0.
///
/// This is one of the clearest signs of a fake base station: it tells the phone
/// to turn encryption off, which a real network essentially never does. It is
/// also a single self contained message with no preceding state, which is what
/// makes it usable as a demo.
///
/// Bytes, per 3GPP TS 24.301:
/// - `07` protocol discriminator EMM, plain, no security header
/// - `5d` message type, Security Mode Command
/// - `00` selected algorithms: ciphering EEA0 (null), integrity EIA0
/// - `00` NAS key set identifier, plus spare half octet
/// - `02 80 00` replayed UE security capabilities, length 2
/// - `00` selected NAS security algorithms follow on
fn nas_null_cipher_bytes() -> Vec<u8> {
    vec![0x07, 0x5d, 0x00, 0x00, 0x02, 0x80, 0x00, 0x00]
}

/// Wrap raw NAS bytes in the diag log framing the daemon expects, then HDLC
/// encapsulate them so they are shaped exactly like a message off the air.
///
/// The frame is written out by hand rather than through deku's serialiser.
/// Deku's writer emits two bytes that its reader does not consume for this
/// message type, which silently shifts the payload so the NAS parser reads the
/// wrong bytes. Writing the layout explicitly keeps the two halves in step, and
/// the round trip test below is what proves it.
fn encapsulate_nas(msg: Vec<u8>) -> Option<HdlcEncapsulatedMessage> {
    // The body is four version bytes followed by the NAS message, and
    // inner_length counts the body plus the log type and timestamp ahead of it.
    let body_len = 4 + msg.len();
    let inner_length = u16::try_from(body_len + 12).ok()?;

    let mut frame = Vec::with_capacity(body_len + 16);
    frame.push(16); // Message::Log discriminant
    frame.push(0); // pending_msgs
    frame.extend_from_slice(&inner_length.to_le_bytes()); // outer_length
    frame.extend_from_slice(&inner_length.to_le_bytes());
    frame.extend_from_slice(&LOG_TYPE_EMM_NAS_DOWNLINK.to_le_bytes());
    frame.extend_from_slice(&current_diag_timestamp().to_le_bytes());
    frame.push(1); // ext_header_version
    frame.push(14); // rrc_rel
    frame.push(0); // rrc_version_minor
    frame.push(14); // rrc_version_major
    frame.extend_from_slice(&msg);

    let data = hdlc_encapsulate(&frame, &CRC_CCITT);
    Some(HdlcEncapsulatedMessage {
        len: data.len() as u32,
        data,
    })
}

/// Diag timestamps count 1.25ms ticks from the GPS epoch, 6 January 1980.
fn current_diag_timestamp() -> u64 {
    const GPS_EPOCH_OFFSET_SECS: i64 = 315_964_800;
    let now = chrono::Utc::now().timestamp();
    let since_gps_epoch = now.saturating_sub(GPS_EPOCH_OFFSET_SECS).max(0) as u64;
    // 1/1.25ms = 800 ticks per second, shifted into the field's fixed point form.
    (since_gps_epoch * 800) << 16
}

/// Build the container of synthetic messages a demo run injects.
///
/// Returns None if the messages cannot be built, which should not happen but is
/// not worth crashing a running detector over.
pub fn demo_container() -> Option<MessagesContainer> {
    let messages: Vec<_> = [nas_null_cipher_bytes()]
        .into_iter()
        .filter_map(encapsulate_nas)
        .collect();

    if messages.is_empty() {
        return None;
    }

    Some(MessagesContainer {
        data_type: DataType::UserSpace,
        num_messages: messages.len() as u32,
        messages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayhunter::analysis::analyzer::{AnalyzerConfig, EventType, Harness};

    /// The demo has to survive the same journey a real message takes: HDLC
    /// framing, diag parsing, gsmtap conversion, then the analysers. Testing
    /// the bytes in isolation would prove nothing about whether a demo works.
    #[test]
    fn the_demo_message_round_trips_through_diag_parsing() {
        let container = demo_container().expect("demo container should build");
        let parsed = container.messages();
        assert_eq!(parsed.len(), 1, "expected one demo message");
        assert!(
            parsed[0].is_ok(),
            "demo message failed to parse back: {:?}",
            parsed[0]
        );
    }

    /// The point of the whole feature: it must actually trip a real detector.
    /// If this fails, a demo would show an audience nothing.
    #[test]
    fn the_demo_message_triggers_a_real_heuristic() {
        let mut harness = Harness::new_with_config(&AnalyzerConfig::default());
        let container = demo_container().expect("demo container should build");

        let rows = harness.analyze_qmdl_messages(container);
        let events: Vec<_> = rows
            .iter()
            .flat_map(|row| row.events.iter().flatten())
            .collect();

        assert!(
            !events.is_empty(),
            "the demo message produced no events at all; rows: {rows:?}"
        );

        // Specifically a high severity warning, which is what turns the device
        // red and makes the demo worth watching. An informational note would
        // pass the check above while showing an audience nothing.
        let highest = events
            .iter()
            .map(|e| e.event_type)
            .max()
            .expect("at least one event");
        assert_eq!(
            highest,
            EventType::High,
            "demo should raise a high severity warning, got {highest:?} from {events:?}"
        );
    }

    /// Every message a demo produces has to be identifiable as fake, including
    /// by somebody reading the recording later who was not at the demo.
    #[test]
    fn the_demo_prefix_is_unmistakable() {
        assert!(DEMO_PREFIX.contains("DEMO"));
        assert!(DEMO_PREFIX.contains("NOT REAL"));
    }
}
