//! Toss — LocalSend-compatible desktop client.
//!
//! All network and filesystem I/O lives here in Rust. The frontend only
//! calls Tauri commands and listens to events.

pub mod discovery;
pub mod files;
pub mod identity;
pub mod protocol;
pub mod send;
pub mod server;
pub mod session;
pub mod tls;
pub mod settings;
pub mod trust;
pub mod window;

use protocol::SharedInfo;
use session::Decision;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_opener::OpenerExt;

/// Emitted with the full device list whenever it changes.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

pub struct AppState {
    pub identity: Mutex<identity::Identity>,
    pub info: SharedInfo,
    pub discovery: Arc<discovery::Discovery>,
    pub sessions: Arc<session::SessionManager>,
    pub sender: Arc<send::SendManager>,
    pub settings: Arc<Mutex<settings::Settings>>,
    pub trusted: Arc<Mutex<trust::TrustStore>>,
    pub config_dir: PathBuf,
    pub geometry: Mutex<GeometryState>,
}

/// The window's last known geometry, plus when it was last written out.
pub struct GeometryState {
    pub current: window::Geometry,
    pub last_write: Instant,
}

/// Alias, fingerprint, device model/type and port of this device.
#[tauri::command]
fn get_identity(state: tauri::State<'_, AppState>) -> Result<identity::IdentityInfo, String> {
    let identity = state.identity.lock().map_err(|e| e.to_string())?;
    Ok(identity.info())
}

