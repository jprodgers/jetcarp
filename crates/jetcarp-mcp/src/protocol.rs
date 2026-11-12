//! JSON-RPC 2.0 message handling for MCP.
//!
//! Dual-era server (MCP 2026-07-28 "Versioning and Compatibility"):
//! - **Legacy** clients (2025-11-25 and earlier) open with `initialize`; the reply negotiates one
//!   of [`LEGACY_VERSIONS`]. `notifications/initialized` and `ping` are accepted.
//! - **Modern** clients (2026-07-28) send `io.modelcontextprotocol/protocolVersion` in every
//!   request's `params._meta` and may call `server/discover` first. Unsupported versions get
//!   `UnsupportedProtocolVersionError` (-32022) listing the supported ones.
//!
//! Every result carries `resultType: "complete"`. Tool failures the model can fix (bad arguments,
//! rule errors) come back as tool results with `isError: true`; only malformed requests and
//! unknown tools are JSON-RPC errors.
//!
//! The server offers tools and resources ([`crate::resources`]: the book's pages, references,
//! schemas, and templates for jobs, patterns and Quick Start entries).

use crate::tools::{self, ToolOutput};
use jetcarp_api::world::Host;
use serde_json::{Value, json};

pub const MODERN_VERSION: &str = "2026-07-28";
/// Handshake-era versions, newest first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;
/// `resources/read` of a resource that does not exist.
pub const RESOURCE_NOT_FOUND: i64 = -32002;

/// What this server offers: tools and resources (fixed lists, no subscriptions).
pub fn capabilities() -> Value {
    json!({"tools": {"listChanged": false}, "resources": {"listChanged": false, "subscribe": false}})
}

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

pub fn supported_versions() -> Vec<&'static str> {
    let mut v = vec![MODERN_VERSION];
    v.extend_from_slice(LEGACY_VERSIONS);
    v
}

pub fn server_info() -> Value {
    json!({"name": "jetcarp", "title": "JETCARP cellular automata", "version": jetcarp_core::VERSION})
}

/// Shown to the model by clients that pass server instructions along.
pub const INSTRUCTIONS: &str = "JETCARP runs cellular automata (the Game of Life and many other rules) \
on square, hexagon, triangle, Archimedean, k-uniform and aperiodic (Spectre, hat, Penrose) tilings, \
and keeps a library of the patterns its searches find. Typical flow: world_new (a tiling and rules) \
-> world_edit (draw or paste) -> world_step -> world_read / world_render. Long searches: \
search_start, job_status, then library_query for the finds. Call help for worked examples, \
reference for rule names, tilings, palettes and seeds. The JETCARP book is available as resources \
(jetcarp://docs/...) and through help {\"page\": ...}.";

pub fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn error_response(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({"code": code, "message": message});
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": e})
}

/// A `tools/call` result object from a tool's output.
pub fn tool_result(out: ToolOutput) -> Value {
    json!({"resultType": "complete", "content": out.content, "isError": out.is_error})
}

/// Sends a JSON-RPC notification to the client while a request runs (any thread).
pub type Notifier = std::sync::Arc<dyn Fn(Value) + Send + Sync>;

/// Cancel flags of the `tools/call` requests in flight, by request id (its JSON text). A
/// transport that reads while a call runs (the stdio server, the app link) sets one when a
/// `notifications/cancelled` names its request ([`cancel_request`]).
pub type CancelMap = std::sync::Arc<
    std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
    >,
>;

