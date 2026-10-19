//! iPadOS services the desktop app gets elsewhere from rfd and arboard, which have no iOS backend:
//!
//! - [`pick`]: the Files app's document picker over the window, for Open, Save and folders.
//! - [`pasteboard_set_png`] / [`pasteboard_image`]: images on the system pasteboard.
//! - [`safe_area_insets`]: where the notch, the screen corners and the home indicator are.
//!
//! Empty off iOS.

#[cfg(target_os = "ios")]
mod uikit;
#[cfg(target_os = "ios")]
pub use uikit::{documents_dir, pasteboard_image, pasteboard_set_png, pick, safe_area_insets};

/// What the document picker picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// Files to read, where they are (not copies): Save writes back to the same file.
    Files { multiple: bool },
    /// A folder.
    Folder,
    /// A folder to save `file_name` in: the answer is the file's path in it.
    SaveIn { file_name: String },
}
