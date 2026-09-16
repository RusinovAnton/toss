//! Toss — LocalSend-compatible desktop client.
//!
//! All network and filesystem I/O lives here in Rust. The frontend only
//! calls Tauri commands and listens to events.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_harness_works() {
        assert_eq!(2 + 2, 4);
    }
}
