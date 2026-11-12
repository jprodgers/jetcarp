//! Where the packs that came with the program are looked for (`paths::bundled_packs_from`).

use jetcarp_core::paths::bundled_packs_from;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bundled-packs")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("bin")).unwrap();
    d
}

#[test]
fn the_variable_wins_even_when_its_folder_is_missing() {
    let d = scratch("env");
    std::fs::create_dir_all(d.join("bin/packs")).unwrap();
    let exe = d.join("bin/jetcarp");
    let named = d.join("elsewhere");
    assert_eq!(
        bundled_packs_from(Some(named.as_os_str()), Some(&exe)),
        Some(named)
    );
    // Empty counts as not set, as for every other JETCARP_ variable.
    assert_eq!(
        bundled_packs_from(Some(OsStr::new("")), Some(&exe)),
        Some(d.join("bin/packs"))
    );
}

#[test]
fn share_beside_bin_comes_before_packs_beside_the_program() {
    let d = scratch("order");
    let exe = d.join("bin/jetcarp");
    assert_eq!(bundled_packs_from(None, Some(&exe)), None);
    std::fs::create_dir_all(d.join("bin/packs")).unwrap();
    assert_eq!(
        bundled_packs_from(None, Some(&exe)),
        Some(d.join("bin/packs"))
    );
    std::fs::create_dir_all(d.join("share/jetcarp/packs")).unwrap();
    assert_eq!(
        bundled_packs_from(None, Some(&exe)),
        Some(d.join("share/jetcarp/packs"))
    );
    // A file of that name is not a folder of packs.
    let f = scratch("file");
    std::fs::write(f.join("bin/packs"), b"").unwrap();
    assert_eq!(bundled_packs_from(None, Some(&f.join("bin/jetcarp"))), None);
    assert_eq!(bundled_packs_from(None, None), None);
}
