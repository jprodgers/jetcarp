//! Where the packs that came with the program are looked for (`paths::bundled_packs_from`).

use jetcarp_core::paths::{bundled_packs_from, plain_path};
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

#[test]
fn verbatim_windows_paths_read_as_plain_ones() {
    let p = |s: &str| plain_path(PathBuf::from(s));
    assert_eq!(
        p(r"\\?\C:\Users\ada\jetcarp.exe"),
        PathBuf::from(r"C:\Users\ada\jetcarp.exe")
    );
    assert_eq!(
        p(r"\\?\UNC\server\share\x"),
        PathBuf::from(r"\\?\UNC\server\share\x")
    );
    assert_eq!(
        p("/usr/local/bin/jetcarp"),
        PathBuf::from("/usr/local/bin/jetcarp")
    );
}

#[test]
fn a_chosen_data_folder_is_honoured_and_the_variable_wins() {
    use jetcarp_core::paths::{
        DataSource, chosen_data_dir, data_dir_source_from, default_data_dir, set_chosen_data_dir,
    };
    let d = scratch("chosen-data");
    let settings = d.join("settings");
    let big = d.join("big disk").join("jetcarp");
    // No paths.json: the default.
    assert_eq!(chosen_data_dir(&settings), None);
    assert_eq!(
        data_dir_source_from(None, Some(&settings)),
        (default_data_dir(), DataSource::Default)
    );
    set_chosen_data_dir(&settings, Some(&big)).unwrap();
    assert_eq!(chosen_data_dir(&settings), Some(big.clone()));
    assert_eq!(
        data_dir_source_from(None, Some(&settings)),
        (big.clone(), DataSource::Chosen)
    );
    // The variable wins; empty counts as not set.
    let env = d.join("env");
    assert_eq!(
        data_dir_source_from(Some(env.as_os_str()), Some(&settings)),
        (env, DataSource::Env)
    );
    assert_eq!(
        data_dir_source_from(Some(OsStr::new("")), Some(&settings)).1,
        DataSource::Chosen
    );
    // Relative paths are refused; a file naming one counts as no choice.
    assert!(set_chosen_data_dir(&settings, Some(Path::new("relative/data"))).is_err());
    std::fs::write(settings.join("paths.json"), r#"{"data_dir": "relative"}"#).unwrap();
    assert_eq!(chosen_data_dir(&settings), None);
    std::fs::write(settings.join("paths.json"), "not json").unwrap();
    assert_eq!(chosen_data_dir(&settings), None);
    // Removing the choice brings the default back (and removing it twice is fine).
    set_chosen_data_dir(&settings, Some(&big)).unwrap();
    set_chosen_data_dir(&settings, None).unwrap();
    set_chosen_data_dir(&settings, None).unwrap();
    assert_eq!(chosen_data_dir(&settings), None);
    assert!(!settings.join("paths.json").exists());
}
