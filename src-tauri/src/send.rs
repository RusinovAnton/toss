//! The send side: offer files to a peer, then stream them.
//!
//! Mirror image of `server.rs`. We call the peer's `prepare-upload`, wait for
//! its user to accept, then upload each accepted file with the token we were
//! handed. Folders are walked and sent with relative paths in `fileName`,
//! which is how the protocol preserves structure.

use crate::protocol::{
    read_info, FileDto, PrepareUploadRequest, PrepareUploadResponse, ProtocolType, SharedInfo,
};
use crate::server::{Emitter, SESSION_FINISHED_EVENT, TRANSFER_PROGRESS_EVENT};
use futures_util::stream;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

/// Read size per chunk. Large enough to keep the socket busy, small enough
/// that cancelling takes effect quickly.
const CHUNK_SIZE: usize = 64 * 1024;
/// Same throttle as the receive side.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// How long we wait for the TCP connection. The request itself has no
/// deadline: `prepare-upload` blocks until the peer's user answers.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Guard against a folder tree with a symlink loop or an absurd file count.
const MAX_FILES: usize = 10_000;

/// A peer to send to.
#[derive(Clone, Debug)]
pub struct Target {
    /// The peer's fingerprint, echoed in events so the UI can find its circle.
    pub fingerprint: String,
    pub ip: String,
    pub port: u16,
    pub protocol: ProtocolType,
}

impl Target {
    fn url(&self, path: &str) -> String {
        format!("{}://{}:{}{}", self.protocol.as_str(), self.ip, self.port, path)
    }
}

/// One file to send: what the peer is told, plus where it is on disk.
#[derive(Clone, Debug)]
pub struct OutgoingFile {
    pub id: String,
    /// Relative path for folder sends, plain name otherwise. Always `/`.
    pub file_name: String,
    pub size: u64,
    pub file_type: String,
    pub path: PathBuf,
}

impl OutgoingFile {
    fn to_dto(&self) -> FileDto {
        FileDto {
            id: self.id.clone(),
            file_name: self.file_name.clone(),
            size: self.size,
            file_type: Some(self.file_type.clone()),
            // Hashing means reading every file twice. The field is nullable
            // and the receiver only checks it when present, so it is left out.
            sha256: None,
            preview: None,
            metadata: None,
        }
    }
}

#[derive(Debug)]
pub enum SendError {
    NoFiles,
    /// The peer's user said no, or accepted nothing.
    Declined,
    /// The peer is busy with another session.
    Busy,
    /// The peer wants a PIN, or the one we sent was wrong.
    PinRequired,
    TooManyRequests,
    Cancelled,
    /// Could not reach the peer, or it hung up mid-transfer.
    Connection(String),
    Io(String),
    Protocol(String),
}

impl SendError {
    /// A stable identifier the UI can turn into a one-line tooltip.
    pub fn code(&self) -> &'static str {
        match self {
            SendError::NoFiles => "no-files",
            SendError::Declined => "declined",
            SendError::Busy => "busy",
            SendError::PinRequired => "pin-required",
            SendError::TooManyRequests => "too-many-requests",
            SendError::Cancelled => "cancelled",
            SendError::Connection(_) => "connection-lost",
            SendError::Io(_) => "io-error",
            SendError::Protocol(_) => "protocol-error",
        }
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::NoFiles => f.write_str("nothing to send"),
            SendError::Declined => f.write_str("Declined"),
            SendError::Busy => f.write_str("Busy"),
            SendError::PinRequired => f.write_str("PIN required"),
            SendError::TooManyRequests => f.write_str("Too many requests"),
            SendError::Cancelled => f.write_str("Cancelled"),
            SendError::Connection(e) => write!(f, "Connection lost: {e}"),
            SendError::Io(e) => write!(f, "Cannot read file: {e}"),
            SendError::Protocol(e) => write!(f, "Unexpected answer: {e}"),
        }
    }
}

/// What the frontend receives when a send fails.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendErrorPayload {
    pub code: String,
    pub message: String,
}

