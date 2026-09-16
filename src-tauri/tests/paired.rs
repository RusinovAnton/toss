//! Pairing, over real TLS.
//!
//! These run the actual HTTPS server and the actual client, because the whole
//! point of pairing is what the handshake proves. A plain-HTTP test could not
//! tell a paired device from one claiming to be it.

use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use toss_lib::identity::Identity;
use toss_lib::protocol::{DeviceInfo, DeviceType, ProtocolType, PROTOCOL_VERSION};
use toss_lib::send::{SendManager, Target};
use toss_lib::server::{bind_tls, router, serve, ServerState};
use toss_lib::session::{Decision, SessionManager};
use toss_lib::settings::Settings;
use toss_lib::trust::TrustStore;

type Events = Arc<Mutex<Vec<(String, Value)>>>;

struct Fixture {
    sender: Arc<SendManager>,
    sender_identity: Identity,
    receiver_identity: Identity,
    receiver_trust: Arc<Mutex<TrustStore>>,
    sender_trust: Arc<Mutex<TrustStore>>,
    receiver_state: Arc<ServerState>,
    receiver_events: Events,
    download_dir: PathBuf,
    work_dir: PathBuf,
    port: u16,
}

impl Fixture {
    fn target(&self) -> Target {
        Target {
            fingerprint: self.receiver_identity.fingerprint.clone(),
            ip: "127.0.0.1".into(),
            port: self.port,
            protocol: ProtocolType::Https,
        }
    }

