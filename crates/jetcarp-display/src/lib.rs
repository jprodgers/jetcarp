//! Everything a display needs to show a JETCARP world, without a window toolkit: the tile renderer
//! ([`gpu`]), choosing and colouring what a view shows ([`view`]), stepping on the GPU
//! ([`simgpu`]), and screensaver profiles ([`profiles`]). The app wraps these in egui; a wallpaper
//! or screensaver host draws to its own wgpu surface.
//!
//! **Feature `library`** (on by default): rotations find the Quick Start entries they name in the
//! user's library ([`stage::library_lookup`]), and the macOS saver can be installed
//! ([`saver::install`], which bakes those scenes into the bundle). The saver's own code is built
//! without it, so it links no store, job runner or SQLite.

/// On macOS, names this process `name`. winit makes the application menu's items from the process
/// name ("About JETCARP", "Hide JETCARP", "Quit JETCARP"), and a program that is not an app bundle
/// is otherwise named after its file, `jetcarp`. Call it before the event loop starts. Nothing
/// elsewhere.
pub fn set_process_name(name: &str) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject};
        let (Some(info), Some(strings), Ok(c)) = (
            AnyClass::get(c"NSProcessInfo"),
            AnyClass::get(c"NSString"),
            std::ffi::CString::new(name),
        ) else {
            return;
        };
        unsafe {
            let process: Option<Retained<AnyObject>> = msg_send![info, processInfo];
            let s: Option<Retained<AnyObject>> =
                msg_send![strings, stringWithUTF8String: c.as_ptr()];
            if let (Some(process), Some(s)) = (process, s) {
                let _: () = msg_send![&*process, setProcessName: &*s];
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = name;
}

pub mod autostart;
pub mod caption;
pub mod desktop_picture;
pub mod exit;
pub mod gnome;
pub mod gpu;
pub mod host_settings;
pub mod macsaver;
pub mod painter;
pub mod profiles;
pub mod reseed_presets;
pub mod rotation;
pub mod saver;
pub mod simgpu;
pub mod stage;
pub mod tuning;
pub mod view;
pub mod wallpaper_store;
pub mod winsaver;
pub mod workarea;
