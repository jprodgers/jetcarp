//! macOS's own window actions, asked of AppKit directly.
//!
//! ⌘M minimises the app's window the way its yellow button does (`miniaturize:`), not through
//! egui's `ViewportCommand::Minimized`. That command makes egui-winit record the window as
//! minimised, and on macOS egui-winit never reads that back from the window afterwards (it would
//! deadlock; egui #3494). So after a restore from the Dock, egui went on counting the window as
//! hidden, and eframe never drew it again: the world stepped and AI control answered behind a
//! frozen picture (#131). A window minimised by AppKit itself comes back as the yellow button's
//! does: macOS tells winit it can be seen again, and drawing resumes.

/// Minimise the app's key window (or its main window, if none is key), as its yellow button
/// does. Only on the main thread, where AppKit may be used; elsewhere (in tests) it does nothing.
pub fn miniaturize() {
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, class, msg_send};
    if MainThreadMarker::new().is_none() {
        return;
    }
    // SAFETY: on the main thread; plain AppKit messages to the shared application and its
    // window, each checked for nil before use.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let mut w: *mut AnyObject = msg_send![app, keyWindow];
        if w.is_null() {
            w = msg_send![app, mainWindow];
        }
        if !w.is_null() {
            let _: () = msg_send![w, miniaturize: std::ptr::null_mut::<AnyObject>()];
        }
    }
}