/// Renames this device. Peers are told at once with an announce burst.
#[tauri::command]
async fn set_alias(
    state: tauri::State<'_, AppState>,
    alias: String,
) -> Result<identity::IdentityInfo, String> {
    let alias = alias.trim().to_string();
    if alias.is_empty() {
        return Err("The name cannot be empty".into());
    }
    let info = {
        let mut identity = state.identity.lock().map_err(|e| e.to_string())?;
        identity.alias = alias.clone();
        identity.save(&state.config_dir).map_err(|e| e.to_string())?;
        identity.info()
    };
    state
        .info
        .lock()
        .map_err(|e| e.to_string())?
        .alias
        .clone_from(&alias);

    let discovery = Arc::clone(&state.discovery);
    discovery.announce_burst().await;
    Ok(info)
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

/// Sends files or folders to a discovered device.
///
/// Resolves once every accepted file has been uploaded. A `pin-required`
/// error means the peer wants a PIN: ask the user and call again with it.
#[tauri::command]
async fn send_files(
    state: tauri::State<'_, AppState>,
    device_id: String,
    paths: Vec<String>,
    pin: Option<String>,
) -> Result<send::SendSummary, send::SendErrorPayload> {
    let device = state.discovery.registry.find(&device_id).ok_or_else(|| {
        send::SendErrorPayload {
            code: "unknown-device".into(),
            message: "That device is no longer around".into(),
        }
    })?;
    let target = send::Target {
        fingerprint: device.fingerprint,
        ip: device.ip,
        port: device.port,
        protocol: device.protocol,
    };
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let sender = Arc::clone(&state.sender);
    sender
        .send(target, &paths, pin)
        .await
        .map_err(send::SendErrorPayload::from)
}

/// Stops an in-flight send and tells the peer to drop the session.
#[tauri::command]
async fn cancel_send(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), send::SendErrorPayload> {
    let sender = Arc::clone(&state.sender);
    sender
        .cancel(&session_id)
        .await
        .map_err(send::SendErrorPayload::from)
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

/// The devices this one is paired with.
#[tauri::command]
fn list_trusted(state: tauri::State<'_, AppState>) -> Result<Vec<trust::TrustedDevice>, String> {
    Ok(state.trusted.lock().map_err(|e| e.to_string())?.list())
}

/// Pairs with a discovered device, so its requests are accepted without
/// asking and clipboard text can flow both ways.
///
/// The fingerprint is pinned: from now on that device has to present the same
/// certificate, and a device that does not is refused rather than trusted.
#[tauri::command]
fn trust_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> Result<trust::TrustedDevice, String> {
    let device = state
        .discovery
        .registry
        .find(&device_id)
        .ok_or_else(|| "That device is no longer around".to_string())?;
    let mut trusted = state.trusted.lock().map_err(|e| e.to_string())?;
    let entry = trusted
        .trust(&device.fingerprint, &device.alias)
        .ok_or_else(|| "That device has no fingerprint to pair with".to_string())?;
    trusted.save(&state.config_dir).map_err(|e| e.to_string())?;
    Ok(entry)
}

#[tauri::command]
fn untrust_device(state: tauri::State<'_, AppState>, device_id: String) -> Result<bool, String> {
    let mut trusted = state.trusted.lock().map_err(|e| e.to_string())?;
    let removed = trusted.untrust(&device_id);
    if removed {
        trusted.save(&state.config_dir).map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// Sends text to a paired device, which lands on its clipboard.
#[tauri::command]
async fn send_text(
    state: tauri::State<'_, AppState>,
    device_id: String,
    text: String,
    pin: Option<String>,
) -> Result<send::SendSummary, send::SendErrorPayload> {
    let device = state.discovery.registry.find(&device_id).ok_or_else(|| {
        send::SendErrorPayload {
            code: "unknown-device".into(),
            message: "That device is no longer around".into(),
        }
    })?;
    let target = send::Target {
        fingerprint: device.fingerprint,
        ip: device.ip,
        port: device.port,
        protocol: device.protocol,
    };
    let sender = Arc::clone(&state.sender);
    sender
        .send_text(target, &text, pin)
        .await
        .map_err(send::SendErrorPayload::from)
}

/// Where received files are written. Not configurable in v1.
#[tauri::command]
fn download_dir() -> String {
    default_download_dir().to_string_lossy().to_string()
}

/// Reveals a received file in Finder or Explorer.
#[tauri::command]
fn show_in_folder(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(PathBuf::from(path))
        .map_err(|e| e.to_string())
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config_dir = app.path().app_data_dir()?;
            let identity = identity::Identity::load_or_create(&config_dir)?;
            let settings = Arc::new(Mutex::new(settings::Settings::load(&config_dir)));
            let trusted = Arc::new(Mutex::new(trust::TrustStore::load(&config_dir)));
            let sessions = Arc::new(session::SessionManager::new());
            let info: SharedInfo = Arc::new(Mutex::new(identity.to_device_info()));

            let handle = app.handle().clone();
            let notify = Arc::new(move |devices| {
                if let Err(e) = handle.emit(DEVICES_CHANGED_EVENT, devices) {
                    eprintln!("failed to emit {DEVICES_CHANGED_EVENT}: {e}");
                }
            });
            // `setup` runs on the main thread, outside the async runtime, and
            // binding the multicast socket needs the reactor.
            let discovery = tauri::async_runtime::block_on(discovery::Discovery::new(
                Arc::clone(&info),
                &identity.certificate_pem,
                &identity.private_key_pem,
                notify,
            ))?;
            discovery.start();

            let handle = app.handle().clone();
            let peers = Arc::clone(&discovery);
            let server_state = Arc::new(server::ServerState {
                info: Arc::clone(&info),
                sessions: Arc::clone(&sessions),
                settings: Arc::clone(&settings),
                trusted: Arc::clone(&trusted),
                download_dir: default_download_dir(),
                emit: Arc::new(move |event, payload| {
                    if let Err(e) = handle.emit(event, payload) {
                        eprintln!("failed to emit {event}: {e}");
                    }
                }),
                register_peer: Arc::new(move |peer, ip| peers.register_peer(peer, ip)),
            });

            let handle = app.handle().clone();
            let sender = Arc::new(send::SendManager::new(
                Arc::clone(&info),
                &identity.certificate_pem,
                &identity.private_key_pem,
                Arc::new(move |event, payload| {
                    if let Err(e) = handle.emit(event, payload) {
                        eprintln!("failed to emit {event}: {e}");
                    }
                }),
                Arc::clone(&trusted),
            )?);

            let cert = identity.certificate_pem.clone();
            let key = identity.private_key_pem.clone();
            tauri::async_runtime::spawn(async move {
                // A busy port is not fatal: discovery and sending still work,
                // we just cannot receive until it frees up.
                if let Err(e) = server::start(server_state, &cert, &key).await {
                    eprintln!("receive server not started: {e}");
                }
            });

            let stored = window::Geometry::load(&config_dir);
            app.manage(AppState {
                identity: Mutex::new(identity),
                info,
                discovery,
                sessions,
                sender,
                settings,
                trusted,
                config_dir,
                geometry: Mutex::new(GeometryState {
                    current: stored.unwrap_or_default(),
                    // Far enough in the past that the first move is written.
                    last_write: Instant::now() - window::SAVE_INTERVAL,
                }),
            });

            if let Some(main) = app.get_webview_window("main") {
                if let Some(geometry) = stored {
                    let _ = main.set_size(tauri::PhysicalSize::new(geometry.side, geometry.side));
                    let _ = main.set_position(tauri::PhysicalPosition::new(geometry.x, geometry.y));
                }
                // Seed the remembered geometry from the window itself. A move
                // event carries no size, so without this the first save would
                // record a zero side and be thrown away on the next launch.
                if let (Ok(size), Ok(position)) = (main.inner_size(), main.outer_position()) {
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(mut geometry) = state.geometry.lock() {
                            geometry.current = window::Geometry {
                                x: position.x,
                                y: position.y,
                                side: size.width.max(size.height).max(window::MIN_SIDE),
                            };
                        }
                    }
                }
                watch_window(app.handle().clone(), main);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_identity,
            set_alias,
            list_devices,
            rescan,
            respond_to_request,
            send_files,
            send_text,
            cancel_send,
            list_trusted,
            trust_device,
            untrust_device,
            get_settings,
            set_settings,
            download_dir,
            show_in_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Writes the geometry out, at most every [`window::SAVE_INTERVAL`] unless
/// `force`. Dragging a window fires a stream of events, and a crash should
/// still leave a recent position behind, so neither extreme works alone.
fn save_geometry(state: &mut GeometryState, config_dir: &std::path::Path, force: bool) {
    if !force && state.last_write.elapsed() < window::SAVE_INTERVAL {
        return;
    }
    state.last_write = Instant::now();
    if let Err(e) = state.current.save(config_dir) {
        eprintln!("could not remember the window position: {e}");
    }
}

/// Keeps the window square and remembers where the user put it.
fn watch_window(app: tauri::AppHandle, main: tauri::WebviewWindow) {
    let window = main.clone();
    main.on_window_event(move |event| {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        match event {
            WindowEvent::Resized(size) => {
                if let Some(side) = window::square_side(size.width, size.height) {
                    let _ = window.set_size(tauri::PhysicalSize::new(side, side));
                }
                if let Ok(mut geometry) = state.geometry.lock() {
                    geometry.current.side = size.width.max(size.height).max(window::MIN_SIDE);
                    save_geometry(&mut geometry, &state.config_dir, false);
                }
            }
            WindowEvent::Moved(position) => {
                if let Ok(mut geometry) = state.geometry.lock() {
                    geometry.current.x = position.x;
                    geometry.current.y = position.y;
                    save_geometry(&mut geometry, &state.config_dir, false);
                }
            }
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
                if let Ok(mut geometry) = state.geometry.lock() {
                    save_geometry(&mut geometry, &state.config_dir, true);
                }
            }
            _ => {}
        }
    });
}
