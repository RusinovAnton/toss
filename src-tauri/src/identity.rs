//! Device identity: alias, self-signed TLS certificate and its fingerprint.
//!
//! Wire facts (from the LocalSend protocol repo and the official app source):
//! - fingerprint = SHA-256 of the certificate in DER form, uppercase hex, no separators
//! - deviceType is `desktop` for macOS / Windows / Linux
//! - deviceModel strings the official app sends: `macOS`, `Windows`, `Linux`
//!
//! The identity is persisted as JSON in the OS app-data directory so it is
//! stable across restarts. Peers remember us by fingerprint, so regenerating
//! it would make us look like a new device.

use crate::protocol::{DeviceInfo, DeviceType, ProtocolType, DEFAULT_PORT, PROTOCOL_VERSION};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::CertificateDer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "identity.json";
const CERT_COMMON_NAME: &str = "Toss";

#[derive(Debug)]
pub enum IdentityError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Cert(rcgen::Error),
    Pem(String),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityError::Io(e) => write!(f, "identity io error: {e}"),
            IdentityError::Json(e) => write!(f, "identity json error: {e}"),
            IdentityError::Cert(e) => write!(f, "certificate error: {e}"),
            IdentityError::Pem(e) => write!(f, "pem error: {e}"),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<std::io::Error> for IdentityError {
    fn from(e: std::io::Error) -> Self {
        IdentityError::Io(e)
    }
}
impl From<serde_json::Error> for IdentityError {
    fn from(e: serde_json::Error) -> Self {
        IdentityError::Json(e)
    }
}
impl From<rcgen::Error> for IdentityError {
    fn from(e: rcgen::Error) -> Self {
        IdentityError::Cert(e)
    }
}

/// Everything we persist. Private key stays in Rust; never sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub alias: String,
    pub fingerprint: String,
    pub certificate_pem: String,
    pub private_key_pem: String,
}

/// What the frontend gets from `get_identity()`. Field names match the
/// protocol's device-info object so the same shape can be reused later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    pub alias: String,
    pub fingerprint: String,
    pub device_model: String,
    pub device_type: DeviceType,
    pub port: u16,
    /// The build's own version, so the app can say which one it is. Baked in
    /// at compile time, which is when CI has just written the release number
    /// into `Cargo.toml`; a local build therefore shows whatever the
    /// repository says, which lags between releases.
    pub app_version: String,
}

