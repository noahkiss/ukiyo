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
    pub steps: Vec<&'static str>,
}

const CONNECTION_STEPS: [&str; 4] = [
    "Unplug the USB cable. The camera locks this menu while the cable is in.",
    "On the camera, open SET UP > CONNECTION SETTING > CONNECTION MODE.",
    "", // step 3 names the mode
    "Plug the cable into this Mac. Ukiyo finds the camera on its own.",
];

/// Every mode, in `ukiyo help modes` order.
pub fn all() -> Vec<Mode> {
    let connection = |id, name, gives, pick| {
        let mut steps = CONNECTION_STEPS.to_vec();
        steps[2] = pick;
        Mode { id, name, gives, steps }
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
                "Keep the camera in USB TETHER SHOOTING FIXED.",
                "Set the STILL/MOVIE switch to MOVIE (or STILL for stills settings).",
                "Ukiyo sees the change within a second; no need to unplug.",
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
    }
}
