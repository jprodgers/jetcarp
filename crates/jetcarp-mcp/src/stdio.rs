//! The MCP server on stdin/stdout, used by `jetcarp mcp` and the standalone `jetcarp-mcp` binary.
//!
//! Requests are answered by `jetcarp_mcp::Server` against a private headless world, or, once
//! attached to the JETCARP app (`--app` or the `use_app` tool), world tools are forwarded over the
//! local link and run by the app on its UI thread. stdout carries only JSON-RPC; logs go to stderr.
//!
//! stdin is read on a thread of its own, so a `notifications/cancelled` reaches the call it names
//! while that call runs (here, or in the app: the cancellation is written to the app link at once),
//! and the app's `notifications/progress` for a forwarded call are written to stdout as they come.

use crate::link::{AppClient, EndpointInfo};
use crate::{Server, Session, ToolOutput, protocol, tools};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn use_app_definition() -> Value {
    json!({
        "name": "use_app",
        "title": "Attach to the JETCARP app",
        "description": "Chooses which world the other tools act on. attach=true: the world in the JETCARP desktop app the user is watching, so they see every change as it happens (the user must first switch on 'AI control' in the app's top bar; the app then also offers app_display, its displays and screensaver). attach=false: this server's own private world (the default unless the server was started with --app). Returns the attached world's status.",
        "inputSchema": {"type": "object", "properties": {
            "attach": {"type": "boolean", "description": "true = act on the app's world, false = the private world. Default true."},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "description": "App port, if not the one the app recorded."},
            "token": {"type": "string", "description": "App token shown in its AI control window, if needed."},
        }, "additionalProperties": false},
        "annotations": {"title": "Attach to the JETCARP app", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
    })
}

fn host_control_definition() -> Value {
    json!({
        "name": crate::link::HOST_TOOL,
        "title": "JETCARP background host",
        "description": "Controls the JETCARP background host, the separate program that keeps live scenes running as wallpapers or windows without the app. Returns its status: each screen's rotation, item, generation and caption, memory use, and recent problems. Fails if the host is not running; the user starts it with `jetcarp host start`. Examples: {}, {\"action\": \"next\"}, {\"action\": \"pause\"}, {\"action\": \"wallpaper\", \"on\": false}.",
        "inputSchema": {"type": "object", "properties": {
            "action": {"type": "string", "enum": ["status", "next", "previous", "pause", "resume", "rotation", "wallpaper", "login", "open_app", "reload", "quit"], "description": "Default status. reload rereads host.json. wallpaper shows every screen as the desktop background (on GNOME this needs the extension) or back in a window; login sets whether the host starts at login; open_app opens the JETCARP app."},
            "on": {"type": "boolean", "description": "With wallpaper or login: on or off. Default on."},
            "name": {"type": "string", "description": "With action rotation: the rotation to run (saved)."},
            "screen": {"type": "integer", "minimum": 0, "description": "With next, previous, or rotation: which screen, counted from 0. Default: every screen for next and previous, the first for rotation."},
        }, "additionalProperties": false},
        "annotations": {"title": "JETCARP background host", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false},
    })
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

/// Serve MCP on stdin/stdout until stdin closes. `app`: start attached to the JETCARP app.
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
                    let o = match crate::link::host_call(&args) {
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
    Ok(())
}
