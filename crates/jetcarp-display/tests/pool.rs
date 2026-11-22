//! The desktop background is part of JETCARP's one pool of memory: its memory limit, and the
//! largest world it builds, are what the other JETCARP programs leave of *Reserved for JETCARP*
//! (the reference machine's 8 GB here), never a separate allowance on top. This binary records
//! into a scratch runtime folder, with a fixed reading of its own memory.

use jetcarp_core::machine::pool::{self, Use};
use jetcarp_display::host_settings::HostSettings;

fn rss() -> Option<u64> {
    Some(100 << 20)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn the_desktop_background_yields_to_the_rest_of_jetcarp() {
    let dir = std::env::temp_dir().join(format!("jetcarp-host-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: set before anything in this binary reads it, and this binary has one test.
    unsafe { std::env::set_var("JETCARP_RUNTIME_DIR", &dir) };
    pool::start("the desktop background", rss);
    let mem = pool::folder().unwrap();
    let app = |gib_tenths: u64| {
        pool::record_in(
            &mem,
            &Use {
                pid: 1,
                program: "the app".into(),
                bytes: (gib_tenths << 30) / 10,
                at_ms: now_ms(),
                pauses: false,
            },
        )
        .unwrap();
    };
    let s = HostSettings::default();
    // Alone, the background may use the whole pool.
    assert_eq!(s.memory_limit_mb(), 8192);
    assert_eq!(s.cell_limit(), 64_000_000);
    // The app holds 6 GB: the background may use the 2 GB left, and gives up its largest world
    // past that.
    app(60);
    assert_eq!(s.memory_limit_mb(), 2048);
    // Its worlds' cells stay within what is left (a Life cell takes 8 bytes).
    app(79);
    assert_eq!(s.cell_limit(), ((8u64 << 30) - (79u64 << 30) / 10) / 8);
    // A value of its own in host.json is held to the pool too.
    let own = HostSettings {
        max_memory_mb: 4096,
        ..HostSettings::default()
    };
    app(60);
    assert_eq!(own.memory_limit_mb(), 2048);
    pool::stop();
    let _ = std::fs::remove_dir_all(&dir);
}
