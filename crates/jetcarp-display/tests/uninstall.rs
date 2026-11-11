//! What `jetcarp uninstall` needs from the desktop integration that can be checked without a
//! desktop: finding and removing the GNOME Shell extension's files outside a GNOME session.

use jetcarp_display::gnome::{UUID, installed_in, remove_from};
use std::cell::RefCell;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("display-uninstall")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// GNOME Shell's extensions folder with JETCARP's extension (and another one) in it.
fn installed(name: &str) -> PathBuf {
    let ext = scratch(name);
    for (dir, file) in [
        (UUID, "metadata.json"),
        (UUID, "extension.js"),
        (UUID, "icons/jetcarp-symbolic.svg"),
        ("other@example.org", "metadata.json"),
    ] {
        let p = ext.join(dir).join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "{}").unwrap();
    }
    ext
}

#[test]
fn the_extensions_files_are_found_and_removed_without_gnome_shell() {
    let ext = installed("no-session");
    assert!(installed_in(&ext));
    // Outside a GNOME session the list of enabled extensions cannot be read (on another
    // desktop there is no such schema): the files are removed all the same.
    let set_called = RefCell::new(false);
    remove_from(
        &ext,
        || Err("No such schema \u{201c}org.gnome.shell\u{201d}".into()),
        |_| {
            *set_called.borrow_mut() = true;
            Ok(())
        },
    )
    .unwrap();
    assert!(!ext.join(UUID).exists() && !installed_in(&ext));
    assert!(!*set_called.borrow(), "nothing to write without the list");
    assert!(
        ext.join("other@example.org/metadata.json").exists(),
        "other extensions are left alone"
    );
}

#[test]
fn removing_takes_it_off_the_enabled_list_and_a_failed_write_does_not_keep_the_files() {
    let ext = installed("session");
    let written = RefCell::new(None);
    remove_from(
        &ext,
        || Ok(vec!["a@b".into(), UUID.into()]),
        |l| {
            *written.borrow_mut() = Some(l.to_vec());
            Err("no session bus".into())
        },
    )
    .unwrap();
    assert_eq!(written.borrow().as_deref(), Some(&["a@b".to_string()][..]));
    assert!(!installed_in(&ext));
    // Nothing installed: nothing to do, and no error.
    remove_from(&ext, || Ok(vec![]), |_| panic!("not written")).unwrap();
}
