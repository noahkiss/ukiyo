//! `ukiyo`: the command line tool (docs/agent-interface.md, section 3).
//!
//! Reads (`cmds`): `status`, `settings`, `camera watch`, `catalog show`, `log show` and
//! `help modes`. Writes (`writes`, stage 4): `settings set`, `owner`, `plan`, `camera
//! connect|disconnect|export` and `log note`; every camera write is a plan first. Plus
//! `serve`, the shared camera owner. Everything that touches the camera goes to `serve` over
//! its socket (`conn`); the CLI never opens USB itself.

mod cmds;
mod conn;
mod out;
mod writes;

use clap::{Args, Parser, Subcommand};
use out::{Fail, Out};
use writes::WriteFlags;

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
    /// Live settings with their catalog names, menu paths and value labels; `settings set`
    /// changes them through a plan.
    Settings(SettingsArgs),
    /// Owner strings: copyright, author, file-name prefix, device name.
    Owner {
        #[command(subcommand)]
        cmd: OwnerCmd,
    },
    /// Change plans: show, apply or discard one by its id.
    Plan {
        #[command(subcommand)]
        cmd: PlanCmd,
    },
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
    /// Turns the Lab's connect switch on: ukiyo serve looks for the camera again.
    Connect,
    /// Closes the session cleanly (AUTO POWER OFF restored) and stays off USB until connect.
    Disconnect,
    /// Writes a full property dump to dumps/ in the data folder and prints its path.
    Export,
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// Sets one or more settings through a plan: NAME=VALUE, where NAME is a catalog name,
    /// a menu label or a code, and VALUE a value label or a raw number.
    Set {
        #[arg(value_name = "NAME=VALUE", required = true)]
        pairs: Vec<String>,
        #[command(flatten)]
        flags: WriteFlags,
    },
}

#[derive(Subcommand)]
enum OwnerCmd {
    /// The owner strings the camera holds now.
    Get,
    /// Sets owner strings through a plan: FIELD=VALUE (copyright, author, prefix,
    /// device-name). None is confirmed yet, so this needs --allow-unconfirmed.
    Set {
        #[arg(value_name = "FIELD=VALUE", required = true)]
        pairs: Vec<String>,
        #[command(flatten)]
        flags: WriteFlags,
    },
}

#[derive(Subcommand)]
enum PlanCmd {
    /// A plan's steps and the time it has left.
    Show { plan_id: String },
    /// Applies a plan: re-checks it, writes each step, reads each back.
    Apply {
        plan_id: String,
        #[command(flatten)]
        flags: WriteFlags,
    },
    /// Drops a plan unapplied.
    Discard { plan_id: String },
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct SettingsArgs {
    #[command(subcommand)]
    cmd: Option<SettingsCmd>,
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
    /// Adds a note to the running session's log, tied to a change group with --group.
    Note {
        text: String,
        /// The change group id (from `camera watch` or `log show`).
        #[arg(long, value_name = "N")]
        group: Option<u32>,
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
    if matches!(cli.cmd, Cmd::Status | Cmd::Camera { .. } | Cmd::Settings(_) | Cmd::Owner { .. } | Cmd::Plan { .. } | Cmd::Log { cmd: LogCmd::Note { .. } }) {
        if let Err(f) = conn::check_data_dir() {
            std::process::exit(out.fail(&f));
        }
    }
    // Reads exit 0 on success; writes return their own exit code (writes.rs).
    let ok = |r: Result<(), Fail>| r.map(|()| 0);
    let result = match cli.cmd {
        Cmd::Status => ok(cmds::status(out)),
        Cmd::Camera { cmd: CameraCmd::Watch { codes, duration } } => ok(cmds::watch(out, &codes, duration.as_deref())),
        Cmd::Camera { cmd: CameraCmd::Connect } => writes::camera_connect(out, true),
        Cmd::Camera { cmd: CameraCmd::Disconnect } => writes::camera_connect(out, false),
        Cmd::Camera { cmd: CameraCmd::Export } => writes::camera_export(out),
        Cmd::Settings(SettingsArgs { cmd: Some(SettingsCmd::Set { pairs, flags }), .. }) => writes::settings_set(out, &pairs, flags),
        Cmd::Owner { cmd: OwnerCmd::Get } => writes::owner_get(out),
        Cmd::Owner { cmd: OwnerCmd::Set { pairs, flags } } => writes::owner_set(out, &pairs, flags),
        Cmd::Plan { cmd: PlanCmd::Show { plan_id } } => writes::plan_show(out, &plan_id),
        Cmd::Plan { cmd: PlanCmd::Apply { plan_id, flags } } => writes::plan_apply(out, &plan_id, flags),
        Cmd::Plan { cmd: PlanCmd::Discard { plan_id } } => writes::plan_discard(out, &plan_id),
        Cmd::Log { cmd: LogCmd::Note { text, group } } => writes::log_note(out, &text, group),
        Cmd::Settings(a) => ok(cmds::settings(
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
        )),
        Cmd::Catalog { cmd: CatalogCmd::Show { code, model } } => ok(cmds::catalog_show(out, code.as_deref(), model.as_deref())),
        Cmd::Log { cmd: LogCmd::Show { since } } => ok(cmds::log_show(out, since.as_deref())),
        Cmd::Help { topic: Some(HelpCmd::Modes { mode }) } => ok(cmds::help_modes(out, mode.as_deref())),
        Cmd::Help { topic: None } => {
            use clap::CommandFactory as _;
            let _ = Cli::command().print_help();
            Ok(0)
        }
        Cmd::Serve => unreachable!("handled before parsing"),
    };
    std::process::exit(match result {
        Ok(code) => code,
        Err(f) => out.fail(&f),
    });
}
