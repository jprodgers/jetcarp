//! A one-time import of CART's pattern catalog into a JETCARP library, with every record
//! verified again on this engine. A development tool, not part of the release: CART was
//! JETCARP's private prototype, and its records come in once. See `import.rs`.
//!
//! ```text
//! cargo run --release -p jetcarp-store --example import_cart -- \
//!     --cart <catalog.sqlite> --library <folder or .sqlite> [--report <folder>] [--threads N]
//! ```
//!
//! The CART catalog is opened read-only and immutable. The report folder (default: the current
//! folder) gets `cart-to-new-ids.tsv` (every CART id with its new id, tiling, class and status),
//! `cart-failures.tsv` (the records that failed in every rule, with why),
//! `cart-rules-dropped.tsv` (the rules other records failed in) and `cart-import-report.txt`.

mod import;

fn main() {
    let mut o = import::Options::default();
    let mut args = std::env::args().skip(1);
    let usage = "usage: import_cart --cart <catalog.sqlite> --library <path> [--report <folder>] \
                 [--threads N]";
    while let Some(a) = args.next() {
        let mut val = || {
            args.next().unwrap_or_else(|| {
                eprintln!("{a} needs a value\n{usage}");
                std::process::exit(2)
            })
        };
        match a.as_str() {
            "--cart" => o.cart = val().into(),
            "--library" => o.library = val().into(),
            "--report" => o.report = Some(val().into()),
            "--threads" => {
                o.threads = val().parse().unwrap_or_else(|_| {
                    eprintln!("--threads takes a number\n{usage}");
                    std::process::exit(2)
                })
            }
            "-h" | "--help" => {
                println!("{usage}");
                return;
            }
            _ => {
                eprintln!("unknown argument {a}\n{usage}");
                std::process::exit(2)
            }
        }
    }
    if o.cart.as_os_str().is_empty() || o.library.as_os_str().is_empty() {
        eprintln!("{usage}");
        std::process::exit(2);
    }
    match import::run(&o) {
        Ok(r) => print!("{}", r.text),
        Err(e) => {
            eprintln!("import_cart: {e}");
            std::process::exit(1);
        }
    }
}
