//! `jetcarp-mcp`: the MCP server on its own, without the GPU stack, so it can be built as a fully
//! static binary (`--target x86_64-unknown-linux-musl`) that runs inside sandboxed apps such as
//! the LM Studio Flatpak. Same tools and options as `jetcarp mcp`.
//!
//! Usage: `jetcarp-mcp [--app] [--allow DIR]... [--port N] [--token T]`

fn main() {
    // A PastePattern naming a library pattern finds it in the default library.
    jetcarp_store::install();
    let mut app = false;
    let (mut port, mut token) = (None, None);
    let mut allow = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--app" => app = true,
            "--allow" => match args.next() {
                Some(d) => allow.push(std::path::PathBuf::from(d)),
                None => {
                    eprintln!("jetcarp-mcp: --allow needs a folder");
                    std::process::exit(2);
                }
            },
            "--port" => port = args.next().and_then(|p| p.parse().ok()),
            "--token" => token = args.next(),
            "-h" | "--help" => {
                println!(
                    "jetcarp-mcp [--app] [--allow DIR]... [--port N] [--token T]: JETCARP's MCP server on stdin/stdout.\n\
                     Its tools read and write files only in JETCARP's output and library folders, and in each --allow DIR."
                );
                return;
            }
            "-V" | "--version" => {
                println!("jetcarp-mcp {}", jetcarp_core::VERSION);
                return;
            }
            other => {
                eprintln!("jetcarp-mcp: unknown argument '{other}'");
                std::process::exit(2);
            }
        }
    }
    jetcarp_mcp::tools::allow_dirs(allow);
    if let Err(e) = jetcarp_mcp::stdio::run(app, port, token) {
        eprintln!("jetcarp-mcp: {e}");
        std::process::exit(1);
    }
}
