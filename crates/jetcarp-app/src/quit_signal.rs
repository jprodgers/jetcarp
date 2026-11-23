//! The app ended by a signal (#135): SIGTERM (what `kill` sends, and a system as it shuts down),
//! SIGHUP (the terminal it was started from closing) and SIGINT (Ctrl-C there) close the app as
//! its window's close button and *Quit* do, so it ends through `on_exit`: its
//! layout and settings saved, its jobs stopped at a checkpoint, and AI control's endpoint file
//! (`app-endpoint.json` in the runtime folder) removed. Before, each ended the process at once,
//! and on the Mac an app ended with SIGTERM left its endpoint file behind. A second signal ends
//! the app at once, as before, for one that does not close.
//!
//! The signals' plumbing is `jetcarp_core::quit_signal`, which the background host and
//! `jetcarp mcp --http` share (#278): its handler only wakes a thread of its own, which here asks
//! the window to close (`ViewportCommand::Close`), and eframe answers on the UI thread. A signal
//! ignored when the app started (SIGHUP under `nohup`) stays ignored. On Windows (#319) the same
//! plumbing takes Ctrl-C and the console closing (an app started from a console), and a sign-out
//! or a shutdown, which before ended the app at once: Windows tells winit's windows of a
//! session's end, and winit leaves it to Windows' defaults, so a hidden window of
//! `quit_signal`'s own hears it, and waits for the app to close. `taskkill` without `/F` asks
//! the app's windows to close, which ends it through `on_exit`, as before.

use eframe::egui;

/// From now on, SIGTERM, SIGHUP and SIGINT close the app whose context is `ctx`, as its window's
/// close button does. Once per process (the app has one window of its own); later calls do
/// nothing.
pub(crate) fn install(ctx: &egui::Context) {
    let ctx = ctx.clone();
    jetcarp_core::quit_signal::on_signal(move |_| {
        // As *Quit* does, to the main window whichever is in front; it asks for the frame that
        // answers it.
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
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
    /// What the twin prints if its session window answers the session's end (Windows), which it
    /// must not before the app has ended.
    const SESSION_END_ANSWERED: &str = "quit signal test: the session's end was answered";

    /// As Windows tells every program's windows at a sign-out: `WM_QUERYENDSESSION`, then
    /// `WM_ENDSESSION`, to the twin's session window, from a thread of its own.
    #[cfg(windows)]
    fn tell_the_session_ends() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            ENDSESSION_LOGOFF, SendMessageW, WM_ENDSESSION, WM_QUERYENDSESSION,
        };
        std::thread::spawn(|| {
            let t0 = Instant::now();
            let window = loop {
                if let Some(w) = jetcarp_core::quit_signal::session_window() {
                    break w;
                }
                if t0.elapsed() > Duration::from_secs(30) {
                    println!("quit signal test: no session window");
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let logoff = ENDSESSION_LOGOFF as isize;
            // SAFETY: plain calls, to a window of this process.
            unsafe {
                SendMessageW(window as _, WM_QUERYENDSESSION, 0, logoff);
                SendMessageW(window as _, WM_ENDSESSION, 1, logoff);
            }
            println!("{SESSION_END_ANSWERED}");
            // Windows may end the process once its windows have answered.
            std::process::exit(58);
        });
    }

    /// SIGTERM (what `kill` sends, and a system as it shuts down) ends the app as its window's
    /// close button does: its settings are saved, its jobs stopped at a checkpoint, and AI
    /// control's endpoint file, `app-endpoint.json` in the runtime folder, is removed, and so is
    /// its record in the memory pool (`memory/<pid>.json`). On the Mac, an app ended with SIGTERM
    /// left the endpoint file behind; an app that quit left its memory record.
    ///
    /// On Windows, a sign-out does the same (#319): the twin is told the session ends as Windows
    /// tells every program's windows (`WM_ENDSESSION`, sent to its session window once it is
    /// ready), and it ends before that message has been answered. Before, a sign-out ended it at
    /// once.
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
        let record = root
            .join("run")
            .join(jetcarp_core::machine::pool::DIR)
            .join(format!("{}.json", child.id()));
        assert!(record.exists(), "{}", record.display());

        #[cfg(unix)]
        {
            let pid = child.id().to_string();
            let sent = Command::new("kill").args(["-TERM", &pid]).status().unwrap();
            assert!(sent.success(), "kill -TERM {pid}");
        }
        // On Windows the twin tells itself the session ends, once it is ready.
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
        let answered = rx.try_iter().any(|l| l.contains(SESSION_END_ANSWERED));
        assert!(
            !answered,
            "the session's end was answered before the app had ended"
        );
        assert!(
            !record.exists(),
            "the app left its memory record {}",
            record.display()
        );
    }

    /// The twin: the app as `run` makes it (`crate::new_app`), recording its memory as the
    /// `jetcarp` program does, its photosensitive seizure warning answered in the scratch settings
    /// folder and AI control on at launch (`JETCARP_AI_CONTROL`), run frame by frame until its
    /// window is asked to close, and then ended as eframe ends it, through `on_exit`.
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
        jetcarp_core::machine::pool::start("the app", jetcarp_search::jobs::explore::memory_bytes);
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
            let mut out = ctx.run_pass(input, |ctx| {
                ctx.root_ui(|ui| {
                    eframe::App::logic(&mut app, ui.ctx(), &mut frame);
                    eframe::App::ui(&mut app, ui, &mut frame);
                });
            });
            // No painter takes the frame's textures here; a debug build refuses to drop them
            // unhandled (as the keyboard tests do).
            out.textures_delta.clear();
            if !said && endpoint.exists() {
                println!("{READY}");
                said = true;
                #[cfg(windows)]
                tell_the_session_ends();
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
