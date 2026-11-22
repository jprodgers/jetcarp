//! `jetcarp launcher`: JETCARP among the system's apps, with its icon (#105), through
//! [`jetcarp_display::launcher`]. The installers run `jetcarp launcher install`; `jetcarp
//! uninstall` removes it.

use anyhow::{Result, anyhow};
use jetcarp_display::launcher;
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum LauncherAction {
    /// Add it, or make it again for this program: on Linux a menu entry and the icon in
    /// ~/.local/share; on macOS JETCARP.app in ~/Applications, signed ad hoc on the Mac; on
    /// Windows a Start menu entry.
    Install,
    /// Take it out again.
    Remove,
    /// Say whether it is there.
    Status,
}

pub fn run(action: LauncherAction, json: bool) -> Result<()> {
    match action {
        LauncherAction::Install => {
            let done = launcher::install().map_err(|e| anyhow!(e))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "installed": true,
                        "path": done.path,
                        "program": done.program,
                    }))?
                );
            } else {
                println!("{}; it starts {}", done.said, done.program.display());
            }
        }
        LauncherAction::Remove => {
            let removed = launcher::remove().map_err(|e| anyhow!(e))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({ "installed": false, "removed": removed })
                    )?
                );
            } else if removed.is_empty() {
                println!(
                    "JETCARP was not in {}; nothing to remove",
                    launcher::place()
                );
            } else {
                println!("removed JETCARP from {}:", launcher::place());
                for f in &removed {
                    println!("  {}", f.display());
                }
            }
        }
        LauncherAction::Status => {
            if json {
                let files = launcher::files().map_err(|e| anyhow!(e))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "installed": launcher::installed(),
                        "path": files.first(),
                        "files": files,
                    }))?
                );
            } else {
                println!("{}", launcher::describe());
            }
        }
    }
    Ok(())
}
