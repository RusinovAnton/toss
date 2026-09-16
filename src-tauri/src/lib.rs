//! Toss — LocalSend-compatible desktop client.
//!
//! All network and filesystem I/O lives here in Rust. The frontend only
//! calls Tauri commands and listens to events.

pub mod discovery;
pub mod identity;
pub mod protocol;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Emitted with the full device list whenever it changes.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

pub struct AppState {
    pub identity: Mutex<identity::Identity>,
    pub discovery: Arc<discovery::Discovery>,
}

/// Alias, fingerprint, device model/type and port of this device.
#[tauri::command]
fn get_identity(state: tauri::State<'_, AppState>) -> Result<identity::IdentityInfo, String> {
    let identity = state.identity.lock().map_err(|e| e.to_string())?;
    Ok(identity.info())
}

/// Currently known peers, newest state first seen last. Also pushed as
/// `devices-changed` whenever it changes.
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let identity = identity::Identity::load_or_create(&dir)?;

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

            app.manage(AppState {
                identity: Mutex::new(identity),
                discovery,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_identity, list_devices, rescan])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
