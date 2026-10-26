use deku::DekuContainerRead;
use rayhunter::diag::{CRC_CCITT, Message, diaglog::LogBody};
use rayhunter::hdlc::hdlc_decapsulate;

#[test]
fn captured_ll1_v1_is_fully_decoded() {
    let hex = include_str!("fixtures/cell-site-timing-v1.hex").trim();
    let frame: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let bytes = hdlc_decapsulate(&frame, &CRC_CCITT).unwrap();
    let ((rest, bits), message) = Message::from_bytes((&bytes, 0)).unwrap();
    assert!(rest.is_empty());
    assert_eq!(bits, 0);
    let Message::Log {
        log_type: 0xb114,
        body: LogBody::LteLl1ServingCellTiming { data },
        ..
    } = message
    else {
        panic!("expected LL1 serving cell timing");
    };
    assert_eq!(data.version, 1);
    assert_eq!(data.num_records, 20);
    assert_eq!(data.starting_sub_fn, 9);
    assert_eq!(data.starting_system_fn, 874);
    assert_eq!(data.starting_dl_frame_timing_offs, 39582);
    assert_eq!(data.starting_ul_frame_timing_offs, 39549);
    assert_eq!(data.starting_ul_timing_advance, 2);
    assert_eq!(data.timing_adjustment.len(), 20);
    // The captured -1 adjustment also exercises signed, non-byte-aligned fields.
    assert_eq!(data.timing_adjustment[1].dl_frame_timing_adjustment, -1);
}
