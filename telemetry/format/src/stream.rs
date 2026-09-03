//! Encrypting a file of any size to a recipient key, in chunks.
//!
//! A raw capture can be a hundred megabytes and the device that has to
//! encrypt it has a hundred and sixty of memory, most of it in use. So the
//! file is sealed a chunk at a time under one HPKE context, and opened the
//! same way. Each chunk is authenticated on its own and bound to its
//! position, and the last one is marked, so a stream cannot be truncated,
//! reordered or extended without the receiver noticing.
//!
//! The layout, all integers big endian:
//!
//! ```text
//! magic     "RHTE"
//! version   u8 = 1
//! suite     u8 = 1     DHKEM(P-256, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305
//! enc       65 bytes   the HPKE encapsulated key
//! chunk*    len u32, flags u8, ciphertext[len], tag[16]
//! ```
//!
//! `flags` bit 0 marks the final chunk. The additional data for chunk `i` is
//! `"RHTE-chunk" || i as u64 || flags`, and the HPKE context's own sequence
//! number makes every nonce distinct. The `info` string given to HPKE ties
//! the stream to one submission and one part name, so a ciphertext copied
//! from one submission into another does not open.

use std::io::{Read, Write};

use hpke::aead::{AeadTag, ChaCha20Poly1305};
use hpke::kdf::HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use sha2::{Digest, Sha256};

use crate::keys::{RecipientKem, RecipientPrivateKey, RecipientPublicKey};
use crate::{Error, FORMAT, hex};

type Aead = ChaCha20Poly1305;
type Kdf = HkdfSha256;

const MAGIC: &[u8; 4] = b"RHTE";
const VERSION: u8 = 1;
const SUITE: u8 = 1;
const FLAG_LAST: u8 = 0b1;
const TAG_LEN: usize = 16;
const AAD_PREFIX: &[u8] = b"RHTE-chunk";

/// Plaintext bytes per chunk. A quarter of a megabyte keeps the working set
/// small on the device while keeping the per-chunk overhead (21 bytes)
/// negligible.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// The most a chunk header may claim. Slightly above [`CHUNK_SIZE`] so a
/// future sender may use a larger chunk, but small enough that a corrupt
/// header cannot ask the receiver to allocate gigabytes.
pub const MAX_CHUNK_SIZE: u32 = 4 * 1024 * 1024;

/// The domain separator for one part of one submission.
///
/// Both sides must derive it identically, which is why it is a function
/// here rather than a string each of them formats.
pub fn info_for(submission_id: &str, part_name: &str) -> Vec<u8> {
    format!("{FORMAT}|{submission_id}|{part_name}").into_bytes()
}

/// What sealing produced, for the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    pub plaintext_bytes: u64,
    pub ciphertext_bytes: u64,
    /// SHA-256 of the whole ciphertext stream, as hex.
    pub sha256: String,
}

/// What opening produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub plaintext_bytes: u64,
}

