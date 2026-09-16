//! The receive side: an HTTPS server speaking LocalSend protocol v2.
//!
//! Routes (protocol sections 3.2, 4 and 6.1):
//! - `POST /api/localsend/v2/register`
//! - `POST /api/localsend/v2/prepare-upload`
//! - `POST /api/localsend/v2/upload`
//! - `POST /api/localsend/v2/cancel`
//! - `GET  /api/localsend/v2/info`
//!
//! Nothing is written to disk before the user accepts. The accept step is a
//! round trip through the frontend: the handler parks on a channel that
//! `respond_to_request` feeds.

use crate::files::{candidate_name, is_inside, safe_relative_path};
use crate::protocol::{
    read_info, DeviceInfo, FileDto, PrepareUploadRequest, PrepareUploadResponse, SharedInfo,
    DEFAULT_PORT,
};
use crate::session::{
    build_active_session, Decision, IncomingFile, Sender as PeerSender, SessionError,
    SessionManager,
};
use crate::settings::Settings;
use axum::body::Body;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

/// How long a request waits for the user before it is declined.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);
/// Progress is noisy; one event per file per this interval is plenty.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Guards against a sender that offers the same name forever.
const MAX_NAME_ATTEMPTS: u32 = 1000;

pub const INCOMING_REQUEST_EVENT: &str = "incoming-request";
pub const TRANSFER_PROGRESS_EVENT: &str = "transfer-progress";
pub const SESSION_FINISHED_EVENT: &str = "session-finished";

/// Emits an event to the frontend.
pub type Emitter = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;
/// Hands a peer that contacted us to discovery, so registering works both ways.
pub type PeerSink = Arc<dyn Fn(DeviceInfo, IpAddr) + Send + Sync>;

pub struct ServerState {
    /// This device, as sent in answers.
    pub info: SharedInfo,
    pub sessions: Arc<SessionManager>,
    pub settings: Arc<Mutex<Settings>>,
    pub download_dir: PathBuf,
    pub emit: Emitter,
    pub register_peer: PeerSink,
}

impl ServerState {
    fn required_pin(&self) -> Option<String> {
        self.settings
            .lock()
            .expect("settings poisoned")
            .required_pin()
            .map(str::to_string)
    }

    fn quick_save(&self) -> bool {
        self.settings.lock().expect("settings poisoned").quick_save
    }
}

pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/api/localsend/v2/info", get(info))
        .route("/api/localsend/v2/register", post(register))
        .route("/api/localsend/v2/prepare-upload", post(prepare_upload))
        .route("/api/localsend/v2/upload", post(upload))
        .route("/api/localsend/v2/cancel", post(cancel))
        .with_state(state)
}

async fn info(State(state): State<Arc<ServerState>>) -> Json<DeviceInfo> {
    Json(read_info(&state.info))
}

async fn register(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(sender): Json<DeviceInfo>,
) -> Json<DeviceInfo> {
    (state.register_peer)(sender, peer.ip());
    Json(read_info(&state.info))
}

#[derive(Debug, Deserialize)]
struct PinQuery {
    pin: Option<String>,
}

