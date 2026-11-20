//! Several programs opening and writing one search's working database at once.
//!
//! A search's jobs, the app and the MCP server can each open the same working database with
//! their own connection. These check that none is refused while another is only working.

use jetcarp_search::explore::store;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("explore-store-concurrent")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A working database as a search from before 0.4 left it: only ids in `elites`.
fn format_1(path: &std::path::Path) {
    let c = store::open(path).unwrap();
    c.execute_batch(
        "INSERT INTO elites(id, grp) VALUES (2, 1), (7, 3);
         CREATE TABLE e2(id INTEGER PRIMARY KEY, grp INT NOT NULL);
         INSERT INTO e2 SELECT id, grp FROM elites; DROP TABLE elites;
         ALTER TABLE e2 RENAME TO elites;
         UPDATE meta SET v = 'jetcarp-explore 1' WHERE k = 'format';",
    )
    .unwrap();
}

/// Many connections opening the same format 1 database at once: it is upgraded once, and every
/// open succeeds.
#[test]
fn many_connections_upgrade_an_old_store_at_once() {
    let root = scratch("upgrade");
    let threads = 8;
    let rounds = 40;
    let mut failures = Vec::new();
    for round in 0..rounds {
        let path = root.join(format!("r{round}")).join("search.sqlite");
        format_1(&path);
        let barrier = Arc::new(Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (p, b) = (path.clone(), barrier.clone());
                std::thread::spawn(move || {
                    b.wait();
                    store::open(&p).map(|_| ())
                })
            })
            .collect();
        for h in handles {
            if let Err(x) = h.join().unwrap() {
                failures.push(format!("round {round}: {x}"));
            }
        }
        let c = store::open_read(&path).unwrap();
        let ids: Vec<i64> = c
            .prepare("SELECT id FROM elites WHERE tiling IS NULL ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, [2, 7], "round {round}: the elites, upgraded once");
    }
    assert!(
        failures.is_empty(),
        "{} refused: {failures:#?}",
        failures.len()
    );
}
