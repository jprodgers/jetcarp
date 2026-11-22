//! The MCP server on stdin/stdout, used by `jetcarp mcp` and the standalone `jetcarp-mcp` binary.
//!
//! Requests are answered by `jetcarp_mcp::Server` against a private headless world, or, once
//! attached to the JETCARP app (`--app` or the `use_app` tool), world tools are forwarded over the
//! local link and run by the app on its UI thread. stdout carries only JSON-RPC; logs go to stderr.
//!
//! stdin is read on a thread of its own, so a `notifications/cancelled` reaches the call it names
//! while that call runs (here, or in the app: the cancellation is written to the app link at once),
//! and the app's `notifications/progress` for a forwarded call are written to stdout as they come.
//!
//! When stdin closes (the client has gone), the searches running on the server's own threads are
//! stopped at their next checkpoint, paused and resumable, before the server exits.
//!
//! [`Client`] is one client's state, whatever carries its messages: stdin and stdout here, or one
//! session of the HTTP endpoint ([`crate::http`]), so both offer the same tools on the same terms.

use crate::link::{AppClient, EndpointInfo};
use crate::{Server, Session, ToolOutput, protocol, tools};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn use_app_definition() -> Value {
    json!({
        "name": "use_app",
        "title": "Attach to the JETCARP app",
        "description": "Chooses which world the other tools act on. attach=true: the world in the JETCARP desktop app the user is watching, so they see every change as it happens (the user must first switch it on in the app: Tools, AI control..., Allow AI control; or start the app with `jetcarp --control`. The app then also offers app_display, its displays and screensaver). attach=false: this server's own private world (the default unless the server was started with --app). Returns the attached world's status.",
        "inputSchema": {"type": "object", "properties": {
            "attach": {"type": "boolean", "description": "true = act on the app's world, false = the private world. Default true."},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "description": "App port, if not the one the app recorded."},
            "token": {"type": "string", "description": "App token shown in its AI control window, if needed."},
        }, "additionalProperties": false},
        "annotations": crate::tools::annotations("Attach to the JETCARP app", false, false, true),
    })
}

fn host_control_definition() -> Value {
    // Fit to this computer's `tune` and its options, only while Fit is in this release (#112).
    let fit = jetcarp_core::fit::FIT;
    let mut actions = vec![
        "status",
        "next",
        "previous",
        "pause",
        "resume",
        "rotation",
        "wallpaper",
        "login",
        "open_app",
        "reload",
    ];
    if fit {
        actions.push("tune");
    }
    actions.extend(["scene", "quit"]);
    // The actions and options are told in the help topic `host` (#113): tools/list goes to
    // the model whole with every request, so the tool only names it.
    let mut properties = json!({
        "action": {"type": "string", "enum": actions, "description": "Default status."},
        "on": {"type": "boolean"},
        "name": {"type": "string"},
        "screen": {"type": "integer", "minimum": 0},
    });
    if fit {
        properties["cpu_limit"] = json!({"type": "number"});
        properties["gpu_limit"] = json!({"type": "number"});
        properties["fps"] = json!({"type": "number"});
        properties["pick"] = json!({"type": ["string", "number"]});
        properties["fresh"] = json!({"type": "boolean"});
    }
    json!({
        "name": crate::link::HOST_TOOL,
        "title": "JETCARP background host",
        "description": "Controls the JETCARP background host (live wallpapers or windows, no app needed); returns its status. Needs `jetcarp host start`; scenes wait for the seizure warning. Actions, options: help {\"topic\": \"host\"}.",
        "inputSchema": {"type": "object", "properties": properties, "additionalProperties": false},
        "annotations": crate::tools::annotations("JETCARP background host", false, false, false),
    })
}

/// Whether a `host_control` call asks for *Fit to this computer*: a sweep, its news, or a tuner's
/// hold. Refused while Fit is switched off (#112), whether or not the schema lists it.
fn host_call_fits(args: &Value) -> bool {
    matches!(
        args.get("action").and_then(Value::as_str),
        Some("tune" | "tune_stop" | "tune_events" | "tuning")
    )
}

