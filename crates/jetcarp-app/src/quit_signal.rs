//! The app ended by a signal (#135): SIGTERM (how `kill`, a logout or a system shutting down
//! ends a program), SIGHUP (the terminal it was started from closing) and SIGINT (Ctrl-C there)
//! close the app as its window's close button and *Quit* do, so it ends through `on_exit`: its
//! layout and settings saved, its jobs stopped at a checkpoint, and AI control's endpoint file
//! (`app-endpoint.json` in the runtime folder) removed. Before, each ended the process at once,
//! and on the Mac an app ended with SIGTERM left its endpoint file behind. A second signal ends
//! the app at once, as before, for one that does not close.
//!
//! A signal handler may do next to nothing safely, so this one only writes a byte to a socket
//! pair; a thread of its own waits on the other end and asks the window to close
//! (`ViewportCommand::Close`), which eframe answers on the UI thread. A signal ignored when the
//! app started (SIGHUP under `nohup`) stays ignored. Linux and macOS: Windows has no such
//! signals, and `taskkill` without `/F` asks the window to close, which ends the app through
//! `on_exit` already.

use eframe::egui;
use std::io::Read;
use std::os::fd::IntoRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Once;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

/// The signals that close the app.
const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGHUP, libc::SIGINT];

/// The end of the socket pair the handler writes to (-1 until there is one).
static WAKE: AtomicI32 = AtomicI32::new(-1);
/// How many of the signals have come.
static COUNT: AtomicUsize = AtomicUsize::new(0);

/// The handler: the first signal wakes the thread that closes the window; a second ends the
/// process at once, with the status a shell gives a program a signal ended (128 + its number).
/// Only async-signal-safe calls: `write` and `_exit`, and lock-free atomics.
extern "C" fn on_signal(sig: libc::c_int) {
    if COUNT.fetch_add(1, Ordering::SeqCst) > 0 {
        // SAFETY: `_exit` is async-signal-safe and does not return.
        unsafe { libc::_exit(128 + sig) };
    }
    let fd = WAKE.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = [1u8];
        // SAFETY: `write` is async-signal-safe; `byte` outlives the call. A failure leaves the
        // app running, as an ignored signal would, and a second signal still ends it.
        unsafe { libc::write(fd, byte.as_ptr().cast(), 1) };
    }
}

