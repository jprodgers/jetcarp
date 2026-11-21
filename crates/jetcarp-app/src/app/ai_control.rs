//! AI control: an opt-in local endpoint through which `jetcarp mcp --app` (and so an AI model) acts
//! on the world the user is watching.
//!
//! Off by default. When switched on, `jetcarp_mcp::link::Listener` listens on 127.0.0.1 only and
//! requires the token it records in the user's runtime directory. Switched off, the listener is
//! dropped: nothing accepts on its port and its endpoint file is gone ([`AiControl::switch_off`]). Requests are answered here, on
//! the UI thread, by the same `jetcarp_mcp::Server` the CLI uses; every world change goes through
//! `JetcarpApp::try_exec`, the path behind all of the app's buttons.
//!
//! While AI control is on, the window also offers MCP over HTTP (`jetcarp_mcp::http`), for clients
//! that connect to a URL: off until ticked, on 127.0.0.1 only, with a token of its own. Its
//! requests are answered here too, on the same world, with the same tools as the link's; switching
//! AI control off closes it as well. Two choices are saved with the app's settings
//! ([`AiHttpSettings`]), both off until made: keeping the token for the next time, and the exact
//! origins whose web pages may connect.

use super::JetcarpApp;
use eframe::egui::{self, Color32, RichText};
use jetcarp_core::Command;
use jetcarp_core::world::{Applied, Scene, World};
use jetcarp_mcp::link::{DEFAULT_PORT, Listener};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

/// MCP over HTTP's choices, saved with the app's settings (`ai_http`): whether the token is kept
/// for the next time it is served, and the other origins whose web pages may connect. Both off
/// until chosen.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default)]
pub(super) struct AiHttpSettings {
    pub keep_token: bool,
    pub allow_origins: Vec<String>,
}

impl AiHttpSettings {
    /// As read from a file someone may have edited: what is not an exact origin is dropped, and
    /// the rest put in the form browsers send, each once.
    pub fn sanitize(mut self) -> AiHttpSettings {
        let mut out: Vec<String> = vec![];
        for o in &self.allow_origins {
            if let Ok(o) = jetcarp_mcp::http::parse_origin(o)
                && !out.contains(&o)
            {
                out.push(o);
            }
        }
        self.allow_origins = out;
        self
    }
}

#[derive(Default)]
pub struct AiControl {
    listener: Option<Listener>,
    server: jetcarp_mcp::Server,
    /// MCP over HTTP, while ticked (only while AI control is on).
    http: Option<jetcarp_mcp::http::Listener>,
    http_server: jetcarp_mcp::Server,
    http_error: Option<String>,
    /// Saved with the app's settings.
    pub(super) http_settings: AiHttpSettings,
    /// The origin being typed into the window, and why the last one was refused.
    origin_text: String,
    origin_error: Option<String>,
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
    /// (SECURITY.md). The HTTP endpoint closes with it.
    fn switch_off(&mut self) {
        self.listener = None;
        self.http_off();
    }

    /// Serve MCP over HTTP: on 127.0.0.1, on `port` or else any free port, recording the URL and
    /// token in `endpoint`, with the saved choices (the kept token, the allowed origins). Only
    /// while AI control is on.
    fn http_on(&mut self, port: u16, endpoint: Option<PathBuf>, wake: Arc<dyn Fn() + Send + Sync>) {
        if self.listener.is_none() {
            return;
        }
        let opts = jetcarp_mcp::http::Options {
            keep_token: self.http_settings.keep_token,
            allow_origins: self.http_settings.allow_origins.clone(),
            ..Default::default()
        };
        let start = |port: u16| {
            jetcarp_mcp::http::Listener::start_with(
                port,
                endpoint.clone(),
                opts.clone(),
                wake.clone(),
                None,
            )
        };
        let r = start(port).or_else(|_| start(0));
        match r {
            Ok(l) => {
                self.http = Some(l);
                self.http_error = None;
            }
            Err(e) => self.http_error = Some(format!("Could not listen on 127.0.0.1: {e}")),
        }
    }

    /// Stop serving MCP over HTTP: nothing accepts on its port, and its endpoint file is gone
    /// (or keeps the token alone, when it is kept).
    fn http_off(&mut self) {
        self.http = None;
        self.http_server = jetcarp_mcp::Server::default();
    }

    /// Keep the token for the next time, or not (saved).
    fn set_http_keep(&mut self, keep: bool) {
        self.http_settings.keep_token = keep;
        if let Some(h) = &mut self.http {
            h.set_keep(keep);
        }
    }

