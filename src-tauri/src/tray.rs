//! Living in the menu bar.
//!
//! Closing the window puts Toss away rather than quitting it: discovery and
//! the receive server keep running, so files and clipboard text still arrive.
//! The tray icon is how you get the window back, and the only way to actually
//! quit.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// The argument the login item passes, so starting with the machine does not
/// throw a window in your face.
pub const HIDDEN_ARG: &str = "--hidden";

/// Whether the window should be shown at startup.
///
/// Launched by hand: yes. Launched at login: no, it waits in the menu bar.
pub fn should_show_window<I: IntoIterator<Item = String>>(args: I) -> bool {
    !args.into_iter().any(|arg| arg == HIDDEN_ARG)
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Toss", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Toss", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;

    TrayIconBuilder::with_id("toss")
        .icon(icon)
        // A stencil, so macOS can recolour it for light and dark menu bars.
        .icon_as_template(true)
        .tooltip("Toss")
        .menu(&menu)
        // Left click reopens the window; the menu is on right click, which is
        // what people expect of a menu bar app.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Brings the window back, wherever it was left.
pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn a_normal_launch_shows_the_window() {
        assert!(should_show_window(args(&["toss"])));
        assert!(should_show_window(args(&[])));
    }

    #[test]
    fn a_login_launch_stays_in_the_menu_bar() {
        assert!(!should_show_window(args(&["toss", HIDDEN_ARG])));
        assert!(!should_show_window(args(&[HIDDEN_ARG])));
    }

    #[test]
    fn other_arguments_are_ignored() {
        assert!(should_show_window(args(&["toss", "--hide", "-h"])));
    }
}
