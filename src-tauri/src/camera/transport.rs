//! The seam between the connection owner and a camera: `Connector` finds and opens one,
//! `Transport` runs PTP operations on it. `usb` is the real pair; `fake` the test one.

use std::fmt;

/// What a transport reports when an operation cannot finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The device left the bus (unplugged, or the camera powered off).
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
            TransportError::Io(s) => write!(f, "USB error: {s}"),
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
}
