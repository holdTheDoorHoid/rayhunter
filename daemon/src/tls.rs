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
use x509_cert::ext::pkix::{ExtendedKeyUsage, KeyUsage, KeyUsages, SubjectAltName};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::{Time, Validity};

/// The private key, PKCS#8 DER, mode 0600.
pub const KEY_FILE: &str = "tls.key";
/// The certificate, DER. Public by nature; served to anyone who asks.
pub const CERT_FILE: &str = "tls.crt";
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

/// A unit's certificate and the key that goes with it.
#[derive(Clone)]
pub struct TlsIdentity {
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
    pub fn certificate_der(&self) -> &[u8] {
        &self.cert_der
    }

    /// The certificate as PEM, the form every operating system's "install
    /// this certificate" dialog accepts.
    pub fn certificate_pem(&self) -> String {
        use base64::Engine as _;
        let body = base64::engine::general_purpose::STANDARD.encode(self.certificate_der());
        let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
        for line in body.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(line).expect("base64 is ascii"));
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
        pem
    }

    /// SHA-256 of the certificate, which is what a browser shows as the
    /// fingerprint and what a person can compare against the unit's screen.
    pub fn fingerprint(&self) -> [u8; 32] {
        Sha256::digest(&self.cert_der).into()
    }

    /// The fingerprint the way browsers print it: `AB:CD:…`.
    pub fn fingerprint_hex(&self) -> String {
        self.fingerprint()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
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

/// Random bytes from the operating system.
///
/// Keys, tokens and anything else that acts as a credential come from here
/// and nowhere else. `fastrand`, used elsewhere for salts, is quick and
/// predictable, which is fine for a salt and disqualifying for a secret.
pub fn random_bytes(buf: &mut [u8]) -> Result<(), TlsError> {
    getrandom::getrandom(buf).map_err(|e| TlsError::Certificate(format!("no randomness: {e}")))
}

/// Make a fresh key and a certificate for `addresses` and [`LOCAL_NAME`].
///
/// EC P-256, self-signed, valid from the start of 2020 to the end of 2049.
/// The dates are deliberately wide: these units have no battery-backed clock
/// and often no way to set one, so the certificate must not depend on the
/// unit knowing what day it is. A browser that the person has told to
/// proceed does not check them again. The far end stays inside what a
/// two-digit-year `UTCTime` can express, which is why it is 2049 and not
/// later.
pub fn generate(addresses: &[IpAddr]) -> Result<TlsIdentity, TlsError> {
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
    let signer = SigningKey::from(&secret);
    let key_pkcs8_der = secret
        .to_pkcs8_der()
        .map_err(|e| cert_err(&e))?
        .as_bytes()
        .to_vec();

    // A random serial, positive as X.509 requires.
    let mut serial = [0u8; 16];
    random_bytes(&mut serial)?;
    serial[0] &= 0x7f;
    let serial = SerialNumber::new(&serial).map_err(|e| cert_err(&e))?;

    let date = |y, m, d, hh, mm, ss| -> Result<Time, TlsError> {
        let dt = DateTime::new(y, m, d, hh, mm, ss).map_err(|e| cert_err(&e))?;
        Ok(Time::UtcTime(
            UtcTime::from_date_time(dt).map_err(|e| cert_err(&e))?,
        ))
    };
    let validity = Validity {
        not_before: date(2020, 1, 1, 0, 0, 0)?,
        not_after: date(2049, 12, 31, 23, 59, 59)?,
    };

    let subject: Name = "CN=Rayhunter".parse().map_err(|e| cert_err(&e))?;
    let spki =
        SubjectPublicKeyInfoOwned::from_key(*signer.verifying_key()).map_err(|e| cert_err(&e))?;

    // `Manual` adds no extensions of its own, so the certificate carries
    // exactly what is listed here: the names, and that it is for a server.
    let mut builder = CertificateBuilder::new(
        Profile::Manual { issuer: None },
        serial,
        validity,
        subject,
        spki,
        &signer,
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
        .add_extension(&KeyUsage(KeyUsages::DigitalSignature.into()))
        .map_err(|e| cert_err(&e))?;
    builder
        .add_extension(&ExtendedKeyUsage(vec![
            const_oid::db::rfc5280::ID_KP_SERVER_AUTH,
        ]))
        .map_err(|e| cert_err(&e))?;

    let cert = builder.build::<DerSignature>().map_err(|e| cert_err(&e))?;
    let cert_der = cert.to_der().map_err(|e| cert_err(&e))?;

    Ok(TlsIdentity {
        cert_der,
        key_pkcs8_der,
    })
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

/// Load the unit's identity from `dir`, making one if there is none.
///
/// The directory is created with mode 0700 if it does not exist. A stored
/// pair that does not check out is replaced rather than repaired, with a
/// warning, since the alternative is a unit that never comes up over TLS.
///
/// Once made, a certificate is kept. It is not regenerated when the unit's
/// addresses change, because every browser that has accepted it would have
/// to accept it again, and the warning they would see is the same either
/// way.
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

    let key_path = dir.join(KEY_FILE);
    let cert_path = dir.join(CERT_FILE);

    let stored = match (
        tokio::fs::read(&cert_path).await,
        tokio::fs::read(&key_path).await,
    ) {
        (Ok(cert_der), Ok(key_pkcs8_der)) => match check_pair(&cert_der, &key_pkcs8_der) {
            Ok(()) => Some(TlsIdentity {
                cert_der,
                key_pkcs8_der,
            }),
            Err(e) => {
                warn!("the stored TLS identity is unusable, making a new one: {e}");
                None
            }
        },
        (Err(e), _) | (_, Err(e)) if e.kind() == io::ErrorKind::NotFound => None,
        (Err(e), _) => return Err(io_err(&cert_path)(e)),
        (_, Err(e)) => return Err(io_err(&key_path)(e)),
    };
    if let Some(identity) = stored {
        debug!(
            "loaded the TLS identity, fingerprint {}",
            identity.fingerprint_hex()
        );
        return Ok(identity);
    }

    let identity = generate(addresses)?;
    write_atomic(&key_path, &identity.key_pkcs8_der, 0o600).await?;
    write_atomic(&cert_path, &identity.cert_der, 0o644).await?;
    info!(
        "made this unit's TLS identity for {:?}, fingerprint {}",
        identity.subject_alt_names(),
        identity.fingerprint_hex()
    );
    Ok(identity)
}

/// A rustls server configuration for this identity.
///
/// Uses whatever crypto provider the process installed, which for the
/// firmware build is `rustls-rustcrypto`. No ALPN, so clients speak
/// HTTP/1.1, the only version the server side handles.
pub fn server_config(identity: &TlsIdentity) -> Result<Arc<rustls::ServerConfig>, TlsError> {
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(identity.cert_der.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_pkcs8_der.clone())),
        )?;
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

    /// The dates must not depend on the unit knowing what day it is.
    #[test]
    fn the_certificate_is_valid_from_2020_to_2049() {
        let id = generate(&hotspot()).unwrap();
        let cert = Certificate::from_der(id.certificate_der()).unwrap();
        let validity = cert.tbs_certificate.validity;
        assert_eq!(validity.not_before.to_date_time().year(), 2020);
        assert_eq!(validity.not_after.to_date_time().year(), 2049);
        // A leaf, self-signed.
        assert_eq!(cert.tbs_certificate.issuer, cert.tbs_certificate.subject);
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
    /// stay down until somebody fixes it by hand.
    #[tokio::test]
    async fn a_damaged_store_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        std::fs::write(dir.path().join(KEY_FILE), b"not a key").unwrap();
        let second = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
        // And what was written is itself a usable pair.
        let third = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_eq!(second.fingerprint(), third.fingerprint());
    }

    /// A key and certificate that do not belong together are as good as
    /// none. rustls would refuse them at startup otherwise.
    #[tokio::test]
    async fn a_mismatched_pair_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let a = generate(&hotspot()).unwrap();
        let b = generate(&hotspot()).unwrap();
        std::fs::write(dir.path().join(CERT_FILE), &a.cert_der).unwrap();
        std::fs::write(dir.path().join(KEY_FILE), &b.key_pkcs8_der).unwrap();
        let loaded = load_or_generate(dir.path(), &hotspot()).await.unwrap();
        assert_ne!(loaded.fingerprint(), a.fingerprint());
        assert_ne!(loaded.fingerprint(), b.fingerprint());
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
        let config = server_config(&identity).unwrap();
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