/// Whether a `host_control` call would start showing scenes: refused, with the warning, until
/// the photosensitive seizure warning has been accepted (#96), as `jetcarp host wallpaper on` and
/// `login on` are. The rest go to the host as on the command line: reading the status, pausing,
/// hiding, quitting, opening the app (which shows the warning itself), and resuming, the next or
/// previous scene and choosing a rotation, since the host holds every screen until the warning is
/// accepted and its status says so (#125).
fn host_call_shows(args: &Value) -> bool {
    let on = args.get("on").and_then(Value::as_bool).unwrap_or(true);
    match args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("status")
    {
        "wallpaper" | "login" | "tune" => on,
        _ => false,
    }
}

/// A `host_control` call, refused while the warning stands in its way, and refused, naming the
/// actions, when its action isn't one the tool lists, before any host is asked (#125).
fn host_control(args: &Value) -> Result<Value, String> {
    if host_call_fits(args) {
        jetcarp_core::fit::available()?;
    }
    if let Some(a) = args.get("action").and_then(Value::as_str) {
        let def = host_control_definition();
        let known: Vec<&str> = def["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .map(|v| v.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !known.contains(&a) {
            return Err(format!(
                "{}: `action` must be one of {} (got \"{a}\")",
                crate::link::HOST_TOOL,
                known.join(", ")
            ));
        }
    }
    if host_call_shows(args) {
        jetcarp_core::photosensitivity::require("start or show scenes in the background host")?;
    }
    crate::link::host_call(args)
}

struct Link {
    attached: bool,
    client: Option<AppClient>,
    info: Option<EndpointInfo>,
    /// A writer on the app connection, for the stdin thread to forward cancellations.
    app_writer: std::sync::Arc<std::sync::Mutex<Option<std::net::TcpStream>>>,
}

impl Link {
    fn client(&mut self) -> std::result::Result<&mut AppClient, String> {
        if self.client.is_none() {
            let c = AppClient::connect(self.info.clone())?;
            eprintln!(
                "jetcarp mcp: connected to the JETCARP app on port {}",
                c.port
            );
            *self.app_writer.lock().unwrap() = c.writer();
            self.client = Some(c);
        }
        Ok(self.client.as_mut().unwrap())
    }

    /// Forward a tools/call; failures become tool errors so the model sees what to do. The app's
    /// notifications meanwhile (progress) go to `notify`, as they come.
    fn forward(&mut self, msg: &Value, notify: Option<&protocol::Notifier>) -> Value {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let r = self.client().and_then(|c| {
            c.call_with(msg, &mut |note| {
                if let Some(n) = notify {
                    n(note.clone());
                }
            })
        });
        match r {
            Ok(v) => v,
            Err(e) => {
                self.client = None;
                *self.app_writer.lock().unwrap() = None;
                protocol::response(
                    &id,
                    protocol::tool_result(ToolOutput::error(format!(
                        "{e} (to use this server's private world instead, call use_app with attach=false)"
                    ))),
                )
            }
        }
    }
}

/// The app's endpoint from `--port` and `--token`, the rest from the file the app recorded
/// (`None`: neither was given, so the file is read when the app is first reached).
pub(crate) fn info_from(
    port: Option<u16>,
    token: Option<String>,
) -> std::result::Result<Option<EndpointInfo>, String> {
    match (port, token) {
        (None, None) => Ok(None),
        (p, t) => {
            let file = EndpointInfo::read().ok();
            let port = p
                .or(file.as_ref().map(|f| f.port))
                .ok_or("give the app port too")?;
            let token = t
                .or(file.map(|f| f.token))
                .ok_or("give the token shown in the app's AI control window")?;
            Ok(Some(EndpointInfo { port, token }))
        }
    }
}

fn use_app(link: &mut Link, session: &mut Session, args: &Value) -> ToolOutput {
    let attach = args.get("attach").and_then(|a| a.as_bool()).unwrap_or(true);
    if !attach {
        link.attached = false;
        link.client = None;
        return ToolOutput::text(format!(
            "Using this server's private world.\n{}",
            tools::call(session, "world_status", &json!({})).joined_text()
        ));
    }
    let port = args.get("port").and_then(|p| p.as_u64()).map(|p| p as u16);
    let token = args.get("token").and_then(|t| t.as_str()).map(String::from);
    if port.is_some() || token.is_some() {
        match info_from(port, token) {
            Ok(i) => link.info = i,
            Err(e) => return ToolOutput::error(e),
        }
    }
    link.client = None;
    let probe = json!({"jsonrpc": "2.0", "id": "use_app", "method": "tools/call",
                       "params": {"name": "world_status", "arguments": {}}});
    let reply = link.forward(&probe, None);
    let result = &reply["result"];
    if result["isError"] == json!(false) {
        link.attached = true;
        let status = result["content"][0]["text"].as_str().unwrap_or("");
        ToolOutput::text(format!(
            "Attached to the JETCARP app: the tools now act on the world the user is watching.\n{status}"
        ))
    } else {
        let text = result["content"][0]["text"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| reply.to_string());
        ToolOutput::error(text)
    }
}

