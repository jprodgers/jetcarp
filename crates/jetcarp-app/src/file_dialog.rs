//! File dialogs that don't hold up the window (#136).
//!
//! rfd's dialogs block the thread that opens them. On Linux they wait on the desktop portal over
//! D-Bus, so a dialog opened from the frame loop stopped winit's event loop for as long as it was
//! open: the compositor's pings went unanswered, and GNOME said JETCARP might not be responding.
//! There each dialog now runs on a thread of its own, and a later frame picks up its answer.
//! Windows and macOS run their dialogs in a nested event loop that keeps the window answering, so
//! there a dialog still runs where it is opened, and its answer is ready at once.
//!
//! While a dialog is open the world doesn't step and AI control waits, as they did when the dialog
//! held up the frame loop, so what is saved or exported is what was showing when it was asked for.

use eframe::egui;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// Whether dialogs run on a thread of their own here: Linux and the other free Unixes.
#[cfg_attr(test, allow(dead_code))]
const THREADED: bool = cfg!(all(unix, not(target_os = "macos")));

enum Slot {
    Open,
    Answered(Box<dyn Any + Send>),
}

static SLOTS: Mutex<Option<HashMap<&'static str, Slot>>> = Mutex::new(None);

fn slots() -> MutexGuard<'static, Option<HashMap<&'static str, Slot>>> {
    SLOTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn with_slots<R>(f: impl FnOnce(&mut HashMap<&'static str, Slot>) -> R) -> R {
    f(slots().get_or_insert_with(HashMap::new))
}

/// Opens the dialog `dialog` under `key`, unless one is already open under it. `dialog` runs the
/// rfd dialog and returns its answer (None when cancelled).
#[cfg(not(test))]
pub fn open<T: Send + 'static>(
    ctx: &egui::Context,
    key: &'static str,
    dialog: impl FnOnce() -> Option<T> + Send + 'static,
) {
    open_on(THREADED, Some(ctx.clone()), key, dialog);
}

/// In tests no system dialog is ever shown: the request is noted ([`asked`]) and nothing is chosen.
#[cfg(test)]
pub fn open<T: Send + 'static>(
    _ctx: &egui::Context,
    key: &'static str,
    _dialog: impl FnOnce() -> Option<T> + Send + 'static,
) {
    ASKED.lock().unwrap_or_else(|e| e.into_inner()).push(key);
}

#[cfg(test)]
static ASKED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// Whether a dialog was asked for under `key` (tests).
#[cfg(test)]
pub fn asked(key: &str) -> bool {
    ASKED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&key)
}

fn open_on<T: Send + 'static>(
    threaded: bool,
    ctx: Option<egui::Context>,
    key: &'static str,
    dialog: impl FnOnce() -> Option<T> + Send + 'static,
) {
    let opened = with_slots(|s| {
        if s.contains_key(key) {
            return false;
        }
        s.insert(key, Slot::Open);
        true
    });
    if !opened {
        return;
    }
    let answer = move |a: Option<T>| {
        with_slots(|s| s.insert(key, Slot::Answered(Box::new(a))));
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    };
    if !threaded {
        answer(dialog());
        return;
    }
    // A thread that can't be started runs the dialog here instead, as before.
    let run = std::sync::Arc::new(Mutex::new(Some((dialog, answer))));
    let on_thread = run.clone();
    let spawned = std::thread::Builder::new()
        .name("file dialog".into())
        .spawn(move || {
            if let Some((dialog, answer)) = on_thread.lock().ok().and_then(|mut r| r.take()) {
                answer(dialog());
            }
        });
    if spawned.is_err()
        && let Some((dialog, answer)) = run.lock().ok().and_then(|mut r| r.take())
    {
        answer(dialog());
    }
}

/// The answer of the dialog under `key`, once it has been given and not taken before: the chosen
/// path (or paths). None while it is open, when it was cancelled, and when none was opened.
pub fn take<T: 'static>(key: &'static str) -> Option<T> {
    answered(key).flatten()
}

/// Once the dialog under `key` has answered (and not before, nor again): `Some(None)` when it was
/// cancelled, `Some(Some(path))` when something was chosen.
pub fn answered<T: 'static>(key: &'static str) -> Option<Option<T>> {
    with_slots(|s| {
        if !matches!(s.get(key), Some(Slot::Answered(_))) {
            return None;
        }
        match s.remove(key) {
            Some(Slot::Answered(a)) => Some(a.downcast::<Option<T>>().ok().and_then(|a| *a)),
            _ => None,
        }
    })
}

/// For a button: opens the dialog when `clicked`, and returns its answer once given. Call it every
/// frame the button is shown.
pub fn on_click<T: Send + 'static>(
    ctx: &egui::Context,
    key: &'static str,
    clicked: bool,
    dialog: impl FnOnce() -> Option<T> + Send + 'static,
) -> Option<T> {
    if clicked {
        open(ctx, key, dialog);
    }
    take(key)
}

/// Whether any dialog is open now.
pub fn any_open() -> bool {
    with_slots(|s| s.values().any(|v| matches!(v, Slot::Open)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn wait_for<T: 'static>(key: &'static str) -> Option<T> {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(10) {
            if let Some(a) = take::<T>(key) {
                return Some(a);
            }
            if !with_slots(|s| s.contains_key(key)) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no answer under {key}");
    }

    /// The frame that opens a dialog goes on at once, and a later frame gets the answer.
    #[test]
    fn a_dialog_doesnt_hold_up_the_frame() {
        let (release, wait) = mpsc::channel::<()>();
        let t0 = Instant::now();
        open_on(true, None, "test.hold", move || {
            wait.recv().ok();
            Some(std::path::PathBuf::from("chosen.json"))
        });
        assert!(
            t0.elapsed() < Duration::from_millis(500),
            "open waited for the dialog"
        );
        assert!(any_open());
        assert_eq!(
            take::<std::path::PathBuf>("test.hold"),
            None,
            "no answer yet"
        );
        release.send(()).unwrap();
        assert_eq!(
            wait_for::<std::path::PathBuf>("test.hold"),
            Some(std::path::PathBuf::from("chosen.json"))
        );
        assert_eq!(take::<std::path::PathBuf>("test.hold"), None, "taken once");
    }

    /// Where dialogs run in place (Windows, macOS), the answer is there as soon as it is opened.
    #[test]
    fn in_place_the_answer_is_ready_at_once() {
        open_on(false, None, "test.inline", || Some(7u32));
        assert_eq!(take::<u32>("test.inline"), Some(7));
    }

    /// A second click while the dialog is open opens no second dialog; a cancelled one answers None
    /// and frees the key.
    #[test]
    fn one_dialog_at_a_time_and_cancelling_frees_it() {
        let (release, wait) = mpsc::channel::<()>();
        open_on(true, None, "test.once", move || {
            wait.recv().ok();
            None::<String>
        });
        let second = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = second.clone();
        open_on(false, None, "test.once", move || {
            s2.store(true, std::sync::atomic::Ordering::SeqCst);
            Some("again".to_string())
        });
        assert!(
            !second.load(std::sync::atomic::Ordering::SeqCst),
            "a second dialog was opened under the same key"
        );
        release.send(()).unwrap();
        assert_eq!(wait_for::<String>("test.once"), None);
        assert!(!with_slots(|s| s.contains_key("test.once")));
        open_on(false, None, "test.once", || Some("later".to_string()));
        assert_eq!(take::<String>("test.once"), Some("later".to_string()));
    }
}
