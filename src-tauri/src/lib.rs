//! Toss — LocalSend-compatible desktop client.
//!
//! All network and filesystem I/O lives here in Rust. The frontend only
//! calls Tauri commands and listens to events.

pub mod identity;

use std::sync::Mutex;
use tauri::Manager;

pub struct AppState {
    pub identity: Mutex<identity::Identity>,
}

/// Alias, fingerprint, device model/type and port of this device.
#[tauri::command]
fn get_identity(state: tauri::State<'_, AppState>) -> Result<identity::IdentityInfo, String> {
    let identity = state.identity.lock().map_err(|e| e.to_string())?;
    Ok(identity.info())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let identity = identity::Identity::load_or_create(&dir)?;
            app.manage(AppState {
                identity: Mutex::new(identity),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_identity])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
