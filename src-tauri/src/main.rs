// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `ukiyo serve`: the app spawns itself as the shared camera owner, before any Tauri or
    // window code runs (docs/agent-interface.md, "Where the binary lives").
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("serve") {
        std::process::exit(ukiyo_core::serve::main(&args[1..]));
    }
    ukiyo_lib::run()
}
