//! Row descriptors of all 256 elementary rules from one top cell on 240 x 135 squares with dead
//! edges (a 1920 x 1080 screen at 8 px), compared with Wolfram's classes.
//!
//! The reference is Wolfram's four classes for the 88 inequivalent elementary rules (as listed by
//! Martínez, "A Note on Elementary Cellular Automata Classification", 2013), extended to all 256
//! by mirror image and complement. A descriptor counts as agreeing when `empty`/`uniform` meets
//! class 1, `periodic`/`shifting`/`regular` meets class 2, and `nested`/`chaotic` meets class 3
//! or 4. Wolfram's classes describe random starts, so a single cell is not expected to agree
//! everywhere (rule 4 keeps its one cell: `uniform`, class 2); the table measures drift.
//!
//! `cargo run --release -p jetcarp-script --example row_hints [-- --all]` (`--all` prints every rule).

use jetcarp_script::{RunOptions, Screen};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

const CLASS1: &[u8] = &[0, 8, 32, 40, 128, 136, 160, 168];
const CLASS3: &[u8] = &[18, 22, 30, 45, 60, 90, 105, 122, 126, 146, 150];
const CLASS4: &[u8] = &[41, 54, 106, 110];
const CLASS2: &[u8] = &[
    1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 19, 23, 24, 25, 26, 27, 28, 29, 33, 34, 35, 36,
    37, 38, 42, 43, 44, 46, 50, 51, 56, 57, 58, 62, 72, 73, 74, 76, 77, 78, 94, 104, 108, 130, 132,
    134, 138, 140, 142, 152, 154, 156, 162, 164, 170, 172, 178, 184, 200, 204, 232,
];

/// The rule with left and right swapped.
fn mirror(r: u8) -> u8 {
    (0..8).fold(0, |m, k| {
        let (l, c, x) = (k >> 2 & 1, k >> 1 & 1, k & 1);
        m | ((r >> k) & 1) << (x << 2 | c << 1 | l)
    })
}

/// The rule with 0 and 1 exchanged.
fn complement(r: u8) -> u8 {
    (0..8).fold(0, |m, k| m | (1 - ((r >> (7 - k)) & 1)) << k)
}

/// Wolfram class (1 to 4) of every rule.
fn classes() -> [u8; 256] {
    let mut c = [0u8; 256];
    for (k, list) in [CLASS1, CLASS2, CLASS3, CLASS4].iter().enumerate() {
        for &r in *list {
            for x in [r, mirror(r), complement(r), mirror(complement(r))] {
                c[x as usize] = k as u8 + 1;
            }
        }
    }
    assert!(c.iter().all(|&x| x > 0), "every rule has a class");
    c
}

fn main() {
    let all = std::env::args().any(|a| a == "--all");
    let root = std::env::temp_dir().join(format!("jetcarp-row-hints-{}", std::process::id()));
    let src = r#"
        for r in 0..256 {
            let w = world(#{ tiling: "square", size: "screen", boundary: "dead" });
            w.add_ca(`W${r}`, #{ seed: "top_center" });
            w.run(#{ until: "done", max_gens: 5000 });
            let x = w.describe();
            print(`${r} ${x.row_class} ${x.row_hint} ${x.row_entropy}`);
        }
    "#;
    let o = RunOptions {
        out_root: Some(root.clone()),
        screen: Screen {
            width: 1920,
            height: 1080,
            cell_px: 8.0,
        },
        ..Default::default()
    };
    let s = jetcarp_script::run(src, o, |_| {}, Arc::new(AtomicBool::new(false)));
    let _ = std::fs::remove_dir_all(&root);
    assert!(s.ok, "{:?}", s.error);
    let cls = classes();
    let mut agree = 0;
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    let mut misses = Vec::new();
    for line in &s.log {
        let f: Vec<&str> = line.split(' ').collect();
        let r: usize = f[0].parse().unwrap();
        let label = match f[1] {
            "aperiodic" => f[2].to_string(),
            c => c.to_string(),
        };
        *counts.entry(label.clone()).or_default() += 1;
        let want = cls[r];
        let ok = match label.as_str() {
            "empty" | "uniform" => want == 1,
            "periodic" | "shifting" | "regular" => want == 2,
            _ => want >= 3,
        };
        agree += ok as usize;
        if !ok {
            misses.push(format!("{r}:{label}/{want}"));
        }
        if all {
            println!("{r:3} class {want} {label:9} entropy {}", f[3]);
        }
    }
    println!("labels: {counts:?}");
    println!("agreement with Wolfram's classes: {agree} of 256");
    println!("disagreements (rule:label/class): {}", misses.join(" "));
}
