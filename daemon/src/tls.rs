//! The web interface over TLS, with a certificate the unit makes for itself.
//!
//! Every client of a WPA2 hotspot that knows the WiFi password can decrypt
//! the others' traffic, so anything the web interface sends in the clear, a
//! password, a session cookie, a recording, is readable by whoever else is on
//! the hotspot. TLS is what closes that. The certificate is not signed by
//! anyone a browser trusts, which costs one warning per browser; pairing is
//! what turns that unknown certificate into a known device.
//!
//! Nothing secret is in the firmware image. Units are flashed in bulk from
//! one image, so the key is made on the unit itself the first time it
//! starts, stored under the auth directory, and never leaves it. A copy of
//! the image, or of `config.toml`, holds nothing that lets anyone impersonate
//! a unit.
//!
//! Built entirely from the RustCrypto stack that the daemon already links
//! for its outbound HTTPS: `p256` for the key, `x509-cert` to assemble the
//! certificate, `rustls` with the `rustls-rustcrypto` provider to serve it.
//! No `ring`, no `aws-lc`, because the firmware build has no C toolchain to
//! link them with.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use der::asn1::{Ia5String, OctetString, UtcTime};
use der::{DateTime, Decode, Encode};
use log::{debug, info, warn};
use p256::ecdsa::{DerSignature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use x509_cert::Certificate;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{ExtendedKeyUsage, SubjectAltName};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::{Time, Validity};

/// The server (leaf) private key, PKCS#8 DER, mode 0600.
pub const KEY_FILE: &str = "tls.key";
/// The server (leaf) certificate, DER. Public by nature.
pub const CERT_FILE: &str = "tls.crt";
/// The unit's own certificate authority: the key, mode 0600, and the
/// certificate a person installs to stop the browser warning for good.
pub const CA_KEY_FILE: &str = "ca.key";
pub const CA_CERT_FILE: &str = "ca.crt";

/// How long a leaf is issued for. Apple accepts a private authority's
/// server certificate only up to 825 days; this leaves a margin.
const LEAF_DAYS: i64 = 800;
/// A leaf this close to its end is replaced at the next check.
const RENEW_BEFORE_DAYS: i64 = 60;
/// A clock reading earlier than this is treated as no reading at all:
/// these units boot in the past when nothing has set their time.
const PLAUSIBLE_YEAR: i32 = 2025;
/// The name the unit answers to over mDNS, once it does. In the certificate
/// from the start so that adding the responder later costs no new warning.
pub const LOCAL_NAME: &str = "rayhunter.local";

/// How long a client gets to finish its handshake.
///
/// Generous for a phone on a bad link, and short enough that a connection
/// which never speaks TLS at all, a plain HTTP client that found the port,
/// does not hold a slot for long.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("certificate: {0}")]
    Certificate(String),
    #[error("TLS configuration: {0}")]
    Config(#[from] rustls::Error),
}

/// A unit's certificate authority, and the server certificate it issued.
///
/// Browsers are shown the server (leaf) certificate with the authority
/// behind it. A browser that has been told to trust the authority accepts
/// any leaf it issues, which is what lets the leaf be replaced freely: when
/// it nears its end, when an address is added, or when the unit learns
/// what time it is.
#[derive(Clone)]
pub struct TlsIdentity {
    ca_cert_der: Vec<u8>,
    ca_key_pkcs8_der: Vec<u8>,
    cert_der: Vec<u8>,
    key_pkcs8_der: Vec<u8>,
}

impl std::fmt::Debug for TlsIdentity {
    /// The key stays out of logs and error messages.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsIdentity")
            .field("fingerprint", &self.fingerprint_hex())
            .finish_non_exhaustive()
    }
}

impl TlsIdentity {
    #[cfg(test)]
    pub fn certificate_der(&self) -> &[u8] {
        &self.cert_der
    }

    pub fn ca_der(&self) -> &[u8] {
        &self.ca_cert_der
    }

