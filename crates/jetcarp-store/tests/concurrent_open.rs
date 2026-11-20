//! Several programs opening and writing the same library at once.
//!
//! The app, the desktop host, the MCP server and search jobs can all open one library at the same
//! moment, each with its own connection. A busy timeout makes a connection wait for another's
//! write, but SQLite does not wait in every case, so these check that no connection is refused
//! with "database is locked" while another is only working.

use jetcarp_store::Library;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("concurrent-open")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Many connections opening a library that does not exist yet, all at once, each then writing:
/// every one succeeds (one makes the library, the others wait for it and use it).
#[test]
fn many_connections_make_a_new_library_at_once() {
    let root = scratch("new");
    let threads = 8;
    let rounds = 100;
    let mut failures = Vec::new();
    for round in 0..rounds {
        let path = root.join(format!("r{round}")).join("library.sqlite");
        let barrier = Arc::new(Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (p, b) = (path.clone(), barrier.clone());
                std::thread::spawn(move || {
                    let lib = Library::open(&p);
                    b.wait();
                    lib.import_aliases(&[]).map(|_| ())
                })
            })
            .collect();
        for h in handles {
            if let Err(x) = h.join().unwrap() {
                failures.push(format!("round {round}: {x}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        failures.is_empty(),
        "{} of {} opens failed; the first: {:?}",
        failures.len(),
        threads * rounds,
        failures.first()
    );
}

/// The new, empty library file held exclusively by another connection when JETCARP opens it (as
/// one connection holds it for a moment while switching it to WAL, or any program writing it
/// in SQLite's rollback-journal mode): the open waits for it, instead of failing at once with
/// "database is locked", which SQLite's busy timeout does not prevent for the switch to WAL.
#[test]
fn a_new_library_held_by_another_connection_is_waited_for() {
    let root = scratch("held");
    let path = root.join("library.sqlite");
    let mut other = rusqlite::Connection::open(&path).unwrap();
    let tx = other
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    tx.execute_batch("CREATE TABLE held (x)").unwrap();
    let opened = {
        let p = path.clone();
        std::thread::spawn(move || {
            let t = Instant::now();
            let r = Library::open(&p).import_aliases(&[]).map(|_| ());
            (r, t.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    // Rolled back, so the file is left empty for JETCARP to make the library in.
    tx.rollback().unwrap();
    drop(other);
    let (r, waited) = opened.join().unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(r.is_ok(), "the open failed instead of waiting: {r:?}");
    assert!(
        waited >= Duration::from_millis(200),
        "it should have waited for the other connection ({waited:?})"
    );
}
