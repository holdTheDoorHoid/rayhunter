//! Answer to `rayhunter.local` on the networks the unit is on.
//!
//! A hotspot's address is a number nobody remembers, and on a home LAN it
//! is a number nobody even knows. `.local` names resolve natively on iOS,
//! macOS, Android, Windows 10+, and on Linux with avahi or systemd-resolved,
//! so a responder here means `https://rayhunter.local:8443` works from any
//! of them, and it is the name the unit's certificate already carries.
//!
//! This is the smallest responder that does the job: it listens on the
//! multicast group, answers `A` (and `ANY`) questions for the one name with
//! the unit's address on the network the question came from, and announces
//! itself when an address appears. No service discovery, no probing, no
//! conflict defence; nothing else on these networks is called rayhunter.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub const NAME: &str = "rayhunter.local";
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const PORT: u16 = 5353;
/// How long resolvers may cache the answer. Short, because the STA address
/// can change with the network.
const TTL: u32 = 120;

const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
/// The cache-flush bit on a class, telling resolvers this answer replaces
/// what they had for the name.
const CACHE_FLUSH: u16 = 0x8000;

/// The addresses the unit currently has, one per network, kept up to date
/// by whoever brings interfaces up and down.
pub type SharedAddresses = Arc<RwLock<Vec<Ipv4Addr>>>;

/// One question out of a query.
#[derive(Debug, PartialEq)]
pub struct Question {
    pub name: String,
    pub qtype: u16,
    /// Whether the asker wants a unicast reply (the QU bit).
    pub unicast: bool,
}

/// Pull the questions out of a DNS message. Answers and anything after the
/// question section are ignored; compression pointers end a name, which is
/// fine for queries, where they hardly ever appear.
pub fn parse_questions(packet: &[u8]) -> Vec<Question> {
    let mut out = Vec::new();
    if packet.len() < 12 {
        return out;
    }
    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    if flags & 0x8000 != 0 {
        // A response, not a query.
        return out;
    }
    let qdcount = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let mut pos = 12;
    for _ in 0..qdcount.min(16) {
        let mut labels = Vec::new();
        loop {
            let Some(&len) = packet.get(pos) else {
                return out;
            };
            pos += 1;
            if len == 0 {
                break;
            }
            if len & 0xC0 == 0xC0 {
                // A pointer: one more byte, then the name is over.
                pos += 1;
                break;
            }
            let end = pos + len as usize;
            let Some(label) = packet.get(pos..end) else {
                return out;
            };
            labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
            pos = end;
        }
        let Some(rest) = packet.get(pos..pos + 4) else {
            return out;
        };
        let qtype = u16::from_be_bytes([rest[0], rest[1]]);
        let qclass = u16::from_be_bytes([rest[2], rest[3]]);
        pos += 4;
        out.push(Question {
            name: labels.join("."),
            qtype,
            unicast: qclass & CACHE_FLUSH != 0,
        });
    }
    out
}

/// Whether a question is one this responder answers.
pub fn is_for_us(q: &Question) -> bool {
    q.name == NAME && (q.qtype == TYPE_A || q.qtype == TYPE_ANY)
}

/// An authoritative response carrying one `A` record for [`NAME`].
///
/// Message id zero and no question section, as multicast responses are
/// sent; the cache-flush bit set, since there is exactly one right answer.
pub fn build_response(address: Ipv4Addr) -> Vec<u8> {
    let mut p = Vec::with_capacity(64);
    p.extend_from_slice(&[0, 0]); // id
    p.extend_from_slice(&0x8400u16.to_be_bytes()); // response, authoritative
    p.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]); // qd, an, ns, ar
    for label in NAME.split('.') {
        p.push(label.len() as u8);
        p.extend_from_slice(label.as_bytes());
    }
    p.push(0);
    p.extend_from_slice(&TYPE_A.to_be_bytes());
    p.extend_from_slice(&(CLASS_IN | CACHE_FLUSH).to_be_bytes());
    p.extend_from_slice(&TTL.to_be_bytes());
    p.extend_from_slice(&4u16.to_be_bytes());
    p.extend_from_slice(&address.octets());
    p
}

/// The unit's address on the same network as `peer`, or the first one.
pub fn address_for(addresses: &[Ipv4Addr], peer: Ipv4Addr) -> Option<Ipv4Addr> {
    addresses
        .iter()
        .copied()
        .find(|a| a.octets()[..3] == peer.octets()[..3])
        .or_else(|| addresses.first().copied())
}

fn open_socket() -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.set_multicast_ttl_v4(255)?;
    socket.set_multicast_loop_v4(false)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, PORT)).into())?;
    UdpSocket::from_std(socket.into())
}

