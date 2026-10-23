//! The seam between the connection owner and a camera: `Connector` finds and opens one,
//! `Transport` runs PTP operations on it. `usb` and `wifi` are the real pairs; `fake` the
//! test one (and `fake_wifi` puts it behind a loopback Wi-Fi endpoint).

use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;

/// How the owner reaches the camera.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    /// The USB cable (CONNECTION MODE: USB TETHER SHOOTING FIXED, USB RAW CONV. ...).
    #[default]
    Usb,
    /// Wireless tether (CONNECTION MODE: WIRELESS TETHER SHOOTING FIXED), on the Mac's
    /// network.
    Wifi,
}

impl TransportKind {
    /// `usb` or `wifi`, as the preference and the CLI write it.
    pub fn id(self) -> &'static str {
        match self {
            TransportKind::Usb => "usb",
            TransportKind::Wifi => "wifi",
        }
    }

    /// `usb`, `wifi` or `wi-fi`, any case.
    pub fn parse(s: &str) -> Option<TransportKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "usb" => Some(TransportKind::Usb),
            "wifi" | "wi-fi" => Some(TransportKind::Wifi),
            _ => None,
        }
    }

    /// `USB` or `Wi-Fi`, for messages.
    pub fn label(self) -> &'static str {
        match self {
            TransportKind::Usb => "USB",
            TransportKind::Wifi => "Wi-Fi",
        }
    }
}

/// What a transport reports when an operation cannot finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The device left the bus or the network (unplugged, out of reach, or powered off).
    Gone,
    /// No answer in time.
    Timeout,
    /// Anything else: a stall, a short or malformed container.
    Io(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Gone => f.write_str("device gone"),
            TransportError::Timeout => f.write_str("timed out"),
            TransportError::Io(s) => f.write_str(s),
        }
    }
}

/// The result of one PTP operation: the response code and the data phase, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub rc: u16,
    pub data: Vec<u8>,
}

/// One open, claimed camera. Each call is one PTP transaction.
pub trait Transport: Send {
    fn op(&mut self, code: u16, params: &[u32], data: Option<&[u8]>) -> Result<Reply, TransportError>;
}

/// Finds and opens a camera.
pub trait Connector: Send {
    /// Whether a camera is on the bus. Cheap; never claims it.
    fn present(&mut self) -> bool;
    /// Claims the camera. `reset` resets the USB device first; the owner asks for it at
    /// most once, and only after OpenSession on a fresh claim failed.
    fn open(&mut self, reset: bool) -> Result<Box<dyn Transport>, TransportError>;
    /// USB or Wi-Fi.
    fn kind(&self) -> TransportKind {
        TransportKind::Usb
    }
    /// Why the connector cannot look for a camera at all (a port it cannot bind), for the
    /// status reason. `None` when it can.
    fn problem(&mut self) -> Option<String> {
        None
    }
}
