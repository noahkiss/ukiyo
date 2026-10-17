//! `ukiyo`: the command line tool (docs/agent-interface.md, section 3). Stage 2 has only
//! `serve`, the shared camera owner; the other commands come with stage 3.

const USAGE: &str = "usage: ukiyo serve [--idle-exit SECONDS]";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("serve") => ukiyo_core::serve::main(&args[1..]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            0
        }
        Some("--version" | "-V") => {
            println!("ukiyo {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
