//! AI control: an opt-in local endpoint through which `jetcarp mcp --app` (and so an AI model) acts
//! on the world the user is watching.
//!
//! Off by default. When switched on, `jetcarp_mcp::link::Listener` listens on 127.0.0.1 only and
//! requires the token it records in the user's runtime directory. Requests are answered here, on
//! the UI thread, by the same `jetcarp_mcp::Server` the CLI uses; every world change goes through
//! `JetcarpApp::try_exec`, the path behind all of the app's buttons.

use super::JetcarpApp;
use jetcarp_core::Command;
use jetcarp_core::world::{Applied, Scene, World};
use jetcarp_mcp::link::{DEFAULT_PORT, Listener};
use eframe::egui::{self, Color32, RichText};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Default)]
pub struct AiControl {
    listener: Option<Listener>,
    server: jetcarp_mcp::Server,
    pub window_open: bool,
    /// `jetcarp-app --control`: switch on at launch (not saved).
    pub start_at_launch: bool,
    error: Option<String>,
    served: u64,
    last: String,
    env_checked: bool,
}

impl jetcarp_mcp::Host for JetcarpApp {
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
        self.ai.listener = None;
    }

    pub(super) fn ai_start(&mut self, ctx: &egui::Context) {
        let c = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || c.request_repaint());
        let r = Listener::start(DEFAULT_PORT, wake.clone()).or_else(|_| Listener::start(0, wake));
        match r {
            Ok(l) => {
                self.ai.listener = Some(l);
                self.ai.error = None;
            }
            Err(e) => self.ai.error = Some(format!("Could not listen on 127.0.0.1: {e}")),
        }
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
            .size(380.0, 220.0)
            .configure(|w| w.resizable(false));
        crate::popout::tool_window(ctx, spec, &mut open, |ui| {
            ui.label(
                "Lets an AI model drive the world in this window through the MCP server: \
                          `jetcarp mcp --app`, or `jetcarp mcp` and its use_app tool. \
                          Every change it makes is a command, shown in the command log.",
            );
            ui.add_space(4.0);
            let mut on = self.ai.listener.is_some();
            if ui
                .checkbox(&mut on, "Allow AI control (127.0.0.1 only)")
                .changed()
            {
                if on {
                    self.ai_start(ctx);
                } else {
                    self.ai.listener = None;
                }
            }
            if let Some(e) = &self.ai.error {
                ui.colored_label(Color32::LIGHT_RED, e);
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
                    ui.label("Connections");
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
                        "The CLI finds the port and token automatically (they are stored in a file only you can read). \
                         Switching off closes the endpoint.",
                    ).small());
            }
        });
        if !open {
            self.ai.window_open = false;
        }
    }
}
