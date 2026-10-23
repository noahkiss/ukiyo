//! Read-only wireless tether probe through the core's `WifiConnector`: discovery, the init,
//! OpenSession, GetDeviceInfo, and a read of D36B (battery) and D16E (USB mode). No writes.
//! Takes `owner.lock` like every camera client, so it refuses while Ukiyo owns the camera.
//!
//! Camera in CONNECTION MODE > WIRELESS TETHER SHOOTING FIXED, on the Mac's network:
//!   cargo run -p ukiyo-core --example wifi_probe [WAIT_SECONDS]

use std::time::{Duration, Instant};
use ukiyo_core::camera::ptp::{self, Value};
use ukiyo_core::camera::transport::Connector;
use ukiyo_core::camera::wifi::{broadcast_targets, WifiConfig, WifiConnector};

fn main() {
    let wait: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(60);
    let data = ukiyo_core::paths::data_dir();
    let _lock = match ukiyo_core::serve::OwnerLock::try_acquire(&ukiyo_core::paths::lock_path(&data)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("owner.lock is held ({e:?}): quit Ukiyo and ukiyo serve first");
            std::process::exit(1);
        }
    };
    let targets = broadcast_targets();
    println!("broadcasting from {:?}", targets.iter().map(|(ip, _)| ip.to_string()).collect::<Vec<_>>());
    let mut c = WifiConnector::start(WifiConfig { client_name: "ukiyo-probe".into(), ..WifiConfig::default() });
    let end = Instant::now() + Duration::from_secs(wait);
    while !c.present() {
        if let Some(p) = c.problem() {
            println!("discovery: {p}");
        }
        if Instant::now() >= end {
            println!("no camera answered discovery in {wait} s");
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("invitation received; init");
    let mut t = match c.open(false) {
        Ok(t) => t,
        Err(e) => {
            println!("open failed: {e}");
            std::process::exit(3);
        }
    };
    let open = t.op(ptp::OP_OPEN_SESSION, &[1], None);
    println!("OpenSession: {:?}", open.as_ref().map(|r| ptp::rc_name(r.rc)));
    if let Ok(r) = t.op(ptp::OP_GET_DEVICE_INFO, &[], None) {
        match ptp::device_info(&r.data) {
            Ok(i) => println!("{} {} fw {}: {} operations, {} properties", i.manufacturer, i.model, i.firmware, i.operations.len(), i.properties.len()),
            Err(e) => println!("GetDeviceInfo: {} ({})", ptp::rc_name(r.rc), e.0),
        }
    }
    for (code, t_) in [(0xD36Bu16, 0xFFFFu16), (0xD16E, 0x0004)] {
        match t.op(ptp::OP_GET_PROP_VALUE, &[u32::from(code)], None) {
            Ok(r) if r.rc == ptp::RC_OK => println!("{code:#06x} = {}", match ptp::decode_value(t_, &r.data) { Value::Str(s) => s, v => v.key() }),
            Ok(r) => println!("{code:#06x}: {}", ptp::rc_name(r.rc)),
            Err(e) => println!("{code:#06x}: {e}"),
        }
    }
    let _ = t.op(ptp::OP_CLOSE_SESSION, &[], None);
    println!("closed");
}