/// One MCP client of `jetcarp mcp`, whatever carries its messages (stdin and stdout, or one
/// session of the HTTP endpoint): its protocol state, its private world, and its link to the app.
/// Every client is offered the same tools: the shared ones, `use_app`, `host_control`, and
/// `app_display`, which is passed to the app once attached.
pub struct Client {
    /// The protocol state. The transport sets its `notifier` (where progress goes), and may give
    /// it a `cancels` map of its own before taking a [`Canceller`].
    pub server: Server,
    session: Session,
    link: Link,
}

impl Client {
    /// A client acting on a private world, or (`app`) on the world in the JETCARP app, reached at
    /// `info` (`None`: the endpoint the app recorded).
    pub fn new(app: bool, info: Option<EndpointInfo>) -> Client {
        let mut server = Server::new();
        server.extra_tools.push(use_app_definition());
        server.extra_tools.push(host_control_definition());
        // App-only tools are forwarded to the app (an error while not attached).
        server.extra_tools.extend(crate::app_tools::definitions());
        Client {
            server,
            session: Session::new(),
            link: Link {
                attached: app,
                client: None,
                info,
                app_writer: Default::default(),
            },
        }
    }

    /// When attached, connect to the app now, so that a missing app is reported at the start
    /// (the connection is tried again on each tool call).
    pub fn connect_app(&mut self) {
        if self.link.attached
            && let Err(e) = self.link.client()
        {
            eprintln!("jetcarp mcp: {e} (will retry on each tool call)");
        }
    }

    /// Answer one message: `host_control` and `use_app` here, the world tools passed to the app
    /// while attached, everything else by the server. `None`: no reply is due.
    pub fn answer(&mut self, msg: &Value) -> Option<Value> {
        if msg["method"] == "tools/call" && msg.get("id").is_some() {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let id = msg["id"].clone();
            if name == crate::link::HOST_TOOL {
                let args = msg["params"]["arguments"].clone();
                let o = match host_control(&args) {
                    Ok(v) => ToolOutput::text(v.to_string()),
                    Err(e) => ToolOutput::error(e),
                };
                return Some(protocol::response(&id, protocol::tool_result(o)));
            } else if name == "use_app" {
                let args = msg["params"]["arguments"].clone();
                let o = use_app(&mut self.link, &mut self.session, &args);
                if !self.link.attached {
                    *self.link.app_writer.lock().unwrap() = None;
                }
                return Some(protocol::response(&id, protocol::tool_result(o)));
            } else if self.link.attached && (tools::exists(name) || crate::app_tools::handles(name))
            {
                let notify = self.server.notifier.clone();
                return Some(self.link.forward(msg, notify.as_ref()));
            }
        }
        self.server.handle(&mut self.session, msg)
    }

    /// [`answer`](Self::answer) one line of input, which may not be JSON at all.
    pub fn answer_line(&mut self, line: &str) -> Option<String> {
        match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.answer(&msg).map(|r| r.to_string()),
            Err(_) => self.server.handle_line(&mut self.session, line),
        }
    }

    /// What another thread needs to cancel this client's calls while one runs.
    pub fn canceller(&self) -> Canceller {
        Canceller {
            cancels: self.server.cancels.clone(),
            app_writer: self.link.app_writer.clone(),
        }
    }
}

/// Cancels a [`Client`]'s calls from another thread: the transport's reader, which goes on
/// reading while a call runs.
#[derive(Clone)]
pub struct Canceller {
    cancels: protocol::CancelMap,
    /// A writer on the app connection, while attached.
    app_writer: std::sync::Arc<std::sync::Mutex<Option<std::net::TcpStream>>>,
}