/// Run the responder until `shutdown`.
///
/// `addresses` is read on every question, and joined to the multicast group
/// as new ones appear, so an address that arrives later, the STA one when
/// the unit joins a home network, is served without a restart.
pub fn run(task_tracker: &TaskTracker, addresses: SharedAddresses, shutdown: CancellationToken) {
    task_tracker.spawn(async move {
        let socket = match open_socket() {
            Ok(s) => s,
            Err(e) => {
                warn!("mDNS responder could not open its socket: {e}");
                return;
            }
        };
        let mut joined: Vec<Ipv4Addr> = Vec::new();
        let mut buf = [0u8; 1500];
        let group = SocketAddrV4::new(GROUP, PORT);
        loop {
            // Join the group on any interface that has appeared since the
            // last look, and say hello on it.
            let current = addresses.read().await.clone();
            for addr in &current {
                if joined.contains(addr) {
                    continue;
                }
                match socket.join_multicast_v4(GROUP, *addr) {
                    Ok(()) => {
                        joined.push(*addr);
                        info!("answering to {NAME} as {addr}");
                        let _ = socket2::SockRef::from(&socket).set_multicast_if_v4(addr);
                        let _ = socket.send_to(&build_response(*addr), group).await;
                    }
                    Err(e) => debug!("could not join mDNS on {addr}: {e}"),
                }
            }
            joined.retain(|a| current.contains(a));

            let (len, peer) = tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                r = socket.recv_from(&mut buf) => match r {
                    Ok(v) => v,
                    Err(e) => {
                        debug!("mDNS receive: {e}");
                        continue;
                    }
                },
            };
            let SocketAddr::V4(peer) = peer else {
                continue;
            };
            let questions = parse_questions(&buf[..len]);
            let Some(q) = questions.iter().find(|q| is_for_us(q)) else {
                continue;
            };
            let Some(ours) = address_for(&current, *peer.ip()) else {
                continue;
            };
            let response = build_response(ours);
            let _ = socket2::SockRef::from(&socket).set_multicast_if_v4(&ours);
            // A legacy resolver asking from an ordinary port, or one that
            // set the QU bit, gets its answer directly; everyone else hears
            // it on the group, as the protocol expects.
            if q.unicast || peer.port() != PORT {
                let _ = socket.send_to(&response, peer).await;
            } else {
                let _ = socket.send_to(&response, group).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16, qclass: u16) -> Vec<u8> {
        let mut p = vec![0x12, 0x34, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            p.push(label.len() as u8);
            p.extend_from_slice(label.as_bytes());
        }
        p.push(0);
        p.extend_from_slice(&qtype.to_be_bytes());
        p.extend_from_slice(&qclass.to_be_bytes());
        p
    }

    #[test]
    fn a_question_for_our_name_is_recognised_whatever_its_case() {
        let qs = parse_questions(&query("RayHunter.LOCAL", TYPE_A, CLASS_IN));
        assert_eq!(qs.len(), 1);
        assert!(is_for_us(&qs[0]));
        assert!(!qs[0].unicast);
        let qs = parse_questions(&query("rayhunter.local", TYPE_ANY, CLASS_IN | CACHE_FLUSH));
        assert!(is_for_us(&qs[0]));
        assert!(qs[0].unicast, "the QU bit asks for a unicast reply");
    }

    #[test]
    fn other_names_types_and_responses_are_ignored() {
        assert!(!is_for_us(
            &parse_questions(&query("printer.local", TYPE_A, CLASS_IN))[0]
        ));
        assert!(!is_for_us(
            &parse_questions(&query("rayhunter.local", 28, CLASS_IN))[0]
        )); // AAAA
        let mut resp = query("rayhunter.local", TYPE_A, CLASS_IN);
        resp[2] = 0x84;
        assert!(parse_questions(&resp).is_empty());
        assert!(parse_questions(&[0u8; 5]).is_empty());
        // Truncated inside a label.
        let mut short = query("rayhunter.local", TYPE_A, CLASS_IN);
        short.truncate(16);
        assert!(parse_questions(&short).is_empty());
    }

    #[test]
    fn the_response_is_one_authoritative_a_record_with_cache_flush() {
        let r = build_response(Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(&r[..12], &[0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
        let name_end = 12 + 1 + 9 + 1 + 5 + 1;
        assert_eq!(&r[12..name_end], b"\x09rayhunter\x05local\x00");
        assert_eq!(&r[name_end..name_end + 2], &TYPE_A.to_be_bytes());
        assert_eq!(&r[name_end + 2..name_end + 4], &0x8001u16.to_be_bytes());
        assert_eq!(&r[name_end + 4..name_end + 8], &TTL.to_be_bytes());
        assert_eq!(&r[name_end + 8..name_end + 10], &[0, 4]);
        assert_eq!(&r[name_end + 10..], &[192, 168, 1, 1]);
        // And it parses as a response, not a query.
        assert!(parse_questions(&r).is_empty());
    }

    #[test]
    fn the_answer_is_the_address_on_the_askers_network() {
        let ours = [Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(10, 0, 0, 7)];
        assert_eq!(
            address_for(&ours, Ipv4Addr::new(10, 0, 0, 20)),
            Some(Ipv4Addr::new(10, 0, 0, 7))
        );
        assert_eq!(
            address_for(&ours, Ipv4Addr::new(192, 168, 1, 50)),
            Some(Ipv4Addr::new(192, 168, 1, 1))
        );
        assert_eq!(
            address_for(&ours, Ipv4Addr::new(172, 16, 0, 9)),
            Some(Ipv4Addr::new(192, 168, 1, 1)),
            "an unknown network gets the first address rather than nothing"
        );
        assert_eq!(address_for(&[], Ipv4Addr::new(10, 0, 0, 1)), None);
    }
}
