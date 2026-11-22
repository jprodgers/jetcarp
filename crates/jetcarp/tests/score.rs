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
