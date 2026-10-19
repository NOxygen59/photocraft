//! UIKit through objc2: the only module of this crate that allows `unsafe` (Objective-C interop:
//! the document picker's delegate protocol, the window's raw view, UIPasteboard's unsafe
//! accessors).
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController, UIPasteboard, UIView};
use objc2_uniform_type_identifiers::{UTType, UTTypeFolder, UTTypeItem};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::PickKind;

/// The app's Documents folder (shown in Files as On My iPad › PhotoCraft), made if missing.
pub fn documents_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("HOME")?).join("Documents");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

type Done = Box<dyn FnOnce(Option<Vec<String>>)>;

struct Ivars {
    done: RefCell<Option<Done>>,
    /// Save: the file name, joined to the folder picked.
    save_as: Option<String>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotoCraftDocumentPickerDelegate"]
    #[ivars = Ivars]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let paths = urls
                .iter()
                .filter_map(|url| {
                    // Kept for the session: the app reads the file, and writes it on Save, later.
                    let _ = unsafe { url.startAccessingSecurityScopedResource() };
                    Some(url.path()?.to_string())
                })
                .collect::<Vec<_>>();
            let paths = match &self.ivars().save_as {
                Some(name) => paths.into_iter().take(1).map(|folder| Path::new(&folder).join(name).to_string_lossy().into_owned()).collect(),
                None => paths,
            };
            self.answer((!paths.is_empty()).then_some(paths));
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.answer(None);
        }
    }
);

impl PickerDelegate {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    fn answer(&self, paths: Option<Vec<String>>) {
        let done = self.ivars().done.borrow_mut().take();
        if let Some(done) = done {
            done(paths);
        }
    }
}

thread_local! {
    /// The picker's delegate (the picker holds it weakly), until the next picker replaces it.
    static DELEGATE: RefCell<Option<Retained<PickerDelegate>>> = const { RefCell::new(None) };
}

/// Show the document picker over `window`, starting in `start` (else the Documents folder).
/// `done` gets the paths picked, or `None` when cancelled. Returns false (and never calls `done`)
/// when no picker could be shown: not on the main thread, or no window to show it over.
pub fn pick(window: &impl HasWindowHandle, kind: PickKind, start: Option<PathBuf>, done: impl FnOnce(Option<Vec<String>>) + 'static) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("no document picker: not on the main thread");
        return false;
    };
    let Some(handle) = window.window_handle().ok() else { return false };
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else { return false };
    // SAFETY: a UIKit window handle's view is the window's UIView, alive while the window is.
    let view: &UIView = unsafe { handle.ui_view.cast::<UIView>().as_ref() };
    let Some(mut presenter) = view.window().and_then(|w| w.rootViewController()) else { return false };
    while let Some(next) = presenter.presentedViewController() {
        presenter = next;
    }

    let (folders, multiple, save_as) = match kind {
        PickKind::Files { multiple } => (false, multiple, None),
        PickKind::Folder => (true, false, None),
        PickKind::SaveIn { file_name } => (true, false, Some(file_name)),
    };
    let start = start.filter(|p| p.is_dir()).or_else(documents_dir);
    let content_type: &UTType = unsafe { if folders { UTTypeFolder } else { UTTypeItem } };
    let types = NSArray::from_slice(&[content_type]);
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(UIDocumentPickerViewController::alloc(mtm), &types, false);
    picker.setAllowsMultipleSelection(multiple);
    picker.setShouldShowFileExtensions(true);
    if let Some(start) = start {
        picker.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&start.to_string_lossy()))));
    }
    let delegate = PickerDelegate::new(mtm, Ivars { done: RefCell::new(Some(Box::new(done))), save_as });
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
    presenter.presentViewController_animated_completion(&picker, true, None);
    true
}

/// Put a PNG on the system pasteboard (replacing what was there).
pub fn pasteboard_set_png(png: &[u8]) {
    autoreleasepool(|_| {
        UIPasteboard::generalPasteboard().setData_forPasteboardType(&NSData::with_bytes(png), &NSString::from_str("public.png"));
    });
}

/// The pasteboard's image as encoded data (PNG, JPEG, HEIC, TIFF, GIF or WebP, the first held),
/// for the app's own codecs to decode.
pub fn pasteboard_image() -> Option<Vec<u8>> {
    const TYPES: &[&str] = &["public.png", "public.jpeg", "public.heic", "public.tiff", "com.compuserve.gif", "org.webmproject.webp"];
    autoreleasepool(|_| {
        let pasteboard = UIPasteboard::generalPasteboard();
        TYPES.iter().find_map(|t| {
            let data = pasteboard.dataForPasteboardType(&NSString::from_str(t))?.to_vec();
            (!data.is_empty()).then_some(data)
        })
    })
}

/// The key window's safe area insets in points: top, left, bottom, right (zeros off the main
/// thread or without a window).
pub fn safe_area_insets() -> [f32; 4] {
    let Some(mtm) = MainThreadMarker::new() else { return [0.0; 4] };
    let app = objc2_ui_kit::UIApplication::sharedApplication(mtm);
    // `keyWindow` is deprecated from iOS 15 for scenes, which this app (winit 0.30) doesn't use.
    let window: Option<Retained<objc2_ui_kit::UIWindow>> = unsafe { msg_send![&app, keyWindow] };
    let Some(window) = window else { return [0.0; 4] };
    let i = window.safeAreaInsets();
    [i.top, i.left, i.bottom, i.right].map(|v| v as f32)
}
