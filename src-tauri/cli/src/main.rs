//! `ukiyo`: the command line tool (docs/agent-interface.md, section 3).
//!
//! Stage 3 is read-only: `status`, `settings`, `camera watch`, `catalog show`, `log show`
//! and `help modes`, plus `serve`, the shared camera owner. Every camera read goes to `serve`
//! over its socket (`conn`); the CLI never opens USB itself.

mod cmds;
mod conn;
mod out;

use clap::{Args, Parser, Subcommand};
use out::{Fail, Out};

#[derive(Parser)]
#[command(name = "ukiyo", version, about = "Ukiyo: manage a Fujifilm X-series camera over USB", disable_help_subcommand = true)]
struct Cli {
    /// One JSON document on stdout, nothing else. Errors too.
    #[arg(long, global = true)]
    json: bool,
    /// No progress lines on stderr.
    #[arg(long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Link, mode, model, battery, card, and what works now.
    Status,
    /// The camera: the live change feed.
    Camera {
        #[command(subcommand)]
        cmd: CameraCmd,
    },
    /// Live settings with their catalog names, menu paths and value labels.
    Settings(SettingsArgs),
    /// The property catalog of a camera model.
    Catalog {
        #[command(subcommand)]
        cmd: CatalogCmd,
    },
    /// The session log.
    Log {
        #[command(subcommand)]
        cmd: LogCmd,
    },
    /// Help on a topic: `ukiyo help modes [tether|rawconv|movie]`.
    Help {
        #[command(subcommand)]
        topic: Option<HelpCmd>,
    },
    /// The headless camera owner (spawned on demand): `ukiyo serve [--idle-exit S]`.
    Serve,
}

#[derive(Subcommand)]
enum CameraCmd {
    /// Prints the change feed, live, until Ctrl-C. With --json, one JSON object per line.
    Watch {
        /// Only changes of this code (repeatable): 0xd381.
        #[arg(long = "code", value_name = "C")]
        codes: Vec<String>,
        /// Stop after this long: 60s, 5m, 1h, or plain seconds.
        #[arg(long = "for", value_name = "DURATION")]
        duration: Option<String>,
    },
}

#[derive(Args)]
struct SettingsArgs {
    /// Words that must all appear in the code, name, menu path or notes.
    query: Vec<String>,
    /// A menu path prefix: "SET UP > POWER MANAGEMENT".
    #[arg(long, value_name = "PATH")]
    menu: Option<String>,
    /// Only this code (repeatable): 0xd381. Works in any USB mode.
    #[arg(long = "code", value_name = "C")]
    codes: Vec<String>,
    /// Only the favourite settings (not built yet).
    #[arg(long)]
    favourites: bool,
    /// Only codes with no catalog name.
    #[arg(long)]
    unnamed: bool,
    /// Only codes whose catalog confidence is not `confirmed`.
    #[arg(long)]
    unconfirmed: bool,
    /// The menu tree with row counts, instead of rows.
    #[arg(long)]
    menus: bool,
}

#[derive(Subcommand)]
enum CatalogCmd {
    /// Every catalog entry, or one code's entry in full.
    Show {
        /// A code (0xd381) or a catalog name (AutoPowerOff).
        code: Option<String>,
        /// The model (X-T5). Default: the connected camera's, else the only catalog.
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(Subcommand)]
enum LogCmd {
    /// The newest session's log, or every line since a time.
    Show {
        /// 10m, 2h, 1d, 2026-10-17, "2026-10-17 14:30", or RFC 3339.
        #[arg(long, value_name = "T")]
        since: Option<String>,
    },
}

#[derive(Subcommand)]
enum HelpCmd {
    /// How to set the camera's USB mode, or the STILL/MOVIE switch.
    Modes {
        /// tether, rawconv or movie. Default: all three.
        mode: Option<String>,
    },
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `serve` keeps its own parser: the app binary runs the same `serve::main`.
    if args.get(1).map(String::as_str) == Some("serve") {
        std::process::exit(ukiyo_core::serve::main(&args[2..]));
    }
    let wants_json = args.iter().any(|a| a == "--json");
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                let _ = e.print();
                std::process::exit(0);
            }
            if wants_json {
                let first = e.to_string().lines().next().unwrap_or("usage error").trim_start_matches("error: ").to_string();
                std::process::exit(Out { json: true, quiet: true }.fail(&Fail::usage(first).hint("ukiyo --help")));
            }
            let _ = e.print();
            std::process::exit(2);
        }
    };
    let out = Out { json: cli.json, quiet: cli.quiet };
    if matches!(cli.cmd, Cmd::Status | Cmd::Camera { .. } | Cmd::Settings(_)) {
        if let Err(f) = conn::check_data_dir() {
            std::process::exit(out.fail(&f));
        }
    }
    let result = match cli.cmd {
        Cmd::Status => cmds::status(out),
        Cmd::Camera { cmd: CameraCmd::Watch { codes, duration } } => cmds::watch(out, &codes, duration.as_deref()),
        Cmd::Settings(a) => cmds::settings(
            out,
            cmds::SettingsQuery {
                query: a.query,
                menu: a.menu,
                codes: a.codes,
                favourites: a.favourites,
                unnamed: a.unnamed,
                unconfirmed: a.unconfirmed,
                menus: a.menus,
            },
        ),
        Cmd::Catalog { cmd: CatalogCmd::Show { code, model } } => cmds::catalog_show(out, code.as_deref(), model.as_deref()),
        Cmd::Log { cmd: LogCmd::Show { since } } => cmds::log_show(out, since.as_deref()),
        Cmd::Help { topic: Some(HelpCmd::Modes { mode }) } => cmds::help_modes(out, mode.as_deref()),
        Cmd::Help { topic: None } => {
            use clap::CommandFactory as _;
            let _ = Cli::command().print_help();
            Ok(())
        }
        Cmd::Serve => unreachable!("handled before parsing"),
    };
    std::process::exit(match result {
        Ok(()) => 0,
        Err(f) => out.fail(&f),
    });
}
