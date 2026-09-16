//! LocalSend protocol v2 wire types.
//!
//! Field names and semantics are copied from the protocol repo
//! (<https://github.com/localsend/protocol>) and cross-checked against the
//! official app's Rust core. Nothing here may be invented.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::Ipv4Addr;

/// Protocol version we announce. 2.2 is what app 1.18+ speaks.
pub const PROTOCOL_VERSION: &str = "2.2";
/// Default HTTP(S) and multicast port.
pub const DEFAULT_PORT: u16 = 53317;
/// Multicast group. Inside 224.0.0.0/24 because some Android devices only
/// accept that range.
pub const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Mobile,
    Desktop,
    Web,
    Headless,
    Server,
}

impl Default for DeviceType {
    fn default() -> Self {
        DeviceType::Desktop
    }
}

/// Unknown `deviceType` values must not break parsing; protocol section 7.1
/// says implementations fall back to `desktop`.
pub mod device_type_opt {
    use super::DeviceType;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<DeviceType>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_str(match v {
                DeviceType::Mobile => "mobile",
                DeviceType::Desktop => "desktop",
                DeviceType::Web => "web",
                DeviceType::Headless => "headless",
                DeviceType::Server => "server",
            }),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<DeviceType>, D::Error> {
        let value = Option::<String>::deserialize(deserializer)?;
        Ok(value.map(|v| match v.to_lowercase().as_str() {
            "mobile" => DeviceType::Mobile,
            "desktop" => DeviceType::Desktop,
            "web" => DeviceType::Web,
            "headless" => DeviceType::Headless,
            "server" => DeviceType::Server,
            _ => DeviceType::Desktop,
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolType {
    Http,
    Https,
}

impl ProtocolType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProtocolType::Http => "http",
            ProtocolType::Https => "https",
        }
    }
}

/// The device object used by the announce message and by `/register`
/// requests. Identical field set in both directions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(
        default,
        with = "device_type_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub device_type: Option<DeviceType>,
    /// SHA-256 of the certificate in HTTPS mode; ignored by peers there, but
    /// still sent because the protocol requires the field.
    pub fingerprint: String,
    /// Senders always send these two, but defaults keep a sloppy client from
    /// failing the whole request.
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_protocol")]
    pub protocol: ProtocolType,
    #[serde(default)]
    pub download: bool,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_protocol() -> ProtocolType {
    ProtocolType::Https
}

/// An announce datagram: a [`DeviceInfo`] plus the `announce` flag.
///
/// `announce: true` asks everyone to answer. Answers sent over UDP carry
/// `announce: false`, which must not trigger another answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnounceMessage {
    #[serde(flatten)]
    pub device: DeviceInfo,
    #[serde(default)]
    pub announce: bool,
}

/// The body of a `/register` response. The protocol drops `port` and
/// `protocol` here, so both are optional and the caller falls back to the
/// address it probed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResponse {
    pub alias: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub device_model: Option<String>,
    #[serde(default, with = "device_type_opt")]
    pub device_type: Option<DeviceType>,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub protocol: Option<ProtocolType>,
    #[serde(default)]
    pub download: bool,
}

