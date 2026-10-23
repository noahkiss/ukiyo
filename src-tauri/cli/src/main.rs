//! `ukiyo`: the command line tool (docs/agent-interface.md, section 3).
//!
//! Reads (`cmds`): `status`, `settings`, `camera watch`, `catalog show`, `log show` and
//! `help modes`. `mcp` (stage 5) is the MCP server on stdio, a client of `serve` like the
//! rest. Writes (`writes`, stage 4): `settings set`, `owner`, `plan`, `camera
//! connect|disconnect|export` and `log note`; every camera write is a plan first. Plus
//! `serve`, the shared camera owner. Everything that touches the camera goes to `serve` over
//! its socket (`conn`); the CLI never opens USB itself.

mod cmds;
mod conn;
mod library;
mod mcp;
mod mcp_tools;
mod out;
mod recipes;
mod schema;
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
    /// Prints the version of the CLI and MCP surface (schema.rs) and exits.
    #[arg(long)]
    schema_version: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Link, mode, model, battery, card, and what works now.
    Status {
        /// Also this camera's history: connects and shots per month.
        #[arg(long)]
        history: bool,
    },
    /// The camera: the live change feed.
    Camera {
        #[command(subcommand)]
        cmd: CameraCmd,
    },
    /// Live settings with their catalog names, menu paths and value labels; `settings set`
    /// changes them through a plan.
    Settings(SettingsArgs),
    /// Presets: named settings and values, applied through a plan.
    Presets {
        #[command(subcommand)]
        cmd: PresetsCmd,
    },
    /// Snapshots: the camera's settings at one moment; compare and restore them.
    Snapshots {
        #[command(subcommand)]
        cmd: SnapshotsCmd,
    },
    /// Recipes: the library, the bundled catalogue, and the camera's C1-C7 slots.
    Recipes {
        #[command(subcommand)]
        cmd: RecipesCmd,
    },
    /// App preferences in settings.json: theme, brand mark, app icon, keep-awake.
    Prefs {
        #[command(subcommand)]
        cmd: PrefsCmd,
    },
    /// Deleted and replaced presets and snapshots, kept 30 days.
    Trash {
        #[command(subcommand)]
        cmd: TrashCmd,
    },
    /// Owner strings: copyright, author, file-name prefix.
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
    /// The MCP server on stdio, for coding agents: `ukiyo mcp`.
    Mcp,
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
    /// Takes one shot through a plan: PC priority, half-press, release, and the shot from
    /// the tether buffer into shots/ in the data folder. Tether mode, stills.
    Capture {
        #[command(flatten)]
        flags: writes::WriteFlags,
    },
    /// Picks how ukiyo serve reaches the camera: usb or wifi (wireless tether). Without a
    /// value, prints the current one. Remembered in settings.json.
    Transport {
        /// usb or wifi.
        kind: Option<String>,
    },
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
    /// The favourite settings: add or remove.
    Fav {
        #[command(subcommand)]
        cmd: FavCmd,
    },
}