    /// A new token: the old one stops working at once, and its sessions end.
    fn renew_http_token(&mut self) {
        if let Some(h) = &mut self.http {
            match h.renew_token() {
                Ok(()) => {
                    self.http_server = jetcarp_mcp::Server::default();
                    self.http_error = None;
                }
                Err(e) => self.http_error = Some(format!("No new token: {e}")),
            }
        }
    }

    /// Let web pages from the origin `text` connect (saved), if it is an exact one.
    pub(super) fn allow_http_origin(&mut self, text: &str) -> Result<(), String> {
        let o = jetcarp_mcp::http::parse_origin(text)?;
        if !self.http_settings.allow_origins.contains(&o) {
            self.http_settings.allow_origins.push(o);
        }
        self.apply_http_origins();
        Ok(())
    }

    /// No longer let web pages from the origin `o` connect (saved).
    fn remove_http_origin(&mut self, o: &str) {
        self.http_settings.allow_origins.retain(|x| x != o);
        self.apply_http_origins();
    }

    fn apply_http_origins(&self) {
        if let Some(h) = &self.http {
            h.set_origins(self.http_settings.allow_origins.clone());
        }
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

    fn ai_http_start(&mut self, ctx: &egui::Context) {
        let c = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || c.request_repaint());
        self.ai.http_on(
            jetcarp_mcp::http::DEFAULT_PORT,
            jetcarp_mcp::link::runtime_file(jetcarp_mcp::http::APP_ENDPOINT),
            wake,
        );
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
        let over_http = self.ai.http.as_ref().map(|h| h.poll()).unwrap_or_default();
        if pending.is_empty() && over_http.is_empty() {
            return;
        }
        let mut server = std::mem::take(&mut self.ai.server);
        // Cancellations reach the link's flags while a call runs; progress goes back over the
        // link to `jetcarp mcp`, which writes it to its client.
        server.cancels = l.cancels.clone();
        for p in pending {
            let msg = p.message.clone();
            server.notifier = Some(p.notifier());
            self.ai_count(&msg);
            let reply = server.handle(self, &msg);
            server.notifier = None;
            p.reply(reply);
        }
        self.ai.server = server;
        // MCP over HTTP: the same tools on the same world. Each session's cancellations reach its
        // own flags; progress goes back on the request's event stream.
        let mut server = std::mem::take(&mut self.ai.http_server);
        for p in over_http {
            let msg = p.message.clone();
            server.cancels = p.cancels();
            server.notifier = Some(p.notifier());
            self.ai_count(&msg);
            let reply = server.handle(self, &msg);
            server.notifier = None;
            p.reply(reply);
        }
        self.ai.http_server = server;
        self.colors_dirty = true;
    }

    /// Counts a tool call for the window.
    fn ai_count(&mut self, msg: &Value) {
        if msg["method"] == "tools/call" {
            self.ai.served += 1;
            self.ai.last = msg["params"]["name"].as_str().unwrap_or("?").to_string();
        }
    }

    /// The port AI control listens on, when on.
    pub(super) fn ai_port(&self) -> Option<u16> {
        self.ai.listener.as_ref().map(|l| l.port)
    }

    /// The port MCP over HTTP listens on, when on.
    #[cfg(test)]
    pub(super) fn ai_http_port(&self) -> Option<u16> {
        self.ai.http.as_ref().map(|l| l.port)
    }

