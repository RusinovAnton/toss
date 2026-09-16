//! Toss — LocalSend-compatible desktop client.
//!
//! All network and filesystem I/O lives here in Rust. The frontend only
//! calls Tauri commands and listens to events.

pub mod clipboard;
pub mod discovery;
pub mod files;
pub mod identity;
pub mod protocol;
pub mod send;
pub mod server;
pub mod session;
pub mod tls;
pub mod tray;
pub mod settings;
pub mod trust;
pub mod window;

use protocol::SharedInfo;
use session::Decision;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::collections::HashSet;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
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
    pub clipboard: Arc<clipboard::ClipboardSync>,
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
    trust_sender: Option<bool>,
) -> Result<(), String> {
    // Trusting from the card is how a device earns "no more questions", so
    // it is granted against the fingerprint the handshake proved, not the one
    // the sender wrote in its request.
    if trust_sender.unwrap_or(false) {
        if let Some(sender) = state.sessions.pending_sender(&session_id) {
            match sender.verified_fingerprint {
                Some(fingerprint) => {
                    let mut trusted = state.trusted.lock().map_err(|e| e.to_string())?;
                    trusted.trust(&fingerprint, &sender.alias);
                    trusted.save(&state.config_dir).map_err(|e| e.to_string())?;
                }
                // Without a certificate there is nothing to remember, and
                // trusting the claim would trust anyone who copies it.
                None => eprintln!(
                    "not trusting {}: it presented no certificate",
                    sender.alias
                ),
            }
        }
    }
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    settings.save(&state.config_dir).map_err(|e| e.to_string())?;
    let mut current = state.settings.lock().map_err(|e| e.to_string())?;
    *current = settings;
    let updated = current.clone();
    drop(current);
    set_login_item(&app, updated.start_at_login);
    Ok(updated)
}

