//! `jetcarp api` (hidden): JETCARP's operations by name, for automation and tests.
//!
//! - `jetcarp api list`: every operation, whether it only reads, and whether it acts on a world.
//! - `jetcarp api schema OP`: its input and output JSON Schemas.
//! - `jetcarp api call OP [JSON]`: run it (`JSON` is its input; `-` reads it from stdin; none is
//!   `{}`) and print its output as JSON, or `--text` for the text the command line shows. A
//!   failure prints `{"error": {"kind", "message", "hint"}}` and exits with status 1. World
//!   operations act on a fresh headless world (one per call).

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use jetcarp_api::op::{self, Quiet};
use jetcarp_api::{ApiError, Ctx};
use serde_json::{Value, json};
use std::io::Read;

#[derive(Args)]
pub struct ApiArgs {
    #[command(subcommand)]
    cmd: ApiCmd,
}

#[derive(Subcommand)]
enum ApiCmd {
    /// Every operation: name, read only, on a world, what it does.
    List,
    /// An operation's input and output JSON Schemas.
    Schema { op: String },
    /// Run an operation and print its output as JSON.
    Call {
        op: String,
        /// The input as JSON (`-` reads it from stdin; default `{}`).
        input: Option<String>,
        /// Print the text the command line shows instead of JSON.
        #[arg(long)]
        text: bool,
    },
}

fn fail(e: &ApiError) -> ! {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"error": e})).unwrap_or_default()
    );
    std::process::exit(1)
}

pub fn run(cx: &Ctx, a: ApiArgs, _json: bool) -> Result<()> {
    match a.cmd {
        ApiCmd::List => {
            let ops: Vec<Value> = jetcarp_api::ops::OPS
                .iter()
                .map(|o| {
                    json!({"name": o.name(), "read_only": o.read_only(), "world": o.on_world(),
                           "summary": o.summary()})
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&ops)?);
        }
        ApiCmd::Schema { op } => {
            let o = jetcarp_api::find(&op)
                .unwrap_or_else(|| fail(&ApiError::not_found(format!("no operation {op}"))));
            let v =
                json!({"name": o.name(), "input": o.input_schema(), "output": o.output_schema()});
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        ApiCmd::Call { op, input, text } => {
            let input: Value = match input.as_deref() {
                None => json!({}),
                Some("-") => {
                    let mut s = String::new();
                    std::io::stdin()
                        .read_to_string(&mut s)
                        .context("reading the input from stdin")?;
                    serde_json::from_str(&s).context("the input is not JSON")?
                }
                Some(s) => serde_json::from_str(s).context("the input is not JSON")?,
            };
            let c = jetcarp_api::call(cx, &op, input, &Quiet).unwrap_or_else(|e| fail(&e));
            if text {
                print!("{}", c.text);
            } else {
                println!("{}", op::to_pretty(&c.value));
            }
            if let Err(e) = c.check {
                if text {
                    anyhow::bail!(e);
                }
                fail(&e);
            }
        }
    }
    Ok(())
}
