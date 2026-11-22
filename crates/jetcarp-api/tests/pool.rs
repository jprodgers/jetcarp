//! JETCARP's one pool of memory across programs: a world must fit what the other JETCARP
//! programs leave of *Reserved for JETCARP*, as their records in the runtime folder say. This
//! binary is the only one that records (it holds one test), into a scratch runtime folder, with
//! a fixed reading of its own memory; the process's limits are the reference machine's, so the
//! pool is 8 GB and one world may take 4 GB.

mod common;

use jetcarp_api::ops::world::{check_world_memory, check_world_memory_replacing};
use jetcarp_api::{ErrorKind, Limits};
use jetcarp_core::machine::pool::{self, Use};
use jetcarp_core::{CaSpec, SizeSpec, TilingSpec, World};

fn square(w: u32, h: u32) -> TilingSpec {
    let mut t = TilingSpec::new("square");
    t.size = Some(SizeSpec::Rect { w, h });
    t
}

/// This program holds 200 MB.
fn rss() -> Option<u64> {
    Some(200 << 20)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[test]
fn a_world_must_fit_what_the_other_programs_leave() {
    let dir = common::scratch("pool-runtime");
    // SAFETY: set before anything in this binary reads it, and this binary has one test.
    unsafe { std::env::set_var("JETCARP_RUNTIME_DIR", &dir) };
    pool::start("jetcarp run", rss);
    let mem = pool::folder().unwrap();
    assert!(mem.join(format!("{}.json", std::process::id())).exists());
    let life = [CaSpec::new("B3/S23")];
    let l = Limits::for_this_machine();
    let background = |gib_tenths: u64, at_ms: u64| {
        pool::record_in(
            &mem,
            &Use {
                pid: 1,
                program: "the desktop background".into(),
                bytes: (gib_tenths << 30) / 10,
                at_ms,
                pauses: true,
            },
        )
        .unwrap();
    };
    // Nothing else running: a 2 GB world fits (the world limit is 4 GB, the pool 8 GB).
    let two_gb = square(16_000, 16_000);
    assert!(check_world_memory(&two_gb, &life, &l).is_ok());
    // The desktop background holds 7 GB: 8 - 7 - 0.2 leaves 0.8 GB. The 2 GB world, within the
    // world limit, is over what is left, and refused saying who holds what.
    background(70, now_ms());
    let e = check_world_memory(&two_gb, &life, &l).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Limit);
    assert_eq!(
        e.message,
        "this world needs 1.9 GB, and 824 MB of the 8 GB reserved for JETCARP is left (the desktop \
         background 7 GB, jetcarp run 200 MB): close something, or reserve more for JETCARP \
         (Tools → This computer…, or `jetcarp limits`)"
    );
    // A small one still fits.
    assert!(check_world_memory(&square(3000, 3000), &life, &l).is_ok());
    // An AI's worlds draw on the same pool.
    let ai = Limits::for_an_ai();
    assert!(check_world_memory(&square(1900, 1900), &life, &ai).is_ok());
    // A world that replaces one gives that one's memory back first: 0.9 GB is over the 0.8 left,
    // but not once the 2 GB world it replaces is counted as free.
    let nine_tenths = square(11_000, 11_000);
    assert!(check_world_memory(&nine_tenths, &life, &l).is_err());
    let current = World::new(two_gb.clone(), 1).unwrap();
    assert!(check_world_memory_replacing(&nine_tenths, &life, &l, Some(&current)).is_ok());
    // A program gone quiet (its record not refreshed for 10 seconds) is not counted.
    background(70, now_ms() - 11_000);
    assert!(check_world_memory(&two_gb, &life, &l).is_ok());
    // Unlimited (tests, library users) checks no pool.
    background(79, now_ms());
    assert!(check_world_memory(&two_gb, &life, &Limits::NONE).is_ok());
    // Its record goes when the program ends.
    pool::stop();
    assert!(!mem.join(format!("{}.json", std::process::id())).exists());
}