impl Canceller {
    /// If `msg` is a `notifications/cancelled`: set the flag of the call it names, and pass it to
    /// the app, where a forwarded call runs. Returns whether it was one.
    pub fn cancel(&self, msg: &Value) -> bool {
        if !protocol::cancel_request(&self.cancels, msg) {
            return false;
        }
        if let Some(w) = self.app_writer.lock().unwrap().as_mut() {
            let _ = writeln!(w, "{msg}");
            let _ = w.flush();
        }
        true
    }
}

/// Serve MCP on stdin/stdout until stdin closes, then stop the jobs running on the server's own
/// threads ([`stop_own_jobs`]). `app`: start attached to the JETCARP app.
pub fn run(app: bool, port: Option<u16>, token: Option<String>) -> Result<(), String> {
    let mut client = Client::new(app, info_from(port, token)?);
    client.connect_app();
    // stdout is locked per message: progress notifications come from worker threads while a
    // request runs.
    client.server.notifier = Some(std::sync::Arc::new(|msg: Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{msg}");
        let _ = out.flush();
    }));
    // stdin on its own thread: cancellations act at once, the rest is answered in order.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    {
        let canceller = client.canceller();
        std::thread::Builder::new()
            .name("jetcarp-mcp-stdin".into())
            .spawn(move || {
                for line in std::io::stdin().lock().lines() {
                    let Ok(line) = line else { return };
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(&line)
                        && canceller.cancel(&v)
                    {
                        continue;
                    }
                    if tx.send(line).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }
    for line in rx {
        if let Some(r) = client.answer_line(&line) {
            let mut out = std::io::stdout().lock();
            writeln!(out, "{r}").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
    stop_own_jobs("the client has gone");
    Ok(())
}

/// How long the server waits, once its client has gone, for its own jobs to stop.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// stdin has closed: the client has gone, and the server exits (`why` says so: "the client has
/// gone", or "the server is closing" as `jetcarp mcp --http` stops). The searches started on its
/// own threads (`search_start` without `detach`) would end with it, left marked running with
/// nothing running them (#64): stop each at its next checkpoint, as `job_stop` does, so it is
/// left paused and resumable. Searches in processes of their own carry on.
pub(crate) fn stop_own_jobs(why: &str) {
    let reg = tools::ctx().registry();
    let running: Vec<i64> = {
        let lib = tools::ctx().library();
        reg.jobs(&lib)
            .into_iter()
            .filter(|j| reg.is_running(&lib, *j))
            .collect()
    };
    if !running.is_empty() {
        let ids: Vec<String> = running.iter().map(|j| j.to_string()).collect();
        say(format_args!(
            "jetcarp mcp: {why}; stopping job {} at its next checkpoint",
            ids.join(", ")
        ));
    }
    for (job, ended) in reg.stop_all(STOP_WAIT) {
        if !running.contains(&job) {
            continue;
        }
        match ended {
            Some(Ok(o)) => say(format_args!(
                "jetcarp mcp: job {job} is {}",
                jetcarp_search::jobs::status_name(o.status)
            )),
            Some(Err(e)) => say(format_args!("jetcarp mcp: job {job} failed: {e}")),
            None => say(format_args!(
                "jetcarp mcp: job {job} had not stopped after {} s; `jetcarp jobs resume {job}` \
                 carries it on",
                STOP_WAIT.as_secs()
            )),
        }
    }
}

/// A line on standard error that never panics, as `eprintln!` does when standard error has gone:
/// the terminal a server was started from closing (SIGHUP) stops it, and it says what it stops
/// as it goes.
fn say(line: std::fmt::Arguments) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

#[cfg(test)]
mod tests {
    use super::{host_call_fits, host_call_shows, host_control, host_control_definition};
    use serde_json::json;

    /// While *Fit to this computer* is switched off (0.4.2, #112), `host_control` has no `tune`
    /// action or options in its schema, and refuses one all the same, before asking any host.
    #[test]
    fn fit_is_not_offered_or_done_while_switched_off() {
        if jetcarp_core::fit::FIT {
            return;
        }
        let d = host_control_definition();
        let props = &d["inputSchema"]["properties"];
        let actions = props["action"]["enum"].as_array().unwrap();
        assert!(!actions.iter().any(|a| a == "tune"), "{actions:?}");
        assert!(actions.iter().any(|a| a == "scene"), "{actions:?}");
        for k in ["cpu_limit", "gpu_limit", "fps", "pick"] {
            assert!(props.get(k).is_none(), "{k} in {props}");
        }
        assert!(!d.to_string().contains("tune"), "{d}");
        for a in ["tune", "tune_stop", "tune_events", "tuning"] {
            let call = json!({"action": a, "on": false});
            assert!(host_call_fits(&call));
            assert_eq!(
                host_control(&call).unwrap_err(),
                jetcarp_core::fit::HELD_BACK,
                "{a}"
            );
        }
        assert!(!host_call_fits(&json!({"action": "status"})));
    }

    /// With Fit in the release (0.4.3, #113), `host_control` offers `tune` with its limits and a
    /// pick, placed from the last fit while it is current (#77); `tune` without one measures.
    #[test]
    fn fit_is_offered_with_its_options() {
        if !jetcarp_core::fit::FIT {
            return;
        }
        let d = host_control_definition();
        let props = &d["inputSchema"]["properties"];
        let actions = props["action"]["enum"].as_array().unwrap();
        assert!(actions.iter().any(|a| a == "tune"), "{actions:?}");
        for k in ["cpu_limit", "gpu_limit", "fps", "pick", "fresh"] {
            assert!(props.get(k).is_some(), "{k} in {props}");
        }
        let words = crate::help_tools::help_host();
        assert!(
            words.contains("- tune: fit the desktop background"),
            "{words}"
        );
        for k in ["cpu_limit", "gpu_limit", "fps", "pick", "fresh", "screen"] {
            assert!(words.contains(&format!("`{k}`")), "{k} in {words}");
        }
    }

    /// `host_control` is short again: its long text is the help topic `host`, which its
    /// description names, so `tools/list` stays well within its budget with every one of Fit's
    /// options back, the frame rate and `fresh` included (#113).
    #[test]
    fn host_control_is_short_and_its_text_is_a_help_topic() {
        if !jetcarp_core::fit::FIT {
            return;
        }
        let d = host_control_definition();
        let size = d.to_string().len();
        assert!(size < 900, "host_control is {size} bytes: {d}");
        let desc = d["description"].as_str().unwrap();
        assert!(desc.contains(r#"help {"topic": "host"}"#), "{desc}");
        let props = &d["inputSchema"]["properties"];
        for k in ["cpu_limit", "gpu_limit", "fps", "pick", "fresh"] {
            assert!(props.get(k).is_some(), "{k} in {props}");
        }
    }

    /// The `host_control` calls that start showing scenes wait for the photosensitive seizure
    /// warning (#96); the rest go to the host, which holds every screen until it is accepted, as
    /// the command line's do (#125).
    #[test]
    fn host_calls_that_start_or_show_scenes_are_known() {
        for a in [
            json!({"action": "wallpaper"}),
            json!({"action": "wallpaper", "on": true}),
            json!({"action": "login"}),
            json!({"action": "tune", "dry_run": true}),
        ] {
            assert!(host_call_shows(&a), "{a}");
        }
        for a in [
            json!({}),
            json!({"action": "status"}),
            json!({"action": "pause"}),
            json!({"action": "wallpaper", "on": false}),
            json!({"action": "login", "on": false}),
            json!({"action": "tune", "on": false}),
            json!({"action": "scene"}),
            json!({"action": "reload"}),
            json!({"action": "open_app"}),
            json!({"action": "quit"}),
            json!({"action": "resume"}),
            json!({"action": "next"}),
            json!({"action": "previous", "screen": 1}),
            json!({"action": "rotation", "name": "x"}),
        ] {
            assert!(!host_call_shows(&a), "{a}");
        }
    }

    /// An action the tool doesn't list is refused, naming the ones it does, before any host is
    /// asked (#125); with no host running, "not running" would otherwise be the answer.
    #[test]
    fn an_unknown_action_is_refused_naming_the_actions() {
        let e = host_control(&json!({"action": "start"})).unwrap_err();
        assert!(e.contains("`action` must be one of"), "{e}");
        assert!(e.contains("status") && e.contains("(got \"start\")"), "{e}");
    }
}
