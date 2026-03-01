//! Extracts the NSWindow from an eframe Frame on macOS.

use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Extracts the NSWindow from the eframe Frame via raw-window-handle.
///
/// The Frame provides a raw `NSView` pointer. We retain it as an `NSView`,
/// then call `.window()` to get the enclosing `NSWindow`.
pub fn get_ns_window(frame: &eframe::Frame) -> Result<Retained<NSWindow>, String> {
    let handle = frame
        .window_handle()
        .map_err(|e| format!("Failed to get window handle: {}", e))?;

    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit_handle) => {
            let ns_view_ptr = appkit_handle.ns_view.as_ptr();
            // SAFETY: The pointer came from eframe's WindowHandle, which guarantees
            // it points to a valid NSView. We are on the main thread because eframe's
            // update() runs on the main thread.
            let ns_view: Retained<NSView> = unsafe { Retained::retain(ns_view_ptr.cast()) }
                .ok_or("Failed to retain NSView from raw pointer")?;
            ns_view
                .window()
                .ok_or_else(|| "NSView is not installed in a window".to_string())
        }
        other => Err(format!("Unexpected window handle type: {:?}", other)),
    }
}