/// From now on, SIGTERM, SIGHUP and SIGINT close the app whose context is `ctx`, as its window's
/// close button does. Once per process (the app has one window of its own); later calls do
/// nothing.
pub(crate) fn install(ctx: &egui::Context) {
    static ONCE: Once = Once::new();
    let ctx = ctx.clone();
    ONCE.call_once(move || {
        let Ok((mut woken, wake)) = UnixStream::pair() else {
            return;
        };
        let watcher = std::thread::Builder::new()
            .name("jetcarp-quit-signal".into())
            .spawn(move || {
                let mut byte = [0u8];
                if woken.read_exact(&mut byte).is_ok() {
                    // As *Quit* does, to the main window whichever is in front; it asks for the
                    // frame that answers it.
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
            });
        if watcher.is_err() {
            return;
        }
        WAKE.store(wake.into_raw_fd(), Ordering::SeqCst);
        let handler: extern "C" fn(libc::c_int) = on_signal;
        for sig in SIGNALS {
            // SAFETY: plain structs filled in before use; the handler is async-signal-safe.
            unsafe {
                let mut was: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(sig, std::ptr::null(), &mut was) == 0
                    && was.sa_sigaction == libc::SIG_IGN
                {
                    continue;
                }
                let mut sa: libc::sigaction = std::mem::zeroed();
                sa.sa_sigaction = handler as libc::sighandler_t;
                sa.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut sa.sa_mask);
                libc::sigaction(sig, &sa, std::ptr::null_mut());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    //! The whole app, headless, in a process of its own with every folder a scratch one, as the
    //! keyboard tests run it (`app::keyboard_tests`): the test the suite runs starts its
    //! `#[ignore]`d twin, which runs frames as eframe would, and ends as eframe ends the app when
    //! its window is asked to close (`on_exit`).
    use eframe::egui;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// Set in the twin's process by the test.
    const CHILD: &str = "JETCARP_QUIT_SIGNAL_TEST_CHILD";
    /// What the twin prints once AI control has recorded its endpoint.
    const READY: &str = "quit signal test: the endpoint is recorded";

    /// SIGTERM (how `kill`, a logout or a system shutting down ends a program) ends the app as
    /// its window's close button does: its settings are saved, its jobs stopped at a checkpoint,
    /// and AI control's endpoint file, `app-endpoint.json` in the runtime folder, is removed. On
    /// the Mac, an app ended with SIGTERM left the file behind.
    #[test]
    fn sigterm_ends_the_app_as_closing_its_window_does() {
        let root = jetcarp_core::scratch::ScratchDir::new("quit-signal");
        let endpoint = root.join("run").join("app-endpoint.json");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "quit_signal::tests::child_runs_until_its_window_is_closed",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
        for (var, sub) in [
            ("JETCARP_CONFIG_DIR", "config"),
            ("JETCARP_DATA_DIR", "data"),
            ("JETCARP_LIBRARY", "data/library.sqlite"),
            ("JETCARP_OUT", "out"),
            ("JETCARP_RUNTIME_DIR", "run"),
        ] {
            cmd.env(var, root.join(sub));
        }
        cmd.env(CHILD, "1")
            .env("JETCARP_GPU", "off")
            .env("JETCARP_AI_CONTROL", "1");
        for v in [
            "JETCARP_OFFSCREEN_OUT",
            "JETCARP_SCREENSHOT",
            "JETCARP_MENU",
            "JETCARP_REQUIRE_GPU",
        ] {
            cmd.env_remove(v);
        }
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the twin");
        let out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        let mut said = Vec::new();
        let ready = loop {
            match rx.recv_timeout(Duration::from_secs(120)) {
                Ok(l) if l.contains(READY) => break true,
                Ok(l) => said.push(l),
                Err(_) => break false,
            }
        };
        if !ready {
            let _ = child.kill();
        }
        assert!(
            ready,
            "the twin never recorded its endpoint:\n{}",
            said.join("\n")
        );
        assert!(endpoint.exists(), "{}", endpoint.display());

        let pid = child.id().to_string();
        let sent = Command::new("kill").args(["-TERM", &pid]).status().unwrap();
        assert!(sent.success(), "kill -TERM {pid}");
        let t0 = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break Some(s);
            }
            if t0.elapsed() > Duration::from_secs(60) {
                let _ = child.kill();
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        let status = status.expect("the app ends within a minute of SIGTERM");
        assert!(
            !endpoint.exists(),
            "SIGTERM left {} behind (the app ended with {status:?}); its stderr:\n{err}",
            endpoint.display()
        );
        assert!(
            status.success(),
            "the app ends as on_exit ends it, with 0: {status:?}; its stderr:\n{err}"
        );
    }

    /// The twin: the app as `run` makes it (`crate::new_app`), its photosensitive seizure warning
    /// answered in the scratch settings folder and AI control on at launch
    /// (`JETCARP_AI_CONTROL`), run frame by frame until its window is asked to close, and then
    /// ended as eframe ends it, through `on_exit`.
    #[test]
    #[ignore = "started by sigterm_ends_the_app_as_closing_its_window_does, in a scratch process"]
    fn child_runs_until_its_window_is_closed() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        let settings = jetcarp_core::paths::config_dir().expect("JETCARP_CONFIG_DIR is set");
        jetcarp_core::photosensitivity::record_in(
            &settings,
            jetcarp_core::photosensitivity::Answer::NoDisorder,
            "test",
        )
        .unwrap();
        let endpoint = jetcarp_mcp::link::endpoint_file().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = crate::new_app(&cc, crate::cli::LaunchArgs::default());
        let mut frame = eframe::Frame::_new_kittest();
        let (t0, mut time, mut said) = (Instant::now(), 0.0, false);
        loop {
            time += 1.0 / 30.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(time),
                ..Default::default()
            };
            let out = ctx.run_pass(input, |ctx| {
                ctx.root_ui(|ui| {
                    eframe::App::logic(&mut app, ui.ctx(), &mut frame);
                    eframe::App::ui(&mut app, ui, &mut frame);
                });
            });
            if !said && endpoint.exists() {
                println!("{READY}");
                said = true;
            }
            let close = out
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::Close));
            if close {
                // Ends the process.
                eframe::App::on_exit(&mut app);
            }
            if t0.elapsed() > Duration::from_secs(180) {
                println!("quit signal test: no close asked for in three minutes");
                jetcarp_display::exit::end_process(3);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
