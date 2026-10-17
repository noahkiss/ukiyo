//! ukiyo-core: everything the app does, with no Tauri in it (docs/agent-interface.md).
//!
//! - `camera`: PTP, the USB and fake cameras, and the one connection owner.
//! - `catalog`: the per-model property catalog.
//! - `recipes`: the recipe parser and the bundled recipe catalogue.
//! - `lab`: the mirror of the owner's state, the session log, export and labels.
//! - `api`: the requests, responses and events every front end speaks.

pub mod api;
pub mod camera;
pub mod catalog;
pub mod lab;
pub mod recipes;
