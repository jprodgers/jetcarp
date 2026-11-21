//! AI control: an opt-in local endpoint through which `jetcarp mcp --app` (and so an AI model) acts
//! on the world the user is watching.
//!
//! Off by default. When switched on, `jetcarp_mcp::link::Listener` listens on 127.0.0.1 only and
//! requires the token it records in the user's runtime directory. Switched off, the listener is
//! dropped: nothing accepts on its port and its endpoint file is gone ([`AiControl::switch_off`]). Requests are answered here, on
//! the UI thread, by the same `jetcarp_mcp::Server` the CLI uses; every world change goes through
//! `JetcarpApp::try_exec`, the path behind all of the app's buttons.

use super::JetcarpApp;
use eframe::egui::{self, Color32, RichText};
use jetcarp_core::Command;
use jetcarp_core::world::{Applied, Scene, World};
use jetcarp_mcp::link::{DEFAULT_PORT, Listener};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
pub struct AiControl {
    listener: Option<Listener>,
    server: jetcarp_mcp::Server,
    pub window_open: bool,
    /// `jetcarp --control`: switch on at launch (not saved).
    pub start_at_launch: bool,
    error: Option<String>,
    served: u64,
    last: String,
    env_checked: bool,
}

impl AiControl {
    /// Switch AI control on: listen on 127.0.0.1, on `port` or else any free port, recording the
    /// port and token in `endpoint`.
    fn switch_on(
        &mut self,
        port: u16,
        endpoint: Option<PathBuf>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) {
        let r = Listener::start_at(port, endpoint.clone(), wake.clone())
            .or_else(|_| Listener::start_at(0, endpoint, wake));
        match r {
            Ok(l) => {
                self.listener = Some(l);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not listen on 127.0.0.1: {e}")),
        }
    }

    /// Switch AI control off. Dropping the listener stops it accepting connections and removes
    /// its endpoint file, so the app takes no commands until it is switched on again
    /// (SECURITY.md).
    fn switch_off(&mut self) {
        self.listener = None;
    }
}

impl jetcarp_api::world::Host for JetcarpApp {
    fn world(&self) -> &World {
        &self.world
    }
    fn exec(&mut self, cmd: Command) -> Result<Applied, String> {
        self.try_exec(cmd)
    }
    fn replace(&mut self, scene: Scene) -> Result<(), String> {
        self.message = None;
        self.load_scene(&scene);
        if let Some((m, true)) = &self.message {
            return Err(m.clone());
        }
        // The model expects its world to hold still between calls; the user can press Run.
        self.playing = false;
        self.message = Some(("World replaced by AI control.".into(), false));
        Ok(())
    }
    fn scene(&self) -> Scene {
        self.current_scene()
    }
    fn info(&self) -> Value {
        json!({
            "kind": "app",
            "note": "this is the world shown in the JETCARP app window; the user can change it too",
            "playing": self.playing,
            "playing_note": if self.playing { "the app is running generations on its own (the user pressed Run), so the generation keeps advancing between calls" } else { "paused" },
            "scene_name": self.scene_name,
            "screensaver": self.saver_active(),
        })
    }
    fn has_display(&self) -> bool {
        true
    }
    fn display(&mut self, args: &Value) -> Result<Value, String> {
        self.display_api(args)
    }
}

impl JetcarpApp {
    pub(super) fn ai_stop(&mut self) {
        self.ai.switch_off();
    }

    pub(super) fn ai_start(&mut self, ctx: &egui::Context) {
        let c = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || c.request_repaint());
        self.ai
            .switch_on(DEFAULT_PORT, jetcarp_mcp::link::endpoint_file(), wake);
    }

    /// Answer queued requests. Called once per frame on the UI thread.
    /// `JETCARP_AI_CONTROL=1` (or the settings' `ai_control_at_launch`) switches AI control on at
    /// startup.
    pub(super) fn ai_poll(&mut self, ctx: &egui::Context) {
        if !self.ai.env_checked {
            self.ai.env_checked = true;
            if std::env::var("JETCARP_AI_CONTROL").is_ok_and(|v| v == "1" || v == "on")
                || self.shell.ai_at_launch
                || self.ai.start_at_launch
            {
                self.ai_start(ctx);
            }
        }
        let Some(l) = &self.ai.listener else { return };
        let pending = l.poll();
        if pending.is_empty() {
            return;
        }
        let mut server = std::mem::take(&mut self.ai.server);
        // Cancellations reach the link's flags while a call runs; progress goes back over the
        // link to `jetcarp mcp`, which writes it to its client.
        server.cancels = l.cancels.clone();
        for p in pending {
            let msg = p.message.clone();
            server.notifier = Some(p.notifier());
            if msg["method"] == "tools/call" {
                self.ai.served += 1;
                self.ai.last = msg["params"]["name"].as_str().unwrap_or("?").to_string();
            }
            let reply = server.handle(self, &msg);
            server.notifier = None;
            p.reply(reply);
        }
        self.ai.server = server;
        self.colors_dirty = true;
    }

