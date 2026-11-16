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
    let (tune, with_on, every_screen) = if fit {
        (
            "; tune sweeps cell sizes on screen (pick: recommended, finest, own or px)",
            "With wallpaper, login or tune",
            "next, previous and tune",
        )
    } else {
        ("", "With wallpaper or login", "next and previous")
    };
    let mut properties = json!({
        "action": {"type": "string", "enum": actions, "description": format!("Default status. wallpaper: every screen on the desktop background (GNOME: with the extension) or in a window; login: start at login or not; open_app opens the app; reload rereads host.json{tune}; scene: a screen's scene.")},
        "on": {"type": "boolean", "description": format!("{with_on}: on or off. Default on.")},
        "name": {"type": "string", "description": "With rotation: the one to run (saved)."},
        "screen": {"type": "integer", "minimum": 0, "description": format!("Counted from 0. Default: every screen for {every_screen}, else the first.")},
    });
    if fit {
        properties["cpu_limit"] = json!({"type": "number"});
        properties["gpu_limit"] = json!({"type": "number"});
        properties["fps"] = json!({"type": "number"});
        properties["pick"] = json!({"type": ["string", "number"]});
    }
    json!({
        "name": crate::link::HOST_TOOL,
        "title": "JETCARP background host",
        "description": "Controls the JETCARP background host, which runs live scenes as wallpapers or windows without the app. Returns its status (each screen's scene, cell size and caption, and recent problems). Fails if it is not running (`jetcarp host start` starts it). Refuses scenes until the user accepts the seizure warning.",
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

/// Whether a `host_control` call would start or show scenes: refused, with the warning, until
/// the photosensitive seizure warning has been accepted (#96). Reading the status, pausing,
/// hiding, quitting and opening the app (which shows the warning itself) are not.
fn host_call_shows(args: &Value) -> bool {
    let on = args.get("on").and_then(Value::as_bool).unwrap_or(true);
    match args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("status")
    {
        "resume" | "next" | "previous" | "rotation" => true,
        "wallpaper" | "login" | "tune" => on,
        _ => false,
    }
}

/// A `host_control` call, refused while the warning stands in its way.
fn host_control(args: &Value) -> Result<Value, String> {
    if host_call_fits(args) {
        jetcarp_core::fit::available()?;
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

    /// Forward a tools/call; failures become tool errors so the model sees what to do.
    fn forward(&mut self, msg: &Value) -> Value {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let r = self.client().and_then(|c| {
            c.call_with(msg, &mut |note| {
                // Progress from the app, passed on as it comes.
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{note}");
                let _ = out.flush();
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

fn info_from(
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
    let reply = link.forward(&probe);
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

/// Serve MCP on stdin/stdout until stdin closes, then stop the jobs running on the server's own
/// threads ([`stop_own_jobs`]). `app`: start attached to the JETCARP app.
pub fn run(app: bool, port: Option<u16>, token: Option<String>) -> Result<(), String> {
    let mut server = Server::new();
    server.extra_tools.push(use_app_definition());
    server.extra_tools.push(host_control_definition());
    // App-only tools are forwarded to the app (an error while not attached).
    server.extra_tools.extend(crate::app_tools::definitions());
    let mut session = Session::new();
    let mut link = Link {
        attached: app,
        client: None,
        info: info_from(port, token)?,
        app_writer: Default::default(),
    };
    if app {
        match link.client() {
            Ok(_) => {}
            Err(e) => eprintln!("jetcarp mcp: {e} (will retry on each tool call)"),
        }
    }
    // stdout is locked per message: progress notifications come from worker threads while a
    // request runs.
    server.notifier = Some(std::sync::Arc::new(|msg: Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{msg}");
        let _ = out.flush();
    }));
    // stdin on its own thread: cancellations act at once, the rest is answered in order.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    {
        let cancels = server.cancels.clone();
        let app_writer = link.app_writer.clone();
        std::thread::Builder::new()
            .name("jetcarp-mcp-stdin".into())
            .spawn(move || {
                for line in std::io::stdin().lock().lines() {
                    let Ok(line) = line else { return };
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(&line)
                        && protocol::cancel_request(&cancels, &v)
                    {
                        if let Some(w) = app_writer.lock().unwrap().as_mut() {
                            let _ = writeln!(w, "{line}");
                            let _ = w.flush();
                        }
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
        let reply: Option<String> = match serde_json::from_str::<Value>(&line) {
            Ok(msg) if msg["method"] == "tools/call" && msg.get("id").is_some() => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let id = msg["id"].clone();
                if name == crate::link::HOST_TOOL {
                    let args = msg["params"]["arguments"].clone();
                    let o = match host_control(&args) {
                        Ok(v) => ToolOutput::text(v.to_string()),
                        Err(e) => ToolOutput::error(e),
                    };
                    Some(protocol::response(&id, protocol::tool_result(o)).to_string())
                } else if name == "use_app" {
                    let args = msg["params"]["arguments"].clone();
                    let o = use_app(&mut link, &mut session, &args);
                    if !link.attached {
                        *link.app_writer.lock().unwrap() = None;
                    }
                    Some(protocol::response(&id, protocol::tool_result(o)).to_string())
                } else if link.attached && (tools::exists(name) || crate::app_tools::handles(name))
                {
                    Some(link.forward(&msg).to_string())
                } else {
                    server.handle(&mut session, &msg).map(|r| r.to_string())
                }
            }
            Ok(msg) => server.handle(&mut session, &msg).map(|r| r.to_string()),
            Err(_) => server.handle_line(&mut session, &line),
        };
        if let Some(r) = reply {
            let mut out = std::io::stdout().lock();
            writeln!(out, "{r}").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
    stop_own_jobs();
    Ok(())
}

/// How long the server waits, once its client has gone, for its own jobs to stop.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// stdin has closed: the client has gone, and the server exits. The searches started on its own
/// threads (`search_start` without `detach`) would end with it, left marked running with
/// nothing running them (#64): stop each at its next checkpoint, as `job_stop` does, so it is
/// left paused and resumable. Searches in processes of their own carry on.
fn stop_own_jobs() {
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
        eprintln!(
            "jetcarp mcp: the client has gone; stopping job {} at its next checkpoint",
            ids.join(", ")
        );
    }
    for (job, ended) in reg.stop_all(STOP_WAIT) {
        if !running.contains(&job) {
            continue;
        }
        match ended {
            Some(Ok(o)) => eprintln!(
                "jetcarp mcp: job {job} is {}",
                jetcarp_search::jobs::status_name(o.status)
            ),
            Some(Err(e)) => eprintln!("jetcarp mcp: job {job} failed: {e}"),
            None => eprintln!(
                "jetcarp mcp: job {job} had not stopped after {} s; `jetcarp jobs resume {job}` \
                 carries it on",
                STOP_WAIT.as_secs()
            ),
        }
    }
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

    /// The `host_control` calls that start or show scenes wait for the photosensitive seizure
    /// warning; reading, pausing, hiding and quitting do not (#96).
    #[test]
    fn host_calls_that_start_or_show_scenes_are_known() {
        for a in [
            json!({"action": "resume"}),
            json!({"action": "next"}),
            json!({"action": "previous", "screen": 1}),
            json!({"action": "rotation", "name": "x"}),
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
        ] {
            assert!(!host_call_shows(&a), "{a}");
        }
    }
}
