//! iPadOS services the desktop app gets elsewhere from rfd and arboard, which have no iOS backend:
//!
//! - [`pick`]: the Files app's document picker over the window, for Open, Save and folders.
//! - [`pasteboard_set_png`] / [`pasteboard_image`]: images on the system pasteboard.
//! - [`safe_area_insets`]: where the notch, the screen corners and the home indicator are.
//! - [`resources`]: the memory and CPU PhotoCraft uses, and the battery level.
//!
//! Empty off iOS.

#[cfg(target_os = "ios")]
mod uikit;
#[cfg(target_os = "ios")]
pub use uikit::{documents_dir, pasteboard_image, pasteboard_set_png, pick, resources, safe_area_insets};

/// What PhotoCraft itself uses, read by [`resources`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resources {
    /// The app's memory footprint (what iPadOS counts against it), in bytes.
    pub memory_bytes: u64,
    /// The app's share of all CPU cores since the previous reading, 0–100.
    pub cpu_percent: f32,
    /// The iPad's battery level, 0–100 (`None` when iPadOS doesn't know).
    pub battery_percent: Option<f32>,
    pub charging: bool,
}

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