    /// MCP over HTTP's two choices, under its URL and token: keeping the token for the next time
    /// (and making a new one), and the web pages' origins that may connect. Both saved.
    fn ai_http_choices(&mut self, ui: &mut egui::Ui) {
        let mut keep = self.ai.http_settings.keep_token;
        if ui
            .checkbox(&mut keep, "Keep this token for next time")
            .on_hover_text(
                "The client need not be given a new token each time; but whoever has this one \
                 keeps their access until you make a new one",
            )
            .changed()
        {
            self.ai.set_http_keep(keep);
        }
        if ui
            .button("New token")
            .on_hover_text("The old token stops working at once, and its clients must reconnect")
            .clicked()
        {
            self.ai.renew_http_token();
        }
        ui.add_space(4.0);
        ui.label("Web pages that may connect (exact origins; they need the token too):");
        let mut remove = None;
        for o in &self.ai.http_settings.allow_origins {
            ui.horizontal(|ui| {
                ui.label(RichText::new(o).monospace());
                if ui.small_button("Remove").clicked() {
                    remove = Some(o.clone());
                }
            });
        }
        if let Some(o) = remove {
            self.ai.remove_http_origin(&o);
        }
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.ai.origin_text)
                    .hint_text("https://client.example")
                    .desired_width(220.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Allow").clicked() || enter {
                let text = std::mem::take(&mut self.ai.origin_text);
                match self.ai.allow_http_origin(&text) {
                    Ok(()) => self.ai.origin_error = None,
                    Err(e) => {
                        self.ai.origin_error = Some(e);
                        self.ai.origin_text = text;
                    }
                }
            }
        });
        if let Some(e) = &self.ai.origin_error {
            ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), e);
        }
    }

    pub(super) fn ai_window(&mut self, ctx: &egui::Context) {
        if !self.ai.window_open {
            return;
        }
        let mut open = true;
        let spec = crate::popout::Spec::new(super::menu::ToolWin::AiControl.key(), "AI control")
            .size(420.0, 420.0)
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
                         can read). Switching off closes it. For a client that connects to a URL, \
                         serve MCP over HTTP below.",
                    ).small());
            }
            if self.ai.listener.is_some() {
                ui.add_space(4.0);
                ui.separator();
                let mut http_on = self.ai.http.is_some();
                if ui
                    .checkbox(&mut http_on, "Also serve MCP over HTTP (127.0.0.1 only)")
                    .on_hover_text(
                        "For MCP clients that connect to a URL (Postman, web tools) instead of \
                         starting jetcarp mcp",
                    )
                    .changed()
                {
                    if http_on {
                        self.ai_http_start(ctx);
                    } else {
                        self.ai.http_off();
                    }
                }
                if let Some(e) = &self.ai.http_error {
                    ui.colored_label(crate::theme::ink(ui, Color32::LIGHT_RED), e);
                }
                if let Some(h) = &self.ai.http {
                    let (url, token, header) = (h.url(), h.token.clone(), h.header());
                    egui::Grid::new("ai_http_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            for (name, shown, copied) in [
                                ("URL", url.clone(), url),
                                ("Token", token.clone(), token),
                                ("Header", "Authorization: Bearer …".to_string(), header),
                            ] {
                                ui.label(name);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(shown).monospace());
                                    if ui.small_button("Copy").clicked() {
                                        ui.ctx().copy_text(copied);
                                    }
                                });
                                ui.end_row();
                            }
                            ui.label("Open connections");
                            ui.label(h.connection_count().to_string());
                            ui.end_row();
                            ui.label("Sessions");
                            ui.label(h.session_ids().len().to_string());
                            ui.end_row();
                        });
                    ui.label(
                        RichText::new(
                            "Give the client the URL and the header (or the token, where it asks \
                             for a bearer token). It reaches this window's world with the same \
                             tools as above. Only programs on this computer can connect.",
                        )
                        .small(),
                    );
                }
                if self.ai.http.is_some() {
                    self.ai_http_choices(ui);
                }
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

    /// The status of a `GET /mcp` to the HTTP endpoint on `port`, with `token` if given: it
    /// needs no answer from the UI thread (401 without the token, 405 with it).
    fn http_status(port: u16, token: Option<&str>) -> Option<u16> {
        http_get(port, token, None)?.split(' ').nth(1)?.parse().ok()
    }

    /// The whole answer to a `GET /mcp`, with `token` and `origin` if given.
    fn http_get(port: u16, token: Option<&str>, origin: Option<&str>) -> Option<String> {
        use std::io::Read;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let origin = origin
            .map(|o| format!("Origin: {o}\r\n"))
            .unwrap_or_default();
        write!(
            s,
            "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}{origin}Connection: close\r\n\r\n"
        )
        .ok()?;
        let mut answer = String::new();
        let _ = s.read_to_string(&mut answer);
        Some(answer)
    }

    /// The app's two HTTP choices (saved with its settings): kept, the token comes back when
    /// HTTP is unticked and ticked again, until *New token* replaces it (the old one is refused
    /// at once); unkept, the file goes. Origins the user allows reach the endpoint, with CORS
    /// headers, as soon as they are added, and are refused again once removed; a wildcard is
    /// never added, and a saved file's wildcards are dropped.
    #[test]
    fn http_keeps_its_token_when_asked_and_lets_listed_pages_in() {
        let dir = std::env::temp_dir().join(format!("jetcarp-ai-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (link, file) = (
            dir.join("app-endpoint.json"),
            dir.join("app-http-endpoint.json"),
        );
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut ai = AiControl::default();
        ai.switch_on(0, Some(link), wake.clone());
        ai.set_http_keep(true);
        ai.http_on(0, Some(file.clone()), wake.clone());
        let t1 = ai.http.as_ref().unwrap().token.clone();
        ai.http_off();
        assert!(file.exists(), "kept: the file keeps the token");
        ai.http_on(0, Some(file.clone()), wake.clone());
        let (t, port) = {
            let h = ai.http.as_ref().unwrap();
            (h.token.clone(), h.port)
        };
        assert_eq!(t, t1, "the kept token comes back");
        ai.renew_http_token();
        let t2 = ai.http.as_ref().unwrap().token.clone();
        assert_ne!(t2, t1);
        assert_eq!(
            http_status(port, Some(&t1)),
            Some(401),
            "the old one is refused"
        );
        assert_eq!(http_status(port, Some(&t2)), Some(405));

        // Origins.
        let o = "https://client.example";
        let from = |o: &str| http_get(port, Some(&t2), Some(o)).unwrap_or_default();
        assert!(from(o).starts_with("HTTP/1.1 403"), "not listed yet");
        assert!(ai.allow_http_origin("*").is_err());
        assert!(ai.allow_http_origin("https://*.example").is_err());
        ai.allow_http_origin("HTTPS://Client.Example:443").unwrap();
        assert_eq!(ai.http_settings.allow_origins, vec![o.to_string()]);
        let a = from(o);
        assert!(a.starts_with("HTTP/1.1 405"), "{a}");
        assert!(
            a.contains(&format!("Access-Control-Allow-Origin: {o}\r\n")),
            "{a}"
        );
        assert!(from("https://other.example").starts_with("HTTP/1.1 403"));
        ai.remove_http_origin(o);
        assert!(from(o).starts_with("HTTP/1.1 403"), "removed");
        let saved = AiHttpSettings {
            keep_token: true,
            allow_origins: vec![
                "*".into(),
                o.into(),
                "https://client.example/".into(),
                o.into(),
            ],
        };
        assert_eq!(saved.sanitize().allow_origins, vec![o.to_string()]);

        // Unkept: the file goes when HTTP stops.
        ai.set_http_keep(false);
        ai.http_off();
        assert!(!file.exists());
        ai.switch_off();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SECURITY.md: MCP over HTTP is off until it is ticked, only while AI control is on, and
    /// closes when it is unticked or AI control is switched off; switched on again, AI control
    /// does not bring it back.
    #[test]
    fn http_listens_only_when_asked_and_closes_with_ai_control() {
        let dir = std::env::temp_dir().join(format!("jetcarp-ai-http-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (link, file) = (
            dir.join("app-endpoint.json"),
            dir.join("app-http-endpoint.json"),
        );
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut ai = AiControl::default();
        ai.http_on(0, Some(file.clone()), wake.clone());
        assert!(ai.http.is_none(), "not while AI control is off");
        assert!(!file.exists());

        ai.switch_on(0, Some(link.clone()), wake.clone());
        assert!(ai.http.is_none(), "AI control alone does not serve HTTP");
        assert!(!file.exists());

        ai.http_on(0, Some(file.clone()), wake.clone());
        let (port, token) = {
            let h = ai.http.as_ref().expect("ticked");
            assert_eq!(h.url(), format!("http://127.0.0.1:{}/mcp", h.port));
            (h.port, h.token.clone())
        };
        assert!(file.exists(), "the URL and token are recorded");
        assert_eq!(http_status(port, None), Some(401), "the token is needed");
        assert_eq!(http_status(port, Some(&token)), Some(405), "and enough");

        ai.http_off();
        assert!(!file.exists(), "unticked: the file is removed");
        assert!(
            closed_soon(port),
            "unticked: nothing accepts on port {port}"
        );
        assert!(ai.listener.is_some(), "AI control stays on");

        ai.http_on(0, Some(file.clone()), wake.clone());
        let port = {
            let h = ai.http.as_ref().expect("ticked again");
            assert_ne!(h.token, token, "a new token each time it is ticked");
            assert_eq!(
                http_status(h.port, Some(&token)),
                Some(401),
                "the old one fails"
            );
            h.port
        };
        ai.switch_off();
        assert!(
            ai.http.is_none() && !file.exists(),
            "closed with AI control"
        );
        assert!(closed_soon(port), "off: nothing accepts on port {port}");
        ai.switch_on(0, Some(link), wake);
        assert!(ai.http.is_none(), "AI control on again: HTTP stays off");
        ai.switch_off();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
