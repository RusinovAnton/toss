//! End-to-end tests for the send side, driven against our own receive server.
//!
//! Both halves of the protocol run in-process: `SendManager` talks to the
//! same axum router the app serves, over plain HTTP on an ephemeral port.

use serde_json::Value;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use toss_lib::identity::Identity;
use toss_lib::protocol::{DeviceInfo, DeviceType, ProtocolType, PROTOCOL_VERSION};
use toss_lib::send::{SendManager, Target};
use toss_lib::server::{router, ServerState};
use toss_lib::session::{Decision, SessionManager};
use toss_lib::settings::Settings;

type Events = Arc<Mutex<Vec<(String, Value)>>>;

struct Pair {
    sender: Arc<SendManager>,
    target: Target,
    receiver_state: Arc<ServerState>,
    receiver_events: Events,
    sender_events: Events,
    download_dir: PathBuf,
    work_dir: PathBuf,
}

impl Pair {
    fn events(events: &Events, name: &str) -> Vec<Value> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    fn saved(&self) -> Vec<String> {
        let mut names = Vec::new();
        collect(&self.download_dir, &self.download_dir, &mut names);
        names.sort();
        names
    }

    fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.work_dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// Answers the receiver's next incoming request.
    fn auto_respond(&self, accept: bool) {
        let state = Arc::clone(&self.receiver_state);
        let events = Arc::clone(&self.receiver_events);
        tokio::spawn(async move {
            for _ in 0..1000 {
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

fn collect(base: &PathBuf, dir: &PathBuf, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(base, &path, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_string_lossy().to_string());
        }
    }
}

fn recorder() -> (Events, Arc<dyn Fn(&str, Value) + Send + Sync>) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    (
        events,
        Arc::new(move |event: &str, payload: Value| {
            sink.lock().unwrap().push((event.to_string(), payload));
        }),
    )
}

async fn pair(settings: Settings) -> Pair {
    let run = uuid::Uuid::new_v4();
    let download_dir = std::env::temp_dir().join(format!("toss-send-in-{run}"));
    let work_dir = std::env::temp_dir().join(format!("toss-send-out-{run}"));
    std::fs::create_dir_all(&download_dir).unwrap();
    std::fs::create_dir_all(&work_dir).unwrap();

    let (receiver_events, receiver_emit) = recorder();
    let receiver_state = Arc::new(ServerState {
        info: Arc::new(Mutex::new(DeviceInfo {
            alias: "Receiver".into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("macOS".into()),
            device_type: Some(DeviceType::Desktop),
            fingerprint: "RECEIVER".into(),
            port: 53317,
            protocol: ProtocolType::Http,
            download: false,
        })),
        sessions: Arc::new(SessionManager::new()),
        settings: Arc::new(Mutex::new(settings)),
        download_dir: download_dir.clone(),
        emit: receiver_emit,
        register_peer: Arc::new(|_, _| {}),
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::clone(&receiver_state));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let identity = Identity::generate().unwrap();
    let (sender_events, sender_emit) = recorder();
    let sender = SendManager::new(
        Arc::new(Mutex::new(identity.to_device_info())),
        &identity.certificate_pem,
        &identity.private_key_pem,
        sender_emit,
    )
    .unwrap();

    Pair {
        sender: Arc::new(sender),
        target: Target {
            fingerprint: "RECEIVER".into(),
            ip: addr.ip().to_string(),
            port: addr.port(),
            protocol: ProtocolType::Http,
        },
        receiver_state,
        receiver_events,
        sender_events,
        download_dir,
        work_dir,
    }
}

fn quick_save() -> Settings {
    Settings {
        quick_save: true,
        ..Settings::default()
    }
}

#[tokio::test]
async fn files_arrive_at_the_other_side() {
    let pair = pair(quick_save()).await;
    let one = pair.write("one.txt", b"first");
    let two = pair.write("two.txt", b"second");

    let summary = pair
        .sender
        .send(pair.target.clone(), &[one, two], None)
        .await
        .unwrap();

    assert_eq!(summary.files_sent, 2);
    assert_eq!(summary.bytes_sent, 11);
    assert_eq!(pair.saved(), vec!["one.txt", "two.txt"]);
    assert_eq!(
        std::fs::read_to_string(pair.download_dir.join("one.txt")).unwrap(),
        "first"
    );
}

#[tokio::test]
async fn a_nested_folder_keeps_its_structure() {
    let pair = pair(quick_save()).await;
    pair.write("holiday/readme.txt", b"hi");
    pair.write("holiday/2024/one.txt", b"one");
    pair.write("holiday/2024/june/two.txt", b"two");
    let folder = pair.work_dir.join("holiday");

    pair.sender
        .send(pair.target.clone(), &[folder], None)
        .await
        .unwrap();

    assert_eq!(
        pair.saved(),
        vec![
            "holiday/2024/june/two.txt",
            "holiday/2024/one.txt",
            "holiday/readme.txt",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(pair.download_dir.join("holiday/2024/june/two.txt")).unwrap(),
        "two"
    );
}

#[tokio::test]
async fn a_large_file_survives_the_round_trip() {
    let pair = pair(quick_save()).await;
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let path = pair.write("big.bin", &payload);

    pair.sender
        .send(pair.target.clone(), &[path], None)
        .await
        .unwrap();

    let received = std::fs::read(pair.download_dir.join("big.bin")).unwrap();
    assert_eq!(received.len(), payload.len());
    assert_eq!(received, payload);

    let progress = Pair::events(&pair.sender_events, "transfer-progress");
    assert!(
        progress.len() > 1,
        "a multi-chunk file should report progress more than once"
    );
    assert!(progress.iter().all(|e| e["direction"] == "send"));
    assert_eq!(
        progress.last().unwrap()["bytesReceived"].as_u64().unwrap(),
        payload.len() as u64
    );
}

#[tokio::test]
async fn a_refusal_is_reported_as_declined() {
    let pair = pair(Settings::default()).await;
    pair.auto_respond(false);
    let path = pair.write("one.txt", b"first");

    let error = pair
        .sender
        .send(pair.target.clone(), &[path], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "declined");
    assert!(pair.saved().is_empty());
}

#[tokio::test]
async fn accepting_only_some_files_sends_only_those() {
    let pair = pair(Settings::default()).await;
    let one = pair.write("one.txt", b"first");
    let two = pair.write("two.txt", b"second");

    // Accept whichever file the receiver lists first.
    let state = Arc::clone(&pair.receiver_state);
    let events = Arc::clone(&pair.receiver_events);
    tokio::spawn(async move {
        for _ in 0..1000 {
            let pending = events
                .lock()
                .unwrap()
                .iter()
                .find(|(event, _)| event == "incoming-request")
                .map(|(_, payload)| payload.clone());
            if let Some(payload) = pending {
                let session = payload["sessionId"].as_str().unwrap().to_string();
                let first = payload["files"][0]["id"].as_str().unwrap().to_string();
                let _ = state.sessions.respond(&session, Decision::Accept(vec![first]));
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    let summary = pair
        .sender
        .send(pair.target.clone(), &[one, two], None)
        .await
        .unwrap();
    assert_eq!(summary.files_sent, 1);
    assert_eq!(pair.saved().len(), 1);
}

#[tokio::test]
async fn a_busy_receiver_is_reported_as_busy() {
    let pair = pair(Settings::default()).await;
    let path = pair.write("one.txt", b"first");

    // Hold the receiver's only session slot with an unanswered request.
    let held = pair.receiver_state.sessions.begin("held").unwrap();

    let error = pair
        .sender
        .send(pair.target.clone(), &[path], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "busy");
    drop(held);
}

#[tokio::test]
async fn a_pin_is_asked_for_and_then_accepted() {
    let pair = pair(Settings {
        pin: Some("123456".into()),
        quick_save: true,
    })
    .await;
    let path = pair.write("one.txt", b"first");

    let error = pair
        .sender
        .send(pair.target.clone(), &[path.clone()], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "pin-required");

    let wrong = pair
        .sender
        .send(pair.target.clone(), &[path.clone()], Some("000000".into()))
        .await
        .unwrap_err();
    assert_eq!(wrong.code(), "pin-required");

    let summary = pair
        .sender
        .send(pair.target.clone(), &[path], Some("123456".into()))
        .await
        .unwrap();
    assert_eq!(summary.files_sent, 1);
}

#[tokio::test]
async fn sending_nothing_is_refused_before_any_request() {
    let pair = pair(quick_save()).await;
    let empty = pair.work_dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();

    let error = pair
        .sender
        .send(pair.target.clone(), &[empty], None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "no-files");
    assert!(Pair::events(&pair.receiver_events, "incoming-request").is_empty());
}

#[tokio::test]
async fn an_unreachable_peer_is_reported_as_a_lost_connection() {
    let pair = pair(quick_save()).await;
    let path = pair.write("one.txt", b"first");
    let nowhere = Target {
        fingerprint: "NOBODY".into(),
        ip: "127.0.0.1".into(),
        // Port 1 is not something anyone listens on.
        port: 1,
        protocol: ProtocolType::Http,
    };

    let error = pair.sender.send(nowhere, &[path], None).await.unwrap_err();
    assert_eq!(error.code(), "connection-lost");
}

#[tokio::test]
async fn cancelling_stops_the_transfer_and_frees_the_receiver() {
    let run = uuid::Uuid::new_v4();
    let download_dir = std::env::temp_dir().join(format!("toss-cancel-in-{run}"));
    let work_dir = std::env::temp_dir().join(format!("toss-cancel-out-{run}"));
    std::fs::create_dir_all(&download_dir).unwrap();
    std::fs::create_dir_all(&work_dir).unwrap();

    let (receiver_events, receiver_emit) = recorder();
    let receiver_state = Arc::new(ServerState {
        info: Arc::new(Mutex::new(DeviceInfo {
            alias: "Receiver".into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("macOS".into()),
            device_type: Some(DeviceType::Desktop),
            fingerprint: "RECEIVER".into(),
            port: 53317,
            protocol: ProtocolType::Http,
            download: false,
        })),
        sessions: Arc::new(SessionManager::new()),
        settings: Arc::new(Mutex::new(quick_save())),
        download_dir: download_dir.clone(),
        emit: receiver_emit,
        register_peer: Arc::new(|_, _| {}),
    });
    let _ = &receiver_events;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::clone(&receiver_state));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    // Cancelling from inside the progress callback removes the race: the flag
    // is set before the upload stream reads its next chunk.
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let (sender_events, _) = recorder();
    let recorded = Arc::clone(&sender_events);
    let identity = Identity::generate().unwrap();
    let sender = SendManager::new(
        Arc::new(Mutex::new(identity.to_device_info())),
        &identity.certificate_pem,
        &identity.private_key_pem,
        Arc::new(move |event: &str, payload: Value| {
            recorded.lock().unwrap().push((event.to_string(), payload));
            if event == "transfer-progress" {
                flag.store(true, Ordering::SeqCst);
            }
        }),
    )
    .unwrap();

    let path = work_dir.join("huge.bin");
    std::fs::write(&path, vec![7u8; 8_000_000]).unwrap();

    let error = sender
        .send_with_flag(
            Target {
                fingerprint: "RECEIVER".into(),
                ip: addr.ip().to_string(),
                port: addr.port(),
                protocol: ProtocolType::Http,
            },
            &[path],
            None,
            cancelled,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "cancelled");

    // The receiver notices the broken body a moment later and removes the
    // partial file, so give its task a chance to run.
    let mut left = Vec::new();
    for _ in 0..200 {
        left.clear();
        collect(&download_dir, &download_dir, &mut left);
        if left.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(left.is_empty(), "partial file was left on disk: {left:?}");

    let finished = Pair::events(&sender_events, "session-finished");
    assert_eq!(finished.last().unwrap()["status"], "cancelled");
}

#[tokio::test]
async fn a_completed_send_reports_it_finished() {
    let pair = pair(quick_save()).await;
    let path = pair.write("one.txt", b"first");

    pair.sender
        .send(pair.target.clone(), &[path], None)
        .await
        .unwrap();

    let finished = Pair::events(&pair.sender_events, "session-finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["status"], "completed");
    assert_eq!(finished[0]["direction"], "send");
}