    fn events(&self, name: &str) -> Vec<Value> {
        self.receiver_events
            .lock()
            .unwrap()
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    fn saved_files(&self) -> Vec<String> {
        std::fs::read_dir(&self.download_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.work_dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// Answers the next request, for the cases where nobody is paired.
    fn auto_respond(&self, accept: bool) {
        let state = Arc::clone(&self.receiver_state);
        let events = Arc::clone(&self.receiver_events);
        tokio::spawn(async move {
            for _ in 0..2000 {
                let pending = events
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(event, _)| event == "incoming-request")
                    .map(|(_, payload)| payload.clone());
                if let Some(payload) = pending {
                    let session = payload["sessionId"].as_str().unwrap().to_string();
                    let ids: Vec<String> = if accept {
                        payload["files"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|f| f["id"].as_str().unwrap().to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let decision = if ids.is_empty() {
                        Decision::Decline
                    } else {
                        Decision::Accept(ids)
                    };
                    let _ = state.sessions.respond(&session, decision);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
    }
}

async fn fixture() -> Fixture {
    let run = uuid::Uuid::new_v4();
    let download_dir = std::env::temp_dir().join(format!("toss-paired-in-{run}"));
    let work_dir = std::env::temp_dir().join(format!("toss-paired-out-{run}"));
    std::fs::create_dir_all(&download_dir).unwrap();
    std::fs::create_dir_all(&work_dir).unwrap();

    let receiver_identity = Identity::generate().unwrap();
    let sender_identity = Identity::generate().unwrap();

    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let receiver_trust = Arc::new(Mutex::new(TrustStore::default()));
    let receiver_state = Arc::new(ServerState {
        info: Arc::new(Mutex::new(DeviceInfo {
            alias: "Receiver".into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("macOS".into()),
            device_type: Some(DeviceType::Desktop),
            fingerprint: receiver_identity.fingerprint.clone(),
            port: 53317,
            protocol: ProtocolType::Https,
            download: false,
        })),
        sessions: Arc::new(SessionManager::new()),
        settings: Arc::new(Mutex::new(Settings::default())),
        trusted: Arc::clone(&receiver_trust),
        download_dir: download_dir.clone(),
        emit: Arc::new(move |event, payload| {
            sink.lock().unwrap().push((event.to_string(), payload));
        }),
        register_peer: Arc::new(|_, _| {}),
    });

    // Port 0 lets the OS pick, so tests never fight over 53317.
    let listener = bind_tls(
        0,
        &receiver_identity.certificate_pem,
        &receiver_identity.private_key_pem,
    )
    .await
    .unwrap();
    let port = {
        use axum::serve::Listener;
        listener.local_addr().unwrap().addr.port()
    };
    let app = router(Arc::clone(&receiver_state));
    tokio::spawn(async move {
        serve(listener, app).await.unwrap();
    });

    let sender_trust = Arc::new(Mutex::new(TrustStore::default()));
    let sender = Arc::new(
        SendManager::new(
            Arc::new(Mutex::new(sender_identity.to_device_info())),
            &sender_identity.certificate_pem,
            &sender_identity.private_key_pem,
            Arc::new(|_, _| {}),
            Arc::clone(&sender_trust),
        )
        .unwrap(),
    );

    Fixture {
        sender,
        sender_identity,
        receiver_identity,
        receiver_trust,
        sender_trust,
        receiver_state,
        receiver_events: events,
        download_dir,
        work_dir,
        port,
    }
}

#[tokio::test]
async fn the_handshake_proves_which_device_is_asking() {
    let fixture = fixture().await;
    fixture.auto_respond(true);
    let path = fixture.write("one.txt", b"first");

    fixture
        .sender
        .send(fixture.target(), &[path], None)
        .await
        .unwrap();

    let request = &fixture.events("incoming-request")[0];
    // Not the fingerprint the sender wrote in the body, the one it proved.
    assert_eq!(
        request["sender"]["verifiedFingerprint"].as_str().unwrap(),
        fixture.sender_identity.fingerprint
    );
}

#[tokio::test]
async fn an_unpaired_device_still_has_to_ask() {
    let fixture = fixture().await;
    fixture.auto_respond(false);
    let path = fixture.write("one.txt", b"first");

    let error = fixture
        .sender
        .send(fixture.target(), &[path], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "declined");
    assert_eq!(fixture.events("incoming-request").len(), 1);
}

#[tokio::test]
async fn a_paired_device_is_accepted_without_asking() {
    let fixture = fixture().await;
    fixture
        .receiver_trust
        .lock()
        .unwrap()
        .trust(&fixture.sender_identity.fingerprint, "Sender");
    let path = fixture.write("one.txt", b"first");

    // Nobody answers anything here: pairing is the answer.
    let summary = fixture
        .sender
        .send(fixture.target(), &[path], None)
        .await
        .unwrap();

    assert_eq!(summary.files_sent, 1);
    assert_eq!(fixture.saved_files(), vec!["one.txt"]);
    assert!(
        fixture.events("incoming-request").is_empty(),
        "a paired device should not raise a card"
    );
}

#[tokio::test]
async fn pairing_someone_else_does_not_let_this_device_in() {
    let fixture = fixture().await;
    // A different device is paired, not the one sending.
    let stranger = Identity::generate().unwrap();
    fixture
        .receiver_trust
        .lock()
        .unwrap()
        .trust(&stranger.fingerprint, "Someone else");
    fixture.auto_respond(false);
    let path = fixture.write("one.txt", b"first");

    let error = fixture
        .sender
        .send(fixture.target(), &[path], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "declined");
    assert_eq!(fixture.events("incoming-request").len(), 1);
}

#[tokio::test]
async fn clipboard_text_arrives_as_text_and_never_as_a_file() {
    let fixture = fixture().await;
    // Paired, not merely trusted: the clipboard is what pairing is for.
    fixture.receiver_trust.lock().unwrap().set_paired(
        &fixture.sender_identity.fingerprint,
        "Sender",
        true,
    );

    let summary = fixture
        .sender
        .send_text(fixture.target(), "hello from the other side", None)
        .await
        .unwrap();
    assert_eq!(summary.files_sent, 1);

    let received = fixture.events("text-received");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["text"], "hello from the other side");
    assert_eq!(
        received[0]["peer"].as_str().unwrap(),
        fixture.sender_identity.fingerprint
    );
    assert!(
        fixture.saved_files().is_empty(),
        "a message must not land in Downloads"
    );

    let finished = fixture.events("session-finished");
    assert_eq!(finished.last().unwrap()["kind"], "text");
    assert!(!fixture.receiver_state.sessions.is_busy());
}

#[tokio::test]
async fn text_from_a_device_that_is_only_trusted_is_saved_as_a_file() {
    let fixture = fixture().await;
    // Trusted, so it needs no answer, but not paired, so it has no claim on
    // the clipboard.
    fixture
        .receiver_trust
        .lock()
        .unwrap()
        .trust(&fixture.sender_identity.fingerprint, "Sender");

    fixture
        .sender
        .send_text(fixture.target(), "not for your clipboard", None)
        .await
        .unwrap();

    assert!(
        fixture.events("text-received").is_empty(),
        "an unpaired device must not reach the clipboard"
    );
    assert_eq!(fixture.saved_files(), vec!["message.txt"]);
    assert_eq!(
        std::fs::read_to_string(fixture.download_dir.join("message.txt")).unwrap(),
        "not for your clipboard"
    );
}

#[tokio::test]
async fn a_message_from_an_unknown_device_is_still_asked_about() {
    let fixture = fixture().await;
    fixture.auto_respond(false);

    let error = fixture
        .sender
        .send_text(fixture.target(), "peek at this", None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "declined");
    assert_eq!(fixture.events("incoming-request").len(), 1);
}

#[tokio::test]
async fn trusting_a_device_does_not_pair_it() {
    let fixture = fixture().await;
    fixture
        .receiver_trust
        .lock()
        .unwrap()
        .trust(&fixture.sender_identity.fingerprint, "Sender");
    let path = fixture.write("one.txt", b"first");

    // Files go through without a card...
    fixture
        .sender
        .send(fixture.target(), &[path], None)
        .await
        .unwrap();
    assert!(fixture.events("incoming-request").is_empty());

    // ...but the clipboard does not.
    fixture
        .sender
        .send_text(fixture.target(), "hello", None)
        .await
        .unwrap();
    assert!(fixture.events("text-received").is_empty());
}

#[tokio::test]
async fn sending_to_a_paired_device_requires_its_certificate() {
    let fixture = fixture().await;
    fixture
        .sender_trust
        .lock()
        .unwrap()
        .trust(&fixture.receiver_identity.fingerprint, "Receiver");
    fixture
        .receiver_trust
        .lock()
        .unwrap()
        .trust(&fixture.sender_identity.fingerprint, "Sender");
    let path = fixture.write("one.txt", b"first");

    let summary = fixture
        .sender
        .send(fixture.target(), &[path], None)
        .await
        .unwrap();
    assert_eq!(summary.files_sent, 1);
}

#[tokio::test]
async fn an_impostor_at_the_paired_address_is_refused() {
    let fixture = fixture().await;
    // We paired with a device whose certificate is not the one answering.
    let impostor = Identity::generate().unwrap();
    fixture
        .sender_trust
        .lock()
        .unwrap()
        .trust(&impostor.fingerprint, "Receiver");
    let mut target = fixture.target();
    // The radar still points at the same address under the paired name.
    target.fingerprint = impostor.fingerprint.clone();
    let path = fixture.write("one.txt", b"first");

    let error = fixture
        .sender
        .send(target, &[path], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "wrong-device");
    assert!(fixture.events("incoming-request").is_empty());
}
