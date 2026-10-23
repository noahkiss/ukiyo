//! The camera: PTP framing (`ptp`), the transport seam (`transport`) with the USB
//! (`usb`), wireless tether (`wifi`) and fake (`fake`, `fake_wifi`) cameras behind it, and
//! the one connection owner (`owner`).

pub mod clock;
pub mod fake;
pub mod fake_wifi;
pub mod owner;
pub mod ptp;
pub mod transport;
pub mod usb;
pub mod wifi;

#[cfg(test)]
mod owner_tests;