#[derive(Subcommand)]
enum FavCmd {
    /// Adds settings to the favourites: a catalog name, a menu label or a code.
    Add {
        #[arg(value_name = "NAME", required = true)]
        names: Vec<String>,
    },
    /// Removes settings from the favourites.
    Rm {
        #[arg(value_name = "NAME", required = true)]
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
enum PresetsCmd {
    /// Every preset, or those whose name or settings match the words.
    Ls { query: Vec<String> },
    /// One preset's settings and values.
    Show { id: String },
    /// Saves a preset: the current values of SETTING..., or --set NAME=VALUE..., or a
    /// snapshot's values. A preset of that name is replaced; the old one goes to the trash.
    Save {
        name: String,
        /// Settings whose values the camera holds now.
        #[arg(value_name = "SETTING")]
        settings: Vec<String>,
        /// A setting and its value, given outright (repeatable).
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// Every value of a snapshot.
        #[arg(long, value_name = "ID")]
        from_snapshot: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Applies a preset to the camera, through a plan.
    Apply {
        id: String,
        #[command(flatten)]
        flags: WriteFlags,
    },
    /// Moves a preset to the trash.
    Rm { id: String },
}

#[derive(Subcommand)]
enum SnapshotsCmd {
    /// Every snapshot, newest first.
    Ls,
    /// One snapshot's settings and values.
    Show { id: String },
    /// Reads the camera's settings and saves them (tether mode). Default name: the time.
    Save { name: Option<String> },
    /// Compares two snapshots, or one with the camera now.
    Diff {
        a: String,
        b: Option<String>,
        /// With one snapshot: count unconfirmed settings as restorable, not blocked.
        #[arg(long)]
        allow_unconfirmed: bool,
    },
    /// Writes a snapshot back to the camera, through a plan.
    Restore {
        id: String,
        #[command(flatten)]
        flags: WriteFlags,
    },
    /// Moves a snapshot to the trash.
    Rm { id: String },
}

#[derive(Subcommand)]
enum RecipesCmd {
    /// The recipes in the library, or those whose name, film simulation or credit match.
    Ls {
        query: Vec<String>,
        /// Only this sensor generation: "X-Trans V".
        #[arg(long)]
        sensor: Option<String>,
        /// Only this film simulation: "classic chrome".
        #[arg(long)]
        sim: Option<String>,
    },
    /// Searches the bundled catalogue, most popular first.
    Search {
        query: Vec<String>,
        #[arg(long)]
        sensor: Option<String>,
        #[arg(long)]
        sim: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// One recipe's settings: a library name, recipe:<name> or cat:<id>.
    Show { id: String },
    /// Reads a recipe from text, a post's URL, or the catalogue; --save keeps it.
    Import {
        /// cat:<id> from `recipes search`.
        cat: Option<String>,
        /// A file with the recipe text, or - for stdin.
        #[arg(long, value_name = "FILE")]
        text: Option<String>,
        /// A recipe post.
        #[arg(long)]
        url: Option<String>,
        /// The name in the library. Default: the post's.
        #[arg(long)]
        name: Option<String>,
        /// Save it to the library (else show it only).
        #[arg(long)]
        save: bool,
    },
    /// Changes settings of a saved recipe: FIELD=VALUE as a post writes them
    /// (Highlight=+1.5, "Grain=Weak, Small"); an empty value clears one.
    Edit {
        id: String,
        #[arg(value_name = "FIELD=VALUE", required = true)]
        pairs: Vec<String>,
    },
    /// Moves a recipe to the trash.
    Rm { id: String },
    /// Reads a custom slot (C1-C7), or all seven, from the camera (USB RAW CONV. mode).
    Read {
        slot: Option<String>,
        /// Compare the slot with a recipe: a library name, recipe:<name> or cat:<id>.
        #[arg(long, value_name = "ID")]
        compare: Option<String>,
    },
    /// Writes a recipe to a custom slot through a plan (USB RAW CONV. mode).
    Write {
        slot: String,
        id: String,
        #[command(flatten)]
        flags: WriteFlags,
    },
}

#[derive(Subcommand)]
enum PrefsCmd {
    /// Every preference.
    Get,
    /// Sets preferences: theme=system|light|dark, brand-mark=ID, app-icon=ID,
    /// keep-awake=on|off, in-use-minutes=N, close-menu-to-write=on|off.
    Set {
        #[arg(value_name = "KEY=VALUE", required = true)]
        pairs: Vec<String>,
    },
}

#[derive(Subcommand)]
enum TrashCmd {
    /// What the trash holds, newest first.
    Ls,
    /// Puts an item back under its old id.
    Restore { id: String },
}

#[derive(Subcommand)]
enum OwnerCmd {
    /// The owner strings the camera holds now.
    Get,
    /// Sets owner strings through a plan: FIELD=VALUE (copyright, author,
    /// prefix). None is confirmed yet, so this needs --allow-unconfirmed.
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
    /// Only the favourite settings.
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
    if args.iter().any(|a| a == "--schema-version") {
        if wants_json {
            println!("{}", serde_json::json!({ "schema_version": schema::SCHEMA_VERSION }));
        } else {
            println!("{}", schema::SCHEMA_VERSION);
        }
        std::process::exit(0);
    }
    for d in schema::cli_deprecations(schema::DEPRECATIONS, &args[1..]) {
        eprintln!("ukiyo: {}", d.warning());
    }
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
    if matches!(cli.cmd, Cmd::Status { .. } | Cmd::Recipes { cmd: RecipesCmd::Read { .. } | RecipesCmd::Write { .. } } | Cmd::Presets { .. } | Cmd::Snapshots { .. } | Cmd::Prefs { .. } | Cmd::Camera { .. } | Cmd::Settings(_) | Cmd::Owner { .. } | Cmd::Plan { .. } | Cmd::Log { cmd: LogCmd::Note { .. } }) {
        if let Err(f) = conn::check_data_dir() {
            std::process::exit(out.fail(&f));
        }
    }
    // Reads exit 0 on success; writes return their own exit code (writes.rs).
    let ok = |r: Result<(), Fail>| r.map(|()| 0);
    let result = match cli.cmd {
        Cmd::Status { history } => ok(cmds::status(out, history)),
        Cmd::Presets { cmd } => match cmd {
            PresetsCmd::Ls { query } => library::presets_ls(out, &query),
            PresetsCmd::Show { id } => library::presets_show(out, &id),
            PresetsCmd::Save { name, settings, set, from_snapshot, notes } => library::presets_save(out, library::PresetSave { name, settings, set, from_snapshot, notes }),
            PresetsCmd::Apply { id, flags } => library::presets_apply(out, &id, flags),
            PresetsCmd::Rm { id } => library::rm(out, ukiyo_core::library::Kind::Preset, &id),
        },
        Cmd::Snapshots { cmd } => match cmd {
            SnapshotsCmd::Ls => library::snapshots_ls(out),
            SnapshotsCmd::Show { id } => library::snapshots_show(out, &id),
            SnapshotsCmd::Save { name } => library::snapshots_save(out, name),
            SnapshotsCmd::Diff { a, b, allow_unconfirmed } => library::snapshots_diff(out, &a, b.as_deref(), allow_unconfirmed),
            SnapshotsCmd::Restore { id, flags } => library::snapshots_restore(out, &id, flags),
            SnapshotsCmd::Rm { id } => library::rm(out, ukiyo_core::library::Kind::Snapshot, &id),
        },
        Cmd::Recipes { cmd } => match cmd {
            RecipesCmd::Ls { query, sensor, sim } => recipes::ls(out, &query, sensor, sim),
            RecipesCmd::Search { query, sensor, sim, limit } => recipes::search(out, &query, sensor, sim, limit),
            RecipesCmd::Show { id } => recipes::show(out, &id),
            RecipesCmd::Import { cat, text, url, name, save } => recipes::import(out, recipes::Import { text, url, cat, name, save }),
            RecipesCmd::Edit { id, pairs } => recipes::edit(out, &id, &pairs),
            RecipesCmd::Rm { id } => recipes::rm(out, &id),
            RecipesCmd::Read { slot, compare } => recipes::read(out, slot.as_deref(), compare.as_deref()),
            RecipesCmd::Write { slot, id, flags } => recipes::write(out, &slot, &id, flags),
        },
        Cmd::Prefs { cmd: PrefsCmd::Get } => library::prefs_get(out),
        Cmd::Prefs { cmd: PrefsCmd::Set { pairs } } => library::prefs_set(out, &pairs),
        Cmd::Trash { cmd: TrashCmd::Ls } => library::trash_ls(out),
        Cmd::Trash { cmd: TrashCmd::Restore { id } } => library::trash_restore(out, &id),
        Cmd::Settings(SettingsArgs { cmd: Some(SettingsCmd::Fav { cmd: FavCmd::Add { names } }), .. }) => library::fav(out, true, &names),
        Cmd::Settings(SettingsArgs { cmd: Some(SettingsCmd::Fav { cmd: FavCmd::Rm { names } }), .. }) => library::fav(out, false, &names),
        Cmd::Camera { cmd: CameraCmd::Watch { codes, duration } } => ok(cmds::watch(out, &codes, duration.as_deref())),
        Cmd::Camera { cmd: CameraCmd::Connect } => writes::camera_connect(out, true),
        Cmd::Camera { cmd: CameraCmd::Disconnect } => writes::camera_connect(out, false),
        Cmd::Camera { cmd: CameraCmd::Export } => writes::camera_export(out),
        Cmd::Camera { cmd: CameraCmd::Capture { flags } } => writes::camera_capture(out, flags),
        Cmd::Camera { cmd: CameraCmd::Transport { kind } } => writes::camera_transport(out, kind.as_deref()),
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
        Cmd::Mcp => Ok(mcp::main()),
        Cmd::Serve => unreachable!("handled before parsing"),
    };
    std::process::exit(match result {
        Ok(code) => code,
        Err(f) => out.fail(&f),
    });
}