    /// The chain browsers are shown: the leaf, then the authority.
    pub fn chain(&self) -> Vec<CertificateDer<'static>> {
        vec![
            CertificateDer::from(self.cert_der.clone()),
            CertificateDer::from(self.ca_cert_der.clone()),
        ]
    }

    pub fn ca_fingerprint(&self) -> [u8; 32] {
        Sha256::digest(&self.ca_cert_der).into()
    }

    pub fn ca_fingerprint_hex(&self) -> String {
        hex_colon(&self.ca_fingerprint())
    }

    pub fn ca_pem(&self) -> String {
        pem("CERTIFICATE", &self.ca_cert_der)
    }

    /// The authority's name, as a person sees it in a certificate list.
    pub fn ca_name(&self) -> String {
        Certificate::from_der(&self.ca_cert_der)
            .map(|c| c.tbs_certificate.subject.to_string())
            .map(|s| s.trim_start_matches("CN=").to_string())
            .unwrap_or_else(|_| "Rayhunter".to_string())
    }

    /// When the leaf runs out, RFC 3339.
    pub fn leaf_not_after(&self) -> Option<String> {
        validity_of(&self.cert_der).map(|(_, after)| after.to_rfc3339())
    }

    /// An Apple configuration profile that installs the authority: one
    /// tap on an iPhone, iPad or Mac, then a confirmation in Settings.
    ///
    /// The identifiers are derived from the authority's fingerprint, so a
    /// profile downloaded twice is the same profile, updated rather than
    /// duplicated.
    pub fn mobileconfig(&self) -> String {
        use base64::Engine as _;
        let fp = self.ca_fingerprint();
        let short = hex(&fp[..2]).to_uppercase();
        let uuid = |salt: u8| {
            let mut b = fp;
            b[0] ^= salt;
            format!(
                "{}-{}-4{}-{}-{}",
                hex(&b[0..4]),
                hex(&b[4..6]),
                &hex(&b[6..8])[1..],
                hex(&b[8..10]),
                hex(&b[10..16])
            )
            .to_uppercase()
        };
        let data = base64::engine::general_purpose::STANDARD.encode(&self.ca_cert_der);
        let name = self.ca_name();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>PayloadContent</key>
	<array>
		<dict>
			<key>PayloadCertificateFileName</key>
			<string>rayhunter-{short}.cer</string>
			<key>PayloadContent</key>
			<data>{data}</data>
			<key>PayloadDescription</key>
			<string>Trusts the certificate authority of one Rayhunter unit, so its web interface opens without a warning.</string>
			<key>PayloadDisplayName</key>
			<string>{name}</string>
			<key>PayloadIdentifier</key>
			<string>org.rayhunter.ca.{short}</string>
			<key>PayloadType</key>
			<string>com.apple.security.root</string>
			<key>PayloadUUID</key>
			<string>{u1}</string>
			<key>PayloadVersion</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>PayloadDescription</key>
	<string>After installing, turn on full trust for "{name}" under Settings, General, About, Certificate Trust Settings.</string>
	<key>PayloadDisplayName</key>
	<string>{name}</string>
	<key>PayloadIdentifier</key>
	<string>org.rayhunter.profile.{short}</string>
	<key>PayloadRemovalDisallowed</key>
	<false/>
	<key>PayloadType</key>
	<string>Configuration</string>
	<key>PayloadUUID</key>
	<string>{u2}</string>
	<key>PayloadVersion</key>
	<integer>1</integer>
</dict>
</plist>
"#,
            u1 = uuid(1),
            u2 = uuid(2),
        )
    }

    /// The server certificate as PEM.
    pub fn certificate_pem(&self) -> String {
        pem("CERTIFICATE", &self.cert_der)
    }

    /// SHA-256 of the certificate, which is what a browser shows as the
    /// fingerprint and what a person can compare against the unit's screen.
    pub fn fingerprint(&self) -> [u8; 32] {
        Sha256::digest(&self.cert_der).into()
    }

    /// The fingerprint the way browsers print it: `AB:CD:…`.
    pub fn fingerprint_hex(&self) -> String {
        hex_colon(&self.fingerprint())
    }

    /// The names and addresses the certificate is for, as `DNS:…` and
    /// `IP:…`, in the order they were written.
    pub fn subject_alt_names(&self) -> Vec<String> {
        subject_alt_names(&self.cert_der).unwrap_or_default()
    }
}

