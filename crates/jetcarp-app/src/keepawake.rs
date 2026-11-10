//! Keep the computer awake: while held, the screen does not blank or lock and the computer does
//! not suspend.
//!
//! Linux tries, in order: the XDG desktop portal (`org.freedesktop.portal.Inhibit.Inhibit` with
//! flags suspend | idle; it works inside Flatpak too, and the inhibition lasts until its request
//! object is closed or the connection ends), then `org.freedesktop.ScreenSaver.Inhibit`, then
//! `org.gnome.SessionManager.Inhibit`. Windows uses `SetThreadExecutionState` (held by the thread
//! that took it, so a small thread of its own holds it). Other systems report that keep-awake is
//! not available.
//!
//! Taking and releasing are blocking D-Bus calls with a three-second timeout; they run when the
//! user or a screensaver asks, not every frame.

/// Who holds the inhibition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// `org.freedesktop.portal.Inhibit` (XDG desktop portal).
    Portal,
    /// `org.freedesktop.ScreenSaver`.
    ScreenSaver,
    /// `org.gnome.SessionManager`.
    GnomeSession,
    /// `SetThreadExecutionState` (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    Windows,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::Portal => "XDG desktop portal (org.freedesktop.portal.Inhibit)",
            Backend::ScreenSaver => "org.freedesktop.ScreenSaver",
            Backend::GnomeSession => "org.gnome.SessionManager",
            Backend::Windows => "SetThreadExecutionState",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Backend::Portal => "portal",
            Backend::ScreenSaver => "screensaver",
            Backend::GnomeSession => "gnome-session",
            Backend::Windows => "windows",
        }
    }
}

/// Keep-awake: off, or held by one backend. Dropping it releases the inhibition.
#[derive(Default)]
pub struct KeepAwake {
    held: Option<imp::Held>,
    /// Why the last attempt failed (every backend's error).
    pub error: Option<String>,
}

impl KeepAwake {
    pub fn active(&self) -> bool {
        self.held.is_some()
    }

    pub fn backend(&self) -> Option<Backend> {
        self.held.as_ref().map(|h| h.backend())
    }

