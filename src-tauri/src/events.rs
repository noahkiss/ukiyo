//! The window's Tauri events: one per `ukiyo_core::api::Event` variant. tauri-specta names
//! each event after its struct (`lab-status`, ...), so these names are part of the bindings.

use serde::{Deserialize, Serialize};
use specta::Type;
use ukiyo_core::camera::owner::{ChangeGroup, PropRow, Status};
use ukiyo_core::catalog::Catalog;

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LabStatus(pub Status);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LabProps(pub Vec<PropRow>);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct LabChanges(pub ChangeGroup);

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CatalogChanged(pub Option<Catalog>);
