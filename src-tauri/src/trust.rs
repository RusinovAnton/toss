//! Paired devices.
//!
//! Trusting a device means two things: its requests are accepted without
//! asking, and clipboard text may flow between it and us. Both are dangerous
//! if "which device" can be forged, so trust is keyed on the SHA-256
//! fingerprint of the peer's TLS certificate, taken from the handshake rather
//! than from anything the peer claims in a request body.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "trusted.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    /// SHA-256 of the device's certificate, uppercase hex. The identity.
    pub fingerprint: String,
    /// Remembered so a paired device that is switched off still has a name.
    pub alias: String,
    /// Unix milliseconds.
    pub trusted_at: u64,
}

/// The paired devices, keyed by fingerprint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrustStore {
    devices: HashMap<String, TrustedDevice>,
}

impl TrustStore {
    pub fn file_path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// An unreadable store is treated as empty: failing open would mean
    /// trusting nobody, which is the safe direction.
    pub fn load(dir: &Path) -> TrustStore {
        match std::fs::read(TrustStore::file_path(dir)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!("trusted devices unreadable ({e}); starting empty");
                TrustStore::default()
            }),
            Err(_) => TrustStore::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, TrustStore::file_path(dir))
    }

    /// Fingerprints are compared case-insensitively: the protocol says
    /// uppercase hex, but a peer that sends lowercase is still the same
    /// device.
    fn key(fingerprint: &str) -> String {
        fingerprint.trim().to_ascii_uppercase()
    }

    pub fn is_trusted(&self, fingerprint: &str) -> bool {
        !fingerprint.is_empty() && self.devices.contains_key(&TrustStore::key(fingerprint))
    }

    pub fn get(&self, fingerprint: &str) -> Option<&TrustedDevice> {
        self.devices.get(&TrustStore::key(fingerprint))
    }

    /// Adds or renames a paired device. Returns the stored entry.
    pub fn trust(&mut self, fingerprint: &str, alias: &str) -> Option<TrustedDevice> {
        let key = TrustStore::key(fingerprint);
        if key.is_empty() {
            return None;
        }
        let device = TrustedDevice {
            fingerprint: key.clone(),
            alias: alias.to_string(),
            trusted_at: self
                .devices
                .get(&key)
                .map(|existing| existing.trusted_at)
                .unwrap_or_else(now_ms),
        };
        self.devices.insert(key, device.clone());
        Some(device)
    }

    pub fn untrust(&mut self, fingerprint: &str) -> bool {
        self.devices.remove(&TrustStore::key(fingerprint)).is_some()
    }

    /// Paired devices, oldest pairing first.
    pub fn list(&self) -> Vec<TrustedDevice> {
        let mut list: Vec<TrustedDevice> = self.devices.values().cloned().collect();
        list.sort_by_key(|device| device.trusted_at);
        list
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toss-trust-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn nothing_is_trusted_to_begin_with() {
        let store = TrustStore::default();
        assert!(store.is_empty());
        assert!(!store.is_trusted("ABC"));
    }

    #[test]
    fn a_paired_device_is_remembered() {
        let mut store = TrustStore::default();
        store.trust("ABC", "Great Strawberry");
        assert!(store.is_trusted("ABC"));
        assert_eq!(store.get("ABC").unwrap().alias, "Great Strawberry");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn fingerprints_match_whatever_their_case() {
        let mut store = TrustStore::default();
        store.trust("abc123", "Peer");
        assert!(store.is_trusted("ABC123"));
        assert!(store.is_trusted("AbC123"));
        assert_eq!(store.get("abc123").unwrap().fingerprint, "ABC123");
    }

    #[test]
    fn an_empty_fingerprint_is_never_trusted() {
        let mut store = TrustStore::default();
        assert!(store.trust("", "Nobody").is_none());
        assert!(!store.is_trusted(""));
        assert!(store.is_empty());
    }

    #[test]
    fn re_pairing_keeps_the_original_date_and_updates_the_name() {
        let mut store = TrustStore::default();
        let first = store.trust("ABC", "Old Name").unwrap();
        let second = store.trust("ABC", "New Name").unwrap();
        assert_eq!(second.trusted_at, first.trusted_at);
        assert_eq!(second.alias, "New Name");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn unpairing_removes_it() {
        let mut store = TrustStore::default();
        store.trust("ABC", "Peer");
        assert!(store.untrust("abc"));
        assert!(!store.is_trusted("ABC"));
        assert!(!store.untrust("ABC"));
    }

    #[test]
    fn the_store_survives_a_round_trip() {
        let dir = temp_dir();
        let mut store = TrustStore::default();
        store.trust("ABC", "One");
        store.trust("DEF", "Two");
        store.save(&dir).unwrap();
        let loaded = TrustStore::load(&dir);
        assert_eq!(loaded.len(), 2);
        assert!(loaded.is_trusted("def"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_corrupt_store_trusts_nobody() {
        let dir = temp_dir();
        std::fs::write(TrustStore::file_path(&dir), b"{ not json").unwrap();
        assert!(TrustStore::load(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_list_is_ordered_by_when_they_were_paired() {
        let mut store = TrustStore::default();
        store.trust("A", "First");
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.trust("B", "Second");
        let list = store.list();
        assert_eq!(list[0].alias, "First");
        assert_eq!(list[1].alias, "Second");
    }
}
