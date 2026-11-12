//! App-only tools: `app_display` is listed and answered by a host with a display (the app),
//! and refused with a useful error by the headless session; and the world tools act on the app's
//! world through its host.

mod common;

use jetcarp_core::world::Applied;
use jetcarp_mcp::{Host, Server, Session, tools};
use serde_json::{Value, json};

fn req(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// A host with a display: records what it was asked and answers with its state.
struct FakeApp {
    s: Session,
    screensaver: bool,
    cell_px: f64,
    calls: Vec<Value>,
}

impl Host for FakeApp {
    fn world(&self) -> &jetcarp_core::World {
        &self.s.world
    }
    fn exec(&mut self, cmd: jetcarp_core::Command) -> Result<Applied, String> {
        self.s.exec(cmd)
    }
    fn replace(&mut self, scene: jetcarp_core::Scene) -> Result<(), String> {
        self.s.replace(scene)
    }
    fn has_display(&self) -> bool {
        true
    }
    fn display(&mut self, args: &Value) -> Result<Value, String> {
        self.calls.push(args.clone());
        if let Some(b) = args["screensaver"].as_bool() {
            self.screensaver = b;
        }
        if let Some(c) = args["cell_px"].as_f64() {
            if !(1.0..=64.0).contains(&c) {
                return Err("`cell_px` is 1 to 64".into());
            }
            self.cell_px = c;
        }
        Ok(json!({"mode": "window", "cell_px": self.cell_px,
                  "screensaver": {"active": self.screensaver}}))
    }
}

fn names(server: &mut Server, host: &mut dyn Host) -> Vec<String> {
    let r = server
        .handle(host, &req(1, "tools/list", json!({})))
        .unwrap();
    r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

fn call(server: &mut Server, host: &mut dyn Host, args: Value) -> Value {
    let r = server
        .handle(
            host,
            &req(
                5,
                "tools/call",
                json!({"name": "app_display", "arguments": args}),
            ),
        )
        .unwrap();
    r["result"].clone()
}

fn text(r: &Value) -> String {
    r["content"][0]["text"].as_str().unwrap_or("").to_string()
}

#[test]
fn listed_only_for_hosts_with_a_display() {
    common::setup("app_tools");
    let mut server = Server::new();
    let mut headless = Session::new();
    let n = names(&mut server, &mut headless);
    assert!(!n.iter().any(|x| x == "app_display"));
    assert_eq!(n, tools::names());
    let mut app = FakeApp {
        s: Session::new(),
        screensaver: false,
        cell_px: 8.0,
        calls: vec![],
    };
    let n = names(&mut server, &mut app);
    assert_eq!(n.last().map(String::as_str), Some("app_display"));
    // Its schema is well formed like the others.
    let r = server
        .handle(&mut app, &req(2, "tools/list", json!({})))
        .unwrap();
    let t = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "app_display")
        .unwrap()
        .clone();
    assert_eq!(t["inputSchema"]["type"], "object");
    for (k, p) in t["inputSchema"]["properties"].as_object().unwrap() {
        assert!(p["description"].is_string(), "{k}");
    }
}

#[test]
fn calls_reach_the_host() {
    common::setup("app_tools");
    let mut server = Server::new();
    let mut app = FakeApp {
        s: Session::new(),
        screensaver: false,
        cell_px: 8.0,
        calls: vec![],
    };
    let r = call(&mut server, &mut app, json!({}));
    assert_eq!(r["isError"], false);
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(v["screensaver"]["active"], false);
    let r = call(
        &mut server,
        &mut app,
        json!({"screensaver": true, "cell_px": 1}),
    );
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(v["screensaver"]["active"], true);
    assert_eq!(v["cell_px"], 1.0);
    assert_eq!(app.calls.len(), 2);
    // The host's errors come back as tool errors.
    let r = call(&mut server, &mut app, json!({"cell_px": 100}));
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("1 to 64"));
    // The world tools act on the app's world, through its commands.
    let r = common::call(
        &mut server,
        &mut app,
        "world_new",
        json!({"width": 16, "height": 16, "cas": ["Life"]}),
    );
    common::ok(&r);
    common::ok(&common::call(
        &mut server,
        &mut app,
        "world_step",
        json!({"generations": 3}),
    ));
    assert_eq!(app.s.world.generation, 3);
}

#[test]
fn headless_session_explains() {
    common::setup("app_tools");
    let mut server = Server::new();
    let mut s = Session::new();
    let r = call(&mut server, &mut s, json!({"screensaver": true}));
    assert_eq!(r["isError"], true);
    assert!(
        text(&r).contains("only available in the JETCARP app"),
        "{}",
        text(&r)
    );
}
