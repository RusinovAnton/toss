//! TLS identity: who is really on the other end of a connection.
//!
//! Peers use self-signed certificates, so a certificate authority proves
//! nothing here. What identifies a device is the SHA-256 fingerprint of its
//! certificate, exactly as in the protocol. These verifiers turn that into
//! something enforceable:
//!
//! - our server asks every client for a certificate and records its
//!   fingerprint, so "this request came from a paired device" is a fact about
//!   the handshake rather than a claim in the request body;
//! - our client pins the fingerprint of a paired device, so nobody who has
//!   taken over its address can pretend to be it.

use crate::identity::fingerprint_from_der;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use tokio_rustls::rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName, Error, SignatureScheme,
};

/// The provider rustls uses for signature verification here.
fn provider() -> Arc<CryptoProvider> {
    CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()))
}

/// Installs the process-wide crypto provider. Safe to call repeatedly.
pub fn install_provider() {
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// Accepts any client certificate, purely so one is presented and its
/// fingerprint can be read off the handshake.
///
/// This is not a hole: without it the server would learn nothing at all about
/// the client. The certificate is not trusted because it verifies, it is
/// matched against the paired fingerprints afterwards. Client certificates
/// stay optional so peers that offer none can still send files the ordinary
/// way, as an unpaired device.
#[derive(Debug)]
pub struct AnyClientCert {
    provider: Arc<CryptoProvider>,
    no_hints: Vec<DistinguishedName>,
}

impl AnyClientCert {
    pub fn new() -> Self {
        AnyClientCert {
            provider: provider(),
            no_hints: Vec::new(),
        }
    }
}

impl Default for AnyClientCert {
    fn default() -> Self {
        AnyClientCert::new()
    }
}

impl ClientCertVerifier for AnyClientCert {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        false
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &self.no_hints
    }

    fn verify_client_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
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
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Verifies a server by certificate fingerprint.
///
/// With `expected` set the handshake fails unless the certificate hashes to
/// that fingerprint, which is what makes pairing meaningful. With `None` any
/// certificate is accepted, which is the protocol's normal mode for a device
/// we have never met.
#[derive(Debug)]
pub struct PinnedServerCert {
    expected: Option<String>,
    provider: Arc<CryptoProvider>,
    /// Set when a handshake was refused over the fingerprint.
    ///
    /// A TLS failure reaches the caller as an opaque connection error, and
    /// "this is not the device you paired with" deserves to be told apart
    /// from "the network dropped". Reading it off a flag beats matching on
    /// the text of someone else's error message.
    rejected: Arc<AtomicBool>,
}

impl PinnedServerCert {
    pub fn new(expected: Option<String>) -> Self {
        PinnedServerCert {
            expected: expected.map(|f| f.trim().to_ascii_uppercase()),
            provider: provider(),
            rejected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Shared with the caller, which clears it before a request and checks it
    /// if that request fails.
    pub fn rejection_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.rejected)
    }
}

impl ServerCertVerifier for PinnedServerCert {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let Some(expected) = &self.expected else {
            return Ok(ServerCertVerified::assertion());
        };
        let actual = fingerprint_from_der(end_entity);
        if &actual == expected {
            Ok(ServerCertVerified::assertion())
        } else {
            self.rejected.store(true, Ordering::SeqCst);
            eprintln!("refusing a paired device: expected {expected}, got {actual}");
            Err(Error::InvalidCertificate(CertificateError::Other(
                tokio_rustls::rustls::OtherError(Arc::new(FingerprintMismatch)),
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
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
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
struct FingerprintMismatch;

impl std::fmt::Display for FingerprintMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("certificate fingerprint does not match the paired device")
    }
}

impl std::error::Error for FingerprintMismatch {}

/// The fingerprint of the first certificate a peer presented, if any.
pub fn peer_fingerprint(certs: Option<&[CertificateDer<'static>]>) -> Option<String> {
    certs?.first().map(|cert| fingerprint_from_der(cert))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    #[test]
    fn a_peer_without_a_certificate_has_no_fingerprint() {
        assert_eq!(peer_fingerprint(None), None);
        assert_eq!(peer_fingerprint(Some(&[])), None);
    }

    #[test]
    fn the_peer_fingerprint_is_the_certificate_hash() {
        use tokio_rustls::rustls::pki_types::pem::PemObject;
        let identity = Identity::generate().unwrap();
        let der =
            CertificateDer::from_pem_slice(identity.certificate_pem.as_bytes()).unwrap();
        let certs = vec![der.into_owned()];
        assert_eq!(peer_fingerprint(Some(&certs)), Some(identity.fingerprint));
    }

    #[test]
    fn pinning_is_case_insensitive_about_the_fingerprint() {
        let pinned = PinnedServerCert::new(Some("abc".into()));
        assert_eq!(pinned.expected.as_deref(), Some("ABC"));
    }

    #[test]
    fn an_unpinned_verifier_accepts_anything() {
        install_provider();
        let identity = Identity::generate().unwrap();
        use tokio_rustls::rustls::pki_types::pem::PemObject;
        let der = CertificateDer::from_pem_slice(identity.certificate_pem.as_bytes()).unwrap();
        let verifier = PinnedServerCert::new(None);
        assert!(verifier
            .verify_server_cert(
                &der,
                &[],
                &ServerName::try_from("192.168.1.5").unwrap(),
                &[],
                UnixTime::now()
            )
            .is_ok());
    }

    #[test]
    fn a_pinned_verifier_rejects_a_different_certificate() {
        install_provider();
        use tokio_rustls::rustls::pki_types::pem::PemObject;
        let ours = Identity::generate().unwrap();
        let theirs = Identity::generate().unwrap();
        let der = CertificateDer::from_pem_slice(theirs.certificate_pem.as_bytes()).unwrap();

        let matching = PinnedServerCert::new(Some(theirs.fingerprint.clone()));
        assert!(matching
            .verify_server_cert(
                &der,
                &[],
                &ServerName::try_from("192.168.1.5").unwrap(),
                &[],
                UnixTime::now()
            )
            .is_ok());

        let mismatched = PinnedServerCert::new(Some(ours.fingerprint));
        assert!(mismatched
            .verify_server_cert(
                &der,
                &[],
                &ServerName::try_from("192.168.1.5").unwrap(),
                &[],
                UnixTime::now()
            )
            .is_err());
    }

    #[test]
    fn a_refused_handshake_raises_the_flag() {
        install_provider();
        use tokio_rustls::rustls::pki_types::pem::PemObject;
        let ours = Identity::generate().unwrap();
        let theirs = Identity::generate().unwrap();
        let der = CertificateDer::from_pem_slice(theirs.certificate_pem.as_bytes()).unwrap();

        let verifier = PinnedServerCert::new(Some(ours.fingerprint));
        let flag = verifier.rejection_flag();
        assert!(!flag.load(Ordering::SeqCst));
        let _ = verifier.verify_server_cert(
            &der,
            &[],
            &ServerName::try_from("192.168.1.5").unwrap(),
            &[],
            UnixTime::now(),
        );
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn client_certificates_are_requested_but_not_required() {
        let verifier = AnyClientCert::new();
        assert!(verifier.offer_client_auth());
        assert!(!verifier.client_auth_mandatory());
        assert!(verifier.root_hint_subjects().is_empty());
    }
}