    /// The port AI control listens on, when on.
    pub(super) fn ai_port(&self) -> Option<u16> {
        self.ai.listener.as_ref().map(|l| l.port)
    }

    pub(super) fn ai_window(&mut self, ctx: &egui::Context) {
        if !self.ai.window_open {
            return;
        }
        let mut open = true;
        let spec = crate::popout::Spec::new(super::menu::ToolWin::AiControl.key(), "AI control")
            .size(400.0, 260.0)
            .configure(|w| w.resizable(false));
        crate::popout::tool_window(ctx, spec, &mut open, |ui| {
            ui.label(
                "Lets an AI assistant drive the world in this window. Connect the assistant to \
                          JETCARP's MCP server with the command below (or plain jetcarp mcp, and its \
                          use_app tool), as the book's MCP tutorial shows. Every change it makes is \
                          a command, shown in the command log.",
            );
            ui.horizontal(|ui| {
                ui.label(RichText::new("jetcarp mcp --app").monospace());
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text("jetcarp mcp --app".into());
                }
            });
            ui.add_space(4.0);
            let mut on = self.ai.listener.is_some();
            if ui
                .checkbox(&mut on, "Allow AI control (127.0.0.1 only)")
                .changed()
            {
                if on {
                    self.ai_start(ctx);
                } else {
                    self.ai_stop();
                }
            }
            if let Some(e) = &self.ai.error {
                ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), e);
            }
            if let Some(l) = &self.ai.listener {
                egui::Grid::new("ai_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Address");
                    ui.label(RichText::new(format!("127.0.0.1:{}", l.port)).monospace());
                    ui.end_row();
                    ui.label("Token");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}…", &l.token[..8])).monospace());
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(l.token.clone());
                        }
                    });
                    ui.end_row();
                    // The links open now, not a running total: "Connections 0" after a client had come
                    // and gone read as if none ever had (#60).
                    ui.label("Open connections");
                    ui.label(l.connection_count().to_string());
                    ui.end_row();
                    ui.label("Tool calls");
                    let last = if self.ai.last.is_empty() {
                        String::new()
                    } else {
                        format!(" (last: {})", self.ai.last)
                    };
                    ui.label(format!("{}{last}", self.ai.served));
                    ui.end_row();
                });
                ui.label(RichText::new(
                        "The address is the private link that command uses to reach this window, \
                         not an MCP endpoint for HTTP clients: it speaks JETCARP's own protocol. The \
                         server finds the port and token by itself (they are stored in a file only you \
                         can read). Switching off closes it.",
                    ).small());
            }
        });
        if !open {
            self.ai.window_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::{Duration, Instant};

    /// Whether a client with `token` is let in on `port`: it connects, sends the token and is
    /// told `ok`.
    fn lets_in(port: u16, token: &str) -> bool {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let Ok(s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
            return false;
        };
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut w = s.try_clone().unwrap();
        if writeln!(w, "{}", json!({"jetcarp_token": token})).is_err() {
            return false;
        }
        let mut line = String::new();
        let _ = BufReader::new(s).read_line(&mut line);
        serde_json::from_str::<Value>(&line).is_ok_and(|v| v["ok"] == true)
    }

    /// Whether nothing accepts a connection on `port` within a few seconds. The listener's
    /// thread closes its socket within moments of being told to stop; until then the system
    /// still accepts connections for it.
    fn closed_soon(port: u16) -> bool {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            if TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_err() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// SECURITY.md: the app takes commands only while AI control is switched on. It starts off;
    /// on, it lets a client with the token in and records the endpoint; off, nothing accepts on
    /// the port it used and the endpoint file is gone; on again, a new listener lets a client in,
    /// with a new token.
    #[test]
    fn nothing_listens_while_ai_control_is_off() {
        let dir = std::env::temp_dir().join(format!("jetcarp-ai-control-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("app-endpoint.json");
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut ai = AiControl::default();
        assert!(ai.listener.is_none(), "AI control starts off");

        ai.switch_on(0, Some(file.clone()), wake.clone());
        let (port, token) = {
            let l = ai.listener.as_ref().expect("switched on");
            assert!(l.wrote_file());
            (l.port, l.token.clone())
        };
        assert!(file.exists(), "the endpoint is recorded");
        assert!(
            lets_in(port, &token),
            "on: a client with the token is let in"
        );

        ai.switch_off();
        assert!(!file.exists(), "off: the endpoint file is removed");
        assert!(closed_soon(port), "off: nothing accepts on port {port}");

        ai.switch_on(0, Some(file.clone()), wake);
        let l = ai.listener.as_ref().expect("switched on again");
        assert_ne!(l.token, token, "a new token each time");
        assert!(file.exists(), "the endpoint is recorded again");
        assert!(lets_in(l.port, &l.token), "on again: let in");
        ai.switch_off();
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
