//! macOS: a wallpaper is a borderless window at desktop level, under the Finder's icons, on every
//! Space, ignoring the mouse. One per monitor. Wave 1 showed this works on macOS 27.

use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use std::ffi::c_void;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

const DESKTOP_KEY: i32 = 2; // kCGDesktopWindowLevelKey
const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const STATIONARY: usize = 1 << 4;
const IGNORES_CYCLE: usize = 1 << 6;
/// `NSWindowOcclusionStateVisible`.
const OCCLUSION_VISIBLE: usize = 1 << 1;
/// `NSApplicationActivationPolicyAccessory`: no Dock icon and no place in the app switcher, as a
/// menu-bar program.
const POLICY_ACCESSORY: isize = 1;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
    fn CGSessionCopyCurrentDictionary() -> *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

fn ns_window(window: &Window) -> Result<*mut AnyObject, String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let view = h.ns_view.as_ptr() as *mut AnyObject;
    let w: *mut AnyObject = unsafe { msg_send![view, window] };
    if w.is_null() {
        Err("the view has no window yet".into())
    } else {
        Ok(w)
    }
}

/// Moves `window` down to the desktop, on every Space, out of the window cycle and the mouse's way.
pub fn place_on_desktop(window: &Window) -> Result<(), String> {
    let w = ns_window(window)?;
    unsafe {
        let level = CGWindowLevelForKey(DESKTOP_KEY);
        let _: () = msg_send![w, setLevel: level as isize];
        let behaviour = CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE;
        let _: () = msg_send![w, setCollectionBehavior: behaviour];
        let _: () = msg_send![w, setIgnoresMouseEvents: Bool::YES];
        let _: () = msg_send![w, setHasShadow: Bool::NO];
        let _: () = msg_send![w, setExcludedFromWindowsMenu: Bool::YES];
    }
    Ok(())
}

/// Whether any part of `window` can be seen (macOS works this out for each window).
pub fn visible(window: &Window) -> bool {
    let Ok(w) = ns_window(window) else {
        return true;
    };
    let state: usize = unsafe { msg_send![w, occlusionState] };
    state & OCCLUSION_VISIBLE != 0
}

/// Whether the session is locked (the lock screen is up, or another user is switched in).
pub fn locked() -> bool {
    unsafe {
        let dict = CGSessionCopyCurrentDictionary();
        if dict.is_null() {
            return false;
        }
        // A CFDictionary is an NSDictionary.
        let d = dict as *mut AnyObject;
        let key: *mut AnyObject = msg_send![
            class!(NSString),
            stringWithUTF8String: c"CGSSessionScreenIsLocked".as_ptr()
        ];
        let v: *mut AnyObject = msg_send![d, objectForKey: key];
        let on = !v.is_null() && {
            let b: Bool = msg_send![v, boolValue];
            b.as_bool()
        };
        CFRelease(dict);
        on
    }
}

/// Keeps the host out of the Dock and the app switcher: it lives in the menu bar (its tray menu),
/// with or without plain windows, which Mission Control still shows. A bare program would
/// otherwise get a Dock icon, the generic one of a Unix program, named `jetcarp` (seen on the Mac
/// in rc1 when the screens became windows).
pub fn stay_out_of_dock() {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let current: isize = msg_send![app, activationPolicy];
        if current != POLICY_ACCESSORY {
            let _: Bool = msg_send![app, setActivationPolicy: POLICY_ACCESSORY];
        }
    }
}
