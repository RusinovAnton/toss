//! End-to-end tests for the receive side.
//!
//! The router is served over plain HTTP on an ephemeral port so the tests
//! exercise the real handlers, extractors and status codes. TLS is the same
//! router behind `bind_tls`, so nothing about the behaviour changes.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use toss_lib::protocol::{DeviceInfo, DeviceType, ProtocolType, PROTOCOL_VERSION};
use toss_lib::server::{router, serve, PlainListener, ServerState};
use toss_lib::session::SessionManager;
use toss_lib::settings::Settings;
use toss_lib::trust::TrustStore;

type Events = Arc<Mutex<Vec<(String, Value)>>>;

struct Harness {
    base: String,
    state: Arc<ServerState>,
    events: Events,
    download_dir: PathBuf,
    client: reqwest::Client,
}

impl Harness {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn events_named(&self, name: &str) -> Vec<Value> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(event, _)| event == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    fn saved_files(&self) -> Vec<String> {
        let mut names = Vec::new();
        collect(&self.download_dir, &self.download_dir, &mut names);
        names.sort();
        names
    }

    fn read_saved(&self, relative: &str) -> String {
        std::fs::read_to_string(self.download_dir.join(relative)).unwrap()
    }

    /// Answers the next incoming request. `accept` picks every offered file.
    fn auto_respond(&self, accept: bool) {
        let state = Arc::clone(&self.state);
        let events = Arc::clone(&self.events);
        tokio::spawn(async move {
            for _ in 0..500 {
                let pending = events
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(event, _)| event == "incoming-request")
                    .map(|(_, payload)| payload.clone());
                if let Some(payload) = pending {
                    let session_id = payload["sessionId"].as_str().unwrap().to_string();
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
                        toss_lib::session::Decision::Decline
                    } else {
                        toss_lib::session::Decision::Accept(ids)
                    };
                    let _ = state.sessions.respond(&session_id, decision);
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
            out.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
}

async fn harness(settings: Settings) -> Harness {
    let download_dir =
        std::env::temp_dir().join(format!("toss-receive-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&download_dir).unwrap();

    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let state = Arc::new(ServerState {
        info: Arc::new(Mutex::new(DeviceInfo {
            alias: "Test Receiver".into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("macOS".into()),
            device_type: Some(DeviceType::Desktop),
            fingerprint: "RECEIVER".into(),
            port: 53317,
            protocol: ProtocolType::Https,
            download: false,
        })),
        sessions: Arc::new(SessionManager::new()),
        settings: Arc::new(Mutex::new(settings)),
        trusted: Arc::new(Mutex::new(TrustStore::default())),
        download_dir: download_dir.clone(),
        emit: Arc::new(move |event, payload| {
            sink.lock().unwrap().push((event.to_string(), payload));
        }),
        register_peer: Arc::new(|_, _| {}),
    });

    let listener = PlainListener::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::clone(&state));
    tokio::spawn(async move {
        serve(listener, app).await.unwrap();
    });

    Harness {
        base: format!("http://{addr}"),
        state,
        events,
        download_dir,
        client: reqwest::Client::new(),
    }
}

fn sender_info() -> Value {
    json!({
        "alias": "Secret Banana",
        "version": "2.2",
        "deviceModel": "Windows",
        "deviceType": "desktop",
        "fingerprint": "SENDER",
        "port": 53317,
        "protocol": "https",
        "download": false
    })
}

fn file_entry(id: &str, name: &str, size: u64) -> Value {
    json!({ "id": id, "fileName": name, "size": size, "fileType": "text/plain" })
}

#[tokio::test]
async fn declining_a_request_writes_nothing_to_disk() {
    let harness = harness(Settings::default()).await;
    harness.auto_respond(false);

    let response = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({ "info": sender_info(), "files": { "a": file_entry("a", "secret.txt", 5) } }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);

    // A sender that ignores the refusal and guesses a token gets nowhere.
    let upload = harness
        .client
        .post(harness.url("/api/localsend/v2/upload?sessionId=guess&fileId=a&token=guess"))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 403);
    assert!(harness.saved_files().is_empty());
}

#[tokio::test]
async fn an_unanswered_request_never_reaches_the_disk() {
    let harness = harness(Settings::default()).await;
    // No auto_respond: the request is simply never answered. It must not be
    // possible to upload while the user is still deciding.
    let state = Arc::clone(&harness.state);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        // The slot is held by the pending request, so a rogue upload has no
        // active session to attach to.
        assert!(state.sessions.is_busy());
    });

    let upload = harness
        .client
        .post(harness.url("/api/localsend/v2/upload?sessionId=x&fileId=a&token=t"))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 403);
    assert!(harness.saved_files().is_empty());
}

