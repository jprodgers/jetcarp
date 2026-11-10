//! `jetcarp doctor` (what JETCARP sees on this machine, for bug reports) and `jetcarp uninstall`
//! (undo what JETCARP set up outside its own folders).

use anyhow::Result;
use clap::Args;
use jetcarp_core::paths;
use jetcarp_display::{autostart, gnome, saver};
use jetcarp_mcp::link::host_call;
use serde_json::json;
use std::path::Path;

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Do it. Without this, only show what would be done.
    #[arg(long)]
    pub yes: bool,
    /// Also delete JETCARP's settings folder.
    #[arg(long)]
    pub settings: bool,
    /// Also delete JETCARP's data folder (the pattern library, search databases, packs). This
    /// cannot be undone.
    #[arg(long)]
    pub data: bool,
}

fn folder_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    };
    rd.filter_map(|e| e.ok())
        .map(|e| {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => folder_size(&path),
                Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => 0,
            }
        })
        .sum()
}

fn human(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < units.len() {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", units[i])
    }
}

fn jetcarp_env() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("JETCARP_"))
        .collect();
    v.sort();
    v
}

pub fn doctor(a: DoctorArgs) -> Result<()> {
    let adapters = jetcarp_gpu::adapter_reports();
    let host_running = host_call(&json!({"action": "status"})).is_ok();
    let library = jetcarp_core::library::Library::default_path();
    let data = paths::data_dir();
    let report = json!({
        "version": jetcarp_core::VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "threads": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "folders": {
            "settings": paths::config_dir(),
            "data": data,
            "data_size_bytes": folder_size(&data),
            "output": paths::output_dir(),
            "library": library,
            "library_exists": library.exists(),
        },
        "gpu": adapters.iter().map(|g| json!({
            "name": g.name, "backend": g.backend, "kind": g.kind, "driver": g.driver,
            "chosen": g.chosen, "problem": g.problem,
        })).collect::<Vec<_>>(),
        "desktop": {
            "host_running": host_running,
            "starts_at_login": autostart::enabled(),
            "gnome_extension": if cfg!(target_os = "linux") { Some(gnome::state().describe()) } else { None },
            "screen_saver": if cfg!(any(windows, target_os = "macos")) { Some(saver::describe()) } else { None },
        },
        "environment": jetcarp_env().into_iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
    });
    if a.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "JETCARP {} on {} {} ({} threads)\n",
        jetcarp_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        report["threads"]
    );
    println!("Folders");
    print!(
        "{}",
        paths::describe()
            .lines()
            .map(|l| format!("  {l}\n"))
            .collect::<String>()
    );
    println!(
        "  library:  {} ({})",
        library.display(),
        if library.exists() {
            human(std::fs::metadata(&library).map(|m| m.len()).unwrap_or(0))
        } else {
            "not created yet".into()
        }
    );
    println!("  data in use: {}\n", human(folder_size(&data)));
    println!("Graphics adapters (* = used by the GPU simulation)");
    if adapters.is_empty() {
        println!("  none found: simulation runs on the CPU");
    }
    for g in &adapters {
        println!(
            "  {} {} ({}, {}{})",
            if g.chosen { "*" } else { " " },
            g.name,
            g.backend,
            g.kind,
            if g.driver.is_empty() {
                String::new()
            } else {
                format!(", {}", g.driver)
            }
        );
        if let Some(p) = &g.problem {
            println!("      {p}");
        }
    }
    println!("\nDesktop");
    println!(
        "  background host: {}",
        if host_running { "running" } else { "not running" }
    );
    println!(
        "  starts at login: {}",
        if autostart::enabled() { "yes" } else { "no" }
    );
    if cfg!(target_os = "linux") {
        println!("  {}", gnome::state().describe());
    }
    if cfg!(any(windows, target_os = "macos")) {
        println!("  {}", saver::describe());
    }
    let env = jetcarp_env();
    if !env.is_empty() {
        println!("\nEnvironment");
        for (k, v) in env {
            println!("  {k}={v}");
        }
    }
    Ok(())
}

pub fn uninstall(a: UninstallArgs) -> Result<()> {
    let mut steps: Vec<(String, Box<dyn Fn() -> Result<(), String>>)> = vec![];
    if host_call(&json!({"action": "status"})).is_ok() {
        steps.push((
            "stop the background host (it puts your own desktop picture back)".into(),
            Box::new(|| host_call(&json!({"action": "quit"})).map(|_| ())),
        ));
    }
    if autostart::enabled() {
        steps.push((
            "stop the host starting at login".into(),
            Box::new(|| autostart::set(false, &autostart::host_command()?)),
        ));
    }
    if cfg!(target_os = "linux")
        && !matches!(
            gnome::state(),
            gnome::ExtensionState::NoGnome | gnome::ExtensionState::NotInstalled
        )
    {
        steps.push((
            "remove the GNOME Shell extension".into(),
            Box::new(gnome::remove),
        ));
    }
    if cfg!(windows) && jetcarp_display::winsaver::scr_path().is_ok_and(|p| p.exists()) {
        steps.push(("remove the Windows screen saver".into(), Box::new(saver::remove)));
    }
    if cfg!(target_os = "macos") && jetcarp_display::macsaver::bundle_path().is_ok_and(|p| p.exists())
    {
        steps.push(("remove the macOS screen saver".into(), Box::new(saver::remove)));
    }
    let config = paths::config_dir();
    if let Some(c) = config.clone().filter(|c| c.exists()) {
        if a.settings {
            steps.push((
                format!("delete the settings folder {}", c.display()),
                Box::new(move || std::fs::remove_dir_all(&c).map_err(|e| e.to_string())),
            ));
        } else if let Some(record) = Some(c.join("host-program")).filter(|p| p.exists()) {
            steps.push((
                "forget the recorded host command".into(),
                Box::new(move || std::fs::remove_file(&record).map_err(|e| e.to_string())),
            ));
        }
    }
    let data = paths::data_dir();
    if a.data && data.exists() {
        let size = human(folder_size(&data));
        let d = data.clone();
        steps.push((
            format!("delete the data folder {} ({size})", data.display()),
            Box::new(move || std::fs::remove_dir_all(&d).map_err(|e| e.to_string())),
        ));
    }
    if steps.is_empty() {
        println!("Nothing to undo: JETCARP has not set anything up outside its folders.");
    } else {
        println!(
            "{}",
            if a.yes { "Undoing:" } else { "Would undo (run again with --yes):" }
        );
        for (what, run) in &steps {
            if a.yes {
                match run() {
                    Ok(()) => println!("  done: {what}"),
                    Err(e) => println!("  FAILED: {what}: {e}"),
                }
            } else {
                println!("  {what}");
            }
        }
    }
    if !a.data && data.exists() {
        println!(
            "\nKept the data folder {} ({}); add --data to delete it.",
            data.display(),
            human(folder_size(&data))
        );
    }
    println!(
        "Your pictures and exports in {} are never deleted.",
        paths::output_dir().display()
    );
    println!(
        "\nTo remove the program itself, use what installed it: `pip uninstall jetcarp`, `pipx \
         uninstall jetcarp`, `uv tool uninstall jetcarp`, `brew uninstall jetcarp`, `cargo \
         uninstall jetcarp`, or delete its folder."
    );
    if a.yes && steps.iter().any(|(w, _)| w.starts_with("remove the GNOME")) {
        println!("Log out and back in once to finish removing the GNOME extension.");
    }
    Ok(())
}
