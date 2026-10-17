//! The USB mode walkthrough (task-klgaidjz): how to put the camera in the mode a feature needs.
//! The CLI prints it as `ukiyo help modes`, and every `needs_*` error points at it. The window's
//! Connect page shows the same steps (`app/src/shell/ModeNotice.tsx`).

use serde::Serialize;

/// One mode a person can set on the camera.
#[derive(Debug, Clone, Serialize)]
pub struct Mode {
    /// `tether`, `rawconv` or `movie`: the argument to `ukiyo help modes`.
    pub id: &'static str,
    /// The name as the camera's menu shows it.
    pub name: &'static str,
    /// What Ukiyo can do in this mode.
    pub gives: &'static str,
    pub steps: Vec<String>,
}

/// Where CONNECTION MODE sits from the X-T5 generation on (owner's manuals in `catalog/menus/`).
pub const NETWORK_MENU: &[&str] = &["NETWORK/USB SETTING", "CONNECTION MODE"];
/// Where it sits on the X-T3 and X-T4 generation.
pub const SETUP_MENU: &[&str] = &["SET UP", "CONNECTION SETTING", "CONNECTION MODE"];
/// Where it sits on the X-T2 generation, under the older item name.
pub const PC_MENU: &[&str] = &["SET UP", "CONNECTION SETTING", "PC CONNECTION MODE"];

/// The CONNECTION MODE menu path for a model (`X-T5`, `X-T30 II`), from its owner's manual.
/// `None` for a body without a CONNECTION MODE item in `catalog/menus/`.
/// `app/src/shell/ModeNotice.tsx` holds the same table.
pub fn connection_menu(model: &str) -> Option<&'static [&'static str]> {
    let slug = model.trim().to_ascii_lowercase().replace(' ', "-");
    match slug.as_str() {
        "x-t5" | "x-t50" | "x-e5" | "x-m5" | "x-s20" | "x100vi" => Some(NETWORK_MENU),
        "x-t3" | "x-t4" | "x-t30" | "x-t30-ii" | "x-s10" | "x-e4" | "x100v" => Some(SETUP_MENU),
        "x-t2" | "x-h1" | "x-pro2" | "x-pro3" | "x100f" => Some(PC_MENU),
        _ => None,
    }
}

/// The "open this menu" step: the model's path, or both common paths for an unknown model.
pub fn open_menu_step(model: Option<&str>) -> String {
    match model.and_then(connection_menu) {
        Some(path) => format!("On the camera, open {}.", path.join(" > ")),
        None => format!("On the camera, open {}. On the X-T4 and older: {}.", NETWORK_MENU.join(" > "), SETUP_MENU.join(" > ")),
    }
}

/// Every mode, in `ukiyo help modes` order, with the menu path for any body.
pub fn all() -> Vec<Mode> {
    all_for(None)
}

/// Every mode, with the menu path for `model`.
pub fn all_for(model: Option<&str>) -> Vec<Mode> {
    let connection = |id, name, gives, pick: &str| Mode {
        id,
        name,
        gives,
        steps: vec![
            "Unplug the USB cable. The camera locks this menu while the cable is in.".into(),
            open_menu_step(model),
            pick.into(),
            "Plug the cable into this Mac. Ukiyo finds the camera on its own.".into(),
        ],
    };
    vec![
        connection(
            "tether",
            "USB TETHER SHOOTING FIXED",
            "Read and change settings, the Hub, the Lab. The camera keeps working.",
            "Pick USB TETHER SHOOTING FIXED.",
        ),
        connection(
            "rawconv",
            "USB RAW CONV./BACKUP RESTORE",
            "Read and write recipe slots C1 to C7. The camera is locked until you unplug.",
            "Pick USB RAW CONV./BACKUP RESTORE.",
        ),
        Mode {
            id: "movie",
            name: "STILL/MOVIE switch at MOVIE",
            gives: "Movie settings, in tether mode. Stills settings need the switch at STILL.",
            steps: vec![
                "Keep the camera in USB TETHER SHOOTING FIXED.".into(),
                "Set the STILL/MOVIE switch to MOVIE (or STILL for stills settings).".into(),
                "Ukiyo sees the change within a second; no need to unplug.".into(),
            ],
        },
    ]
}

/// One mode by id (`tether`, `rawconv`, `movie`); `raw-conv` and `rawConv` also work.
pub fn find(id: &str) -> Option<Mode> {
    let id = id.to_ascii_lowercase().replace(['-', '_', '.'], "");
    all().into_iter().find(|m| m.id == id)
}

/// The command that prints a mode's walkthrough.
pub fn walkthrough(id: &str) -> String {
    format!("ukiyo help modes {id}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn modes_resolve() {
        assert_eq!(super::find("RAW-CONV").unwrap().id, "rawconv");
        assert_eq!(super::find("rawConv").unwrap().name, "USB RAW CONV./BACKUP RESTORE");
        assert!(super::find("cardreader").is_none());
        assert!(super::all().iter().all(|m| m.steps.iter().all(|s| !s.is_empty())));
        assert_eq!(super::all_for(Some("X-T5"))[0].steps[1], "On the camera, open NETWORK/USB SETTING > CONNECTION MODE.");
        assert!(super::all()[0].steps[1].contains("SET UP > CONNECTION SETTING > CONNECTION MODE"));
    }

    /// The table matches every owner's manual in `catalog/menus/`.
    #[test]
    fn connection_menu_matches_the_manuals() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/menus");
        let mut n = 0;
        for f in std::fs::read_dir(&dir).unwrap() {
            let path = f.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let model = d["model"].as_str().unwrap();
            let mut found = None;
            for t in d["tabs"].as_array().unwrap() {
                let items = t["pages"].as_array().into_iter().flatten().flat_map(|p| p["items"].as_array().into_iter().flatten());
                for item in items {
                    let name = item["name"].as_str().unwrap_or_default();
                    if name.ends_with("CONNECTION MODE") {
                        let tab = t["name"].as_str().unwrap().to_string();
                        found = Some(match t["menu"].as_str() {
                            Some("setup") => vec!["SET UP".to_string(), tab, name.to_string()],
                            _ => vec![tab, name.to_string()],
                        });
                    }
                }
            }
            let table = super::connection_menu(model).map(|p| p.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            assert_eq!(table, found, "{model}");
            n += 1;
        }
        assert!(n > 10, "read {n} menu files");
    }
}