/// Brings the login item in line with the setting.
///
/// The two can drift: the user may remove Toss from their login items in
/// System Settings, and the app should not quietly put it back except when
/// asked.
fn set_login_item(app: &tauri::AppHandle, wanted: bool) {
    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    if current == wanted {
        return;
    }
    let result = if wanted {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(e) = result {
        eprintln!("could not change the login item: {e}");
    }
}


/// The devices this one is paired with.
#[tauri::command]
fn list_trusted(state: tauri::State<'_, AppState>) -> Result<Vec<trust::TrustedDevice>, String> {
    Ok(state.trusted.lock().map_err(|e| e.to_string())?.list())
}

/// Trusts a discovered device, so its transfers are accepted without asking.
///
/// The fingerprint is pinned: from now on that device has to present the same
/// certificate, and one that does not is refused rather than trusted.
#[tauri::command]
fn trust_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> Result<trust::TrustedDevice, String> {
    let (fingerprint, alias) = known_device(&state, &device_id)?;
    let mut trusted = state.trusted.lock().map_err(|e| e.to_string())?;
    let entry = trusted
        .trust(&fingerprint, &alias)
        .ok_or_else(|| "That device has no fingerprint to trust".to_string())?;
    trusted.save(&state.config_dir).map_err(|e| e.to_string())?;
    Ok(entry)
}

/// Pairs a device, which trusts it and lets clipboard text flow when the
/// shared clipboard is on. `paired: false` steps it back to merely trusted.
#[tauri::command]
fn pair_device(
    state: tauri::State<'_, AppState>,
    device_id: String,
    paired: bool,
) -> Result<trust::TrustedDevice, String> {
    let (fingerprint, alias) = known_device(&state, &device_id)?;
    let mut trusted = state.trusted.lock().map_err(|e| e.to_string())?;
    let entry = trusted
        .set_paired(&fingerprint, &alias, paired)
        .ok_or_else(|| "That device has no fingerprint to pair with".to_string())?;
    trusted.save(&state.config_dir).map_err(|e| e.to_string())?;
    Ok(entry)
}

/// Forgets a device: no longer trusted, no longer paired.
#[tauri::command]
fn forget_device(state: tauri::State<'_, AppState>, device_id: String) -> Result<bool, String> {
    let mut trusted = state.trusted.lock().map_err(|e| e.to_string())?;
    let removed = trusted.forget(&device_id);
    if removed {
        trusted.save(&state.config_dir).map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// The fingerprint and name of a device, from the radar or from what is
/// already known about it. A device that has gone quiet can still be
/// unpaired.
fn known_device(
    state: &tauri::State<'_, AppState>,
    device_id: &str,
) -> Result<(String, String), String> {
    if let Some(device) = state.discovery.registry.find(device_id) {
        return Ok((device.fingerprint, device.alias));
    }
    let trusted = state.trusted.lock().map_err(|e| e.to_string())?;
    trusted
        .get(device_id)
        .map(|known| (known.fingerprint.clone(), known.alias.clone()))
        .ok_or_else(|| "That device is no longer around".to_string())
}

/// Sends whatever is on the clipboard to a device, by hand.
///
/// Clipboard sync does this by itself for paired devices; this is for the
/// one-off case and for devices that are not paired.
#[tauri::command]
async fn send_clipboard(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    device_id: String,
) -> Result<send::SendSummary, send::SendErrorPayload> {
    let text = app.clipboard().read_text().unwrap_or_default();
    if text.trim().is_empty() {
        return Err(send::SendErrorPayload {
            code: "empty-clipboard".into(),
            message: "The clipboard is empty".into(),
        });
    }
    // Ours already, so it does not come back as a change to share.
    state.clipboard.remember(&text);
    send_text_to(&state, device_id, text, None).await
}

/// Sends text to a paired device, which lands on its clipboard.
#[tauri::command]
async fn send_text(
    state: tauri::State<'_, AppState>,
    device_id: String,
    text: String,
    pin: Option<String>,
) -> Result<send::SendSummary, send::SendErrorPayload> {
    send_text_to(&state, device_id, text, pin).await
}

async fn send_text_to(
    state: &tauri::State<'_, AppState>,
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
        // Launched at login it starts in the menu bar, not on screen.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![tray::HIDDEN_ARG]),
        ))
        .setup(|app| {
            let config_dir = app.path().app_data_dir()?;
            let identity = identity::Identity::load_or_create(&config_dir)?;
            let settings = Arc::new(Mutex::new(settings::Settings::load(&config_dir)));
            let trusted = Arc::new(Mutex::new(trust::TrustStore::load(&config_dir)));
            let clipboard = Arc::new(clipboard::ClipboardSync::new());
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
            let incoming_clipboard = Arc::clone(&clipboard);
            let server_state = Arc::new(server::ServerState {
                info: Arc::clone(&info),
                sessions: Arc::clone(&sessions),
                settings: Arc::clone(&settings),
                trusted: Arc::clone(&trusted),
                download_dir: default_download_dir(),
                emit: Arc::new(move |event, payload| {
                    // Text from another device goes onto the clipboard here
                    // rather than in the frontend, so the sync loop records
                    // it as ours and does not bounce it back.
                    if event == server::TEXT_RECEIVED_EVENT {
                        if let Some(text) = payload["text"].as_str() {
                            incoming_clipboard.remember(text);
                            if let Err(e) = handle.clipboard().write_text(text.to_string()) {
                                eprintln!("could not write the clipboard: {e}");
                            }
                        }
                    }
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
            // Read before the settings move into the managed state.
            let login_wanted = settings
                .lock()
                .map(|s| s.start_at_login)
                .unwrap_or(false);

            spawn_clipboard_sync(
                app.handle().clone(),
                Arc::clone(&clipboard),
                Arc::clone(&settings),
                Arc::clone(&trusted),
                Arc::clone(&discovery),
                Arc::clone(&sender),
            );

            app.manage(AppState {
                identity: Mutex::new(identity),
                info,
                discovery,
                sessions,
                sender,
                settings,
                trusted,
                clipboard,
                config_dir,
                geometry: Mutex::new(GeometryState {
                    current: stored.unwrap_or_default(),
                    // Far enough in the past that the first move is written.
                    last_write: Instant::now() - window::SAVE_INTERVAL,
                }),
            });

            tray::build(app.handle())?;
            set_login_item(app.handle(), login_wanted);

            if let Some(main) = app.get_webview_window("main") {
                if !tray::should_show_window(std::env::args()) {
                    let _ = main.hide();
                }
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
            send_clipboard,
            cancel_send,
            list_trusted,
            trust_device,
            pair_device,
            forget_device,
            get_settings,
            set_settings,
            download_dir,
            show_in_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Watches the clipboard and pushes changes to paired devices.
///
/// Off unless the user turns it on: pairing a device should not, by itself,
/// start a copy of everything they copy leaving the machine.
fn spawn_clipboard_sync(
    app: tauri::AppHandle,
    clipboard: Arc<clipboard::ClipboardSync>,
    settings: Arc<Mutex<settings::Settings>>,
    trusted: Arc<Mutex<trust::TrustStore>>,
    discovery: Arc<discovery::Discovery>,
    sender: Arc<send::SendManager>,
) {
    // Devices with a clipboard already on its way, so a slow one does not
    // collect a queue of stale copies.
    let in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(clipboard::POLL_INTERVAL);
        loop {
            ticker.tick().await;

            let enabled = settings
                .lock()
                .map(|s| s.clipboard_sync)
                .unwrap_or(false);
            if !enabled {
                // Whatever was copied while off is not sent when it comes
                // back on; only what happens next is.
                clipboard.reset();
                continue;
            }

            let paired: Vec<discovery::Device> = {
                let store = match trusted.lock() {
                    Ok(store) => store,
                    Err(_) => continue,
                };
                discovery
                    .registry
                    .list()
                    .into_iter()
                    // Trusted is not enough: the clipboard is what pairing is
                    // for.
                    .filter(|device| store.is_paired(&device.fingerprint))
                    .collect()
            };
            if paired.is_empty() {
                continue;
            }

            let current = app.clipboard().read_text().ok();
            let Some(text) = clipboard.take_change(current) else {
                continue;
            };

            for device in paired {
                {
                    let mut busy = in_flight.lock().expect("clipboard senders poisoned");
                    if !busy.insert(device.fingerprint.clone()) {
                        continue;
                    }
                }
                let sender = Arc::clone(&sender);
                let in_flight = Arc::clone(&in_flight);
                let text = text.clone();
                tauri::async_runtime::spawn(async move {
                    let target = send::Target {
                        fingerprint: device.fingerprint.clone(),
                        ip: device.ip.clone(),
                        port: device.port,
                        protocol: device.protocol,
                    };
                    if let Err(e) = sender.send_text(target, &text, None).await {
                        eprintln!("clipboard to {}: {e}", device.alias);
                    }
                    in_flight
                        .lock()
                        .expect("clipboard senders poisoned")
                        .remove(&device.fingerprint);
                });
            }
        }
    });
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
            // Closing puts Toss in the menu bar rather than quitting it, so
            // files and clipboard text still arrive. Quit lives in the tray
            // menu.
            WindowEvent::CloseRequested { api, .. } => {
                if let Ok(mut geometry) = state.geometry.lock() {
                    save_geometry(&mut geometry, &state.config_dir, true);
                }
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Destroyed => {
                if let Ok(mut geometry) = state.geometry.lock() {
                    save_geometry(&mut geometry, &state.config_dir, true);
                }
            }
            _ => {}
        }
    });
}
