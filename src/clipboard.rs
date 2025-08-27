use crate::render::NotifyMessage;
use arboard::Clipboard;

/// Copies the provided content to the system clipboard.
///
/// Returns a NotifyMessage indicating success or failure of the operation.
/// Handles both Clipboard::new() failures and clipboard.set_text() failures gracefully.
pub fn copy_to_clipboard(content: &str) -> NotifyMessage {
    match Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(content) {
            Ok(_) => NotifyMessage::Success("Copied to clipboard".to_string()),
            Err(e) => NotifyMessage::Error(format!("Failed to copy to clipboard: {}", e)),
        },
        Err(e) => NotifyMessage::Error(format!("Failed to access clipboard: {}", e)),
    }
}
