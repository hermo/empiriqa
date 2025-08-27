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

/// Helper function to handle clipboard operations with empty content validation.
///
/// Takes content and an empty message, validates the content is not empty,
/// and either copies to clipboard or returns an error message.
pub fn copy_with_validation(content: &str, empty_message: &str) -> NotifyMessage {
    if content.trim().is_empty() {
        NotifyMessage::Error(empty_message.to_string())
    } else {
        copy_to_clipboard(content)
    }
}

/// Helper function for pipeline clipboard operations.
///
/// Validates pipeline text and copies to clipboard with appropriate error handling.
pub fn copy_pipeline_to_clipboard(pipeline_text: &str) -> NotifyMessage {
    copy_with_validation(pipeline_text, "Pipeline is empty")
}

/// Helper function for output clipboard operations.
///
/// Validates output text and copies to clipboard with appropriate error handling.
pub fn copy_output_to_clipboard(output_text: &str) -> NotifyMessage {
    copy_with_validation(output_text, "Output queue is empty")
}
