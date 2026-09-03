//! The keys on both sides, and how they are written down.
//!
//! **Recipient keys** belong to the service. There are two: the *ingest* key,
//! which the internet-facing server holds so it can open summary bundles as
//! they arrive, and the *archive* key, which it does not hold, so that a raw
//! capture encrypted to it is opaque to anyone who breaks into the server.
//! Both are P-256 keys used with HPKE.
//!
//! **Submitter keys** belong to a unit. A submission is signed with one so
//! the server can refuse forgeries and can honour a withdrawal from the same
//! unit later. They are P-256 ECDSA keys, the curve the unit already uses for
//! its own TLS certificate, and they are meant to be rotated.
//!
//! Public keys travel as standard base64 of the uncompressed SEC1 point (65
//! bytes). A key's **id** is the first sixteen hex characters of the SHA-256
//! of those bytes, and its **fingerprint** the whole hash in groups of four,
//! for a person to compare on two screens.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hpke::kem::DhP256HkdfSha256;
use hpke::{Deserializable, Kem, Serializable};
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use sha2::{Digest, Sha256};

use crate::{Error, hex};

/// The KEM every recipient key uses.
pub type RecipientKem = DhP256HkdfSha256;

/// A service key that submissions are encrypted to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientPublicKey(pub(crate) <RecipientKem as Kem>::PublicKey);

/// The matching private half. Held by the server for the ingest key, and by
/// nobody online for the archive key.
#[derive(Clone)]
pub struct RecipientPrivateKey(pub(crate) <RecipientKem as Kem>::PrivateKey);

impl RecipientPrivateKey {
    /// A fresh key pair from the operating system's randomness.
    pub fn generate() -> (RecipientPrivateKey, RecipientPublicKey) {
        let (sk, pk) = RecipientKem::gen_keypair(&mut rand_core::OsRng);
        (RecipientPrivateKey(sk), RecipientPublicKey(pk))
    }

    pub fn public_key(&self) -> RecipientPublicKey {
        RecipientPublicKey(RecipientKem::sk_to_pk(&self.0))
    }

    /// The raw 32-byte scalar, base64. This is the secret; treat the string
    /// as one.
    pub fn to_base64(&self) -> String {
        B64.encode(self.0.to_bytes())
    }

    pub fn from_base64(text: &str) -> Result<Self, Error> {
        let bytes = B64.decode(text.trim())?;
        let sk = <RecipientKem as Kem>::PrivateKey::from_bytes(&bytes)
            .map_err(|e| Error::Key(format!("not a recipient private key: {e}")))?;
        Ok(RecipientPrivateKey(sk))
    }
}

impl RecipientPublicKey {
    pub fn to_base64(&self) -> String {
        B64.encode(self.0.to_bytes())
    }

    pub fn from_base64(text: &str) -> Result<Self, Error> {
        let bytes = B64.decode(text.trim())?;
        let pk = <RecipientKem as Kem>::PublicKey::from_bytes(&bytes)
            .map_err(|e| Error::Key(format!("not a recipient public key: {e}")))?;
        Ok(RecipientPublicKey(pk))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }

    /// The short name the manifest uses to say which key a part was
    /// encrypted to.
    pub fn key_id(&self) -> String {
        key_id_of(&self.to_bytes())
    }

    /// The full hash, for a person to compare.
    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.to_bytes())
    }
}

/// The first sixteen hex characters of the SHA-256 of a public key's bytes.
pub fn key_id_of(public_key_bytes: &[u8]) -> String {
    let digest = Sha256::digest(public_key_bytes);
    hex(&digest[..8])
}

