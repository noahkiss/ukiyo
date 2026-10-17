//! The camera: PTP framing (`ptp`), the transport seam (`transport`) with the USB
//! (`usb`) and fake (`fake`) cameras behind it, and the one connection owner (`owner`).

pub mod clock;
pub mod fake;
pub mod owner;
pub mod ptp;
pub mod transport;
pub mod usb;

#[cfg(test)]
mod owner_tests;
