//! MAC addresses and variable-length address prefixes.
//!
//! Surveillance signatures cannot assume a prefix is exactly three bytes. The
//! KeyW Corporation allocation discussed in EFForg/rayhunter#1042 is
//! `70:b3:d5:7c:b` — nine hex digits, ending on a nibble boundary, because it
//! is a MA-S block carved out of an IEEE registry prefix. [`MacPrefix`]
//! therefore measures its length in nibbles, not bytes.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A 48-bit hardware address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const fn new(octets: [u8; 6]) -> Self {
        MacAddr(octets)
    }

    pub const fn octets(&self) -> [u8; 6] {
        self.0
    }

    /// True when the locally-administered bit is set, which is how nearly all
    /// MAC randomisation schemes mark a synthesised address. Such an address
    /// says nothing durable about the hardware, so OUI-based signatures must
    /// not be applied to it.
    pub const fn is_locally_administered(&self) -> bool {
        self.0[0] & 0x02 != 0
    }

    /// True for group (multicast/broadcast) addresses.
    pub const fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }

    /// True when this address is a plausible subject for OUI matching: a
    /// globally-administered unicast address.
    pub const fn is_globally_unique(&self) -> bool {
        !self.is_locally_administered() && !self.is_multicast()
    }

    /// Parse `aa:bb:cc:dd:ee:ff`, also accepting `-` separators or none at all.
    pub fn parse(s: &str) -> Result<Self, MacParseError> {
        let digits = collect_hex_digits(s)?;
        if digits.len() != 12 {
            return Err(MacParseError::WrongLength {
                nibbles: digits.len(),
            });
        }
        let mut octets = [0u8; 6];
        for (i, octet) in octets.iter_mut().enumerate() {
            *octet = (digits[i * 2] << 4) | digits[i * 2 + 1];
        }
        Ok(MacAddr(octets))
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = self.0;
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            o[0], o[1], o[2], o[3], o[4], o[5]
        )
    }
}

impl fmt::Debug for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MacAddr({self})")
    }
}

impl Serialize for MacAddr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        MacAddr::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// A prefix of a MAC address, measured in nibbles so that allocations which do
/// not end on a byte boundary can be expressed exactly.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacPrefix {
    /// Significant nibbles, left-aligned. Trailing nibbles are always zero so
    /// that two equal prefixes compare equal.
    octets: [u8; 6],
    nibbles: u8,
}

impl MacPrefix {
    /// Longest prefix that carries no information: matching everything is
    /// almost certainly a configuration error, so parsing rejects it.
    pub const MIN_NIBBLES: u8 = 2;
    pub const MAX_NIBBLES: u8 = 12;

    /// Parse a prefix such as `70:b3:d5`, `70:b3:d5:7c:b`, or `70b3d57cb`.
    ///
    /// Separators are optional and may be `:` or `-`. The length in nibbles is
    /// whatever the caller wrote, so `70:b3:d5:7c:b` is nine nibbles and
    /// matches only addresses whose fifth octet begins with `b`.
    pub fn parse(s: &str) -> Result<Self, MacParseError> {
        let digits = collect_hex_digits(s)?;
        let nibbles = digits.len();
        if nibbles < Self::MIN_NIBBLES as usize || nibbles > Self::MAX_NIBBLES as usize {
            return Err(MacParseError::WrongLength { nibbles });
        }
        let mut octets = [0u8; 6];
        for (i, digit) in digits.iter().enumerate() {
            let shift = if i % 2 == 0 { 4 } else { 0 };
            octets[i / 2] |= digit << shift;
        }
        Ok(MacPrefix {
            octets,
            nibbles: nibbles as u8,
        })
    }

    pub const fn nibbles(&self) -> u8 {
        self.nibbles
    }

    /// True when `addr` begins with this prefix.
    ///
    /// Whole octets are compared directly; a trailing odd nibble compares only
    /// the high half of the next octet.
    pub fn matches(&self, addr: &MacAddr) -> bool {
        let whole = (self.nibbles / 2) as usize;
        if self.octets[..whole] != addr.0[..whole] {
            return false;
        }
        if self.nibbles % 2 == 1 {
            let expected = self.octets[whole] & 0xf0;
            let actual = addr.0[whole] & 0xf0;
            if expected != actual {
                return false;
            }
        }
        true
    }

    /// How specific this prefix is, used to prefer the tightest match when
    /// several signatures cover the same address.
    pub const fn specificity(&self) -> u8 {
        self.nibbles
    }
}

impl fmt::Display for MacPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Render as colon-separated octets, with a bare nibble at the end when
        // the prefix stops mid-octet: 70:b3:d5:7c:b
        let whole = (self.nibbles / 2) as usize;
        for i in 0..whole {
            if i > 0 {
                write!(f, ":")?;
            }
            write!(f, "{:02x}", self.octets[i])?;
        }
        if self.nibbles % 2 == 1 {
            if whole > 0 {
                write!(f, ":")?;
            }
            write!(f, "{:x}", self.octets[whole] >> 4)?;
        }
        Ok(())
    }
}

impl fmt::Debug for MacPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MacPrefix({self}/{})", self.nibbles)
    }
}

