use crate::gps::{GpsRecord, load_gps_records};
use crate::qmdl_store::FileKind;
use crate::server::ServerState;

use crate::config::GpsMode;
use anyhow::Error;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use log::error;
use rayhunter::gsmtap::parser as gsmtap_parser;
use rayhunter::pcap::{GpsPoint, GsmtapPcapWriter};
use rayhunter::qmdl::QmdlMessageReader;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, duplex};
use tokio_util::io::ReaderStream;

#[cfg_attr(feature = "apidocs", utoipa::path(
    get,
    path = "/api/pcap/{name}",
    tag = "Recordings",
    responses(
        (status = StatusCode::OK, description = "PCAP conversion successful", content_type = "application/vnd.tcpdump.pcap"),
        (status = StatusCode::NOT_FOUND, description = "Could not find file {name}"),
        (status = StatusCode::SERVICE_UNAVAILABLE, description = "QMDL file is empty")
    ),
    params(
        ("name" = String, Path, description = "QMDL filename to convert and download")
    ),
    summary = "Download a PCAP file",
    description = "Stream a PCAP file to a client in chunks by converting the QMDL data for file {name} written so far."
))]
pub async fn get_pcap(
    State(state): State<Arc<ServerState>>,
    Path(mut qmdl_name): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let qmdl_store = state.qmdl_store_lock.read().await;
    if qmdl_name.ends_with("pcapng") {
        qmdl_name = qmdl_name.trim_end_matches(".pcapng").to_string();
    }
    let (entry_index, entry) = qmdl_store.entry_for_name(&qmdl_name).ok_or((
        StatusCode::NOT_FOUND,
        format!("couldn't find manifest entry with name {qmdl_name}"),
    ))?;
    if entry.qmdl_size_bytes == 0 {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "QMDL file is empty, try again in a bit!".to_string(),
        ));
    }
    let qmdl_file = qmdl_store
        .open_file(entry_index, FileKind::Qmdl)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?
        .ok_or((StatusCode::NOT_FOUND, "QMDL file not found".to_string()))?;
    let qmdl_reader = QmdlMessageReader::new(qmdl_file)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?;
    let (reader, writer) = duplex(1024);
    let gps_records = load_gps_records_for_entry(&state, entry_index).await;
    drop(qmdl_store);

    tokio::spawn(async move {
        if let Err(e) = generate_pcap_data(writer, qmdl_reader, gps_records).await {
            error!("failed to generate PCAP: {e:?}");
        }
    });

    let headers = [(CONTENT_TYPE, "application/vnd.tcpdump.pcap")];
    let body = Body::from_stream(ReaderStream::new(reader));
    Ok((headers, body).into_response())
}

pub(crate) async fn load_gps_records_for_entry(
    state: &Arc<ServerState>,
    entry_index: usize,
) -> Vec<GpsRecord> {
    let qmdl_store = state.qmdl_store_lock.read().await;
    match qmdl_store.open_file(entry_index, FileKind::Gps).await {
        Ok(Some(file)) => load_gps_records(file).await,
        Ok(None) => {
            let gps_mode = qmdl_store
                .manifest
                .entries
                .get(entry_index)
                .and_then(|e| e.gps_mode);
            if gps_mode.is_some_and(|m| m != GpsMode::Disabled) {
                error!(
                    "GPS storage expected for entry {entry_index} (mode: {gps_mode:?}) but not found"
                );
            }
            vec![]
        }
        Err(e) => {
            error!("failed to open GPS storage: {e}");
            vec![]
        }
    }
}

/// Sort key for a GPS record. A record with no packet timestamp (a fixed-mode
/// coordinate, or a fix received before the first packet) sorts first, as the
/// earliest. This is only ever used for ordering and the `<=` partition below,
/// never in a subtraction, so the sentinel cannot overflow anything.
fn record_sort_key(r: &GpsRecord) -> i64 {
    r.latest_packet_timestamp.unwrap_or(i64::MIN)
}

/// How far a record sits from a packet in time, or `None` when the record has
/// no packet timestamp to compare against. Computed in `i128` so that no pair
/// of `i64` timestamps can overflow the subtraction — the previous code
/// subtracted an `i64::MIN` sentinel and wrapped or panicked.
fn distance_to(r: &GpsRecord, packet_timestamp: i64) -> Option<i128> {
    r.latest_packet_timestamp
        .map(|ts| (i128::from(packet_timestamp) - i128::from(ts)).abs())
}

fn find_nearest_gps(records: &[GpsRecord], packet_timestamp: i64) -> Option<GpsPoint> {
    if records.is_empty() {
        return None;
    }
    let idx = records.partition_point(|r| record_sort_key(r) <= packet_timestamp);
    let record = if idx == 0 {
        &records[0]
    } else if idx >= records.len() {
        &records[records.len() - 1]
    } else {
        let (before, after) = (&records[idx - 1], &records[idx]);
        // A record with no packet timestamp cannot be measured against the
        // packet, so a neighbour that does have one is always preferred.
        match (
            distance_to(before, packet_timestamp),
            distance_to(after, packet_timestamp),
        ) {
            (Some(b), Some(a)) => {
                if b <= a {
                    before
                } else {
                    after
                }
            }
            (Some(_), None) => before,
            (None, Some(_)) => after,
            (None, None) => before,
        }
    };
    Some(GpsPoint {
        latitude: record.lat,
        longitude: record.lon,
        // Fall back to the wall-clock time for records with no packet
        // timestamp, rather than emitting the i64::MIN sentinel as a real time.
        unix_ts: record.latest_packet_timestamp.unwrap_or(record.system_time),
    })
}

