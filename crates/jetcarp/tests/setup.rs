//! `jetcarp setup` end to end, through the real program, on a scratch folder: every `JETCARP_`
//! folder in Cargo's scratch folder for this target, and a golden pack as the pack that came
//! with the program.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("cli-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let golden =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../jetcarp-pack/tests/golden/minimal");
        copy_dir(&golden, &d.join("bundled/minimal"));
        Scratch(d)
    }

    /// `jetcarp ARGS` with every folder in the scratch folder (none inherited: not CI's, not
    /// the user's), and no terminal to ask in.
    fn run(&self, args: &[&str]) -> Output {
        let d = &self.0;
        Command::new(env!("CARGO_BIN_EXE_jetcarp"))
            .args(args)
            .env("JETCARP_CONFIG_DIR", d.join("config"))
            .env("JETCARP_DATA_DIR", d.join("data"))
            .env("JETCARP_OUT", d.join("out"))
            .env("JETCARP_RUNTIME_DIR", d.join("runtime"))
            .env("JETCARP_BUNDLED_PACKS", d.join("bundled"))
            .env_remove("JETCARP_LIBRARY")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run jetcarp")
    }

    fn json(&self, args: &[&str]) -> Value {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: {e}: {}", String::from_utf8_lossy(&o.stdout)))
    }
}

#[test]
fn setup_explorer_yes_end_to_end() {
    let s = Scratch::new("setup-explorer");
    let st = s.json(&["setup", "--status", "--json"]);
    assert_eq!(st["done"], false);
    assert_eq!(st["bundled"][0]["title"], "minimal");
    assert_eq!(st["bundled"][0]["starter"], true);
    // The photosensitive seizure warning comes first (#96): with no terminal to ask it in, and
    // no answer saved, setup refuses with the warning, whatever else it was asked.
    let answer = s.0.join("config/photosensitivity.json");
    for args in [&["setup"][..], &["setup", "--profile", "explorer", "--yes"]] {
        let o = s.run(args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{args:?}");
        assert!(err.contains("PHOTOSENSITIVE SEIZURE WARNING"), "{err}");
        assert!(err.contains("--seizure-warning no-disorder"), "{err}");
    }
    assert!(!answer.exists() && !s.0.join("config/setup.json").exists());
    // `--seizure-warning` with nothing else to do saves only the answer.
    let o = s.run(&["setup", "--yes", "--seizure-warning", "no-disorder"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let saved = std::fs::read_to_string(&answer).unwrap();
    assert!(saved.contains("\"answer\": \"no_disorder\""), "{saved}");
    assert!(!s.0.join("config/setup.json").exists());
    // With no terminal and no profile, it says what to do instead of asking.
    let o = s.run(&["setup"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--profile"));
    let o = s.run(&["setup", "--yes"]);
    assert!(!o.status.success());
    // Explorer: the starter pack and a Quick Start build of a few seconds.
    let o = s.run(&[
        "setup",
        "--profile",
        "explorer",
        "--yes",
        "--quickstart-minutes",
        "0.05",
        "--threads",
        "2",
        "--build-tilings",
        "square",
        "--build-families",
        "life_like",
    ]);
    let (out, err) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    assert!(o.status.success(), "{err}");
    // It printed what it would do, then did it.
    assert!(err.contains("setting up JETCARP as Explorer:"), "{err}");
    assert!(err.contains("import minimal"), "{err}");
    // Who signed it, and that it came with JETCARP (the program's own folder of packs here).
    assert!(err.contains("the pack came with JETCARP"), "{err}");
    assert!(out.contains("; signed by "), "{out}");
    assert!(err.contains("search for 3 seconds"), "{err}");
    assert!(out.contains("minimal imported: 3 patterns new"), "{out}");
    assert!(out.contains("Quick Start build"), "{out}");
    assert!(out.contains("recorded Explorer"), "{out}");
    assert!(
        s.0.join("config/setup.json").is_file(),
        "the choice is recorded"
    );
    let st = s.json(&["setup", "--status", "--json"]);
    assert_eq!(st["done"], true);
    assert_eq!(st["record"]["profile"], "explorer");
    let jobs = st["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|j| j["status"] == "done"), "{jobs:?}");
    assert_eq!(st["bundled"][0]["imported"], true);
    assert!(
        st["counts"]["quickstart"]["pack"].as_u64() >= Some(1),
        "{st}"
    );
    // The jobs are ordinary jobs.
    let l = s.json(&["jobs", "list", "--json"]);
    let kinds: Vec<&str> = l["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"pack_import") && kinds.contains(&"explore"),
        "{kinds:?}"
    );
    // The pack's Quick Start entry is in the library.
    let q = s.json(&["library", "quickstart", "--source", "pack", "--json"]);
    assert!(!q.as_array().unwrap().is_empty(), "{q}");
    // Blank slate afterwards: recorded, and nothing more done.
    let o = s.run(&["setup", "--profile", "blank-slate", "--yes"]);
    assert!(o.status.success());
    let st = s.json(&["setup", "--status", "--json"]);
    assert_eq!(st["record"]["profile"], "blank-slate");
}

/// `--seizure-warning at-risk` goes on, after the advice not to use JETCARP, and saves the answer,
/// which keeps the strobe guard on for good (#96, #97).
#[test]
fn setup_with_the_at_risk_answer_goes_on_and_saves_it() {
    let s = Scratch::new("setup-at-risk");
    let o = s.run(&[
        "setup",
        "--profile",
        "blank-slate",
        "--yes",
        "--seizure-warning",
        "at-risk",
    ]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    assert!(err.contains("PLEASE DON'T USE JETCARP"), "{err}");
    let saved = std::fs::read_to_string(s.0.join("config/photosensitivity.json")).unwrap();
    assert!(saved.contains("\"answer\": \"at_risk\""), "{saved}");
    assert!(saved.contains(env!("CARGO_PKG_VERSION")), "{saved}");
    assert!(s.0.join("config/setup.json").is_file());
    // Answered once, `--yes` needs no answer again.
    let o = s.run(&["setup", "--profile", "blank-slate", "--yes"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
