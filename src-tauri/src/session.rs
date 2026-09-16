//! Receive-side session state.
//!
//! The protocol allows one transfer session at a time; a second sender gets
//! `409`. A session lives in two stages: **pending**, while the user decides,
//! and **active**, once files were accepted and tokens handed out.

use crate::protocol::FileDto;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// How long an active session may sit without any upload activity before a
/// new sender may take the receiver over.
///
/// Without this, a sender that dies mid-transfer, or one whose connection
/// drops, would hold the single session slot until the app restarts.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// What the user chose for a pending request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Accept exactly these file ids. An empty list is a decline.
    Accept(Vec<String>),
    Decline,
}

/// A file the sender offered, after its name passed validation.
#[derive(Clone, Debug)]
pub struct IncomingFile {
    pub id: String,
    /// The name as sent, kept for display and for the event payload.
    pub file_name: String,
    /// The validated relative path the file is written to.
    pub relative_path: PathBuf,
    pub size: u64,
    pub file_type: Option<String>,
    pub sha256: Option<String>,
    pub token: String,
}

impl IncomingFile {
    pub fn new(dto: &FileDto, relative_path: PathBuf, token: String) -> Self {
        IncomingFile {
            id: dto.id.clone(),
            file_name: dto.file_name.clone(),
            relative_path,
            size: dto.size,
            file_type: dto.file_type.clone(),
            sha256: dto.sha256.clone(),
            token,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sender {
    pub alias: String,
    /// What the sender claimed in the request body.
    pub fingerprint: String,
    /// What its certificate proved, when there was one. Trust is granted
    /// against this, never against the claim.
    pub verified_fingerprint: Option<String>,
    pub device_model: Option<String>,
    pub ip: IpAddr,
}

pub struct ActiveSession {
    pub id: String,
    pub sender: Sender,
    pub files: HashMap<String, IncomingFile>,
    /// The text, when this session is a clipboard message rather than files.
    /// Such a session is never written to disk.
    pub message: Option<String>,
    /// Flipped by `/cancel`. In-flight uploads check it between chunks.
    pub cancelled: Arc<AtomicBool>,
    completed: HashSet<String>,
    saved_paths: Vec<PathBuf>,
    /// Bytes written per file, so the UI can show one arc for the whole
    /// session rather than restarting it at every file.
    received: HashMap<String, u64>,
    last_activity: Instant,
}

impl ActiveSession {
    /// Whether every accepted file has been written.
    pub fn is_finished(&self) -> bool {
        self.completed.len() == self.files.len()
    }

    /// Total size of every accepted file.
    pub fn total_bytes(&self) -> u64 {
        self.files.values().map(|file| file.size).sum()
    }
}

struct Pending {
    id: String,
    /// Kept so answering can also grant trust, which needs the fingerprint
    /// the handshake proved.
    sender: Sender,
    responder: oneshot::Sender<Decision>,
}

#[derive(Default)]
struct Inner {
    pending: Option<Pending>,
    active: Option<ActiveSession>,
}

/// Why a session could not be claimed or a file could not be uploaded.
#[derive(Debug, PartialEq, Eq)]
pub enum SessionError {
    /// Another session is pending or active: protocol `409`.
    Busy,
    /// Unknown session, unknown file, wrong token or wrong IP: protocol `403`.
    Rejected,
}

#[derive(Default)]
pub struct SessionManager {
    inner: Mutex<Inner>,
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("session manager poisoned")
    }

    pub fn is_busy(&self) -> bool {
        let inner = self.lock();
        inner.pending.is_some() || inner.active.is_some()
    }

    /// Drops an active session that has gone quiet, so an abandoned transfer
    /// does not block the receiver for good.
    fn evict_if_idle(inner: &mut Inner, timeout: Duration) {
        let idle = inner
            .active
            .as_ref()
            .is_some_and(|active| active.last_activity.elapsed() >= timeout);
        if idle {
            if let Some(active) = inner.active.take() {
                active.cancelled.store(true, Ordering::SeqCst);
                eprintln!("dropped session {} after {timeout:?} of silence", active.id);
            }
        }
    }

    /// Claims the single session slot and registers the channel the user's
    /// decision arrives on. `Err(Busy)` when someone else already holds it.
    pub fn begin(
        &self,
        id: &str,
        sender: Sender,
    ) -> Result<oneshot::Receiver<Decision>, SessionError> {
        self.begin_with_timeout(id, sender, IDLE_TIMEOUT)
    }

    /// [`SessionManager::begin`] with an explicit idle timeout, for tests.
    pub fn begin_with_timeout(
        &self,
        id: &str,
        sender: Sender,
        timeout: Duration,
    ) -> Result<oneshot::Receiver<Decision>, SessionError> {
        let mut inner = self.lock();
        Self::evict_if_idle(&mut inner, timeout);
        if inner.pending.is_some() || inner.active.is_some() {
            return Err(SessionError::Busy);
        }
        let (tx, rx) = oneshot::channel();
        inner.pending = Some(Pending {
            id: id.to_string(),
            sender,
            responder: tx,
        });
        Ok(rx)
    }

    /// Delivers the user's answer. Fails when the request is gone, which
    /// happens if the sender gave up or the request timed out.
    pub fn respond(&self, session_id: &str, decision: Decision) -> Result<(), String> {
        let pending = {
            let mut inner = self.lock();
            match &inner.pending {
                Some(p) if p.id == session_id => inner.pending.take(),
                Some(_) => return Err("no request with that session id is waiting".into()),
                None => return Err("no request is waiting for an answer".into()),
            }
        };
        pending
            .ok_or_else(|| "no request is waiting for an answer".to_string())?
            .responder
            .send(decision)
            .map_err(|_| "the sender stopped waiting for an answer".to_string())
    }

    /// Drops a pending request without answering it, e.g. after a timeout.
    pub fn clear_pending(&self, session_id: &str) {
        let mut inner = self.lock();
        if inner.pending.as_ref().is_some_and(|p| p.id == session_id) {
            inner.pending = None;
        }
    }

    /// Promotes the accepted files to the active session.
    pub fn activate(&self, session: ActiveSession) {
        let mut inner = self.lock();
        inner.pending = None;
        inner.active = Some(session);
    }

    /// Looks up a file for upload, checking the token and the sender's IP.
    /// Returns the file and the session's cancellation flag.
    pub fn authorize_upload(
        &self,
        session_id: &str,
        file_id: &str,
        token: &str,
        peer: IpAddr,
    ) -> Result<(IncomingFile, Arc<AtomicBool>), SessionError> {
        let mut inner = self.lock();
        let Some(active) = inner.active.as_mut() else {
            return Err(SessionError::Rejected);
        };
        if active.id != session_id {
            // Someone else holds the slot; the protocol calls that 409.
            return Err(SessionError::Busy);
        }
        if active.sender.ip != peer {
            return Err(SessionError::Rejected);
        }
        let file = active.files.get(file_id).ok_or(SessionError::Rejected)?;
        // Constant-time comparison is not warranted: the token is a v4 uuid
        // handed to this exact peer moments ago over TLS.
        if file.token != token {
            return Err(SessionError::Rejected);
        }
        let authorized = (file.clone(), Arc::clone(&active.cancelled));
        active.last_activity = Instant::now();
        Ok(authorized)
    }

    /// Records how far one file has got. Returns the session's progress as
    /// (bytes done, bytes expected) so the UI can draw a single arc.
    pub fn record_progress(
        &self,
        session_id: &str,
        file_id: &str,
        bytes: u64,
    ) -> Option<(u64, u64)> {
        let mut inner = self.lock();
        let active = inner.active.as_mut()?;
        if active.id != session_id {
            return None;
        }
        active.received.insert(file_id.to_string(), bytes);
        Some((active.received.values().sum(), active.total_bytes()))
    }

    /// Records a finished file. Returns the saved paths once the whole
    /// session is done, and `None` while files are still missing.
    pub fn complete_file(
        &self,
        session_id: &str,
        file_id: &str,
        saved_path: PathBuf,
    ) -> Option<Vec<PathBuf>> {
        let mut inner = self.lock();
        let active = inner.active.as_mut()?;
        if active.id != session_id {
            return None;
        }
        active.completed.insert(file_id.to_string());
        active.saved_paths.push(saved_path);
        active.last_activity = Instant::now();
        if !active.is_finished() {
            return None;
        }
        let session = inner.active.take()?;
        Some(session.saved_paths)
    }

    /// Cancels a session from either side. Returns whether anything was
    /// cancelled.
    pub fn cancel(&self, session_id: &str) -> bool {
        let mut inner = self.lock();
        let mut cancelled = false;
        if inner.active.as_ref().is_some_and(|a| a.id == session_id) {
            if let Some(active) = inner.active.take() {
                active.cancelled.store(true, Ordering::SeqCst);
                cancelled = true;
            }
        }
        if inner.pending.as_ref().is_some_and(|p| p.id == session_id) {
            if let Some(pending) = inner.pending.take() {
                let _ = pending.responder.send(Decision::Decline);
                cancelled = true;
            }
        }
        cancelled
    }

    /// The device behind a request that is still waiting for an answer.
    pub fn pending_sender(&self, session_id: &str) -> Option<Sender> {
        let inner = self.lock();
        inner
            .pending
            .as_ref()
            .filter(|pending| pending.id == session_id)
            .map(|pending| pending.sender.clone())
    }

    /// The text of the active session, when it is a clipboard message.
    pub fn active_message(&self, session_id: &str) -> Option<String> {
        let inner = self.lock();
        inner
            .active
            .as_ref()
            .filter(|active| active.id == session_id)
            .and_then(|active| active.message.clone())
    }

    /// The sender of the active session, for progress events.
    pub fn active_sender(&self, session_id: &str) -> Option<Sender> {
        let inner = self.lock();
        inner
            .active
            .as_ref()
            .filter(|a| a.id == session_id)
            .map(|a| a.sender.clone())
    }
}

/// Builds an active session from the files the user accepted.
pub fn build_active_session(
    id: String,
    sender: Sender,
    offered: &[IncomingFile],
    accepted_ids: &[String],
    message: Option<String>,
) -> ActiveSession {
    let accepted: HashSet<&String> = accepted_ids.iter().collect();
    let files = offered
        .iter()
        .filter(|file| accepted.contains(&file.id))
        .map(|file| (file.id.clone(), file.clone()))
        .collect();
    ActiveSession {
        id,
        sender,
        files,
        message,
        cancelled: Arc::new(AtomicBool::new(false)),
        completed: HashSet::new(),
        saved_paths: Vec::new(),
        received: HashMap::new(),
        last_activity: Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn peer() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5))
    }

