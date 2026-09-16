//! Toss — LocalSend-compatible desktop client.
//!
//! All network and filesystem I/O lives here in Rust. The frontend only
//! calls Tauri commands and listens to events.

pub mod discovery;
pub mod files;
pub mod identity;
pub mod protocol;
pub mod server;
pub mod session;
pub mod settings;

use session::Decision;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Emitted with the full device list whenever it changes.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

pub struct AppState {
    pub identity: Mutex<identity::Identity>,
    pub discovery: Arc<discovery::Discovery>,
    pub sessions: Arc<session::SessionManager>,
    pub settings: Arc<Mutex<settings::Settings>>,
    pub config_dir: PathBuf,
}

/// Alias, fingerprint, device model/type and port of this device.
#[tauri::command]
fn get_identity(state: tauri::State<'_, AppState>) -> Result<identity::IdentityInfo, String> {
    let identity = state.identity.lock().map_err(|e| e.to_string())?;
    Ok(identity.info())
}

/// Currently known peers. Also pushed as `devices-changed` whenever it changes.
#[tauri::command]
fn list_devices(state: tauri::State<'_, AppState>) -> Vec<discovery::Device> {
    state.discovery.registry.list()
}

/// Re-announces and scans the local /24. Returns once the scan is done.
#[tauri::command]
async fn rescan(state: tauri::State<'_, AppState>) -> Result<Vec<discovery::Device>, String> {
    let discovery = Arc::clone(&state.discovery);
    discovery.rescan().await;
    Ok(discovery.registry.list())
}

/// Answers an `incoming-request`. An empty `accepted_file_ids` declines.
#[tauri::command]
fn respond_to_request(
    state: tauri::State<'_, AppState>,
    session_id: String,
    accepted_file_ids: Vec<String>,
) -> Result<(), String> {
    let decision = if accepted_file_ids.is_empty() {
        Decision::Decline
    } else {
        Decision::Accept(accepted_file_ids)
    };
    state.sessions.respond(&session_id, decision)
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> Result<settings::Settings, String> {
    Ok(state.settings.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
fn set_settings(
    state: tauri::State<'_, AppState>,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    settings.save(&state.config_dir).map_err(|e| e.to_string())?;
    let mut current = state.settings.lock().map_err(|e| e.to_string())?;
    *current = settings;
    Ok(current.clone())
}

/// Where received files are written. Not configurable in v1.
#[tauri::command]
fn download_dir() -> String {
    default_download_dir().to_string_lossy().to_string()
}

fn default_download_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads")
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_data_dir()?;
            let identity = identity::Identity::load_or_create(&config_dir)?;
            let settings = Arc::new(Mutex::new(settings::Settings::load(&config_dir)));
            let sessions = Arc::new(session::SessionManager::new());

            let handle = app.handle().clone();
            let notify = Arc::new(move |devices| {
                if let Err(e) = handle.emit(DEVICES_CHANGED_EVENT, devices) {
                    eprintln!("failed to emit {DEVICES_CHANGED_EVENT}: {e}");
                }
            });
            // `setup` runs on the main thread, outside the async runtime, and
            // binding the multicast socket needs the reactor.
            let discovery = tauri::async_runtime::block_on(discovery::Discovery::new(
                identity.to_device_info(),
                &identity.certificate_pem,
                &identity.private_key_pem,
                notify,
            ))?;
            discovery.start();

            let handle = app.handle().clone();
            let peers = Arc::clone(&discovery);
            let server_state = Arc::new(server::ServerState {
                info: identity.to_device_info(),
                sessions: Arc::clone(&sessions),
                settings: Arc::clone(&settings),
                download_dir: default_download_dir(),
                emit: Arc::new(move |event, payload| {
                    if let Err(e) = handle.emit(event, payload) {
                        eprintln!("failed to emit {event}: {e}");
                    }
                }),
                register_peer: Arc::new(move |info, ip| peers.register_peer(info, ip)),
            });

            let cert = identity.certificate_pem.clone();
            let key = identity.private_key_pem.clone();
            tauri::async_runtime::spawn(async move {
                // A busy port is not fatal: discovery and sending still work,
                // we just cannot receive until it frees up.
                if let Err(e) = server::start(server_state, &cert, &key).await {
                    eprintln!("receive server not started: {e}");
                }
            });

            app.manage(AppState {
                identity: Mutex::new(identity),
                discovery,
                sessions,
                settings,
                config_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_identity,
            list_devices,
            rescan,
            respond_to_request,
            get_settings,
            set_settings,
            download_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