/// Counts and hashes what passes through it.
struct Counting<W: Write> {
    inner: W,
    written: u64,
    hasher: Sha256,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn aad(index: u64, flags: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(AAD_PREFIX.len() + 9);
    out.extend_from_slice(AAD_PREFIX);
    out.extend_from_slice(&index.to_be_bytes());
    out.push(flags);
    out
}

/// Fill `buf` from `reader` as far as it will go, returning how much was
/// read. Zero means the reader is exhausted.
fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Encrypt everything from `reader` to `recipient`, writing the stream to
/// `writer`.
pub fn seal<R: Read, W: Write>(
    recipient: &RecipientPublicKey,
    info: &[u8],
    mut reader: R,
    writer: W,
) -> Result<Sealed, Error> {
    let (enc, mut ctx) = hpke::setup_sender::<Aead, Kdf, RecipientKem, _>(
        &OpModeS::Base,
        &recipient.0,
        info,
        &mut rand_core::OsRng,
    )
    .map_err(|e| Error::Hpke(format!("could not set up encryption: {e}")))?;

    let mut out = Counting {
        inner: writer,
        written: 0,
        hasher: Sha256::new(),
    };
    out.write_all(MAGIC)?;
    out.write_all(&[VERSION, SUITE])?;
    out.write_all(&enc.to_bytes())?;

    // Read one chunk ahead so the last chunk can be marked as it is written,
    // without an empty trailer after every stream whose length happens to
    // be a multiple of the chunk size.
    let mut current = vec![0u8; CHUNK_SIZE];
    let mut next = vec![0u8; CHUNK_SIZE];
    let mut current_len = read_full(&mut reader, &mut current)?;
    let mut plaintext_bytes = 0u64;
    let mut index = 0u64;
    loop {
        let next_len = if current_len == 0 {
            0
        } else {
            read_full(&mut reader, &mut next)?
        };
        let last = next_len == 0;
        let flags = if last { FLAG_LAST } else { 0 };
        let chunk = &mut current[..current_len];
        let tag = ctx
            .seal_in_place_detached(chunk, &aad(index, flags))
            .map_err(|e| Error::Hpke(format!("could not seal chunk {index}: {e}")))?;
        out.write_all(&(current_len as u32).to_be_bytes())?;
        out.write_all(&[flags])?;
        out.write_all(chunk)?;
        out.write_all(&tag.to_bytes())?;
        plaintext_bytes += current_len as u64;
        index += 1;
        if last {
            break;
        }
        std::mem::swap(&mut current, &mut next);
        current_len = next_len;
    }
    out.flush()?;
    Ok(Sealed {
        plaintext_bytes,
        ciphertext_bytes: out.written,
        sha256: hex(&out.hasher.finalize()),
    })
}

/// Decrypt a stream made by [`seal`] with the matching private key and the
/// same `info`. Nothing is written for a chunk that fails to authenticate,
/// and a stream that ends early or carries anything after its last chunk is
/// an error, so a caller that gets `Ok` has the whole plaintext and only
/// that.
pub fn open<R: Read, W: Write>(
    recipient: &RecipientPrivateKey,
    info: &[u8],
    mut reader: R,
    mut writer: W,
) -> Result<Opened, Error> {
    let mut header = [0u8; 6];
    if read_full(&mut reader, &mut header)? != header.len() {
        return Err(Error::BadHeader);
    }
    if &header[..4] != MAGIC || header[4] != VERSION || header[5] != SUITE {
        return Err(Error::BadHeader);
    }
    let mut enc_bytes = vec![0u8; <RecipientKem as Kem>::EncappedKey::size()];
    if read_full(&mut reader, &mut enc_bytes)? != enc_bytes.len() {
        return Err(Error::BadHeader);
    }
    let enc =
        <RecipientKem as Kem>::EncappedKey::from_bytes(&enc_bytes).map_err(|_| Error::BadHeader)?;
    let mut ctx =
        hpke::setup_receiver::<Aead, Kdf, RecipientKem>(&OpModeR::Base, &recipient.0, &enc, info)
            .map_err(|e| Error::Hpke(format!("could not set up decryption: {e}")))?;

    let mut plaintext_bytes = 0u64;
    let mut index = 0u64;
    let mut chunk = Vec::new();
    loop {
        let mut chunk_header = [0u8; 5];
        let got = read_full(&mut reader, &mut chunk_header)?;
        if got == 0 {
            return Err(Error::Truncated);
        }
        if got != chunk_header.len() {
            return Err(Error::Truncated);
        }
        let len = u32::from_be_bytes([
            chunk_header[0],
            chunk_header[1],
            chunk_header[2],
            chunk_header[3],
        ]);
        if len > MAX_CHUNK_SIZE {
            return Err(Error::ChunkTooLarge(len));
        }
        let flags = chunk_header[4];
        chunk.resize(len as usize, 0);
        if read_full(&mut reader, &mut chunk)? != chunk.len() {
            return Err(Error::Truncated);
        }
        let mut tag_bytes = [0u8; TAG_LEN];
        if read_full(&mut reader, &mut tag_bytes)? != TAG_LEN {
            return Err(Error::Truncated);
        }
        let tag = AeadTag::<Aead>::from_bytes(&tag_bytes).map_err(|_| Error::BadHeader)?;
        ctx.open_in_place_detached(&mut chunk, &aad(index, flags), &tag)
            .map_err(|_| Error::Hpke(format!("chunk {index} failed to authenticate")))?;
        writer.write_all(&chunk)?;
        plaintext_bytes += chunk.len() as u64;
        index += 1;
        if flags & FLAG_LAST != 0 {
            break;
        }
    }
    // Anything after the last chunk was not sealed by the sender.
    let mut trailing = [0u8; 1];
    if read_full(&mut reader, &mut trailing)? != 0 {
        return Err(Error::TrailingData);
    }
    writer.flush()?;
    Ok(Opened { plaintext_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn round_trip(plaintext: &[u8]) -> (Vec<u8>, Sealed) {
        let (sk, pk) = RecipientPrivateKey::generate();
        let info = info_for("0123456789abcdef0123456789abcdef", "summary.zip");
        let mut sealed = Vec::new();
        let summary = seal(&pk, &info, Cursor::new(plaintext), &mut sealed).unwrap();
        let mut opened = Vec::new();
        let result = open(&sk, &info, Cursor::new(&sealed), &mut opened).unwrap();
        assert_eq!(opened, plaintext);
        assert_eq!(result.plaintext_bytes, plaintext.len() as u64);
        assert_eq!(summary.plaintext_bytes, plaintext.len() as u64);
        assert_eq!(summary.ciphertext_bytes, sealed.len() as u64);
        assert_eq!(summary.sha256, hex(&Sha256::digest(&sealed)));
        (sealed, summary)
    }

    #[test]
    fn empty_small_and_multi_chunk_inputs_round_trip() {
        round_trip(b"");
        round_trip(b"hello");
        let big: Vec<u8> = (0..(CHUNK_SIZE * 3 + 17))
            .map(|i| (i % 251) as u8)
            .collect();
        round_trip(&big);
    }

    /// A length that is an exact multiple of the chunk size must not produce
    /// an empty trailing chunk, and must still round trip.
    #[test]
    fn an_exact_multiple_of_the_chunk_size_has_no_empty_trailer() {
        let exact: Vec<u8> = (0..(CHUNK_SIZE * 2)).map(|i| (i % 7) as u8).collect();
        let (sealed, _) = round_trip(&exact);
        let header = 6 + 65;
        let per_chunk = 5 + CHUNK_SIZE + TAG_LEN;
        assert_eq!(sealed.len(), header + 2 * per_chunk);
    }

    #[test]
    fn the_ciphertext_is_not_the_plaintext() {
        let secret = b"310150123456789 is an IMSI".repeat(100);
        let (sealed, _) = round_trip(&secret);
        assert!(!sealed.windows(15).any(|w| w == b"310150123456789"));
    }

    #[test]
    fn the_wrong_key_or_info_does_not_open() {
        let (_, pk) = RecipientPrivateKey::generate();
        let (other_sk, _) = RecipientPrivateKey::generate();
        let info = info_for("id", "part");
        let mut sealed = Vec::new();
        seal(&pk, &info, Cursor::new(b"payload"), &mut sealed).unwrap();
        let mut out = Vec::new();
        assert!(open(&other_sk, &info, Cursor::new(&sealed), &mut out).is_err());
        assert!(
            out.is_empty(),
            "nothing may be written before authentication"
        );

        let (sk, pk) = RecipientPrivateKey::generate();
        let mut sealed = Vec::new();
        seal(&pk, &info, Cursor::new(b"payload"), &mut sealed).unwrap();
        let mut out = Vec::new();
        assert!(
            open(
                &sk,
                &info_for("id", "other-part"),
                Cursor::new(&sealed),
                &mut out
            )
            .is_err()
        );
        assert!(out.is_empty());
    }

    #[test]
    fn tampering_truncation_and_trailing_data_are_all_refused() {
        let (sk, pk) = RecipientPrivateKey::generate();
        let info = info_for("id", "part");
        let plaintext: Vec<u8> = (0..(CHUNK_SIZE + 100)).map(|i| (i % 13) as u8).collect();
        let mut sealed = Vec::new();
        seal(&pk, &info, Cursor::new(&plaintext), &mut sealed).unwrap();

        // A flipped byte in the second chunk's ciphertext.
        let mut tampered = sealed.clone();
        let second_chunk_body = 6 + 65 + 5 + CHUNK_SIZE + TAG_LEN + 5 + 3;
        tampered[second_chunk_body] ^= 0x80;
        let mut out = Vec::new();
        let err = open(&sk, &info, Cursor::new(&tampered), &mut out).unwrap_err();
        assert!(matches!(err, Error::Hpke(_)), "{err}");
        // The first chunk was written before the second failed: the caller
        // sees an error and must discard the output, which is the contract.
        assert_eq!(out.len(), CHUNK_SIZE);

        // Cut off before the last chunk: the first chunk authenticates, the
        // stream then ends.
        let cut = &sealed[..6 + 65 + 5 + CHUNK_SIZE + TAG_LEN];
        let mut out = Vec::new();
        assert!(matches!(
            open(&sk, &info, Cursor::new(cut), &mut out),
            Err(Error::Truncated)
        ));

        // Cut off inside the last chunk.
        let cut = &sealed[..sealed.len() - 3];
        let mut out = Vec::new();
        assert!(matches!(
            open(&sk, &info, Cursor::new(cut), &mut out),
            Err(Error::Truncated)
        ));

        // Bytes after the last chunk.
        let mut extended = sealed.clone();
        extended.push(0);
        let mut out = Vec::new();
        assert!(matches!(
            open(&sk, &info, Cursor::new(&extended), &mut out),
            Err(Error::TrailingData)
        ));

        // The two chunks swapped: each authenticates alone, but not at the
        // other's position.
        let header = 6 + 65;
        let first_len = 5 + CHUNK_SIZE + TAG_LEN;
        let (h, rest) = sealed.split_at(header);
        let (first, second) = rest.split_at(first_len);
        let swapped = [h, second, first].concat();
        let mut out = Vec::new();
        assert!(open(&sk, &info, Cursor::new(&swapped), &mut out).is_err());
    }

    #[test]
    fn garbage_is_refused_without_panicking() {
        let (sk, _) = RecipientPrivateKey::generate();
        let info = info_for("id", "part");
        for input in [
            &b""[..],
            b"RHTE",
            b"RHTE\x01\x01",
            b"not a stream at all, not even close, really not",
            &[b'R', b'H', b'T', b'E', 1, 1][..],
        ] {
            let mut out = Vec::new();
            assert!(open(&sk, &info, Cursor::new(input), &mut out).is_err());
        }
        // A well-formed header followed by a chunk header claiming far more
        // than the format allows must be refused before any allocation.
        let (_, pk) = RecipientPrivateKey::generate();
        let mut sealed = Vec::new();
        seal(&pk, &info, Cursor::new(b"x"), &mut sealed).unwrap();
        let mut huge = sealed[..6 + 65].to_vec();
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        huge.push(0);
        let mut out = Vec::new();
        assert!(matches!(
            open(&sk, &info, Cursor::new(&huge), &mut out),
            Err(Error::ChunkTooLarge(_))
        ));
    }
}
