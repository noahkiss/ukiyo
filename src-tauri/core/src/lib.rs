//! ukiyo-core: everything the app does, with no Tauri in it (docs/agent-interface.md).
//!
//! - `camera`: PTP, the USB and fake cameras, and the one connection owner.
//! - `catalog`: the per-model property catalog.
//! - `recipes`: the recipe parser and the bundled recipe catalogue.
//! - `lab`: the mirror of the owner's state, the session log, export and labels.
//! - `api`: the requests, responses and events every front end speaks.
//! - `paths`: the data folder, `owner.lock` and `ukiyo.sock`.
//! - `serve`: the shared owner (`ukiyo serve`), its socket protocol, and the client.

pub mod api;
pub mod camera;
pub mod catalog;
pub mod lab;
pub mod paths;
pub mod recipes;
pub mod serve;
