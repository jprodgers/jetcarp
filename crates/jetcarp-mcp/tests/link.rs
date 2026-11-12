//! The local link, as SECURITY.md describes it: 128-bit tokens from the operating system's
//! random source, made afresh by each listener; the endpoint file readable by this user only
//! (Unix: folder 0700, file 0600, put right when they were not); loopback only; a wrong token
//! refused and the connection closed; and a round trip with the right one.

mod common;

use common::*;
use jetcarp_mcp::{Server, Session, link};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;

fn bits(token: &str) -> u32 {
    token
        .chars()
        .map(|c| c.to_digit(16).unwrap().count_ones())
        .sum()
}

#[test]
fn tokens_are_128_random_bits() {
    let n = 2000;
    let tokens: Vec<String> = (0..n).map(|_| link::new_token().unwrap()).collect();
    for t in &tokens {
        assert_eq!(t.len(), 32, "{t}");
        assert!(
            t.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{t}"
        );
    }
    let distinct: HashSet<&String> = tokens.iter().collect();
    assert_eq!(distinct.len(), n, "no token repeats");
    // Each bit is set half the time: 2000 x 128 bits, so the share of ones is 0.5 within a few
    // thousandths (a fixed or time-derived token would be far off at some position).
    let ones: u32 = tokens.iter().map(|t| bits(t)).sum();
    let share = ones as f64 / (n as f64 * 128.0);
    assert!((share - 0.5).abs() < 0.01, "share of ones {share}");
    for pos in 0..32 {
        let ones: u32 = tokens.iter().map(|t| bits(&t[pos..pos + 1])).sum::<u32>();
        let share = ones as f64 / (n as f64 * 4.0);
        assert!((share - 0.5).abs() < 0.05, "digit {pos}: share {share}");
    }
}

#[test]
fn each_listener_makes_its_own_token_and_records_it_privately() {
    let root = setup("link");
    let a = link::Listener::start_as(0, "test-a.json", Arc::new(|| {})).unwrap();
    let b = link::Listener::start_as(0, "test-b.json", Arc::new(|| {})).unwrap();
    assert_ne!(a.token, b.token);
    assert_eq!(a.token.len(), 32);
    assert!(a.wrote_file());
    let file = link::runtime_file("test-a.json").unwrap();
    assert!(file.starts_with(root.join("runtime")), "{}", file.display());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["token"], json!(a.token));
    assert_eq!(v["port"], json!(a.port));
    #[cfg(unix)]
    {
        assert_eq!(link::private_mode(&file).unwrap(), 0o600);
        assert_eq!(link::private_mode(file.parent().unwrap()).unwrap(), 0o700);
    }
    drop(a);
    assert!(
        !file.exists(),
        "the listener removes its file when it stops"
    );
    drop(b);
}

#[cfg(unix)]
#[test]
fn a_loose_folder_and_file_are_made_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = setup("link");
    let dir = root.join("loose");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = dir.join("endpoint.json");
    std::fs::write(&file, "old").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    link::write_private(&file, "{\"token\": \"x\"}").unwrap();
    assert_eq!(link::private_mode(&dir).unwrap(), 0o700);
    assert_eq!(link::private_mode(&file).unwrap(), 0o600);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"token\": \"x\"}"
    );
    // A new folder is made 0700 from the start.
    let fresh = root.join("fresh").join("deeper").join("e.json");
    link::write_private(&fresh, "{}").unwrap();
    assert_eq!(link::private_mode(fresh.parent().unwrap()).unwrap(), 0o700);
    assert_eq!(link::private_mode(&fresh).unwrap(), 0o600);
    // No temporary file is left beside it.
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["endpoint.json"]);
}

#[test]
fn a_wrong_token_is_refused_and_the_right_one_works() {
    setup("link");
    let listener = link::Listener::start_as(0, "test-link.json", Arc::new(|| {})).unwrap();
    let port = listener.port;
    let token = listener.token.clone();
    // A fake UI thread: answers queued requests with its own session.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();
    let ui = std::thread::spawn(move || {
        let (mut server, mut host) = (Server::new(), Session::new());
        while !stop2.load(std::sync::atomic::Ordering::SeqCst) {
            for p in listener.poll() {
                let msg = p.message.clone();
                p.reply(server.handle(&mut host, &msg));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    // Wrong tokens, of the same length and not: refused, then the connection is closed.
    for bad in [
        "wrong".to_string(),
        "0".repeat(32),
        format!("{}0", &token[..31]),
    ] {
        if bad == token {
            continue;
        }
        let s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut w = s.try_clone().unwrap();
        writeln!(w, "{}", json!({"jetcarp_token": bad})).unwrap();
        let mut r = BufReader::new(s);
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["ok"], false, "{line}");
        // Anything sent after is never answered: the server has closed its end.
        let _ = writeln!(w, "{}", req(1, "ping", json!({})));
        line.clear();
        assert_eq!(r.read_line(&mut line).unwrap_or(0), 0, "closed: {line}");
    }
    let bad = link::AppClient::connect(Some(link::EndpointInfo {
        port,
        token: "wrong".into(),
    }));
    assert!(bad.is_err());
    let mut c =
        link::AppClient::connect(Some(link::EndpointInfo { port, token })).expect("connect");
    let r = c
        .call(&req(
            1,
            "tools/call",
            json!({"name": "world_new",
            "arguments": {"width": 16, "height": 16, "cas": ["Life"]}}),
        ))
        .unwrap();
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = c
        .call(&req(
            2,
            "tools/call",
            json!({"name": "world_step", "arguments": {"generations": 4}}),
        ))
        .unwrap();
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Generation 4")
    );
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    ui.join().unwrap();
}

#[test]
fn the_listener_is_on_loopback_only() {
    setup("link");
    let l = link::Listener::start_as(0, "test-loopback.json", Arc::new(|| {})).unwrap();
    // Every other address of this machine refuses: only 127.0.0.1 is bound.
    let other = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|u| {
            u.connect("192.0.2.1:9")?;
            u.local_addr()
        })
        .map(|a| a.ip());
    if let Ok(ip) = other
        && !ip.is_loopback()
        && !ip.is_unspecified()
    {
        let r = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(ip, l.port),
            std::time::Duration::from_millis(500),
        );
        assert!(r.is_err(), "reachable on {ip}");
    }
    assert!(std::net::TcpStream::connect(("127.0.0.1", l.port)).is_ok());
}