/// Read the SANs out of a DER certificate.
fn subject_alt_names(cert_der: &[u8]) -> Result<Vec<String>, TlsError> {
    let cert = Certificate::from_der(cert_der).map_err(|e| TlsError::Certificate(e.to_string()))?;
    let mut names = Vec::new();
    for ext in cert.tbs_certificate.extensions.iter().flatten() {
        if ext.extn_id != const_oid::db::rfc5280::ID_CE_SUBJECT_ALT_NAME {
            continue;
        }
        let san = SubjectAltName::from_der(ext.extn_value.as_bytes())
            .map_err(|e| TlsError::Certificate(e.to_string()))?;
        for name in san.0 {
            match name {
                GeneralName::DnsName(dns) => names.push(format!("DNS:{}", dns.as_str())),
                GeneralName::IpAddress(octets) => {
                    let bytes = octets.as_bytes();
                    let ip = match bytes.len() {
                        4 => Some(IpAddr::from(<[u8; 4]>::try_from(bytes).unwrap())),
                        16 => Some(IpAddr::from(<[u8; 16]>::try_from(bytes).unwrap())),
                        _ => None,
                    };
                    if let Some(ip) = ip {
                        names.push(format!("IP:{ip}"));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(names)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_colon(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// PEM, the form every operating system's "install this" dialog accepts.
fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ascii"));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// A certificate's validity window.
fn validity_of(
    cert_der: &[u8],
) -> Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
    let cert = Certificate::from_der(cert_der).ok()?;
    let to_chrono = |t: &Time| {
        let secs = t.to_unix_duration().as_secs() as i64;
        chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
    };
    Some((
        to_chrono(&cert.tbs_certificate.validity.not_before)?,
        to_chrono(&cert.tbs_certificate.validity.not_after)?,
    ))
}

/// The unit's best idea of the time, or `None` if it plainly has none.
pub fn plausible_now() -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::Datelike;
    let now = rayhunter::clock::get_adjusted_now().with_timezone(&chrono::Utc);
    (now.year() >= PLAUSIBLE_YEAR).then_some(now)
}

/// Random bytes from the operating system.
///
/// Keys, tokens and anything else that acts as a credential come from here
/// and nowhere else. `fastrand`, used elsewhere for salts, is quick and
/// predictable, which is fine for a salt and disqualifying for a secret.
pub fn random_bytes(buf: &mut [u8]) -> Result<(), TlsError> {
    getrandom::getrandom(buf).map_err(|e| TlsError::Certificate(format!("no randomness: {e}")))
}

/// A fresh P-256 key, PKCS#8 DER, and its signer.
fn new_key() -> Result<(Vec<u8>, SigningKey), TlsError> {
    let cert_err = |e: &dyn std::fmt::Display| TlsError::Certificate(e.to_string());
    // A key from OS randomness. `from_slice` refuses the vanishingly rare
    // out-of-range value, in which case another draw is taken.
    let secret = loop {
        let mut seed = [0u8; 32];
        random_bytes(&mut seed)?;
        if let Ok(secret) = p256::SecretKey::from_slice(&seed) {
            break secret;
        }
    };
    let der = secret
        .to_pkcs8_der()
        .map_err(|e| cert_err(&e))?
        .as_bytes()
        .to_vec();
    Ok((der, SigningKey::from(&secret)))
}

fn random_serial() -> Result<SerialNumber, TlsError> {
    let mut serial = [0u8; 16];
    random_bytes(&mut serial)?;
    // Positive, as X.509 requires.
    serial[0] &= 0x7f;
    SerialNumber::new(&serial).map_err(|e| TlsError::Certificate(e.to_string()))
}

fn utc(dt: chrono::DateTime<chrono::Utc>) -> Result<Time, TlsError> {
    let cert_err = |e: &dyn std::fmt::Display| TlsError::Certificate(e.to_string());
    let dur = std::time::Duration::from_secs(dt.timestamp().max(0) as u64);
    Ok(Time::UtcTime(
        UtcTime::from_unix_duration(dur).map_err(|e| cert_err(&e))?,
    ))
}

fn fixed_date(y: u16, m: u8, d: u8, hh: u8, mm: u8, ss: u8) -> Result<Time, TlsError> {
    let cert_err = |e: &dyn std::fmt::Display| TlsError::Certificate(e.to_string());
    let dt = DateTime::new(y, m, d, hh, mm, ss).map_err(|e| cert_err(&e))?;
    Ok(Time::UtcTime(
        UtcTime::from_date_time(dt).map_err(|e| cert_err(&e))?,
    ))
}

/// Make the unit's certificate authority.
///
/// Self-signed, `CN=Rayhunter <four hex digits>` so two units' authorities
/// can be told apart in a phone's certificate list, valid 2020 to 2049: a
/// root a person installs by hand is under no validity rule, and a unit
/// with no idea of the date must still be able to make one.
pub fn generate_ca() -> Result<(Vec<u8>, Vec<u8>), TlsError> {
    let cert_err = |e: &dyn std::fmt::Display| TlsError::Certificate(e.to_string());
    let (key_der, signer) = new_key()?;
    let mut tag = [0u8; 2];
    random_bytes(&mut tag)?;
    let subject: Name = format!("CN=Rayhunter {}", hex(&tag).to_uppercase())
        .parse()
        .map_err(|e| cert_err(&e))?;
    let spki =
        SubjectPublicKeyInfoOwned::from_key(*signer.verifying_key()).map_err(|e| cert_err(&e))?;
    let validity = Validity {
        not_before: fixed_date(2020, 1, 1, 0, 0, 0)?,
        not_after: fixed_date(2049, 12, 31, 23, 59, 59)?,
    };
    let builder = CertificateBuilder::new(
        Profile::Root,
        random_serial()?,
        validity,
        subject,
        spki,
        &signer,
    )
    .map_err(|e| cert_err(&e))?;
    let cert = builder.build::<DerSignature>().map_err(|e| cert_err(&e))?;
    Ok((cert.to_der().map_err(|e| cert_err(&e))?, key_der))
}

/// Issue a server certificate from the authority for `addresses` and
/// [`LOCAL_NAME`], valid from a day before `now` for [`LEAF_DAYS`].
///
/// `now` is the unit's best idea of the time; without one, the window
/// starts at the beginning of [`PLAUSIBLE_YEAR`], which a real clock is
/// past and stays past for a couple of years.
pub fn issue_leaf(
    ca_cert_der: &[u8],
    ca_key_pkcs8_der: &[u8],
    addresses: &[IpAddr],
    now: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(Vec<u8>, Vec<u8>), TlsError> {
    let cert_err = |e: &dyn std::fmt::Display| TlsError::Certificate(e.to_string());
    let ca = Certificate::from_der(ca_cert_der).map_err(|e| cert_err(&e))?;
    let ca_secret = p256::SecretKey::from_pkcs8_der(ca_key_pkcs8_der).map_err(|e| cert_err(&e))?;
    let ca_signer = SigningKey::from(&ca_secret);

    let (key_der, signer) = new_key()?;
    let start = now
        .map(|n| n - chrono::TimeDelta::days(1))
        .unwrap_or_else(|| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(
                chrono::NaiveDate::from_ymd_opt(PLAUSIBLE_YEAR, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp(),
                0,
            )
            .unwrap()
        });
    let validity = Validity {
        not_before: utc(start)?,
        not_after: utc(start + chrono::TimeDelta::days(LEAF_DAYS))?,
    };
    let subject: Name = "CN=Rayhunter".parse().map_err(|e| cert_err(&e))?;
    let spki =
        SubjectPublicKeyInfoOwned::from_key(*signer.verifying_key()).map_err(|e| cert_err(&e))?;
    let mut builder = CertificateBuilder::new(
        Profile::Leaf {
            issuer: ca.tbs_certificate.subject.clone(),
            enable_key_agreement: false,
            enable_key_encipherment: false,
            include_subject_key_identifier: true,
        },
        random_serial()?,
        validity,
        subject,
        spki,
        &ca_signer,
    )
    .map_err(|e| cert_err(&e))?;

    let mut names = Vec::with_capacity(addresses.len() + 1);
    names.push(GeneralName::DnsName(
        Ia5String::new(LOCAL_NAME).map_err(|e| cert_err(&e))?,
    ));
    for ip in addresses {
        let octets = match ip {
            IpAddr::V4(v4) => v4.octets().to_vec(),
            IpAddr::V6(v6) => v6.octets().to_vec(),
        };
        names.push(GeneralName::IpAddress(
            OctetString::new(octets).map_err(|e| cert_err(&e))?,
        ));
    }
    builder
        .add_extension(&SubjectAltName(names))
        .map_err(|e| cert_err(&e))?;
    builder
        .add_extension(&ExtendedKeyUsage(vec![
            const_oid::db::rfc5280::ID_KP_SERVER_AUTH,
        ]))
        .map_err(|e| cert_err(&e))?;
    let cert = builder.build::<DerSignature>().map_err(|e| cert_err(&e))?;
    Ok((cert.to_der().map_err(|e| cert_err(&e))?, key_der))
}

/// A whole new identity: authority and leaf. Only tests need one in a
/// single step; the daemon makes and keeps the parts separately.
#[cfg(test)]
pub fn generate(addresses: &[IpAddr]) -> Result<TlsIdentity, TlsError> {
    let (ca_cert_der, ca_key_pkcs8_der) = generate_ca()?;
    let (cert_der, key_pkcs8_der) =
        issue_leaf(&ca_cert_der, &ca_key_pkcs8_der, addresses, plausible_now())?;
    Ok(TlsIdentity {
        ca_cert_der,
        ca_key_pkcs8_der,
        cert_der,
        key_pkcs8_der,
    })
}

/// Why a leaf has to be replaced, if it does.
pub fn leaf_needs_reissue(
    cert_der: &[u8],
    ca_cert_der: &[u8],
    addresses: &[IpAddr],
    now: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<&'static str> {
    let Ok(cert) = Certificate::from_der(cert_der) else {
        return Some("it does not parse");
    };
    let Ok(ca) = Certificate::from_der(ca_cert_der) else {
        return Some("the authority does not parse");
    };
    if cert.tbs_certificate.issuer != ca.tbs_certificate.subject {
        return Some("it was not issued by this authority");
    }
    let sans = subject_alt_names(cert_der).unwrap_or_default();
    if addresses
        .iter()
        .any(|ip| !sans.contains(&format!("IP:{ip}")))
    {
        return Some("an address is missing from it");
    }
    if let (Some(now), Some((before, after))) = (now, validity_of(cert_der)) {
        if now < before {
            return Some("it is not valid yet by the clock");
        }
        if now + chrono::TimeDelta::days(RENEW_BEFORE_DAYS) > after {
            return Some("it is near its end");
        }
    }
    None
}

/// Whether a stored key and certificate belong together and can be used.
///
/// Anything short of that, a file that will not parse, a key whose public
/// half is not the one in the certificate, is treated as no identity at all.
fn check_pair(cert_der: &[u8], key_pkcs8_der: &[u8]) -> Result<(), TlsError> {
    let cert_err = |e: &dyn std::fmt::Display| TlsError::Certificate(e.to_string());
    let cert = Certificate::from_der(cert_der).map_err(|e| cert_err(&e))?;
    let secret = p256::SecretKey::from_pkcs8_der(key_pkcs8_der).map_err(|e| cert_err(&e))?;
    let public_from_key = secret
        .public_key()
        .to_public_key_der()
        .map_err(|e| cert_err(&e))?;
    let public_from_cert = cert
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|e| cert_err(&e))?;
    if public_from_key.as_bytes() != public_from_cert.as_slice() {
        return Err(TlsError::Certificate(
            "the stored key does not belong to the stored certificate".into(),
        ));
    }
    Ok(())
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> TlsError + '_ {
    move |source| TlsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Write a file so that it is either entirely there or not there at all.
///
/// Written to a temporary name and renamed into place. A unit can lose
/// power at any moment, and a half-written key would leave it unable to
/// serve TLS until somebody reset it by hand.
pub async fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), TlsError> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&tmp)
            .await
            .map_err(io_err(&tmp))?;
        file.write_all(bytes).await.map_err(io_err(&tmp))?;
        file.sync_all().await.map_err(io_err(&tmp))?;
    }
    tokio::fs::rename(&tmp, path).await.map_err(io_err(path))?;
    Ok(())
}

async fn read_pair(
    cert_path: &Path,
    key_path: &Path,
) -> Result<Option<(Vec<u8>, Vec<u8>)>, TlsError> {
    match (
        tokio::fs::read(cert_path).await,
        tokio::fs::read(key_path).await,
    ) {
        (Ok(cert), Ok(key)) => match check_pair(&cert, &key) {
            Ok(()) => Ok(Some((cert, key))),
            Err(e) => {
                warn!(
                    "{} is unusable and will be replaced: {e}",
                    cert_path.display()
                );
                Ok(None)
            }
        },
        (Err(e), _) | (_, Err(e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        (Err(e), _) => Err(io_err(cert_path)(e)),
        (_, Err(e)) => Err(io_err(key_path)(e)),
    }
}

/// Load the unit's identity from `dir`, making whatever is missing.
///
/// The directory is created with mode 0700 if it does not exist. The
/// authority is made once and kept for the life of the unit: it is what
/// people install. The leaf is reissued whenever [`leaf_needs_reissue`]
/// says so, which browsers that trust the authority never notice.
pub async fn load_or_generate(dir: &Path, addresses: &[IpAddr]) -> Result<TlsIdentity, TlsError> {
    tokio::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .await
        .map_err(io_err(dir))?;
    // The directory may predate this code, or have been made some other
    // way; what matters is what it is now.
    tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .await
        .map_err(io_err(dir))?;

    let ca_cert_path = dir.join(CA_CERT_FILE);
    let ca_key_path = dir.join(CA_KEY_FILE);
    let (ca_cert_der, ca_key_pkcs8_der) = match read_pair(&ca_cert_path, &ca_key_path).await? {
        Some(pair) => pair,
        None => {
            let pair = generate_ca()?;
            write_atomic(&ca_key_path, &pair.1, 0o600).await?;
            write_atomic(&ca_cert_path, &pair.0, 0o644).await?;
            info!(
                "made this unit's certificate authority, fingerprint {}",
                hex_colon(&Sha256::digest(&pair.0))
            );
            pair
        }
    };

    let cert_path = dir.join(CERT_FILE);
    let key_path = dir.join(KEY_FILE);
    let now = plausible_now();
    let leaf = match read_pair(&cert_path, &key_path).await? {
        Some((cert, key)) => match leaf_needs_reissue(&cert, &ca_cert_der, addresses, now) {
            None => Some((cert, key)),
            Some(why) => {
                info!("reissuing the server certificate: {why}");
                None
            }
        },
        None => None,
    };
    let (cert_der, key_pkcs8_der) = match leaf {
        Some(pair) => pair,
        None => {
            let pair = issue_leaf(&ca_cert_der, &ca_key_pkcs8_der, addresses, now)?;
            write_atomic(&key_path, &pair.1, 0o600).await?;
            write_atomic(&cert_path, &pair.0, 0o644).await?;
            pair
        }
    };
    let identity = TlsIdentity {
        ca_cert_der,
        ca_key_pkcs8_der,
        cert_der,
        key_pkcs8_der,
    };
    info!(
        "TLS identity: authority {}, server certificate for {:?} until {}",
        identity.ca_fingerprint_hex(),
        identity.subject_alt_names(),
        identity.leaf_not_after().unwrap_or_default()
    );
    Ok(identity)
}

/// The server certificate rustls hands out, replaceable while running.
///
/// The leaf changes when the unit learns the time or gains an address;
/// the process does not restart for that, so the acceptor asks here on
/// every handshake.
#[derive(Debug)]
pub struct RotatingCert {
    current: std::sync::RwLock<Arc<rustls::sign::CertifiedKey>>,
}

impl RotatingCert {
    fn certified(identity: &TlsIdentity) -> Result<Arc<rustls::sign::CertifiedKey>, TlsError> {
        let provider = rustls::crypto::CryptoProvider::get_default()
            .ok_or_else(|| TlsError::Certificate("no crypto provider installed".into()))?;
        let key = provider
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                identity.key_pkcs8_der.clone(),
            )))?;
        Ok(Arc::new(rustls::sign::CertifiedKey::new(
            identity.chain(),
            key,
        )))
    }

    pub fn new(identity: &TlsIdentity) -> Result<Self, TlsError> {
        Ok(Self {
            current: std::sync::RwLock::new(Self::certified(identity)?),
        })
    }

    pub fn replace(&self, identity: &TlsIdentity) -> Result<(), TlsError> {
        let next = Self::certified(identity)?;
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = next;
        Ok(())
    }
}

impl rustls::server::ResolvesServerCert for RotatingCert {
    fn resolve(
        &self,
        _client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(
            self.current
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        )
    }
}

/// The identity, its store, and the certificate in service, together so
/// the leaf can be reissued and swapped in from anywhere.
pub struct TlsRenewer {
    dir: PathBuf,
    addresses: Vec<IpAddr>,
    identity: tokio::sync::Mutex<TlsIdentity>,
    pub resolver: Arc<RotatingCert>,
}

impl TlsRenewer {
    pub fn new(
        dir: &Path,
        addresses: Vec<IpAddr>,
        identity: TlsIdentity,
    ) -> Result<Self, TlsError> {
        Ok(Self {
            dir: dir.to_path_buf(),
            addresses,
            resolver: Arc::new(RotatingCert::new(&identity)?),
            identity: tokio::sync::Mutex::new(identity),
        })
    }

    /// A copy of the identity as it stands.
    pub async fn identity(&self) -> TlsIdentity {
        self.identity.lock().await.clone()
    }

    /// Reissue the leaf if it is due, and put it into service.
    ///
    /// Called when the unit learns the time, and on a timer. Returns
    /// whether anything changed.
    pub async fn check(&self) -> bool {
        let mut identity = self.identity.lock().await;
        let now = plausible_now();
        let Some(why) = leaf_needs_reissue(
            &identity.cert_der,
            &identity.ca_cert_der,
            &self.addresses,
            now,
        ) else {
            return false;
        };
        info!("reissuing the server certificate: {why}");
        let issued = match issue_leaf(
            &identity.ca_cert_der,
            &identity.ca_key_pkcs8_der,
            &self.addresses,
            now,
        ) {
            Ok(pair) => pair,
            Err(e) => {
                warn!("could not reissue the server certificate: {e}");
                return false;
            }
        };
        if let Err(e) = write_atomic(&self.dir.join(KEY_FILE), &issued.1, 0o600).await {
            warn!("could not store the new server key: {e}");
            return false;
        }
        if let Err(e) = write_atomic(&self.dir.join(CERT_FILE), &issued.0, 0o644).await {
            warn!("could not store the new server certificate: {e}");
            return false;
        }
        identity.cert_der = issued.0;
        identity.key_pkcs8_der = issued.1;
        match self.resolver.replace(&identity) {
            Ok(()) => {
                info!(
                    "new server certificate in service until {}",
                    identity.leaf_not_after().unwrap_or_default()
                );
                true
            }
            Err(e) => {
                warn!("the new server certificate could not be put into service: {e}");
                false
            }
        }
    }
}

/// A rustls server configuration for this identity.
///
/// Uses whatever crypto provider the process installed, which for the
/// firmware build is `rustls-rustcrypto`. No ALPN, so clients speak
/// HTTP/1.1, the only version the server side handles.
pub fn server_config(resolver: Arc<RotatingCert>) -> Result<Arc<rustls::ServerConfig>, TlsError> {
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    Ok(Arc::new(config))
}

/// A TLS listener axum can serve from.
///
/// Handshakes run in their own tasks and finished connections are handed
/// over on a channel, so one client that connects and then says nothing,
/// or a plain HTTP client poking at the port, cannot hold up the connection
/// after it. With handshakes done inline on the accept path, that is exactly
/// what would happen.
pub struct TlsListener {
    local_addr: SocketAddr,
    ready: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    accept_task: tokio::task::JoinHandle<()>,
}

impl TlsListener {
    /// Bind `addr` and start accepting.
    pub async fn bind(addr: SocketAddr, config: Arc<rustls::ServerConfig>) -> io::Result<Self> {
        let tcp = TcpListener::bind(addr).await?;
        let local_addr = tcp.local_addr()?;
        let acceptor = TlsAcceptor::from(config);
        let (tx, ready) = mpsc::channel(16);
        let accept_task = tokio::spawn(async move {
            loop {
                let (stream, peer) = match tcp.accept().await {
                    Ok(pair) => pair,
                    Err(e) => {
                        // Out of descriptors, or the like. Wait rather than
                        // spin on the same error.
                        warn!("accept on {local_addr} failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                        Ok(Ok(tls)) => {
                            let _ = tx.send((tls, peer)).await;
                        }
                        // Ordinary noise: a browser probing, a plain HTTP
                        // client, a client that changed its mind.
                        Ok(Err(e)) => debug!("TLS handshake from {peer} failed: {e}"),
                        Err(_) => debug!("TLS handshake from {peer} timed out"),
                    }
                });
            }
        });
        Ok(Self {
            local_addr,
            ready,
            accept_task,
        })
    }
}