pub async fn generate_pcap_data<R, W>(
    writer: W,
    mut reader: QmdlMessageReader<R>,
    gps_records: Vec<GpsRecord>,
) -> Result<(), Error>
where
    W: AsyncWrite + Unpin + Send,
    R: AsyncRead + AsyncSeek + Unpin,
{
    let mut pcap_writer = GsmtapPcapWriter::new(writer).await?;
    pcap_writer.write_iface_header().await?;

    while let Some(maybe_msg) = reader.get_next_message().await? {
        match maybe_msg {
            Ok(msg) => {
                // Every frame the record holds, not just the first: a MAC
                // transport block record carries several, and dropping the
                // rest would leave most of that traffic out of the capture.
                for (timestamp, gsmtap_msg) in gsmtap_parser::parse_all(msg)? {
                    let packet_unix_ts = timestamp.to_datetime().timestamp();
                    let gps = find_nearest_gps(&gps_records, packet_unix_ts);
                    pcap_writer
                        .write_gsmtap_message(gsmtap_msg, timestamp, gps.as_ref())
                        .await?;
                }
            }
            Err(e) => error!("error parsing message: {e:?}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(latest_packet_timestamp: i64, lat: f64, lon: f64) -> GpsRecord {
        GpsRecord {
            latest_packet_timestamp: Some(latest_packet_timestamp),
            system_time: 0,
            lat,
            lon,
        }
    }

    #[test]
    fn test_empty_returns_none() {
        assert!(find_nearest_gps(&[], 100).is_none());
    }

    #[test]
    fn test_single_record_always_returned() {
        let records = vec![rec(100, 1.0, 2.0)];
        assert_eq!(find_nearest_gps(&records, 0).unwrap().unix_ts, 100);
        assert_eq!(find_nearest_gps(&records, 200).unwrap().unix_ts, 100);
    }

    #[test]
    fn test_before_all_records_returns_first() {
        let records = vec![rec(100, 1.0, 2.0), rec(200, 3.0, 4.0)];
        assert_eq!(find_nearest_gps(&records, 50).unwrap().unix_ts, 100);
    }

    #[test]
    fn test_after_all_records_returns_last() {
        let records = vec![rec(100, 1.0, 2.0), rec(200, 3.0, 4.0)];
        assert_eq!(find_nearest_gps(&records, 300).unwrap().unix_ts, 200);
    }

    #[test]
    fn test_exact_match() {
        let records = vec![rec(100, 1.0, 2.0), rec(200, 3.0, 4.0), rec(300, 5.0, 6.0)];
        assert_eq!(find_nearest_gps(&records, 200).unwrap().unix_ts, 200);
    }

    #[test]
    fn test_closer_to_before() {
        // packet at 130: delta to before(100)=30, delta to after(200)=70 → picks before
        let records = vec![rec(100, 1.0, 2.0), rec(200, 3.0, 4.0)];
        assert_eq!(find_nearest_gps(&records, 130).unwrap().unix_ts, 100);
    }

    #[test]
    fn test_closer_to_after() {
        // packet at 170: delta to before(100)=70, delta to after(200)=30 → picks after
        let records = vec![rec(100, 1.0, 2.0), rec(200, 3.0, 4.0)];
        assert_eq!(find_nearest_gps(&records, 170).unwrap().unix_ts, 200);
    }

    #[test]
    fn test_equidistant_prefers_before() {
        // packet at 150: delta to before(100)=50, delta to after(200)=50 → tie, picks before
        let records = vec![rec(100, 1.0, 2.0), rec(200, 3.0, 4.0)];
        assert_eq!(find_nearest_gps(&records, 150).unwrap().unix_ts, 100);
    }

    fn rec_no_ts(system_time: i64, lat: f64, lon: f64) -> GpsRecord {
        GpsRecord {
            latest_packet_timestamp: None,
            system_time,
            lat,
            lon,
        }
    }

    /// A record with no packet timestamp (fixed mode, or a fix before the first
    /// packet) must never overflow the distance arithmetic, and must report its
    /// wall-clock time rather than the i64::MIN sentinel.
    #[test]
    fn a_record_without_a_packet_timestamp_does_not_overflow() {
        // The None record sorts first. Mixed with a timestamped record and
        // probed at timestamps near the integer extremes, this used to subtract
        // i64::MIN and wrap or panic.
        let records = vec![rec_no_ts(42, 1.0, 2.0), rec(100, 3.0, 4.0)];
        for probe in [i64::MIN, -1, 0, 50, i64::MAX] {
            let point = find_nearest_gps(&records, probe).expect("some record");
            // Whichever is chosen, the timestamp is a real value, never i64::MIN.
            assert_ne!(point.unix_ts, i64::MIN);
        }
        // A packet before the timestamped record still resolves without panic;
        // the timestamped neighbour is preferred over the un-anchored one.
        assert_eq!(find_nearest_gps(&records, 60).unwrap().unix_ts, 100);
    }

    /// Fixed mode has exactly one record, with no packet timestamp. It must
    /// always be returned, reporting its wall-clock time.
    #[test]
    fn a_lone_fixed_mode_record_is_always_returned() {
        let records = vec![rec_no_ts(1234, 7.0, 8.0)];
        let point = find_nearest_gps(&records, i64::MAX).expect("some record");
        assert_eq!(point.unix_ts, 1234);
        assert_eq!(point.latitude, 7.0);
    }
}