async fn prepare_upload(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<PinQuery>,
    Json(request): Json<PrepareUploadRequest>,
) -> Response {
    if let Some(expected) = state.required_pin() {
        if query.pin.as_deref() != Some(expected.as_str()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    if request.files.is_empty() {
        // Protocol: 204 means there is nothing to transfer.
        return StatusCode::NO_CONTENT.into_response();
    }

    // Validate every name before anything else: a request carrying one bad
    // path is not a request we want to half-accept.
    let mut offered: Vec<IncomingFile> = Vec::with_capacity(request.files.len());
    for (key, dto) in &request.files {
        let dto = normalise_id(key, dto);
        match safe_relative_path(&dto.file_name) {
            Ok(relative) => offered.push(IncomingFile::new(
                &dto,
                relative,
                uuid::Uuid::new_v4().to_string(),
            )),
            Err(e) => {
                eprintln!("rejecting {:?} from {}: {e}", dto.file_name, peer.ip());
                return (StatusCode::BAD_REQUEST, format!("{e}")).into_response();
            }
        }
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let receiver = match state.sessions.begin(&session_id) {
        Ok(receiver) => receiver,
        Err(_) => return StatusCode::CONFLICT.into_response(),
    };

    let sender_fingerprint = request.info.fingerprint.clone();
    let sender = PeerSender {
        alias: request.info.alias.clone(),
        fingerprint: request.info.fingerprint.clone(),
        device_model: request.info.device_model.clone(),
        ip: peer.ip(),
    };

    let quick_save = state.quick_save();
    eprintln!(
        "prepare-upload from {} ({}): {} file(s), quick save {}",
        request.info.alias,
        peer.ip(),
        offered.len(),
        if quick_save { "on" } else { "off" }
    );
    let decision = if quick_save {
        state.sessions.clear_pending(&session_id);
        Decision::Accept(offered.iter().map(|f| f.id.clone()).collect())
    } else {
        (state.emit)(
            INCOMING_REQUEST_EVENT,
            incoming_request_payload(&session_id, &sender, &offered),
        );
        match tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await {
            Ok(Ok(decision)) => decision,
            // Timed out, or the frontend went away without answering.
            _ => {
                state.sessions.clear_pending(&session_id);
                Decision::Decline
            }
        }
    };

    eprintln!("session {session_id}: decision {decision:?}");
    let accepted = match decision {
        Decision::Accept(ids) if !ids.is_empty() => ids,
        _ => {
            state.sessions.clear_pending(&session_id);
            (state.emit)(
                SESSION_FINISHED_EVENT,
                json!({
                    "sessionId": session_id,
                    "status": "declined",
                    "direction": "receive",
                    "peer": sender_fingerprint,
                }),
            );
            return StatusCode::FORBIDDEN.into_response();
        }
    };

    let session = build_active_session(session_id.clone(), sender, &offered, &accepted);
    let tokens = session
        .files
        .values()
        .map(|file| (file.id.clone(), file.token.clone()))
        .collect();
    state.sessions.activate(session);

    Json(PrepareUploadResponse {
        session_id,
        files: tokens,
    })
    .into_response()
}

/// The map key is authoritative: some senders leave `id` inside the object
/// empty even though the protocol repeats it there.
fn normalise_id(key: &str, dto: &FileDto) -> FileDto {
    let mut dto = dto.clone();
    if dto.id.is_empty() {
        dto.id = key.to_string();
    }
    dto
}

fn incoming_request_payload(
    session_id: &str,
    sender: &PeerSender,
    files: &[IncomingFile],
) -> serde_json::Value {
    json!({
        "sessionId": session_id,
        "sender": {
            "alias": sender.alias,
            "fingerprint": sender.fingerprint,
            "deviceModel": sender.device_model,
            "ip": sender.ip.to_string(),
        },
        "files": files.iter().map(|file| json!({
            "id": file.id,
            "fileName": file.file_name,
            "size": file.size,
            "fileType": file.file_type,
        })).collect::<Vec<_>>(),
        "totalSize": files.iter().map(|f| f.size).sum::<u64>(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadQuery {
    session_id: Option<String>,
    file_id: Option<String>,
    token: Option<String>,
}

async fn upload(
    State(state): State<Arc<ServerState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> Response {
    let (Some(session_id), Some(file_id), Some(token)) =
        (query.session_id, query.file_id, query.token)
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let (file, cancelled) =
        match state
            .sessions
            .authorize_upload(&session_id, &file_id, &token, peer.ip())
        {
            Ok(authorized) => authorized,
            Err(SessionError::Busy) => return StatusCode::CONFLICT.into_response(),
            Err(SessionError::Rejected) => return StatusCode::FORBIDDEN.into_response(),
        };

    match receive_file(&state, &session_id, &file, body, &cancelled).await {
        Ok(saved) => {
            let peer = state.sessions.active_sender(&session_id).map(|s| s.fingerprint);
            if let Some(paths) = state.sessions.complete_file(&session_id, &file.id, saved) {
                (state.emit)(
                    SESSION_FINISHED_EVENT,
                    json!({
                        "sessionId": session_id,
                        "status": "completed",
                        "direction": "receive",
                        "peer": peer,
                        "savedTo": state.download_dir.to_string_lossy(),
                        "files": paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
                    }),
                );
            }
            StatusCode::OK.into_response()
        }
        Err(ReceiveError::Cancelled) => StatusCode::CONFLICT.into_response(),
        Err(ReceiveError::Checksum) => StatusCode::UNPROCESSABLE_ENTITY.into_response(),
        Err(ReceiveError::Io(e)) => {
            eprintln!("writing {:?} failed: {e}", file.file_name);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

enum ReceiveError {
    Cancelled,
    Checksum,
    Io(std::io::Error),
}

impl From<std::io::Error> for ReceiveError {
    fn from(e: std::io::Error) -> Self {
        ReceiveError::Io(e)
    }
}

/// Streams the body to disk, emitting throttled progress. The partial file is
/// removed when anything goes wrong, so a failed transfer leaves no rubbish
/// in Downloads.
async fn receive_file(
    state: &Arc<ServerState>,
    session_id: &str,
    file: &IncomingFile,
    body: Body,
    cancelled: &Arc<AtomicBool>,
) -> Result<PathBuf, ReceiveError> {
    let (mut handle, path) = create_unique(&state.download_dir, &file.relative_path).await?;
    let mut hasher = file.sha256.as_ref().map(|_| Sha256::new());
    let mut received: u64 = 0;
    let mut last_emit = Instant::now() - PROGRESS_INTERVAL;
    // What the frontend last heard, so the final event is skipped when it
    // would repeat the previous one. A small file then produces exactly one.
    let mut last_sent: Option<u64> = None;
    let mut stream = body.into_data_stream();

    let outcome = async {
        while let Some(chunk) = stream.next().await {
            if cancelled.load(Ordering::SeqCst) {
                return Err(ReceiveError::Cancelled);
            }
            let chunk = chunk.map_err(|e| ReceiveError::Io(std::io::Error::other(e)))?;
            handle.write_all(&chunk).await?;
            if let Some(hasher) = hasher.as_mut() {
                hasher.update(&chunk);
            }
            received += chunk.len() as u64;
            if last_emit.elapsed() >= PROGRESS_INTERVAL && last_sent != Some(received) {
                last_emit = Instant::now();
                last_sent = Some(received);
                emit_progress(state, session_id, file, received);
            }
        }
        handle.flush().await?;
        handle.sync_all().await?;

        if let (Some(hasher), Some(expected)) = (hasher, file.sha256.as_ref()) {
            let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(ReceiveError::Checksum);
            }
        }
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            if last_sent != Some(received) {
                emit_progress(state, session_id, file, received);
            }
            Ok(path)
        }
        Err(e) => {
            drop(handle);
            let _ = tokio::fs::remove_file(&path).await;
            Err(e)
        }
    }
}

fn emit_progress(state: &Arc<ServerState>, session_id: &str, file: &IncomingFile, received: u64) {
    let (session_done, session_total) = state
        .sessions
        .record_progress(session_id, &file.id, received)
        .unwrap_or((received, file.size));
    (state.emit)(
        TRANSFER_PROGRESS_EVENT,
        json!({
            "sessionId": session_id,
            "fileId": file.id,
            "fileName": file.file_name,
            "bytesReceived": received,
            "totalBytes": file.size,
            // The whole session, which is what the arc on the circle shows.
            "sessionDone": session_done,
            "sessionTotal": session_total,
            "direction": "receive",
            // Which circle on the radar this belongs to.
            "peer": state.sessions.active_sender(session_id).map(|s| s.fingerprint),
        }),
    );
}

/// Creates the file, never overwriting: on a collision the name gains
/// ` (1)`, ` (2)` and so on. `create_new` makes the check and the create one
/// atomic step, so two files racing for the same name cannot both win.
async fn create_unique(
    download_dir: &Path,
    relative: &Path,
) -> std::io::Result<(tokio::fs::File, PathBuf)> {
    let target = download_dir.join(relative);
    if !is_inside(download_dir, &target) {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "file name escapes the download directory",
        ));
    }
    let parent = target.parent().unwrap_or(download_dir).to_path_buf();
    tokio::fs::create_dir_all(&parent).await?;

    let name = relative
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidInput, "no file name"))?;

    for attempt in 0..MAX_NAME_ATTEMPTS {
        let candidate = parent.join(candidate_name(&name, attempt));
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .await
        {
            Ok(handle) => return Ok((handle, candidate)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "too many files with that name",
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelQuery {
    session_id: Option<String>,
}

async fn cancel(State(state): State<Arc<ServerState>>, Query(query): Query<CancelQuery>) -> Response {
    if let Some(session_id) = query.session_id {
        if state.sessions.cancel(&session_id) {
            (state.emit)(
                SESSION_FINISHED_EVENT,
                json!({
                    "sessionId": session_id,
                    "status": "cancelled",
                    "direction": "receive",
                }),
            );
        }
    }
    StatusCode::OK.into_response()
}

/// A TLS-terminating listener for `axum::serve`.
///
/// Handshakes run in their own tasks and finished connections queue up here,
/// so one peer stalling mid-handshake cannot hold up everyone else.
pub struct TlsListener {
    local_addr: SocketAddr,
    incoming: tokio::sync::mpsc::Receiver<(tokio_rustls::server::TlsStream<TcpStream>, SocketAddr)>,
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.incoming.recv().await {
            Some(connection) => connection,
            // The acceptor task is gone. Never returning is the only sane
            // answer here: the trait has no way to report a dead listener,
            // and panicking would take the app down.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}

/// Binds `port` and starts terminating TLS with our own certificate.
pub async fn bind_tls(
    port: u16,
    cert_pem: &str,
    key_pem: &str,
) -> Result<TlsListener, String> {
    use tokio_rustls::rustls::pki_types::pem::PemObject;
    use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use tokio_rustls::rustls::ServerConfig;

    // Installing the provider twice is not an error we care about; another
    // part of the app (or a test) may have done it already.
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let certs = CertificateDer::pem_slice_iter(cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("certificate is not valid PEM: {e:?}"))?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
        .map_err(|e| format!("private key is not valid PEM: {e:?}"))?;

    let mut config = ServerConfig::builder()
        // Peers are identified by certificate fingerprint, not by a CA, so
        // asking for a client certificate would buy nothing here.
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TLS configuration rejected: {e}"))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .map_err(|e| format!("cannot bind port {port}: {e}"))?;
    let local_addr = listener.local_addr().map_err(|e| e.to_string())?;

    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tauri::async_runtime::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(e) => {
                    eprintln!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let acceptor = acceptor.clone();
            let tx = tx.clone();
            tauri::async_runtime::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls) => {
                        let _ = tx.send((tls, peer)).await;
                    }
                    Err(e) => eprintln!("TLS handshake with {peer} failed: {e}"),
                }
            });
        }
    });

    Ok(TlsListener {
        local_addr,
        incoming: rx,
    })
}

/// Starts the HTTPS server. Returns once it is listening; it then runs until
/// the process ends.
pub async fn start(state: Arc<ServerState>, cert_pem: &str, key_pem: &str) -> Result<(), String> {
    use axum::serve::ListenerExt;

    // `tap_io` is the hook axum offers for custom listeners: its blanket impl
    // is what lets handlers extract the peer address of a TLS connection.
    let listener = bind_tls(DEFAULT_PORT, cert_pem, key_pem).await?.tap_io(|_| {});
    let app = router(state);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            eprintln!("HTTPS server stopped: {e}");
        }
    });
    Ok(())
}