    /// Take (on) or release (off). Taking when held does nothing. Returns whether it is held.
    pub fn set(&mut self, on: bool, reason: &str) -> bool {
        if !on {
            if let Some(h) = self.held.take()
                && let Err(e) = h.release()
            {
                self.error = Some(format!("release: {e}"));
            }
            return false;
        }
        if self.held.is_some() {
            return true;
        }
        match imp::take(reason) {
            Ok(h) => {
                self.held = Some(h);
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    /// "Held by …", "off", or the error.
    pub fn describe(&self) -> String {
        match (&self.held, &self.error) {
            (Some(h), _) => format!("held by {}", h.backend().name()),
            (None, Some(e)) => format!("not held: {e}"),
            (None, None) => "off".into(),
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(h) = self.held.take() {
            let _ = h.release();
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Backend;
    use std::collections::HashMap;
    use std::time::Duration;
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedObjectPath, Value};

    /// Portal Inhibit flags: suspend and idle.
    const SUSPEND_IDLE: u32 = 4 | 8;

    pub enum Held {
        /// The request object, on the connection that made it.
        Portal(Connection, OwnedObjectPath),
        ScreenSaver(Connection, u32),
        GnomeSession(Connection, u32),
    }

    impl Held {
        pub fn backend(&self) -> Backend {
            match self {
                Held::Portal(..) => Backend::Portal,
                Held::ScreenSaver(..) => Backend::ScreenSaver,
                Held::GnomeSession(..) => Backend::GnomeSession,
            }
        }

        pub fn release(self) -> Result<(), String> {
            let r = match &self {
                // The portal answers Inhibit before it exports the request object: closing
                // right after taking finds no object, so try again for a moment.
                Held::Portal(c, path) => {
                    let close = || {
                        c.call_method(
                            Some("org.freedesktop.portal.Desktop"),
                            path.as_ref(),
                            Some("org.freedesktop.portal.Request"),
                            "Close",
                            &(),
                        )
                        .map(|_| ())
                    };
                    let mut r = close();
                    for _ in 0..10 {
                        if r.is_ok() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                        r = close();
                    }
                    r
                }
                Held::ScreenSaver(c, cookie) => c
                    .call_method(
                        Some("org.freedesktop.ScreenSaver"),
                        "/org/freedesktop/ScreenSaver",
                        Some("org.freedesktop.ScreenSaver"),
                        "UnInhibit",
                        &(*cookie,),
                    )
                    .map(|_| ()),
                Held::GnomeSession(c, cookie) => c
                    .call_method(
                        Some("org.gnome.SessionManager"),
                        "/org/gnome/SessionManager",
                        Some("org.gnome.SessionManager"),
                        "Uninhibit",
                        &(*cookie,),
                    )
                    .map(|_| ()),
            };
            // Dropping the connection ends every inhibition it holds anyway.
            r.map_err(|e| e.to_string())
        }
    }

    fn session() -> Result<Connection, String> {
        zbus::blocking::connection::Builder::session()
            .and_then(|b| b.method_timeout(Duration::from_secs(3)).build())
            .map_err(|e| format!("no session bus: {e}"))
    }

    pub fn take(reason: &str) -> Result<Held, String> {
        let c = session()?;
        let mut errors = vec![];
        // 1. The portal.
        // A handle token of our own: without one the request object was not there to close
        // (GNOME's portal, version 3).
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let token = format!(
            "jetcarp_keep_awake_{}_{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut opts: HashMap<&str, Value> = HashMap::new();
        opts.insert("reason", Value::from(reason));
        opts.insert("handle_token", Value::from(token.as_str()));
        match c.call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Inhibit"),
            "Inhibit",
            &("", SUSPEND_IDLE, opts),
        ) {
            Ok(m) => match m.body().deserialize::<OwnedObjectPath>() {
                Ok(p) => return Ok(Held::Portal(c, p)),
                Err(e) => errors.push(format!("portal: bad reply: {e}")),
            },
            Err(e) => errors.push(format!("portal: {e}")),
        }
        // 2. org.freedesktop.ScreenSaver (idle only).
        match c.call_method(
            Some("org.freedesktop.ScreenSaver"),
            "/org/freedesktop/ScreenSaver",
            Some("org.freedesktop.ScreenSaver"),
            "Inhibit",
            &("JETCARP", reason),
        ) {
            Ok(m) => match m.body().deserialize::<u32>() {
                Ok(k) => return Ok(Held::ScreenSaver(c, k)),
                Err(e) => errors.push(format!("ScreenSaver: bad reply: {e}")),
            },
            Err(e) => errors.push(format!("ScreenSaver: {e}")),
        }
        // 3. GNOME's session manager (toplevel xid 0: no window).
        match c.call_method(
            Some("org.gnome.SessionManager"),
            "/org/gnome/SessionManager",
            Some("org.gnome.SessionManager"),
            "Inhibit",
            &("jetcarp", 0u32, reason, SUSPEND_IDLE),
        ) {
            Ok(m) => match m.body().deserialize::<u32>() {
                Ok(k) => return Ok(Held::GnomeSession(c, k)),
                Err(e) => errors.push(format!("SessionManager: bad reply: {e}")),
            },
            Err(e) => errors.push(format!("SessionManager: {e}")),
        }
        Err(errors.join("; "))
    }
}

#[cfg(windows)]
mod imp {
    use super::Backend;

    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    /// The execution state belongs to a thread: a small thread sets it and waits to be told
    /// to clear it.
    pub struct Held(std::sync::mpsc::Sender<()>);

    impl Held {
        pub fn backend(&self) -> Backend {
            Backend::Windows
        }
        pub fn release(self) -> Result<(), String> {
            let _ = self.0.send(());
            Ok(())
        }
    }

    pub fn take(_reason: &str) -> Result<Held, String> {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let (ok_tx, ok_rx) = std::sync::mpsc::channel::<bool>();
        std::thread::Builder::new()
            .name("jetcarp-keep-awake".into())
            .spawn(move || {
                // SAFETY: a plain Win32 call with flag constants.
                let prev = unsafe {
                    SetThreadExecutionState(
                        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED,
                    )
                };
                let _ = ok_tx.send(prev != 0);
                let _ = rx.recv();
                // SAFETY: as above; clears the requirement.
                unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            })
            .map_err(|e| e.to_string())?;
        match ok_rx.recv() {
            Ok(true) => Ok(Held(tx)),
            _ => Err("SetThreadExecutionState failed".into()),
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::Backend;

    pub enum Held {}

    impl Held {
        pub fn backend(&self) -> Backend {
            match *self {}
        }
        pub fn release(self) -> Result<(), String> {
            match self {}
        }
    }

    pub fn take(_reason: &str) -> Result<Held, String> {
        Err("keep-awake is not available on this system yet".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Takes the inhibition and releases it through whatever backend this machine has.
    /// Ignored by default (it needs a desktop session); run with `--ignored`.
    #[test]
    #[ignore]
    fn take_and_release() {
        let mut k = KeepAwake::default();
        if !k.set(true, "JETCARP test: keep-awake take and release") {
            panic!("could not take: {}", k.describe());
        }
        let b = k.backend().expect("held");
        println!("keep-awake held by {}", b.name());
        assert!(k.active());
        assert!(k.set(true, "again"), "taking twice keeps it");
        assert!(!k.set(false, ""));
        assert!(!k.active());
        assert!(k.error.is_none(), "release: {:?}", k.error);
    }

    #[test]
    fn off_by_default() {
        let mut k = KeepAwake::default();
        assert!(!k.active());
        assert_eq!(k.describe(), "off");
        assert!(!k.set(false, ""));
    }
}
