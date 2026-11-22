//! `jetcarp score` run through the real program, its output captured (not a terminal).

use std::path::PathBuf;
use std::process::{Command, Output};

/// The tests' own folders: the ones the tests were given (as CI sets them), else folders in
/// Cargo's scratch folder for this target, so a local `cargo test` never writes to the user's
/// own folders. The runtime folder is always one of the test's own, so a measurement here never
/// pauses the user's own desktop background.
fn test_dir(var: &str, name: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch(name))
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("score-tests")
        .join(format!("{name}-{}", std::process::id()))
}

/// `jetcarp ARGS` with the tests' own folders and a runtime folder of its own.
fn jetcarp(args: &[&str], runtime: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jetcarp"))
        .args(args)
        .env("JETCARP_OUT", test_dir("JETCARP_OUT", "out"))
        .env("JETCARP_DATA_DIR", test_dir("JETCARP_DATA_DIR", "data"))
        .env(
            "JETCARP_CONFIG_DIR",
            test_dir("JETCARP_CONFIG_DIR", "config"),
        )
        .env("JETCARP_RUNTIME_DIR", scratch(runtime))
        .output()
        .expect("run jetcarp")
}

/// With its output going to a pipe, `jetcarp score` writes each step on a line of its own, with
/// no erase codes and no carriage returns (#299).
#[test]
fn score_progress_is_one_step_per_line_when_not_a_terminal() {
    let o = jetcarp(
        &["score", "--quick", "--no-gpu", "--no-save"],
        "runtime-lines",
    );
    let err = String::from_utf8_lossy(&o.stderr);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{err}");
    for (name, text) in [("stderr", &err), ("stdout", &out)] {
        assert!(!text.contains('\x1b'), "an escape code in {name}: {text:?}");
        assert!(
            !text.contains('\r'),
            "a carriage return in {name}: {text:?}"
        );
    }
    let lines: Vec<&str> = err.lines().collect();
    for step in [
        "Life soups on the hat, every core",
        "Life soups on the Spectre, every core",
        "A soup hunt on squares, every core",
    ] {
        assert!(
            lines.contains(&step),
            "no line of its own for {step:?}: {err:?}"
        );
    }
    assert!(out.contains("JETCARP Hardware Score: "), "{out}");
}

/// Before measuring, `jetcarp score` asks for other heavy programs to be closed, and goes on by
/// itself (it stays scriptable: no question when it is not run in a terminal, #301).
#[test]
fn score_asks_for_heavy_programs_to_be_closed_first() {
    let o = jetcarp(
        &["score", "--quick", "--no-gpu", "--no-save"],
        "runtime-warning",
    );
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    let warning = err
        .find("Close other heavy programs first")
        .unwrap_or_else(|| panic!("no warning: {err}"));
    let first_step = err.find("Life soups on the hat").expect("the first step");
    assert!(warning < first_step, "{err}");
}

/// Write a record of another JETCARP program into `runtime`'s memory pool, as a program that
/// cannot pause for the measuring (`jetcarp run` stepping a world) leaves it.
fn other_program(runtime: &std::path::Path, program: &str) {
    let dir = runtime.join("memory");
    std::fs::create_dir_all(&dir).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    std::fs::write(
        dir.join("4000002.json"),
        format!(r#"{{"pid": 4000002, "program": "{program}", "bytes": 1000000, "at_ms": {now}}}"#),
    )
    .unwrap();
}

/// A JETCARP program that cannot pause is named, the score is measured all the same (not in a
/// terminal: no question), and it is marked as taken under load, in the output and in
/// `machine.json` (#301).
#[test]
fn a_program_that_cannot_pause_is_named_and_the_score_marked() {
    let runtime = scratch("runtime-other");
    other_program(&runtime, "jetcarp run");
    let o = jetcarp(&["score", "--quick", "--no-gpu"], "runtime-other");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    assert!(
        err.contains("can't be paused") && err.contains("jetcarp run"),
        "{err}"
    );
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("under load"), "{out}");
    other_program(&runtime, "jetcarp run");
    let o = jetcarp(&["--json", "score", "--quick", "--no-gpu"], "runtime-other");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let c = &v["score"]["conditions"];
    assert_eq!(c["under_load"], true, "{v}");
    assert_eq!(c["not_paused"], serde_json::json!(["jetcarp run"]), "{v}");
    let kept: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(v["machine_file"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(kept["score"]["conditions"], *c);
}

/// A processor busy with something else when the measuring starts is noticed, and the score
/// marked as taken under load (#301). Here the test itself keeps every core busy.
#[test]
fn a_busy_processor_marks_the_score() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let stop = Arc::new(AtomicBool::new(false));
    let n = std::thread::available_parallelism().map_or(2, |n| n.get());
    let spinners: Vec<_> = (0..n)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut x = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                }
            })
        })
        .collect();
    let o = jetcarp(
        &["--json", "score", "--quick", "--no-gpu", "--no-save"],
        "runtime-busy",
    );
    stop.store(true, Ordering::Relaxed);
    for s in spinners {
        let _ = s.join();
    }
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let c = &v["score"]["conditions"];
    assert_eq!(c["under_load"], true, "{v}");
    assert!(c["cpu"].as_f64().unwrap() >= 0.15, "{v}");
}
