//! Keeping one clipboard in step with another.
//!
//! The behaviour people expect from Handoff: copy on one machine, paste on
//! the other, with nothing in between. That means watching the clipboard and
//! pushing changes to paired devices, which needs three things handled
//! carefully:
//!
//! - text we just received must not be sent straight back, or two devices
//!   bounce the same string forever;
//! - only changes go out, not the same clipboard over and over;
//! - something enormous on the clipboard is not worth copying across a
//!   network, so there is a ceiling.

use std::sync::Mutex;
use std::time::Duration;

/// How often the clipboard is looked at. Fast enough to feel immediate,
/// slow enough to be free.
pub const POLL_INTERVAL: Duration = Duration::from_millis(700);
/// Clipboards above this are left alone. Sending a novel to another machine
/// on every copy is not what anyone means by seamless.
pub const MAX_SYNC_BYTES: usize = 64 * 1024;

/// Watches for clipboard changes worth sharing.
#[derive(Default)]
pub struct ClipboardSync {
    last_seen: Mutex<Option<String>>,
}

impl ClipboardSync {
    pub fn new() -> Self {
        ClipboardSync::default()
    }

    /// The text to share, if the clipboard has changed into something worth
    /// sharing. Anything returned here is remembered, so it goes out once.
    pub fn take_change(&self, current: Option<String>) -> Option<String> {
        let text = current?;
        if text.is_empty() || text.len() > MAX_SYNC_BYTES {
            // Still remembered, so an oversized clipboard is not re-examined
            // on every tick.
            self.remember(&text);
            return None;
        }
        let mut last = self.last_seen.lock().expect("clipboard state poisoned");
        if last.as_deref() == Some(text.as_str()) {
            return None;
        }
        *last = Some(text.clone());
        Some(text)
    }

    /// Notes text as already known, so it is not sent out.
    ///
    /// Called for anything arriving from another device, which is what stops
    /// the two of them echoing.
    pub fn remember(&self, text: &str) {
        *self.last_seen.lock().expect("clipboard state poisoned") = Some(text.to_string());
    }

    /// Forgets the last clipboard, so the next one counts as a change.
    pub fn reset(&self) {
        *self.last_seen.lock().expect("clipboard state poisoned") = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_clipboard_is_a_change() {
        let sync = ClipboardSync::new();
        assert_eq!(sync.take_change(Some("hello".into())).as_deref(), Some("hello"));
    }

    #[test]
    fn the_same_clipboard_is_only_sent_once() {
        let sync = ClipboardSync::new();
        sync.take_change(Some("hello".into()));
        assert_eq!(sync.take_change(Some("hello".into())), None);
        assert_eq!(sync.take_change(Some("hello".into())), None);
    }

    #[test]
    fn a_new_clipboard_is_sent_again() {
        let sync = ClipboardSync::new();
        sync.take_change(Some("one".into()));
        assert_eq!(sync.take_change(Some("two".into())).as_deref(), Some("two"));
        // And back again, because it really did change.
        assert_eq!(sync.take_change(Some("one".into())).as_deref(), Some("one"));
    }

    #[test]
    fn received_text_is_never_sent_back() {
        let sync = ClipboardSync::new();
        // Another device sent this and it was put on our clipboard.
        sync.remember("from the other machine");
        assert_eq!(sync.take_change(Some("from the other machine".into())), None);
    }

    #[test]
    fn an_empty_or_missing_clipboard_is_not_a_change() {
        let sync = ClipboardSync::new();
        assert_eq!(sync.take_change(None), None);
        assert_eq!(sync.take_change(Some(String::new())), None);
    }

    #[test]
    fn an_enormous_clipboard_is_left_alone() {
        let sync = ClipboardSync::new();
        let huge = "x".repeat(MAX_SYNC_BYTES + 1);
        assert_eq!(sync.take_change(Some(huge.clone())), None);
        // And it is not reconsidered on the next tick either.
        assert_eq!(sync.take_change(Some(huge)), None);
        // Something ordinary after it still goes.
        assert_eq!(sync.take_change(Some("small".into())).as_deref(), Some("small"));
    }

    #[test]
    fn something_exactly_at_the_ceiling_still_goes() {
        let sync = ClipboardSync::new();
        let big = "x".repeat(MAX_SYNC_BYTES);
        assert_eq!(sync.take_change(Some(big.clone())), Some(big));
    }

    #[test]
    fn resetting_makes_the_next_clipboard_count_again() {
        let sync = ClipboardSync::new();
        sync.take_change(Some("hello".into()));
        sync.reset();
        assert_eq!(sync.take_change(Some("hello".into())).as_deref(), Some("hello"));
    }
}
