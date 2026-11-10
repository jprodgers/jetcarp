//! The app's command-line flags: `jetcarp [flags]` with no subcommand opens the app. The environment hooks (`JETCARP_SCREENSHOT`, `JETCARP_SCENE`, ...)
//! keep working alongside them. Display flags change the display settings as the Display pane
//! would (and are saved with them).

use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug, Default, Clone)]
#[command(next_help_heading = "App options (with no subcommand)")]
pub struct LaunchArgs {
    /// Start in screensaver mode (Esc leaves it).
    #[arg(long)]
    pub screensaver: bool,
    /// With --screensaver: quit the app when the screensaver ends.
    #[arg(long)]
    pub quit_on_exit: bool,
    /// Open this scene file (with --screensaver: show it).
    #[arg(long, conflicts_with = "quickstart")]
    pub scene: Option<PathBuf>,
    /// Open this Quick Start entry, e.g. `rule:B3/S23` (with --screensaver: show it).
    #[arg(long)]
    pub quickstart: Option<String>,
    /// A screensaver profile by name (with --screensaver: show it; else load it into the window
    /// to try it).
    #[arg(long, conflicts_with_all = ["scene", "quickstart"])]
    pub profile: Option<String>,
    /// A rotation by name (with --screensaver: run it; else choose it for the screensaver).
    #[arg(long, conflicts_with_all = ["scene", "quickstart", "profile"])]
    pub rotation: Option<String>,
    /// With --screensaver: show what is running (the rule and the grid); saved like the pane's
    /// checkbox.
    #[arg(long)]
    pub caption: bool,
    /// Show the grid on these monitors (indexes from the Display pane or `jetcarp app status`),
    /// one full-screen output each.
    #[arg(long, value_delimiter = ',')]
    pub monitors: Vec<usize>,
    /// The outputs span one canvas (the default).
    #[arg(long, conflicts_with = "mirror")]
    pub span: bool,
    /// The outputs mirror one view.
    #[arg(long)]
    pub mirror: bool,
    /// The main window full screen.
    #[arg(long)]
    pub fullscreen: bool,
    /// Cell size in physical pixels (1 to 64) for Fill, the pixel-exact zoom, and the
    /// screensaver.
    #[arg(long, value_parser = parse_cell_px)]
    pub cell_px: Option<f32>,
    /// Keep the computer awake while the app is open.
    #[arg(long)]
    pub keep_awake: bool,
    /// Start the local control endpoint (AI control) for `jetcarp app` and `jetcarp mcp --app`.
    #[arg(long)]
    pub control: bool,
}

fn parse_cell_px(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|_| format!("{s:?} is not a number"))?;
    if (1.0..=64.0).contains(&v) {
        Ok(v)
    } else {
        Err("cell size is 1 to 64 pixels".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Top {
        #[command(flatten)]
        app: LaunchArgs,
    }

    fn p(args: &[&str]) -> Result<LaunchArgs, clap::Error> {
        let mut v = vec!["jetcarp"];
        v.extend_from_slice(args);
        Top::try_parse_from(v).map(|t| t.app)
    }

    #[test]
    fn flags_parse() {
        let a = p(&[]).unwrap();
        assert!(!a.screensaver && a.monitors.is_empty() && a.cell_px.is_none());
        let a = p(&[
            "--screensaver",
            "--quickstart",
            "rule:B3/S23",
            "--monitors",
            "0,1",
            "--mirror",
            "--cell-px",
            "1",
            "--keep-awake",
            "--control",
        ])
        .unwrap();
        assert!(a.screensaver && a.mirror && !a.span && a.keep_awake && a.control);
        assert_eq!(a.monitors, vec![0, 1]);
        assert_eq!(a.cell_px, Some(1.0));
        assert_eq!(a.quickstart.as_deref(), Some("rule:B3/S23"));
        let a = p(&["--scene", "x.json", "--span"]).unwrap();
        assert_eq!(a.scene, Some(PathBuf::from("x.json")));
        assert!(p(&["--span", "--mirror"]).is_err());
        assert!(p(&["--scene", "a.json", "--quickstart", "b"]).is_err());
        assert!(p(&["--cell-px", "0"]).is_err());
        assert!(p(&["--cell-px", "65"]).is_err());
        assert!(p(&["--no-such-flag"]).is_err());
        let a = p(&["--screensaver", "--profile", "Day & Night"]).unwrap();
        assert!(a.screensaver);
        assert_eq!(a.profile.as_deref(), Some("Day & Night"));
        assert!(p(&["--profile", "Life", "--quickstart", "x"]).is_err());
        assert!(p(&["--profile", "Life", "--scene", "x.json"]).is_err());
        let a = p(&["--screensaver", "--rotation", "Wolfram series"]).unwrap();
        assert_eq!(a.rotation.as_deref(), Some("Wolfram series"));
        assert!(p(&["--rotation", "x", "--profile", "Life"]).is_err());
    }
}