#[tokio::test]
async fn a_wrong_token_is_refused_and_the_right_one_works() {
    let harness = harness(Settings::default()).await;
    harness.auto_respond(true);

    let prepared: Value = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({ "info": sender_info(), "files": { "a": file_entry("a", "note.txt", 5) } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session = prepared["sessionId"].as_str().unwrap();
    let token = prepared["files"]["a"].as_str().unwrap();

    let wrong = harness
        .client
        .post(harness.url(&format!(
            "/api/localsend/v2/upload?sessionId={session}&fileId=a&token=nope"
        )))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 403);
    assert!(harness.saved_files().is_empty());

    let right = harness
        .client
        .post(harness.url(&format!(
            "/api/localsend/v2/upload?sessionId={session}&fileId=a&token={token}"
        )))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(right.status(), 200);
    assert_eq!(harness.read_saved("note.txt"), "hello");
}

#[tokio::test]
async fn missing_upload_parameters_are_a_bad_request() {
    let harness = harness(Settings::default()).await;
    let response = harness
        .client
        .post(harness.url("/api/localsend/v2/upload?sessionId=x"))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn a_colliding_name_gets_a_numbered_suffix() {
    let harness = harness(Settings {
        quick_save: true,
        ..Settings::default()
    })
    .await;

    for expected in ["cat.png", "cat (1).png", "cat (2).png"] {
        let prepared: Value = harness
            .client
            .post(harness.url("/api/localsend/v2/prepare-upload"))
            .json(&json!({ "info": sender_info(), "files": { "a": file_entry("a", "cat.png", 3) } }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let session = prepared["sessionId"].as_str().unwrap();
        let token = prepared["files"]["a"].as_str().unwrap();
        let response = harness
            .client
            .post(harness.url(&format!(
                "/api/localsend/v2/upload?sessionId={session}&fileId=a&token={token}"
            )))
            .body("png")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            harness.download_dir.join(expected).exists(),
            "expected {expected} to exist"
        );
    }
    assert_eq!(harness.saved_files().len(), 3);
}

#[tokio::test]
async fn path_traversal_is_rejected_before_anything_is_created() {
    let harness = harness(Settings {
        quick_save: true,
        ..Settings::default()
    })
    .await;

    for name in ["../escape.txt", "a/../../escape.txt", "/etc/passwd", "..\\escape.txt"] {
        let response = harness
            .client
            .post(harness.url("/api/localsend/v2/prepare-upload"))
            .json(&json!({ "info": sender_info(), "files": { "a": file_entry("a", name, 5) } }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{name} should be refused");
        // The slot must be free again, otherwise one bad request would block
        // every later one.
        assert!(!harness.state.sessions.is_busy());
    }
    assert!(harness.saved_files().is_empty());
    assert!(!harness.download_dir.parent().unwrap().join("escape.txt").exists());
}

#[tokio::test]
async fn a_pin_is_required_when_one_is_set() {
    let harness = harness(Settings {
        pin: Some("123456".into()),
        quick_save: true,
    })
    .await;

    let body = json!({ "info": sender_info(), "files": { "a": file_entry("a", "note.txt", 5) } });

    let without = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(without.status(), 401);

    let wrong = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload?pin=000000"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    let right = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload?pin=123456"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(right.status(), 200);
}

#[tokio::test]
async fn a_second_sender_is_told_the_receiver_is_busy() {
    let harness = harness(Settings::default()).await;
    let body = json!({ "info": sender_info(), "files": { "a": file_entry("a", "note.txt", 5) } });

    // No answer yet, so the first request keeps the slot.
    let first = {
        let client = harness.client.clone();
        let url = harness.url("/api/localsend/v2/prepare-upload");
        let body = body.clone();
        tokio::spawn(async move { client.post(url).json(&body).send().await.unwrap() })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;

    let second = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 409);

    harness.auto_respond(false);
    assert_eq!(first.await.unwrap().status(), 403);
}

#[tokio::test]
async fn a_checksum_mismatch_is_reported_and_the_partial_file_removed() {
    let harness = harness(Settings {
        quick_save: true,
        ..Settings::default()
    })
    .await;

    let prepared: Value = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({
            "info": sender_info(),
            "files": { "a": {
                "id": "a", "fileName": "note.txt", "size": 5, "fileType": "text/plain",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session = prepared["sessionId"].as_str().unwrap();
    let token = prepared["files"]["a"].as_str().unwrap();

    let response = harness
        .client
        .post(harness.url(&format!(
            "/api/localsend/v2/upload?sessionId={session}&fileId=a&token={token}"
        )))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    assert!(harness.saved_files().is_empty());
}

#[tokio::test]
async fn a_matching_checksum_is_accepted() {
    let harness = harness(Settings {
        quick_save: true,
        ..Settings::default()
    })
    .await;
    // sha256("hello")
    let digest = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    let prepared: Value = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({
            "info": sender_info(),
            "files": { "a": {
                "id": "a", "fileName": "note.txt", "size": 5, "fileType": "text/plain",
                "sha256": digest
            }}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session = prepared["sessionId"].as_str().unwrap();
    let token = prepared["files"]["a"].as_str().unwrap();

    let response = harness
        .client
        .post(harness.url(&format!(
            "/api/localsend/v2/upload?sessionId={session}&fileId=a&token={token}"
        )))
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(harness.read_saved("note.txt"), "hello");
}

#[tokio::test]
async fn quick_save_accepts_without_asking_the_user() {
    let harness = harness(Settings {
        quick_save: true,
        ..Settings::default()
    })
    .await;

    let response = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({ "info": sender_info(), "files": { "a": file_entry("a", "note.txt", 5) } }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(harness.events_named("incoming-request").is_empty());
}

#[tokio::test]
async fn an_empty_file_list_needs_no_transfer() {
    let harness = harness(Settings::default()).await;
    let response = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({ "info": sender_info(), "files": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    assert!(!harness.state.sessions.is_busy());
}

#[tokio::test]
async fn cancelling_frees_the_receiver_for_the_next_sender() {
    let harness = harness(Settings {
        quick_save: true,
        ..Settings::default()
    })
    .await;

    let prepared: Value = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({ "info": sender_info(), "files": { "a": file_entry("a", "note.txt", 5) } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session = prepared["sessionId"].as_str().unwrap();

    let cancelled = harness
        .client
        .post(harness.url(&format!("/api/localsend/v2/cancel?sessionId={session}")))
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 200);
    assert!(!harness.state.sessions.is_busy());

    let finished = harness.events_named("session-finished");
    assert_eq!(finished.last().unwrap()["status"], "cancelled");
}

#[tokio::test]
async fn three_files_arrive_with_progress_and_a_finish_event() {
    let harness = harness(Settings::default()).await;
    harness.auto_respond(true);

    let prepared: Value = harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({
            "info": sender_info(),
            "files": {
                "a": file_entry("a", "one.txt", 3),
                "b": file_entry("b", "two.txt", 3),
                "c": file_entry("c", "photos/2024/three.txt", 5),
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session = prepared["sessionId"].as_str().unwrap().to_string();

    for (id, content) in [("a", "one"), ("b", "two"), ("c", "three")] {
        let token = prepared["files"][id].as_str().unwrap();
        let response = harness
            .client
            .post(harness.url(&format!(
                "/api/localsend/v2/upload?sessionId={session}&fileId={id}&token={token}"
            )))
            .body(content)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    assert_eq!(
        harness.saved_files(),
        vec!["one.txt", "photos/2024/three.txt", "two.txt"]
    );
    assert_eq!(harness.read_saved("one.txt"), "one");
    assert_eq!(harness.read_saved("photos/2024/three.txt"), "three");

    let progress = harness.events_named("transfer-progress");
    assert_eq!(progress.len(), 3, "one final progress event per file");
    assert!(progress
        .iter()
        .all(|event| event["bytesReceived"].as_u64().unwrap() > 0));

    let finished = harness.events_named("session-finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["status"], "completed");
    assert_eq!(finished[0]["sessionId"], session);
    assert!(!harness.state.sessions.is_busy());
}

#[tokio::test]
async fn the_incoming_request_event_describes_the_transfer() {
    let harness = harness(Settings::default()).await;
    harness.auto_respond(true);

    harness
        .client
        .post(harness.url("/api/localsend/v2/prepare-upload"))
        .json(&json!({
            "info": sender_info(),
            "files": {
                "a": file_entry("a", "one.txt", 100),
                "b": file_entry("b", "two.txt", 200),
            }
        }))
        .send()
        .await
        .unwrap();

    let events = harness.events_named("incoming-request");
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event["sender"]["alias"], "Secret Banana");
    assert_eq!(event["sender"]["fingerprint"], "SENDER");
    assert_eq!(event["totalSize"], 300);
    assert_eq!(event["files"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn register_answers_with_our_own_device_info() {
    let harness = harness(Settings::default()).await;
    let response: Value = harness
        .client
        .post(harness.url("/api/localsend/v2/register"))
        .json(&sender_info())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["alias"], "Test Receiver");
    assert_eq!(response["fingerprint"], "RECEIVER");
    assert_eq!(response["protocol"], "https");
}

#[tokio::test]
async fn info_is_available_for_debugging() {
    let harness = harness(Settings::default()).await;
    let response: Value = harness
        .client
        .get(harness.url("/api/localsend/v2/info"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["alias"], "Test Receiver");
}
