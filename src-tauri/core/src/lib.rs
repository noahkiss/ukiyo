//! ukiyo-core: everything the app does, with no Tauri in it (docs/agent-interface.md).
//!
//! - `camera`: PTP, the USB and fake cameras, and the one connection owner.
//! - `catalog`: the per-model property catalog.
//! - `recipes`: the recipe parser and the bundled recipe catalogue.
//! - `lab`: the mirror of the owner's state, the session log, export and labels.
//! - `api`: the requests, responses and events every front end speaks.
//! - `paths`: the data folder, `owner.lock` and `ukiyo.sock`.
//! - `read`: read-only views (status, settings rows, menus, change groups) for the CLI and MCP.
//! - `modes`: the USB mode walkthrough.
//! - `session`: reading the session logs back.
//! - `evidence`: the rules on a label from an agent (evidence for `confirmed`, no camera strings).
//! - `plan`: change plans: build, gate, check, and the shapes of an applied plan.
//! - `serve`: the shared owner (`ukiyo serve`), its socket protocol, and the client.

pub mod api;
pub mod camera;
pub mod catalog;
pub mod evidence;
pub mod lab;
pub mod modes;
pub mod paths;
pub mod plan;
pub mod read;
pub mod recipes;
pub mod serve;
pub mod session;