impl From<SendError> for SendErrorPayload {
    fn from(error: SendError) -> Self {
        SendErrorPayload {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendSummary {
    pub session_id: String,
    pub files_sent: usize,
    pub bytes_sent: u64,
}

struct ActiveSend {
    target: Target,
    cancelled: Arc<AtomicBool>,
}

pub struct SendManager {
    client: reqwest::Client,
    info: SharedInfo,
    emit: Emitter,
    active: Mutex<HashMap<String, ActiveSend>>,
}

impl SendManager {
    pub fn new(
        info: SharedInfo,
        cert_pem: &str,
        key_pem: &str,
        emit: Emitter,
    ) -> Result<Self, String> {
        let mut bundle = Vec::with_capacity(cert_pem.len() + key_pem.len());
        bundle.extend_from_slice(cert_pem.as_bytes());
        bundle.extend_from_slice(key_pem.as_bytes());
        let identity = reqwest::Identity::from_pem(&bundle).map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
            .identity(identity)
            // Peers are self-signed and identified by fingerprint, as on the
            // discovery client.
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(SendManager {
            client,
            info,
            emit,
            active: Mutex::new(HashMap::new()),
        })
    }

    /// Offers `paths` to `target` and uploads whatever is accepted.
    pub async fn send(
        &self,
        target: Target,
        paths: &[PathBuf],
        pin: Option<String>,
    ) -> Result<SendSummary, SendError> {
        self.send_with_flag(target, paths, pin, Arc::new(AtomicBool::new(false)))
            .await
    }

    /// [`SendManager::send`] with a cancellation flag the caller owns, for
    /// callers that need to stop the transfer without going through
    /// [`SendManager::cancel`] and its session id.
    pub async fn send_with_flag(
        &self,
        target: Target,
        paths: &[PathBuf],
        pin: Option<String>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SendSummary, SendError> {
        let files = collect_files(paths)?;
        if files.is_empty() {
            return Err(SendError::NoFiles);
        }

        let prepared = self.prepare(&target, &files, pin.as_deref()).await?;
        self.active.lock().expect("sends poisoned").insert(
            prepared.session_id.clone(),
            ActiveSend {
                target: target.clone(),
                cancelled: Arc::clone(&cancelled),
            },
        );

        let outcome = self
            .upload_all(&target, &prepared, &files, &cancelled)
            .await;
        self.active
            .lock()
            .expect("sends poisoned")
            .remove(&prepared.session_id);

        match outcome {
            Ok(bytes_sent) => {
                (self.emit)(
                    SESSION_FINISHED_EVENT,
                    json!({
                        "sessionId": prepared.session_id,
                        "status": "completed",
                        "direction": "send",
                        "peer": target.fingerprint,
                    }),
                );
                Ok(SendSummary {
                    session_id: prepared.session_id,
                    files_sent: prepared.files.len(),
                    bytes_sent,
                })
            }
            Err(error) => {
                (self.emit)(
                    SESSION_FINISHED_EVENT,
                    json!({
                        "sessionId": prepared.session_id,
                        "status": if matches!(error, SendError::Cancelled) { "cancelled" } else { "error" },
                        "direction": "send",
                        "peer": target.fingerprint,
                        "reason": error.code(),
                    }),
                );
                Err(error)
            }
        }
    }

    /// Sends the metadata and waits for the peer's user to answer.
    async fn prepare(
        &self,
        target: &Target,
        files: &[OutgoingFile],
        pin: Option<&str>,
    ) -> Result<PrepareUploadResponse, SendError> {
        let mut url = target.url("/api/localsend/v2/prepare-upload");
        if let Some(pin) = pin {
            url.push_str(&format!("?pin={pin}"));
        }
        let request = PrepareUploadRequest {
            info: read_info(&self.info),
            files: files
                .iter()
                .map(|file| (file.id.clone(), file.to_dto()))
                .collect(),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| SendError::Connection(e.to_string()))?;

        match response.status().as_u16() {
            200 => response
                .json::<PrepareUploadResponse>()
                .await
                .map_err(|e| SendError::Protocol(e.to_string())),
            // Nothing for us to upload; the peer considers it finished.
            204 => Err(SendError::NoFiles),
            401 => Err(SendError::PinRequired),
            403 => Err(SendError::Declined),
            409 => Err(SendError::Busy),
            429 => Err(SendError::TooManyRequests),
            other => Err(SendError::Protocol(format!("HTTP {other}"))),
        }
    }

    /// Uploads the accepted files one after another.
    ///
    /// Sequential on purpose: the protocol allows parallel uploads, but one
    /// file at a time gives honest progress and keeps the receiver's disk
    /// doing one thing.
    async fn upload_all(
        &self,
        target: &Target,
        prepared: &PrepareUploadResponse,
        files: &[OutgoingFile],
        cancelled: &Arc<AtomicBool>,
    ) -> Result<u64, SendError> {
        let by_id: HashMap<&str, &OutgoingFile> =
            files.iter().map(|f| (f.id.as_str(), f)).collect();
        // What the arc on the target's circle is scaled to.
        let session_total: u64 = prepared
            .files
            .keys()
            .filter_map(|id| by_id.get(id.as_str()).map(|file| file.size))
            .sum();
        let mut bytes_sent = 0u64;

        for (file_id, token) in &prepared.files {
            let Some(file) = by_id.get(file_id.as_str()) else {
                // The peer accepted a file we never offered.
                return Err(SendError::Protocol(format!("unknown file id {file_id}")));
            };
            if cancelled.load(Ordering::SeqCst) {
                return Err(SendError::Cancelled);
            }
            self.upload_one(
                target,
                &prepared.session_id,
                file,
                token,
                cancelled,
                bytes_sent,
                session_total,
            )
            .await?;
            bytes_sent += file.size;
        }
        Ok(bytes_sent)
    }

    async fn upload_one(
        &self,
        target: &Target,
        session_id: &str,
        file: &OutgoingFile,
        token: &str,
        cancelled: &Arc<AtomicBool>,
        already_sent: u64,
        session_total: u64,
    ) -> Result<(), SendError> {
        let handle = tokio::fs::File::open(&file.path)
            .await
            .map_err(|e| SendError::Io(e.to_string()))?;

        let emit = Arc::clone(&self.emit);
        let peer = target.fingerprint.clone();
        let session = session_id.to_string();
        let file_id = file.id.clone();
        let file_name = file.file_name.clone();
        let total = file.size;
        let flag = Arc::clone(cancelled);

        let body = reqwest::Body::wrap_stream(stream::unfold(
            (handle, 0u64, Instant::now() - PROGRESS_INTERVAL, None::<u64>),
            move |(mut handle, sent, last_emit, last_sent)| {
                let emit = Arc::clone(&emit);
                let peer = peer.clone();
                let session = session.clone();
                let file_id = file_id.clone();
                let file_name = file_name.clone();
                let flag = Arc::clone(&flag);
                async move {
                    if flag.load(Ordering::SeqCst) {
                        return Some((
                            Err(std::io::Error::other("cancelled")),
                            (handle, sent, last_emit, last_sent),
                        ));
                    }
                    let mut buffer = vec![0u8; CHUNK_SIZE];
                    match handle.read(&mut buffer).await {
                        Ok(0) => None,
                        Ok(read) => {
                            buffer.truncate(read);
                            let sent = sent + read as u64;
                            let (last_emit, last_sent) =
                                if last_emit.elapsed() >= PROGRESS_INTERVAL
                                    && last_sent != Some(sent)
                                {
                                    emit(
                                        TRANSFER_PROGRESS_EVENT,
                                        json!({
                                            "sessionId": session,
                                            "fileId": file_id,
                                            "fileName": file_name,
                                            "bytesReceived": sent,
                                            "totalBytes": total,
                                            "sessionDone": already_sent + sent,
                                            "sessionTotal": session_total,
                                            "direction": "send",
                                            "peer": peer,
                                        }),
                                    );
                                    (Instant::now(), Some(sent))
                                } else {
                                    (last_emit, last_sent)
                                };
                            Some((Ok(buffer), (handle, sent, last_emit, last_sent)))
                        }
                        Err(e) => Some((Err(e), (handle, sent, last_emit, last_sent))),
                    }
                }
            },
        ));

        let url = target.url(&format!(
            "/api/localsend/v2/upload?sessionId={}&fileId={}&token={}",
            urlencode(session_id),
            urlencode(&file.id),
            urlencode(token)
        ));
        let response = self
            .client
            .post(&url)
            .header(reqwest::header::CONTENT_LENGTH, file.size)
            .body(body)
            .send()
            .await
            .map_err(|e| {
                if cancelled.load(Ordering::SeqCst) {
                    SendError::Cancelled
                } else {
                    SendError::Connection(e.to_string())
                }
            })?;

        match response.status().as_u16() {
            200 | 204 => {
                // One last event so the UI lands on 100%.
                (self.emit)(
                    TRANSFER_PROGRESS_EVENT,
                    json!({
                        "sessionId": session_id,
                        "fileId": file.id,
                        "fileName": file.file_name,
                        "bytesReceived": file.size,
                        "totalBytes": file.size,
                        "sessionDone": already_sent + file.size,
                        "sessionTotal": session_total,
                        "direction": "send",
                        "peer": target.fingerprint,
                    }),
                );
                Ok(())
            }
            403 => Err(SendError::Declined),
            409 => Err(SendError::Cancelled),
            422 => Err(SendError::Protocol("checksum mismatch".into())),
            other => Err(SendError::Protocol(format!("HTTP {other}"))),
        }
    }

    /// Stops an in-flight send and tells the peer to drop the session.
    pub async fn cancel(&self, session_id: &str) -> Result<(), SendError> {
        let target = {
            let active = self.active.lock().expect("sends poisoned");
            let Some(send) = active.get(session_id) else {
                return Ok(());
            };
            send.cancelled.store(true, Ordering::SeqCst);
            send.target.clone()
        };
        let url = target.url(&format!(
            "/api/localsend/v2/cancel?sessionId={}",
            urlencode(session_id)
        ));
        // A peer that cannot be told is still cancelled on our side.
        let _ = self.client.post(&url).send().await;
        Ok(())
    }
}

/// Minimal percent-encoding for the query values we build. Session ids,
/// file ids and tokens are uuids in practice, but they arrive from a peer.
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Expands `paths` into the flat file list the protocol wants.
///
/// A folder contributes every file under it, named relative to the folder
/// itself (`photos/2024/cat.png`), which is what preserves structure on the
/// receiving side.
pub fn collect_files(paths: &[PathBuf]) -> Result<Vec<OutgoingFile>, SendError> {
    let mut collected: Vec<(String, PathBuf, u64)> = Vec::new();
    for path in paths {
        let meta = std::fs::symlink_metadata(path).map_err(|e| SendError::Io(e.to_string()))?;
        if meta.is_dir() {
            let root_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "folder".to_string());
            walk(path, &root_name, &mut collected)?;
        } else {
            let meta = std::fs::metadata(path).map_err(|e| SendError::Io(e.to_string()))?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .ok_or_else(|| SendError::Io("path has no file name".into()))?;
            collected.push((name, path.clone(), meta.len()));
        }
    }

    collected.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(collected
        .into_iter()
        .enumerate()
        .map(|(index, (file_name, path, size))| OutgoingFile {
            // Stable within a session and unique, which is all the protocol
            // asks of a file id.
            id: format!("{index}-{}", uuid::Uuid::new_v4()),
            file_type: guess_mime(&file_name).to_string(),
            file_name,
            size,
            path,
        })
        .collect())
}

fn walk(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, PathBuf, u64)>,
) -> Result<(), SendError> {
    let entries = std::fs::read_dir(dir).map_err(|e| SendError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| SendError::Io(e.to_string()))?;
        let path = entry.path();
        let meta = entry
            .metadata()
            .map_err(|e| SendError::Io(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = format!("{prefix}/{name}");

        if meta.is_symlink() {
            // Following links can leave the tree or loop; skip them rather
            // than send something the user did not point at.
            continue;
        }
        if meta.is_dir() {
            walk(&path, &relative, out)?;
        } else {
            out.push((relative, path, meta.len()));
        }
        if out.len() > MAX_FILES {
            return Err(SendError::Io(format!("more than {MAX_FILES} files")));
        }
    }
    Ok(())
}

/// Enough of a MIME table for the types people actually send. The field is
/// only used for the receiver's icon, so the fallback is harmless.
pub fn guess_mime(file_name: &str) -> &'static str {
    let extension = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "dmg" => "application/x-apple-diskimage",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toss-send-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_single_file_keeps_its_plain_name() {
        let dir = temp_dir();
        let path = dir.join("cat.png");
        std::fs::write(&path, b"png").unwrap();
        let files = collect_files(&[path]).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "cat.png");
        assert_eq!(files[0].size, 3);
        assert_eq!(files[0].file_type, "image/png");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_is_sent_with_relative_paths() {
        let dir = temp_dir();
        let root = dir.join("holiday");
        std::fs::create_dir_all(root.join("2024/june")).unwrap();
        std::fs::write(root.join("readme.txt"), b"hi").unwrap();
        std::fs::write(root.join("2024/one.txt"), b"one").unwrap();
        std::fs::write(root.join("2024/june/two.txt"), b"two").unwrap();

        let files = collect_files(&[root]).unwrap();
        let mut names: Vec<String> = files.iter().map(|f| f.file_name.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "holiday/2024/june/two.txt",
                "holiday/2024/one.txt",
                "holiday/readme.txt"
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_ids_are_unique() {
        let dir = temp_dir();
        std::fs::write(dir.join("a.txt"), b"a").unwrap();
        std::fs::write(dir.join("b.txt"), b"b").unwrap();
        let files = collect_files(&[dir.join("a.txt"), dir.join("b.txt")]).unwrap();
        assert_ne!(files[0].id, files[1].id);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_folder_sends_nothing() {
        let dir = temp_dir();
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(collect_files(&[empty]).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn symlinks_inside_a_folder_are_skipped() {
        let dir = temp_dir();
        let root = dir.join("tree");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("real.txt"), b"real").unwrap();
        std::os::unix::fs::symlink(dir.join("nowhere"), root.join("link.txt")).unwrap();
        let files = collect_files(&[root]).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "tree/real.txt");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_path_is_an_error_not_a_panic() {
        let error = collect_files(&[PathBuf::from("/definitely/not/here")]).unwrap_err();
        assert_eq!(error.code(), "io-error");
    }

    #[test]
    fn mime_types_cover_the_common_cases() {
        assert_eq!(guess_mime("a.PNG"), "image/png");
        assert_eq!(guess_mime("a.tar.gz"), "application/gzip");
        assert_eq!(guess_mime("noextension"), "application/octet-stream");
        assert_eq!(guess_mime("notes.md"), "text/markdown");
    }

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(urlencode("abc-123"), "abc-123");
        assert_eq!(urlencode("a b&c=d"), "a%20b%26c%3Dd");
    }

    #[test]
    fn error_codes_are_stable_for_the_ui() {
        assert_eq!(SendError::Declined.code(), "declined");
        assert_eq!(SendError::Busy.code(), "busy");
        assert_eq!(SendError::PinRequired.code(), "pin-required");
        assert_eq!(
            SendError::Connection("x".into()).code(),
            "connection-lost"
        );
    }

    #[test]
    fn the_dto_carries_no_checksum() {
        let file = OutgoingFile {
            id: "a".into(),
            file_name: "cat.png".into(),
            size: 3,
            file_type: "image/png".into(),
            path: PathBuf::from("/tmp/cat.png"),
        };
        let json = serde_json::to_value(file.to_dto()).unwrap();
        assert_eq!(json["fileName"], "cat.png");
        assert_eq!(json["fileType"], "image/png");
        assert!(json.get("sha256").is_none());
    }
}
