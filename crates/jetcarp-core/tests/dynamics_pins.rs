//! Portable dynamics: every built-in Quick Start entry (which together cover every rule family
//! on many tilings), shrunk to a preview-sized grid, reaches a pinned state hash after a fixed
//! number of generations.
//!
//! `geometry_pins` pins the tilings themselves; this pins what the rules do on them. CI runs it on
//! Linux, Windows and macOS, so a pass on all three means the same scene gives the same state on
//! each — the property verification receipts rely on. `tests/data/dynamics_pins.txt` holds the
//! expected values; `JETCARP_PIN_WRITE=1` rewrites it (only after a deliberate change to a rule,
//! a seed generator, or the hash). A family that turns out not to be bit-reproducible across
//! platforms must be listed in `NOT_PORTABLE` with the reason, never silently re-pinned.
use jetcarp_core::*;
use rayon::prelude::*;
use std::collections::BTreeMap;

/// Generations each entry runs.
const GENS: u64 = 48;
/// Cells in each shrunk grid (the Quick Start preview size).
const CELLS: u32 = 4096;
/// Entry id prefixes whose results may differ between platforms, with why. Empty: none known.
const NOT_PORTABLE: &[(&str, &str)] = &[];

fn pins_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/dynamics_pins.txt")
}

/// Per entry: the pinned value (the state hash, version 2, and the generation), and the same
/// with the version 1 hash.
fn run_all() -> BTreeMap<String, (String, String)> {
    let entries = quickstart::entries();
    assert!(entries.len() > 100, "only {} entries", entries.len());
    entries
        .par_iter()
        .map(|e| {
            let scene = quickstart::preview_scene(&e.scene, CELLS);
            let result = World::from_scene(&scene).and_then(|mut w| {
                w.apply(Command::Step { gens: GENS })?;
                Ok((
                    format!("{} gen {}", w.hash_hex(), w.generation),
                    format!("{} gen {}", w.hash_hex_v1(), w.generation),
                ))
            });
            let (v2, v1) = result.unwrap_or_else(|err| {
                let e = format!("error {err}").replace('\n', " ");
                (e.clone(), e)
            });
            (e.id.clone(), (v2, v1))
        })
        .collect()
}

/// The pinned values, by entry id.
fn read_pins(path: &std::path::Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with JETCARP_PIN_WRITE=1)", path.display()))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn quick_start_dynamics_are_pinned() {
    let got = run_all();
    let path = pins_path();
    if std::env::var_os("JETCARP_PIN_WRITE").is_some() {
        // A pin still in the version 1 hash (16 digits) is rewritten in version 2 only when the
        // entry's version 1 hash and generation are the pinned ones: proof that only the hash's
        // version changed, not the state it is the hash of.
        let old = if path.exists() {
            read_pins(&path)
        } else {
            BTreeMap::new()
        };
        let refused: Vec<String> = got
            .iter()
            .filter_map(|(id, (_, v1))| {
                let p = old.get(id)?;
                let is_v1 = p.split(' ').next().is_some_and(|h| h.len() == 16);
                (is_v1 && p != v1).then(|| format!("{id}: pinned {p}, version 1 now {v1}"))
            })
            .collect();
        assert!(
            refused.is_empty(),
            "not written: these entries' state changed, not only the hash's version:\n{}",
            refused.join("\n")
        );
        let mut text = String::from(
            "# Pinned dynamics (tests/dynamics_pins.rs): Quick Start entry id, state hash (version\n\
             # 2) after 48 generations on a 4,096-cell preview grid, and the generation reached.\n",
        );
        for (id, (v, _)) in &got {
            text.push_str(&format!("{id}\t{v}\n"));
        }
        std::fs::write(&path, text).unwrap();
        return;
    }
    let pinned = read_pins(&path);
    let portable = |id: &str| !NOT_PORTABLE.iter().any(|(p, _)| id.starts_with(p));
    let mut bad = vec![];
    for (id, (v, _)) in &got {
        match pinned.get(id) {
            Some(p) if p == v => {}
            Some(p) if portable(id) => bad.push(format!("{id}: pinned {p}, got {v}")),
            Some(_) => {}
            None => bad.push(format!("{id}: not pinned (got {v})")),
        }
    }
    for id in pinned.keys().filter(|k| !got.contains_key(*k)) {
        bad.push(format!("{id}: pinned but no longer a Quick Start entry"));
    }
    assert!(
        bad.is_empty(),
        "{} of {} entries differ from tests/data/dynamics_pins.txt:\n{}",
        bad.len(),
        got.len(),
        bad.join("\n")
    );
}
