//! Time a large sandpile on the CPU: `cargo run --release --example pile_bench -- square 1e6 edge`.

use jetcarp_core::Nb;
use jetcarp_core::pile::{self, Centre, Ctl, Pile, PileSpec};
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let fam = a.first().map(String::as_str).unwrap_or("square");
    let n = pile::parse_grains(a.get(1).map(String::as_str).unwrap_or("1e6")).unwrap();
    let nb = match a.get(2).map(String::as_str).unwrap_or("edge") {
        "edge" => Nb::Edge,
        "touch" => Nb::Touch,
        o => panic!("the neighbourhood is edge or touch, not {o}"),
    };
    let mut p = Pile::new(PileSpec {
        tiling: jetcarp_core::TilingSpec::new(fam),
        nb,
        grains: n,
        centre: Centre::Cell,
    })
    .unwrap();
    let t = Instant::now();
    let mut ctl = Ctl::default();
    while !p.finished() {
        p.next_step(None, &mut ctl).unwrap();
    }
    let (top, sweeps, _) = p.totals();
    println!(
        "{fam} {n} {nb:?}: {:.2} s, {top} topplings, {sweeps} sweeps, board {} cells",
        t.elapsed().as_secs_f64(),
        p.board.cells()
    );
}
