//! A live send against a real peer on the network. Ignored by default.
//!
//! ```sh
//! TOSS_TARGET=192.168.1.5:53317 TOSS_SEND=/path/to/folder \
//!   cargo test --test live_send -- --ignored --nocapture
//! ```
//!
//! Point it at the official LocalSend app to check wire compatibility by
//! hand; the peer's user has to accept the transfer for it to finish.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use toss_lib::identity::Identity;
use toss_lib::protocol::ProtocolType;
use toss_lib::send::{SendManager, Target};

#[tokio::test]
#[ignore = "needs a peer on the network"]
async fn send_to_a_real_peer() {
    let target = std::env::var("TOSS_TARGET").expect("set TOSS_TARGET=ip:port");
    let path = PathBuf::from(std::env::var("TOSS_SEND").expect("set TOSS_SEND=path"));
    let (ip, port) = target.split_once(':').expect("TOSS_TARGET must be ip:port");

    let identity = Identity::generate().unwrap();
    let events = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&events);
    let sender = SendManager::new(
        Arc::new(Mutex::new(identity.to_device_info())),
        &identity.certificate_pem,
        &identity.private_key_pem,
        Arc::new(move |event, payload| {
            if event == "transfer-progress" {
                *counter.lock().unwrap() += 1;
            } else {
                println!("event {event}: {payload}");
            }
        }),
        Arc::new(Mutex::new(toss_lib::trust::TrustStore::default())),
    )
    .unwrap();

    let target = Target {
        fingerprint: std::env::var("TOSS_PEER").unwrap_or_else(|_| "unknown".into()),
        ip: ip.to_string(),
        port: port.parse().expect("port must be a number"),
        protocol: ProtocolType::Https,
    };

    println!("sending {} to {}:{}", path.display(), target.ip, target.port);
    match sender.send(target, &[path], std::env::var("TOSS_PIN").ok()).await {
        Ok(summary) => println!(
            "sent {} file(s), {} bytes, session {} ({} progress events)",
            summary.files_sent,
            summary.bytes_sent,
            summary.session_id,
            events.lock().unwrap()
        ),
        Err(e) => panic!("send failed: {} ({})", e, e.code()),
    }
}