/// One file inside a `prepare-upload` request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDto {
    pub id: String,
    /// May carry a relative path for folder sends, e.g. `photos/cat.png`.
    pub file_name: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    /// Lowercase hex. When present the receiver must verify it and answer
    /// `422` on a mismatch (protocol 2.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FileMetadata>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUploadRequest {
    pub info: DeviceInfo,
    /// Keyed by file id. The key repeats the `id` inside each entry.
    pub files: HashMap<String, FileDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUploadResponse {
    pub session_id: String,
    /// file id -> token, one token per accepted file.
    pub files: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DeviceInfo {
        DeviceInfo {
            alias: "Nice Orange".into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("macOS".into()),
            device_type: Some(DeviceType::Desktop),
            fingerprint: "ABC".into(),
            port: DEFAULT_PORT,
            protocol: ProtocolType::Https,
            download: false,
        }
    }

    #[test]
    fn announce_uses_protocol_field_names() {
        let json = serde_json::to_value(AnnounceMessage {
            device: sample(),
            announce: true,
        })
        .unwrap();
        assert_eq!(json["alias"], "Nice Orange");
        assert_eq!(json["version"], "2.2");
        assert_eq!(json["deviceModel"], "macOS");
        assert_eq!(json["deviceType"], "desktop");
        assert_eq!(json["fingerprint"], "ABC");
        assert_eq!(json["port"], 53317);
        assert_eq!(json["protocol"], "https");
        assert_eq!(json["download"], false);
        assert_eq!(json["announce"], true);
    }

    #[test]
    fn parses_announce_from_official_app() {
        let msg: AnnounceMessage = serde_json::from_str(
            r#"{"alias":"Secret Banana","version":"2.0","deviceModel":"Windows",
                "deviceType":"desktop","fingerprint":"xyz","port":53317,
                "protocol":"https","download":true,"announce":true}"#,
        )
        .unwrap();
        assert!(msg.announce);
        assert_eq!(msg.device.alias, "Secret Banana");
        assert_eq!(msg.device.device_type, Some(DeviceType::Desktop));
        assert!(msg.device.download);
    }

    #[test]
    fn parses_announce_without_optional_fields() {
        let msg: AnnounceMessage = serde_json::from_str(
            r#"{"alias":"A","version":"2.0","fingerprint":"f","port":53317,"protocol":"http"}"#,
        )
        .unwrap();
        assert!(!msg.announce);
        assert_eq!(msg.device.device_model, None);
        assert_eq!(msg.device.device_type, None);
        assert!(!msg.device.download);
        assert_eq!(msg.device.protocol, ProtocolType::Http);
    }

    #[test]
    fn unknown_device_type_falls_back_to_desktop() {
        let msg: AnnounceMessage = serde_json::from_str(
            r#"{"alias":"A","version":"2.0","deviceType":"toaster","fingerprint":"f",
                "port":53317,"protocol":"https"}"#,
        )
        .unwrap();
        assert_eq!(msg.device.device_type, Some(DeviceType::Desktop));
    }

    #[test]
    fn parses_prepare_upload_from_the_protocol_doc() {
        let request: PrepareUploadRequest = serde_json::from_str(
            r#"{"info":{"alias":"Nice Orange","version":"2.0","deviceModel":"Samsung",
                 "deviceType":"mobile","fingerprint":"f","port":53317,"protocol":"https",
                 "download":true},
                "files":{"some file id":{"id":"some file id","fileName":"my image.png",
                 "size":324242,"fileType":"image/jpeg","sha256":"hash","preview":"data",
                 "metadata":{"modified":"2021-01-01T12:34:56Z"}}}}"#,
        )
        .unwrap();
        assert_eq!(request.info.alias, "Nice Orange");
        let file = &request.files["some file id"];
        assert_eq!(file.file_name, "my image.png");
        assert_eq!(file.size, 324242);
        assert_eq!(file.sha256.as_deref(), Some("hash"));
        assert_eq!(
            request.files["some file id"].metadata.as_ref().unwrap().modified.as_deref(),
            Some("2021-01-01T12:34:56Z")
        );
    }

    #[test]
    fn prepare_upload_response_uses_session_id_key() {
        let mut files = HashMap::new();
        files.insert("fileId".to_string(), "token".to_string());
        let json = serde_json::to_value(PrepareUploadResponse {
            session_id: "s1".into(),
            files,
        })
        .unwrap();
        assert_eq!(json["sessionId"], "s1");
        assert_eq!(json["files"]["fileId"], "token");
    }

    #[test]
    fn register_response_without_port_or_protocol_parses() {
        let res: RegisterResponse = serde_json::from_str(
            r#"{"alias":"Nice Orange","version":"2.0","deviceModel":"Samsung",
                "deviceType":"mobile","fingerprint":"f","download":false}"#,
        )
        .unwrap();
        assert_eq!(res.port, None);
        assert_eq!(res.protocol, None);
        assert_eq!(res.device_type, Some(DeviceType::Mobile));
    }
}