impl Serialize for MacPrefix {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MacPrefix {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        MacPrefix::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MacParseError {
    #[error("'{0}' is not a hexadecimal digit")]
    NotHex(char),
    #[error("expected 2-12 hex digits for a prefix or exactly 12 for an address, got {nibbles}")]
    WrongLength { nibbles: usize },
}

/// Strip separators and decode each remaining character as a hex digit.
///
/// Bounded by the caller's input length; callers pass configuration strings, so
/// a malformed value returns an error rather than panicking.
fn collect_hex_digits(s: &str) -> Result<Vec<u8>, MacParseError> {
    let mut out = Vec::with_capacity(12);
    for c in s.chars() {
        if c == ':' || c == '-' || c == '.' {
            continue;
        }
        match c.to_digit(16) {
            Some(d) => out.push(d as u8),
            None => return Err(MacParseError::NotHex(c)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_a_plain_address() {
        let mac = MacAddr::parse("58:32:77:28:7b:a6").unwrap();
        assert_eq!(mac.octets(), [0x58, 0x32, 0x77, 0x28, 0x7b, 0xa6]);
        assert_eq!(mac.to_string(), "58:32:77:28:7b:a6");
    }

    #[test]
    fn accepts_alternative_separators() {
        let a = MacAddr::parse("58-32-77-28-7b-a6").unwrap();
        let b = MacAddr::parse("583277287ba6").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_a_truncated_address() {
        assert!(matches!(
            MacAddr::parse("58:32:77"),
            Err(MacParseError::WrongLength { nibbles: 6 })
        ));
    }

    #[test]
    fn rejects_non_hex_input() {
        assert!(matches!(
            MacAddr::parse("58:32:77:28:7b:zz"),
            Err(MacParseError::NotHex('z'))
        ));
    }

    #[test]
    fn three_byte_prefix_matches_on_octet_boundary() {
        let prefix = MacPrefix::parse("70:b3:d5").unwrap();
        assert_eq!(prefix.nibbles(), 6);
        assert!(prefix.matches(&MacAddr::parse("70:b3:d5:00:00:01").unwrap()));
        assert!(prefix.matches(&MacAddr::parse("70:b3:d5:ff:ff:ff").unwrap()));
        assert!(!prefix.matches(&MacAddr::parse("70:b3:d6:00:00:01").unwrap()));
    }

    /// The KeyW case from EFForg/rayhunter#1042: nine nibbles, so the fifth
    /// octet must begin with `b` but its low nibble is unconstrained.
    #[test]
    fn nibble_length_prefix_constrains_only_the_high_nibble() {
        let prefix = MacPrefix::parse("70:b3:d5:7c:b").unwrap();
        assert_eq!(prefix.nibbles(), 9);

        assert!(prefix.matches(&MacAddr::parse("70:b3:d5:7c:b0:00").unwrap()));
        assert!(prefix.matches(&MacAddr::parse("70:b3:d5:7c:bf:ff").unwrap()));

        // Low nibble of octet 4 differs from b -> no match.
        assert!(!prefix.matches(&MacAddr::parse("70:b3:d5:7c:a0:00").unwrap()));
        assert!(!prefix.matches(&MacAddr::parse("70:b3:d5:7c:c0:00").unwrap()));
        // A neighbouring MA-S block under the same 70:b3:d5 prefix.
        assert!(!prefix.matches(&MacAddr::parse("70:b3:d5:7d:b0:00").unwrap()));
    }

    #[test]
    fn nibble_prefix_round_trips_through_display() {
        let prefix = MacPrefix::parse("70:b3:d5:7c:b").unwrap();
        assert_eq!(prefix.to_string(), "70:b3:d5:7c:b");
        assert_eq!(MacPrefix::parse(&prefix.to_string()).unwrap(), prefix);
    }

    #[test]
    fn full_length_prefix_behaves_like_an_exact_address() {
        let prefix = MacPrefix::parse("70:b3:d5:7c:b1:22").unwrap();
        assert_eq!(prefix.nibbles(), 12);
        assert!(prefix.matches(&MacAddr::parse("70:b3:d5:7c:b1:22").unwrap()));
        assert!(!prefix.matches(&MacAddr::parse("70:b3:d5:7c:b1:23").unwrap()));
    }

    #[test]
    fn rejects_prefixes_that_are_too_short_or_too_long() {
        assert!(MacPrefix::parse("7").is_err());
        assert!(MacPrefix::parse("70:b3:d5:7c:b1:22:33").is_err());
    }

    #[test]
    fn specificity_orders_prefixes_by_length() {
        let broad = MacPrefix::parse("70:b3:d5").unwrap();
        let narrow = MacPrefix::parse("70:b3:d5:7c:b").unwrap();
        assert!(narrow.specificity() > broad.specificity());
    }

    #[test]
    fn identifies_randomised_addresses() {
        // Locally-administered bit set in the first octet: a randomised address.
        let randomised = MacAddr::parse("da:a1:19:00:00:01").unwrap();
        assert!(randomised.is_locally_administered());
        assert!(!randomised.is_globally_unique());

        let burned_in = MacAddr::parse("70:b3:d5:00:00:01").unwrap();
        assert!(!burned_in.is_locally_administered());
        assert!(burned_in.is_globally_unique());

        let group = MacAddr::parse("01:00:5e:00:00:01").unwrap();
        assert!(group.is_multicast());
        assert!(!group.is_globally_unique());
    }

    #[test]
    fn serde_round_trip() {
        let mac = MacAddr::parse("70:b3:d5:7c:b1:22").unwrap();
        let json = serde_json::to_string(&mac).unwrap();
        assert_eq!(json, "\"70:b3:d5:7c:b1:22\"");
        assert_eq!(serde_json::from_str::<MacAddr>(&json).unwrap(), mac);

        let prefix = MacPrefix::parse("70:b3:d5:7c:b").unwrap();
        let json = serde_json::to_string(&prefix).unwrap();
        assert_eq!(json, "\"70:b3:d5:7c:b\"");
        assert_eq!(serde_json::from_str::<MacPrefix>(&json).unwrap(), prefix);
    }
}