impl Identity {
    /// Fresh alias + ECDSA P-256 key pair + self-signed certificate.
    ///
    /// rcgen's default validity (1975..4096) is used on purpose, same as the
    /// official app: certificates never expire and never need rotation.
    pub fn generate() -> Result<Self, IdentityError> {
        let key_pair = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, CERT_COMMON_NAME);
        let cert = params.self_signed(&key_pair)?;
        Ok(Identity {
            alias: random_alias(),
            fingerprint: fingerprint_from_der(cert.der()),
            certificate_pem: cert.pem(),
            private_key_pem: key_pair.serialize_pem(),
        })
    }

    pub fn file_path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// `Ok(None)` when no identity file exists yet.
    pub fn load(dir: &Path) -> Result<Option<Self>, IdentityError> {
        let path = Self::file_path(dir);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let identity: Identity = serde_json::from_slice(&bytes)?;
        Ok(Some(identity))
    }

    /// Write atomically (temp file + rename) so a crash never leaves a half file.
    pub fn save(&self, dir: &Path) -> Result<(), IdentityError> {
        std::fs::create_dir_all(dir)?;
        let path = Self::file_path(dir);
        let tmp = dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Load the stored identity, or create and persist a new one.
    ///
    /// A stored identity whose fingerprint does not match its certificate is
    /// treated as corrupt and replaced.
    pub fn load_or_create(dir: &Path) -> Result<Self, IdentityError> {
        if let Some(existing) = Self::load(dir)? {
            match existing.verify() {
                Ok(()) => return Ok(existing),
                Err(e) => eprintln!("stored identity invalid ({e}); regenerating"),
            }
        }
        let fresh = Self::generate()?;
        fresh.save(dir)?;
        Ok(fresh)
    }

    /// Check the fingerprint really is the hash of the stored certificate and
    /// that the private key still parses.
    pub fn verify(&self) -> Result<(), IdentityError> {
        let recomputed = fingerprint_from_pem(&self.certificate_pem)?;
        if recomputed != self.fingerprint {
            return Err(IdentityError::Pem(format!(
                "fingerprint mismatch: stored {} computed {}",
                self.fingerprint, recomputed
            )));
        }
        KeyPair::from_pem(&self.private_key_pem)?;
        Ok(())
    }

    pub fn info(&self) -> IdentityInfo {
        IdentityInfo {
            alias: self.alias.clone(),
            fingerprint: self.fingerprint.clone(),
            device_model: device_model().to_string(),
            device_type: DeviceType::Desktop,
            port: DEFAULT_PORT,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// This device as the protocol's device object, for announces and
    /// `/register` requests.
    pub fn to_device_info(&self) -> DeviceInfo {
        DeviceInfo {
            alias: self.alias.clone(),
            version: PROTOCOL_VERSION.to_string(),
            device_model: Some(device_model().to_string()),
            device_type: Some(DeviceType::Desktop),
            fingerprint: self.fingerprint.clone(),
            port: DEFAULT_PORT,
            // We always run the HTTPS server (Phase 3). The download API is not
            // implemented, so `download` stays false.
            protocol: ProtocolType::Https,
            download: false,
        }
    }
}

/// SHA-256 of DER bytes as uppercase hex. This is the LocalSend fingerprint.
pub fn fingerprint_from_der(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

pub fn fingerprint_from_pem(pem: &str) -> Result<String, IdentityError> {
    let der = CertificateDer::from_pem_slice(pem.as_bytes())
        .map_err(|e| IdentityError::Pem(format!("{e:?}")))?;
    Ok(fingerprint_from_der(&der))
}

/// Same strings the official desktop app puts in `deviceModel`.
pub fn device_model() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unknown"
    }
}

const ADJECTIVES: &[&str] = &[
    "Adorable", "Brave", "Bright", "Calm", "Clever", "Cool", "Cosmic", "Crispy", "Curious",
    "Dapper", "Eager", "Fancy", "Fluffy", "Friendly", "Gentle", "Giant", "Glowing", "Golden",
    "Happy", "Hidden", "Humble", "Jolly", "Kind", "Lively", "Lucky", "Magic", "Mellow",
    "Mighty", "Nice", "Nimble", "Proud", "Quiet", "Rapid", "Shiny", "Silent", "Sleepy",
    "Smooth", "Snappy", "Sunny", "Swift", "Tiny", "Witty", "Zesty",
];

const NOUNS: &[&str] = &[
    "Apple", "Apricot", "Avocado", "Banana", "Blueberry", "Cherry", "Coconut", "Cranberry",
    "Date", "Fig", "Grape", "Guava", "Kiwi", "Lemon", "Lime", "Lychee", "Mango", "Melon",
    "Nectarine", "Olive", "Orange", "Papaya", "Peach", "Pear", "Pineapple", "Plum",
    "Pomelo", "Quince", "Raspberry", "Strawberry", "Tangerine", "Tomato", "Walnut",
];

/// "Adjective Noun", LocalSend style ("Nice Orange").
pub fn random_alias() -> String {
    let a = ADJECTIVES[rand::random_range(0..ADJECTIVES.len())];
    let n = NOUNS[rand::random_range(0..NOUNS.len())];
    format!("{a} {n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toss-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fingerprint_of_empty_input_is_known_sha256_uppercase() {
        assert_eq!(
            fingerprint_from_der(b""),
            "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
        );
    }

    #[test]
    fn generated_fingerprint_is_64_uppercase_hex_chars() {
        let id = Identity::generate().unwrap();
        assert_eq!(id.fingerprint.len(), 64);
        assert!(id
            .fingerprint
            .chars()
            .all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c)));
    }

    #[test]
    fn fingerprint_matches_recomputation_from_pem() {
        let id = Identity::generate().unwrap();
        assert_eq!(fingerprint_from_pem(&id.certificate_pem).unwrap(), id.fingerprint);
        id.verify().unwrap();
    }

    #[test]
    fn identity_is_stable_across_restarts() {
        let dir = temp_dir();
        let first = Identity::load_or_create(&dir).unwrap();
        let second = Identity::load_or_create(&dir).unwrap();
        assert_eq!(first, second);
        assert!(Identity::file_path(&dir).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn two_generated_identities_differ() {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn corrupt_stored_identity_is_regenerated() {
        let dir = temp_dir();
        let mut broken = Identity::generate().unwrap();
        broken.fingerprint = "0".repeat(64);
        broken.save(&dir).unwrap();
        let loaded = Identity::load_or_create(&dir).unwrap();
        assert_ne!(loaded.fingerprint, broken.fingerprint);
        loaded.verify().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_json_is_an_error_not_a_panic() {
        let dir = temp_dir();
        std::fs::write(Identity::file_path(&dir), b"not json").unwrap();
        assert!(matches!(Identity::load(&dir), Err(IdentityError::Json(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn alias_is_two_capitalised_words() {
        let alias = random_alias();
        let words: Vec<&str> = alias.split(' ').collect();
        assert_eq!(words.len(), 2);
        assert!(words.iter().all(|w| w.chars().next().unwrap().is_ascii_uppercase()));
    }

    #[test]
    fn info_has_protocol_shape() {
        let id = Identity::generate().unwrap();
        let json = serde_json::to_value(id.info()).unwrap();
        assert_eq!(json["deviceType"], "desktop");
        assert_eq!(json["port"], 53317);
        assert!(json["deviceModel"].is_string());
        assert_eq!(json["fingerprint"], id.fingerprint);
        assert!(json.get("privateKeyPem").is_none());
    }

    #[test]
    fn device_info_matches_the_protocol_object() {
        let id = Identity::generate().unwrap();
        let json = serde_json::to_value(id.to_device_info()).unwrap();
        assert_eq!(json["version"], "2.2");
        assert_eq!(json["protocol"], "https");
        assert_eq!(json["deviceType"], "desktop");
        assert_eq!(json["port"], 53317);
        assert_eq!(json["download"], false);
        assert_eq!(json["fingerprint"], id.fingerprint);
    }
}