    fn sender() -> Sender {
        Sender {
            alias: "Secret Banana".into(),
            fingerprint: "F".into(),
            verified_fingerprint: Some("VERIFIED".into()),
            device_model: Some("Windows".into()),
            ip: peer(),
        }
    }

    fn file(id: &str, token: &str) -> IncomingFile {
        IncomingFile {
            id: id.into(),
            file_name: format!("{id}.png"),
            relative_path: PathBuf::from(format!("{id}.png")),
            size: 10,
            file_type: Some("image/png".into()),
            sha256: None,
            token: token.into(),
        }
    }

    fn activate_with(manager: &SessionManager, ids: &[&str]) {
        let offered: Vec<IncomingFile> = ids.iter().map(|id| file(id, "tok")).collect();
        let accepted: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        manager.activate(build_active_session(
            "s1".into(),
            sender(),
            &offered,
            &accepted,
            None,
        ));
    }

    #[test]
    fn a_second_request_is_refused_while_one_is_pending() {
        let manager = SessionManager::new();
        let _rx = manager.begin("s1", sender()).unwrap();
        assert_eq!(manager.begin("s2", sender()).unwrap_err(), SessionError::Busy);
    }

    #[test]
    fn a_second_request_is_refused_while_one_is_active() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a"]);
        assert_eq!(manager.begin("s2", sender()).unwrap_err(), SessionError::Busy);
    }

    #[tokio::test]
    async fn the_decision_reaches_the_waiting_request() {
        let manager = SessionManager::new();
        let rx = manager.begin("s1", sender()).unwrap();
        manager
            .respond("s1", Decision::Accept(vec!["a".into()]))
            .unwrap();
        assert_eq!(rx.await.unwrap(), Decision::Accept(vec!["a".into()]));
    }

    #[test]
    fn answering_an_unknown_session_fails() {
        let manager = SessionManager::new();
        let _rx = manager.begin("s1", sender()).unwrap();
        assert!(manager.respond("other", Decision::Decline).is_err());
    }

    #[test]
    fn clearing_a_pending_request_frees_the_slot() {
        let manager = SessionManager::new();
        let _rx = manager.begin("s1", sender()).unwrap();
        manager.clear_pending("s1");
        assert!(!manager.is_busy());
    }

    #[test]
    fn only_accepted_files_end_up_in_the_session() {
        let offered = vec![file("a", "t1"), file("b", "t2")];
        let session =
            build_active_session("s1".into(), sender(), &offered, &["a".to_string()], None);
        assert_eq!(session.files.len(), 1);
        assert!(session.files.contains_key("a"));
    }

    #[test]
    fn upload_needs_the_right_token() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a"]);
        assert!(manager.authorize_upload("s1", "a", "tok", peer()).is_ok());
        assert_eq!(
            manager.authorize_upload("s1", "a", "wrong", peer()).unwrap_err(),
            SessionError::Rejected
        );
    }

    #[test]
    fn upload_needs_the_right_ip_file_and_session() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a"]);
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(
            manager.authorize_upload("s1", "a", "tok", other).unwrap_err(),
            SessionError::Rejected
        );
        assert_eq!(
            manager.authorize_upload("s1", "missing", "tok", peer()).unwrap_err(),
            SessionError::Rejected
        );
        assert_eq!(
            manager.authorize_upload("other", "a", "tok", peer()).unwrap_err(),
            SessionError::Busy
        );
    }

    #[test]
    fn upload_without_an_active_session_is_rejected() {
        let manager = SessionManager::new();
        assert_eq!(
            manager.authorize_upload("s1", "a", "tok", peer()).unwrap_err(),
            SessionError::Rejected
        );
    }

    #[test]
    fn the_session_finishes_only_once_every_file_arrived() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a", "b"]);
        assert!(manager.complete_file("s1", "a", PathBuf::from("/d/a.png")).is_none());
        let saved = manager
            .complete_file("s1", "b", PathBuf::from("/d/b.png"))
            .expect("session should finish");
        assert_eq!(saved.len(), 2);
        assert!(!manager.is_busy());
    }

    #[test]
    fn cancelling_clears_the_slot_and_flags_in_flight_uploads() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a"]);
        let (_, cancelled) = manager.authorize_upload("s1", "a", "tok", peer()).unwrap();
        assert!(manager.cancel("s1"));
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(!manager.is_busy());
        assert!(!manager.cancel("s1"));
    }

    #[test]
    fn a_waiting_request_remembers_who_sent_it() {
        let manager = SessionManager::new();
        let _rx = manager.begin("s1", sender()).unwrap();
        let waiting = manager.pending_sender("s1").unwrap();
        assert_eq!(waiting.verified_fingerprint.as_deref(), Some("VERIFIED"));
        assert_eq!(manager.pending_sender("other"), None);
    }

    #[test]
    fn a_message_session_remembers_its_text() {
        let manager = SessionManager::new();
        let offered = vec![file("a", "tok")];
        manager.activate(build_active_session(
            "s1".into(),
            sender(),
            &offered,
            &["a".to_string()],
            Some("hello clipboard".into()),
        ));
        assert_eq!(
            manager.active_message("s1").as_deref(),
            Some("hello clipboard")
        );
        assert_eq!(manager.active_message("other"), None);
    }

    #[test]
    fn progress_covers_the_whole_session_not_one_file() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a", "b"]);
        // Each test file is 10 bytes, so the session expects 20.
        assert_eq!(manager.record_progress("s1", "a", 4), Some((4, 20)));
        assert_eq!(manager.record_progress("s1", "b", 6), Some((10, 20)));
        assert_eq!(manager.record_progress("s1", "a", 10), Some((16, 20)));
        assert_eq!(manager.record_progress("other", "a", 1), None);
    }

    #[test]
    fn an_abandoned_session_stops_blocking_the_receiver() {
        let manager = SessionManager::new();
        activate_with(&manager, &["a"]);
        // Still fresh: a new sender has to wait.
        assert_eq!(
            manager
                .begin_with_timeout("s2", sender(), Duration::from_secs(60))
                .unwrap_err(),
            SessionError::Busy
        );
        // Once it has gone quiet, the slot is taken over.
        let (_, cancelled) = manager.authorize_upload("s1", "a", "tok", peer()).unwrap();
        assert!(manager
            .begin_with_timeout("s2", sender(), Duration::from_millis(0))
            .is_ok());
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancelling_a_pending_request_declines_it() {
        let manager = SessionManager::new();
        let rx = manager.begin("s1", sender()).unwrap();
        assert!(manager.cancel("s1"));
        assert_eq!(rx.await.unwrap(), Decision::Decline);
        assert!(!manager.is_busy());
    }
}
