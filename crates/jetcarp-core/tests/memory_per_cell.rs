//! The memory each kind of world takes per cell, measured by counting every allocation while a
//! world of about a million cells is built, seeded and stepped (the peak, so the stepper's own
//! buffers count), and checked against the table the limits use
//! ([`jetcarp_core::machine::cost`]): the table must never be below what was measured, nor far
//! above it.
//!
//! One test in its own binary, so nothing else allocates while it counts.

use jetcarp_core::machine::cost;
use jetcarp_core::{CaSpec, SizeSpec, TilingSpec, World};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = NOW.fetch_add(l.size(), Ordering::SeqCst) + l.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            let now = NOW.fetch_add(l.size(), Ordering::SeqCst) + l.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        NOW.fetch_sub(l.size(), Ordering::SeqCst);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                let now = NOW.fetch_add(new - l.size(), Ordering::SeqCst) + (new - l.size());
                PEAK.fetch_max(now, Ordering::SeqCst);
            } else {
                NOW.fetch_sub(l.size() - new, Ordering::SeqCst);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Peak bytes above the starting point while `f` runs, and what `f` returns.
fn peak_of<T>(f: impl FnOnce() -> T) -> (usize, T) {
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let t = f();
    (PEAK.load(Ordering::SeqCst) - base, t)
}

/// A world of `tiling` with one CA of `rule`, seeded at random (the rule's own seed for agents
/// and sandpiles), stepped `gens` generations; it is dropped inside, so only the peak counts.
fn measure(tiling: &TilingSpec, rules: &[&str], gens: u64) -> (usize, usize) {
    let (bytes, cells) = peak_of(|| {
        let mut w = World::new(tiling.clone(), 7).expect("a world");
        for r in rules {
            w.add_ca(CaSpec::new(r)).expect("a CA");
        }
        for _ in 0..gens {
            w.step();
        }
        w.topo.len()
    });
    (bytes, cells)
}

fn rect(family: &str, w: u32, h: u32) -> TilingSpec {
    let mut t = TilingSpec::new(family);
    t.size = Some(SizeSpec::Rect { w, h });
    t
}

fn catalog(id: &str, w: u32, h: u32) -> TilingSpec {
    let mut t = rect("periodic", w, h);
    t.params = Some(serde_json::json!({ "catalog": id }));
    t
}

fn level(family: &str, l: u32) -> TilingSpec {
    let mut t = TilingSpec::new(family);
    t.size = Some(SizeSpec::Level(l));
    t
}

#[test]
fn memory_per_cell_is_within_the_table() {
    // Warm up: the thread pool, the catalogs and every lazily built table, so they do not count.
    for r in [
        "B3/S23",
        "Sandpile",
        "Lenia",
        "Layer 3/012",
        "R2,C3,S4-9,B5-8",
        "Ant RL",
    ] {
        let _ = measure(&rect("square", 64, 64), &[r], 2);
    }
    let _ = measure(&level("spectre", 3), &["B3/S23"], 2);
    let _ = measure(&level("penrose-rhomb", 4), &["B3/S23"], 2);
    let _ = measure(&level("hat", 3), &["B3/S23"], 2);

    let cases: Vec<(&str, TilingSpec, Vec<&str>, u64)> = vec![
        (
            "Life, squares",
            rect("square", 1024, 1024),
            vec!["B3/S23"],
            4,
        ),
        ("Life, hexagons", rect("hex", 1024, 1024), vec!["B2/S34"], 4),
        (
            "Life, trihexagonal",
            rect("trihex", 600, 600),
            vec!["B3/S23"],
            4,
        ),
        ("Life, 4.8.8", rect("oct", 724, 724), vec!["B3/S23"], 4),
        (
            "Life, 3.4.6.4",
            rect("rhombitrihex", 418, 418),
            vec!["B3/S23"],
            4,
        ),
        ("Life, Cairo", rect("cairo", 512, 512), vec!["B3/S23"], 4),
        (
            "Life, k2-017",
            catalog("k2-017", 300, 300),
            vec!["B3/S23"],
            4,
        ),
        (
            "Generations",
            rect("square", 1024, 1024),
            vec!["B2/S/C3"],
            4,
        ),
        (
            "WireWorld",
            rect("square", 1024, 1024),
            vec!["WireWorld"],
            4,
        ),
        (
            "Larger than Life",
            rect("square", 1024, 1024),
            vec!["R2,C3,S4-9,B5-8"],
            3,
        ),
        (
            "Layered",
            rect("square", 1024, 1024),
            vec!["Layer 3/012"],
            4,
        ),
        ("JACA", rect("square", 1024, 1024), vec!["JACA B3/S23"], 4),
        (
            "Margolus",
            rect("square", 1024, 1024),
            vec!["Margolus BBM"],
            4,
        ),
        (
            "Von Neumann 29",
            rect("square", 1024, 1024),
            vec!["JvN29"],
            4,
        ),
        ("Ant", rect("square", 1024, 1024), vec!["Ant RL"], 4),
        ("Sandpile", rect("square", 1024, 1024), vec!["Sandpile"], 4),
        ("Lenia", rect("square", 1024, 1024), vec!["Lenia"], 2),
        (
            "Two CAs, one field",
            rect("square", 1024, 1024),
            vec!["B3/S23", "B36/S23"],
            4,
        ),
        (
            "Two CAs, two fields",
            rect("square", 1024, 1024),
            vec!["B3/S23", "Sandpile"],
            4,
        ),
        (
            "Life, Spectre patch",
            level("spectre", 7),
            vec!["B3/S23"],
            4,
        ),
        ("Life, hat patch", level("hat", 7), vec!["B3/S23"], 4),
        (
            "Life, Penrose patch",
            level("penrose-rhomb", 7),
            vec!["B3/S23"],
            4,
        ),
    ];
    let mut report = String::new();
    let mut bad = vec![];
    for (name, tiling, rules, gens) in cases {
        let (bytes, cells) = measure(&tiling, &rules, gens);
        let measured = bytes as f64 / cells as f64;
        let specs: Vec<CaSpec> = rules.iter().map(|r| CaSpec::new(r)).collect();
        let table = cost::world_bytes(&tiling, cells as u64, &specs) as f64 / cells as f64;
        report.push_str(&format!(
            "{name:<22} {cells:>9} cells  measured {measured:7.2} B/cell  table {table:7.2}\n"
        ));
        // Never below what was measured (4 MB aside for what a run allocates once, whatever its
        // size: the trihexagonal tiling's rule tables take 2.7 MB); at
        // most 1.5 times it plus 4 bytes a cell.
        let allowed = table * (cells as f64) + (4u64 << 20) as f64;
        if allowed < (bytes as f64) || table > measured * 1.5 + 4.0 {
            bad.push(name);
        }
    }
    println!("{report}");
    assert!(bad.is_empty(), "outside the table: {bad:?}\n{report}");
}
