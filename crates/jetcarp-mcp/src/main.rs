//! `jetcarp-mcp`: the MCP server on its own, without the GPU stack, so it can be built as a fully
//! static binary (`--target x86_64-unknown-linux-musl`) that runs inside sandboxed apps such as
//! the LM Studio Flatpak. Same tools and options as `jetcarp mcp`.
//!
//! Usage: `jetcarp-mcp [--app] [--allow DIR]... [--port N] [--token T] [--http [PORT]] [--json]`

// No unsafe code here, and the compiler keeps it so: this crate takes in other people's data.
#![forbid(unsafe_code)]

fn main() {
    // A PastePattern naming a library pattern finds it in the default library.
    jetcarp_store::install();
    let mut app = false;
    let (mut port, mut token) = (None, None);
    let mut allow = vec![];
    // `--http` with or without a port: Some(None) is the default port.
    let mut http: Option<Option<u16>> = None;
    let mut json = false;
    let mut args = std::env::args().skip(1).peekable();
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
            "--http" => {
                http = Some(
                    args.next_if(|p| p.parse::<u16>().is_ok())
                        .and_then(|p| p.parse().ok()),
                )
            }
            "--json" => json = true,
            "-h" | "--help" => {
                println!(
                    "jetcarp-mcp [--app] [--allow DIR]... [--port N] [--token T] [--http [PORT]] [--json]: JETCARP's MCP server on stdin/stdout.\n\
                     Its tools read and write files only in JETCARP's output and library folders, and in each --allow DIR.\n\
                     --http [PORT]: serve MCP over HTTP instead, at http://127.0.0.1:PORT/mcp (default port {}), on this computer only;\n\
                     clients send the token it prints as an Authorization: Bearer header. --json prints the endpoint as JSON.\n\
                     --port and --token are the app's, for --app.",
                    jetcarp_mcp::http::DEFAULT_PORT
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
    let r = match http {
        None => jetcarp_mcp::stdio::run(app, port, token),
        Some(http_port) => jetcarp_mcp::http::serve(
            jetcarp_mcp::http::Serve {
                port: http_port,
                app,
                app_port: port,
                app_token: token,
                json,
            },
            jetcarp_mcp::http::stop_on_ctrl_c(),
        ),
    };
    if let Err(e) = r {
        eprintln!("jetcarp-mcp: {e}");
        std::process::exit(1);
    }
}