impl Drop for TlsListener {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(pair) => pair,
            // Only reachable once the accept task is gone, which only
            // happens on drop. Nothing more will ever arrive; wait for the
            // shutdown that is already under way rather than return junk.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}

// `mode()` on tokio's OpenOptions and DirBuilder, and `from_mode` on
// Permissions, are unix-only extension traits.
use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn hotspot() -> Vec<IpAddr> {
        vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))]
    }

    #[test]
    fn the_certificate_names_the_hotspot_and_the_local_name() {
        let id = generate(&hotspot()).unwrap();
        let names = id.subject_alt_names();
        assert!(
            names.contains(&"DNS:rayhunter.local".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&"IP:192.168.1.1".to_string()), "{names:?}");
    }

    /// The authority must not depend on the unit knowing what day it is;
    /// the leaf it issues must fit Apple's limit for a private authority.
    #[test]
    fn the_authority_spans_decades_and_the_leaf_under_825_days() {
        let id = generate(&hotspot()).unwrap();
        let ca = Certificate::from_der(id.ca_der()).unwrap();
        assert_eq!(
            ca.tbs_certificate.validity.not_before.to_date_time().year(),
            2020
        );
        assert_eq!(
            ca.tbs_certificate.validity.not_after.to_date_time().year(),
            2049
        );
        assert_eq!(ca.tbs_certificate.issuer, ca.tbs_certificate.subject);
        assert!(id.ca_name().starts_with("Rayhunter "));

        let leaf = Certificate::from_der(id.certificate_der()).unwrap();
        assert_eq!(leaf.tbs_certificate.issuer, ca.tbs_certificate.subject);
        let (before, after) = validity_of(id.certificate_der()).unwrap();
        assert!(after - before <= chrono::TimeDelta::days(825));
        assert!(after - before >= chrono::TimeDelta::days(LEAF_DAYS - 1));
        // Issued for the addresses, in that authority's name.
        assert_eq!(
            leaf_needs_reissue(id.certificate_der(), id.ca_der(), &hotspot(), None),
            None
        );
        assert_eq!(id.chain().len(), 2);
    }

    #[test]
    fn a_leaf_is_reissued_when_an_address_is_new_or_time_runs_out() {
        let id = generate(&hotspot()).unwrap();
        let mut more = hotspot();
        more.push(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 254)));
        assert_eq!(
            leaf_needs_reissue(id.certificate_der(), id.ca_der(), &more, None),
            Some("an address is missing from it")
        );
        let (before, after) = validity_of(id.certificate_der()).unwrap();
        assert_eq!(
            leaf_needs_reissue(
                id.certificate_der(),
                id.ca_der(),
                &hotspot(),
                Some(after - chrono::TimeDelta::days(RENEW_BEFORE_DAYS - 1))
            ),
            Some("it is near its end")
        );
        assert_eq!(
            leaf_needs_reissue(
                id.certificate_der(),
                id.ca_der(),
                &hotspot(),
                Some(before - chrono::TimeDelta::days(30))
            ),
            Some("it is not valid yet by the clock")
        );
        let other = generate(&hotspot()).unwrap();
        assert_eq!(
            leaf_needs_reissue(id.certificate_der(), other.ca_der(), &hotspot(), None),
            Some("it was not issued by this authority")
        );
    }

    /// The Apple profile is a plist carrying the authority, with stable
    /// identifiers so a second download updates the first.
    #[test]
    fn the_profile_carries_the_authority() {
        use base64::Engine as _;
        let id = generate(&hotspot()).unwrap();
        let p = id.mobileconfig();
        assert!(p.contains("<string>com.apple.security.root</string>"));
        let data = p
            .split("<data>")
            .nth(1)
            .unwrap()
            .split("</data>")
            .next()
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap(),
            id.ca_der()
        );
        assert_eq!(p, id.mobileconfig(), "deterministic");
        assert!(p.contains(&id.ca_name()));
        assert!(id.ca_pem().starts_with("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn the_fingerprint_is_printed_the_way_browsers_print_it() {
        let id = generate(&hotspot()).unwrap();
        let hex = id.fingerprint_hex();
        assert_eq!(hex.len(), 32 * 3 - 1);
        assert!(
            hex.split(':')
                .all(|p| p.len() == 2 && u8::from_str_radix(p, 16).is_ok())
        );
        // And debug output never carries the key.
        let debug = format!("{id:?}");
        assert!(debug.contains(&hex[..5]));
        assert!(!debug.contains("key_pkcs8"));
    }

    #[test]
    fn the_pem_form_round_trips_to_the_same_certificate() {
        let id = generate(&hotspot()).unwrap();
        let pem = id.certificate_pem();
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(pem.ends_with("-----END CERTIFICATE-----\n"));
        assert!(pem.lines().all(|l| l.len() <= 64));
        let (_, der) = rustls_pemfile_free_parse(&pem);
        assert_eq!(der, id.certificate_der());
    }

    /// A PEM parser small enough not to need a crate for.
    fn rustls_pemfile_free_parse(pem: &str) -> (String, Vec<u8>) {
        use base64::Engine as _;
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap();
        ("CERTIFICATE".into(), der)
    }

    #[test]
    fn two_units_never_share_an_identity() {
        let a = generate(&hotspot()).unwrap();
        let b = generate(&hotspot()).unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[tokio::test]
    async fn an_identity_is_made_once_and_then_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        let second = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.ca_fingerprint(), second.ca_fingerprint());
        // A new address gets a new leaf under the same authority.
        let mut more = hotspot();
        more.push(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 254)));
        let third = load_or_generate(dir.path(), &more).await.unwrap();
        assert_eq!(third.ca_fingerprint(), first.ca_fingerprint());
        assert_ne!(third.fingerprint(), first.fingerprint());
        assert!(
            third
                .subject_alt_names()
                .contains(&"IP:192.168.1.254".to_string())
        );

        let key_mode = std::fs::metadata(dir.path().join(KEY_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(key_mode, 0o600, "the key must not be world readable");
        let dir_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        // Nothing half written left behind.
        assert!(!dir.path().join("tls.tmp").exists());
    }

    /// A unit with a damaged store must come up with a new identity, not
    /// stay down until somebody fixes it by hand. A damaged leaf costs the
    /// leaf; a damaged authority costs the authority.
    #[tokio::test]
    async fn a_damaged_store_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        std::fs::write(dir.path().join(KEY_FILE), b"not a key").unwrap();
        let second = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.ca_fingerprint(), second.ca_fingerprint());
        std::fs::write(dir.path().join(CA_KEY_FILE), b"not a key").unwrap();
        let third = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_ne!(third.ca_fingerprint(), first.ca_fingerprint());
        let fourth = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_eq!(third.fingerprint(), fourth.fingerprint());
    }

    /// A store from before the authority existed holds a self-signed leaf.
    /// It is replaced under a new authority rather than served on.
    #[tokio::test]
    async fn a_self_signed_leaf_from_an_older_build_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let a = generate(&hotspot()).unwrap();
        let b = generate(&hotspot()).unwrap();
        std::fs::write(dir.path().join(CERT_FILE), &a.cert_der).unwrap();
        std::fs::write(dir.path().join(KEY_FILE), &b.key_pkcs8_der).unwrap();
        let loaded = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_ne!(loaded.fingerprint(), a.fingerprint());
        assert_ne!(loaded.fingerprint(), b.fingerprint());
        assert!(dir.path().join(CA_CERT_FILE).exists());
    }

    #[tokio::test]
    async fn the_leaf_is_swapped_in_service_when_it_is_due() {
        crate::crypto_provider::install_default();
        let dir = tempfile::tempdir().unwrap();
        let identity = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        let old = identity.fingerprint();
        // Ask for an address the leaf does not carry: due at once.
        let mut more = hotspot();
        more.push(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));
        let renewer = TlsRenewer::new(dir.path(), more, identity).unwrap();
        assert!(renewer.check().await);
        assert_ne!(renewer.identity().await.fingerprint(), old);
        assert!(!renewer.check().await, "nothing due the second time");
        let reloaded = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_eq!(
            reloaded.fingerprint(),
            renewer.identity().await.fingerprint()
        );
    }

    /// Accepts the one certificate it was told to, by fingerprint, which is
    /// how a paired browser would ideally treat a unit.
    #[derive(Debug)]
    struct Pinned {
        fingerprint: [u8; 32],
        provider: Arc<rustls::crypto::CryptoProvider>,
    }

    impl rustls::client::danger::ServerCertVerifier for Pinned {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            let got: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
            if got == self.fingerprint {
                Ok(rustls::client::danger::ServerCertVerified::assertion())
            } else {
                Err(rustls::Error::General("wrong certificate".into()))
            }
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.provider
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    async fn fetch_over_tls(addr: SocketAddr, identity: &TlsIdentity) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let provider = rustls::crypto::CryptoProvider::get_default()
            .expect("provider installed")
            .clone();
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned {
                fingerprint: identity.fingerprint(),
                provider,
            }))
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let tcp = TcpStream::connect(addr).await.unwrap();
        let name = rustls::pki_types::ServerName::try_from(LOCAL_NAME).unwrap();
        let mut tls = connector.connect(name, tcp).await.unwrap();
        tls.write_all(b"GET /hello HTTP/1.0\r\nHost: rayhunter.local\r\n\r\n")
            .await
            .unwrap();
        let mut out = Vec::new();
        tls.read_to_end(&mut out).await.unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    /// The whole path, end to end: a generated identity served by axum over
    /// the installed provider, reached by a client that checks the
    /// fingerprint. And a connection that never speaks TLS, opened first,
    /// must not stop it.
    #[tokio::test]
    async fn axum_serves_over_tls_and_a_silent_client_does_not_block_others() {
        crate::crypto_provider::install_default();
        let identity = generate(&hotspot()).unwrap();
        let config = server_config(Arc::new(RotatingCert::new(&identity).unwrap())).unwrap();
        let listener = TlsListener::bind("127.0.0.1:0".parse().unwrap(), config)
            .await
            .unwrap();
        let addr = axum::serve::Listener::local_addr(&listener).unwrap();

        let app = axum::Router::new().route("/hello", axum::routing::get(|| async { "hi" }));
        let shutdown = tokio_util::sync::CancellationToken::new();
        let server = {
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown.cancelled_owned())
                    .await
                    .unwrap();
            })
        };

        // A client that connects and says nothing, held open throughout.
        let _silent = TcpStream::connect(addr).await.unwrap();

        let response =
            tokio::time::timeout(Duration::from_secs(5), fetch_over_tls(addr, &identity))
                .await
                .expect("a real client was blocked behind a silent one");
        assert!(response.starts_with("HTTP/1.0 200"), "{response}");
        assert!(response.ends_with("hi"), "{response}");

        shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
    }
}