/// The whole SHA-256 of a public key's bytes, in groups of four, for a person
/// to compare across two screens.
pub fn fingerprint_of(public_key_bytes: &[u8]) -> String {
    let digest = hex(&Sha256::digest(public_key_bytes));
    digest
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A unit's signing key.
#[derive(Clone)]
pub struct SubmitterKey {
    signing: SigningKey,
}

impl SubmitterKey {
    pub fn generate() -> Self {
        SubmitterKey {
            signing: SigningKey::random(&mut rand_core::OsRng),
        }
    }

    /// The public half as base64 of the uncompressed SEC1 point, the form the
    /// manifest carries.
    pub fn public_key_base64(&self) -> String {
        B64.encode(self.public_key_bytes())
    }

    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.signing
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec()
    }

    pub fn key_id(&self) -> String {
        key_id_of(&self.public_key_bytes())
    }

    /// Sign arbitrary bytes. The signature is the fixed 64-byte `r || s`
    /// form, base64.
    pub fn sign(&self, message: &[u8]) -> String {
        let signature: Signature = self.signing.sign(message);
        B64.encode(signature.to_bytes())
    }

    /// PKCS#8 PEM, the form the unit stores beside its TLS key.
    pub fn to_pkcs8_pem(&self) -> Result<String, Error> {
        let secret = p256::SecretKey::from(&self.signing);
        secret
            .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .map(|pem| pem.to_string())
            .map_err(|e| Error::Key(format!("could not encode the signing key: {e}")))
    }

    pub fn from_pkcs8_pem(pem: &str) -> Result<Self, Error> {
        let secret = p256::SecretKey::from_pkcs8_pem(pem)
            .map_err(|e| Error::Key(format!("not a signing key: {e}")))?;
        Ok(SubmitterKey {
            signing: SigningKey::from(&secret),
        })
    }
}

/// Check a signature made by [`SubmitterKey::sign`] against the public key
/// the manifest carries.
pub fn verify_signature(
    public_key_base64: &str,
    message: &[u8],
    signature_base64: &str,
) -> Result<(), Error> {
    let key_bytes = B64.decode(public_key_base64.trim())?;
    let verifying = VerifyingKey::from_sec1_bytes(&key_bytes)
        .map_err(|e| Error::Key(format!("not a submitter public key: {e}")))?;
    let signature_bytes = B64.decode(signature_base64.trim())?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| Error::BadSignature)?;
    verifying
        .verify(message, &signature)
        .map_err(|_| Error::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_keys_round_trip_through_base64() {
        let (sk, pk) = RecipientPrivateKey::generate();
        let sk2 = RecipientPrivateKey::from_base64(&sk.to_base64()).unwrap();
        assert_eq!(sk2.public_key(), pk);
        let pk2 = RecipientPublicKey::from_base64(&pk.to_base64()).unwrap();
        assert_eq!(pk2, pk);
        assert_eq!(pk.to_bytes().len(), 65, "uncompressed SEC1 point");
        assert!(RecipientPublicKey::from_base64("bm90IGEga2V5").is_err());
        assert!(RecipientPublicKey::from_base64("not base64!").is_err());
    }

    /// The id is what a manifest names a key by, so it must be stable across
    /// processes and never depend on anything but the key bytes.
    #[test]
    fn key_ids_are_stable_and_short() {
        let (_, pk) = RecipientPrivateKey::generate();
        assert_eq!(pk.key_id(), key_id_of(&pk.to_bytes()));
        assert_eq!(pk.key_id().len(), 16);
        assert_eq!(pk.fingerprint().split(' ').count(), 16);
        assert_eq!(key_id_of(b"abc"), "ba7816bf8f01cfea");
    }

    #[test]
    fn submitter_keys_sign_verify_and_round_trip() {
        let key = SubmitterKey::generate();
        let message = b"the manifest bytes exactly as sent";
        let signature = key.sign(message);
        verify_signature(&key.public_key_base64(), message, &signature).unwrap();

        // A changed message, a changed signature, and another key's public
        // half must all be refused.
        assert!(verify_signature(&key.public_key_base64(), b"altered", &signature).is_err());
        let mut broken = B64.decode(&signature).unwrap();
        broken[10] ^= 0x01;
        assert!(verify_signature(&key.public_key_base64(), message, &B64.encode(broken)).is_err());
        let other = SubmitterKey::generate();
        assert!(verify_signature(&other.public_key_base64(), message, &signature).is_err());

        let pem = key.to_pkcs8_pem().unwrap();
        assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        let again = SubmitterKey::from_pkcs8_pem(&pem).unwrap();
        assert_eq!(again.key_id(), key.key_id());
        verify_signature(&again.public_key_base64(), message, &again.sign(message)).unwrap();
    }

    #[test]
    fn garbage_public_keys_and_signatures_are_refused_not_panicked_on() {
        let key = SubmitterKey::generate();
        assert!(verify_signature("", b"m", &key.sign(b"m")).is_err());
        assert!(verify_signature(&key.public_key_base64(), b"m", "").is_err());
        assert!(verify_signature(&key.public_key_base64(), b"m", "AAAA").is_err());
        assert!(verify_signature("AAAA", b"m", &key.sign(b"m")).is_err());
    }
}