/// Handle a `notifications/cancelled` message: set the named request's flag. Returns whether
/// the message was one.
pub fn cancel_request(cancels: &CancelMap, msg: &Value) -> bool {
    if msg.get("method").and_then(|m| m.as_str()) != Some("notifications/cancelled") {
        return false;
    }
    if let Some(id) = msg.get("params").and_then(|p| p.get("requestId"))
        && let Some(f) = cancels.lock().unwrap().get(&id.to_string())
    {
        f.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    true
}

/// The protocol state of one connection (stdio process or app link connection).
#[derive(Default)]
pub struct Server {
    /// Tools added by the transport (the CLI adds `use_app`); listed after the shared tools.
    pub extra_tools: Vec<Value>,
    /// The version agreed in a legacy `initialize`, if any.
    pub legacy_version: Option<String>,
    /// Where `notifications/progress` go when a `tools/call` carries `_meta.progressToken`
    /// (the stdio transport writes them to stdout). None: no progress is sent.
    pub notifier: Option<Notifier>,
    /// Cancel flags of calls in flight (shared with the transport's reader).
    pub cancels: CancelMap,
    /// The request being handled.
    current: Option<String>,
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    /// Handle one line of input (one JSON-RPC message or batch). `None` means no reply is due.
    pub fn handle_line(&mut self, host: &mut dyn Host, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle(host, &msg),
            Err(e) => Some(error_response(
                &Value::Null,
                PARSE_ERROR,
                &format!("parse error: {e}"),
                None,
            )),
        };
        reply.map(|r| r.to_string())
    }

    /// Handle one parsed message. Batches (arrays) are answered element by element.
    pub fn handle(&mut self, host: &mut dyn Host, msg: &Value) -> Option<Value> {
        if let Value::Array(items) = msg {
            if items.is_empty() {
                return Some(error_response(
                    &Value::Null,
                    INVALID_REQUEST,
                    "empty batch",
                    None,
                ));
            }
            let out: Vec<Value> = items.iter().filter_map(|m| self.handle(host, m)).collect();
            return (!out.is_empty()).then_some(Value::Array(out));
        }
        let Some(obj) = msg.as_object() else {
            return Some(error_response(
                &Value::Null,
                INVALID_REQUEST,
                "a JSON-RPC message must be an object",
                None,
            ));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(|m| m.as_str()) else {
            // A response to a request we never sent, or garbage.
            return match id {
                Some(id) if !obj.contains_key("result") && !obj.contains_key("error") => {
                    Some(error_response(&id, INVALID_REQUEST, "missing method", None))
                }
                _ => None,
            };
        };
        let params = obj.get("params").cloned().unwrap_or(json!({}));
        let Some(id) = id else {
            // Notifications (initialized, cancelled, ...) need no reply.
            cancel_request(&self.cancels, msg);
            return None;
        };
        self.current = Some(id.to_string());
        if obj.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
            return Some(error_response(
                &id,
                INVALID_REQUEST,
                "jsonrpc must be \"2.0\"",
                None,
            ));
        }
        let modern = params
            .get("_meta")
            .and_then(|m| m.get(META_VERSION))
            .cloned();
        if let Some(v) = &modern {
            let ok = v
                .as_str()
                .is_some_and(|s| supported_versions().contains(&s));
            if !ok {
                return Some(error_response(
                    &id,
                    UNSUPPORTED_PROTOCOL_VERSION,
                    "Unsupported protocol version",
                    Some(json!({"supported": supported_versions(), "requested": v})),
                ));
            }
        }
        let result = self.dispatch(host, method, &params);
        Some(match result {
            Ok(mut r) => {
                if let Some(o) = r.as_object_mut() {
                    o.entry("resultType").or_insert(json!("complete"));
                    if modern.is_some() {
                        let meta = o.entry("_meta").or_insert(json!({}));
                        meta[META_SERVER_INFO] = server_info();
                    }
                }
                response(&id, r)
            }
            Err((code, message)) => error_response(&id, code, &message, None),
        })
    }

    fn dispatch(
        &mut self,
        host: &mut dyn Host,
        method: &str,
        params: &Value,
    ) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(|v| v.as_str());
                let v = asked
                    .filter(|a| LEGACY_VERSIONS.contains(a))
                    .unwrap_or(LEGACY_VERSIONS[0]);
                self.legacy_version = Some(v.to_string());
                Ok(json!({
                    "protocolVersion": v,
                    "capabilities": capabilities(),
                    "serverInfo": server_info(),
                    "instructions": INSTRUCTIONS,
                }))
            }
            "server/discover" => Ok(json!({
                "supportedVersions": supported_versions(),
                "capabilities": capabilities(),
                "_meta": {META_SERVER_INFO: server_info()},
                "instructions": INSTRUCTIONS,
                "ttlMs": 3_600_000,
                "cacheScope": "public",
            })),
            "ping" | "logging/setLevel" => Ok(json!({})),
            "tools/list" => {
                let mut t = tools::definitions();
                if host.has_display() {
                    t.extend(crate::app_tools::definitions());
                }
                t.extend(self.extra_tools.iter().cloned());
                // A tool's title is its own field from MCP 2025-06-18; older clients read it from
                // the annotations, and newer ones need it only once (the list goes to the model).
                let old = self
                    .legacy_version
                    .as_deref()
                    .is_some_and(|v| v < "2025-06-18");
                if !old {
                    for x in &mut t {
                        if let Some(a) = x.get_mut("annotations").and_then(Value::as_object_mut) {
                            a.remove("title");
                        }
                    }
                }
                Ok(json!({"tools": t, "ttlMs": 3_600_000, "cacheScope": "public"}))
            }
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or((INVALID_PARAMS, "tools/call needs params.name".to_string()))?;
                let args = match params.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(a @ Value::Object(_)) => a.clone(),
                    Some(_) => {
                        return Err((INVALID_PARAMS, "arguments must be an object".into()));
                    }
                };
                if !tools::exists(name) && !crate::app_tools::handles(name) {
                    return Err((INVALID_PARAMS, format!("Unknown tool: {name}")));
                }
                // A progress token asks for notifications/progress while the call runs.
                let progress = match (
                    &self.notifier,
                    params.get("_meta").and_then(|m| m.get("progressToken")),
                ) {
                    (Some(n), Some(tok)) if tok.is_string() || tok.is_number() => {
                        let (n, tok) = (n.clone(), tok.clone());
                        // At most one a second (and the last one), however often the call
                        // reports: a wait for a job reports five times a second.
                        let last = std::sync::Mutex::new(None::<std::time::Instant>);
                        let f: tools::ProgressFn = std::sync::Arc::new(
                            move |done, total, message| {
                                let end = total.is_some_and(|t| done >= t);
                                {
                                    let mut l = last.lock().unwrap_or_else(|e| e.into_inner());
                                    if !end && l.is_some_and(|t| t.elapsed().as_secs_f64() < 1.0) {
                                        return;
                                    }
                                    *l = Some(std::time::Instant::now());
                                }
                                let mut p = json!({"progressToken": tok, "progress": done, "message": message});
                                if let Some(t) = total {
                                    p["total"] = json!(t);
                                }
                                n(
                                    json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": p}),
                                );
                            },
                        );
                        Some(f)
                    }
                    _ => None,
                };
                let key = self.current.clone().unwrap_or_default();
                let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.cancels
                    .lock()
                    .unwrap()
                    .insert(key.clone(), flag.clone());
                let run = std::panic::AssertUnwindSafe(|| {
                    tools::call_ctx(host, name, &args, progress, Some(flag.clone()))
                });
                let caught = std::panic::catch_unwind(run);
                self.cancels.lock().unwrap().remove(&key);
                match caught {
                    Ok(out) => Ok(tool_result(out)),
                    Err(p) => {
                        let msg = p
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_default();
                        Ok(tool_result(ToolOutput::error(format!(
                            "internal error in {name}: {msg}. The world may be partly changed; call world_status."
                        ))))
                    }
                }
            }
            "resources/list" => {
                let mut r = crate::resources::list();
                r["ttlMs"] = json!(3_600_000);
                r["cacheScope"] = json!("public");
                Ok(r)
            }
            "resources/templates/list" => Ok(crate::resources::templates()),
            "resources/read" => {
                let uri = params.get("uri").and_then(|u| u.as_str()).ok_or((
                    INVALID_PARAMS,
                    "resources/read needs params.uri".to_string(),
                ))?;
                // An unknown resource is an error the client can show (MCP's -32002).
                crate::resources::read(uri).map_err(|e| (RESOURCE_NOT_FOUND, e))
            }
            // Not offered (no capability declared), but answer politely if asked.
            "prompts/list" => Ok(json!({"prompts": []})),
            m => Err((METHOD_NOT_FOUND, format!("Method not found: {m}"))),
        }
    }
}
