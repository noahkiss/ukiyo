//! The real transport: PTP over USB bulk endpoints with `nusb`.

use super::ptp::{self, KIND_DATA, KIND_RESPONSE};
use super::transport::{Connector, Reply, Transport, TransportError};
use nusb::descriptors::TransferType;
use nusb::transfer::{Bulk, Direction, In, Out, TransferError as TE};
use nusb::MaybeFuture;
use std::time::Duration;

/// Fujifilm's USB vendor id.
pub const VENDOR_FUJIFILM: u16 = 0x04CB;
const TIMEOUT: Duration = Duration::from_secs(5);
/// A multiple of every bulk max packet size; data phases larger than this span reads.
const READ_CHUNK: usize = 64 * 1024;

pub struct UsbConnector;

fn find() -> Option<nusb::DeviceInfo> {
    nusb::list_devices().wait().ok()?.find(|d| d.vendor_id() == VENDOR_FUJIFILM)
}

/// macOS's PTPCamera agent claims any PTP camera it sees; stop it before claiming.
fn stop_ptpcamera() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/killall")
        .arg("PTPCamera")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn io(e: impl std::fmt::Display) -> TransportError {
    TransportError::Io(e.to_string())
}

impl Connector for UsbConnector {
    fn present(&mut self) -> bool {
        find().is_some()
    }

    fn open(&mut self, reset: bool) -> Result<Box<dyn Transport>, TransportError> {
        let info = find().ok_or(TransportError::Gone)?;
        stop_ptpcamera();
        let mut device = info.open().wait().map_err(io)?;
        if reset {
            device.reset().wait().map_err(io)?;
            drop(device);
            // The device enumerates again after a reset.
            std::thread::sleep(Duration::from_secs(2));
            let info = find().ok_or(TransportError::Gone)?;
            stop_ptpcamera();
            device = info.open().wait().map_err(io)?;
        }
        let iface = match device.claim_interface(0).wait() {
            Ok(i) => i,
            Err(e) if e.kind() == nusb::ErrorKind::Busy => {
                // PTPCamera can relaunch between the kill and the claim: one more try.
                stop_ptpcamera();
                std::thread::sleep(Duration::from_millis(300));
                device.claim_interface(0).wait().map_err(io)?
            }
            Err(e) => return Err(io(e)),
        };
        let (mut ep_in, mut ep_out) = (None, None);
        if let Some(desc) = iface.descriptor() {
            for ep in desc.endpoints() {
                if ep.transfer_type() != TransferType::Bulk {
                    continue;
                }
                match ep.direction() {
                    Direction::In => ep_in = ep_in.or(Some(ep.address())),
                    Direction::Out => ep_out = ep_out.or(Some(ep.address())),
                }
            }
        }
        let (Some(a_in), Some(a_out)) = (ep_in, ep_out) else {
            return Err(io("no bulk endpoints on interface 0"));
        };
        let input = iface.endpoint::<Bulk, In>(a_in).map_err(io)?;
        let output = iface.endpoint::<Bulk, Out>(a_out).map_err(io)?;
        Ok(Box::new(UsbTransport { _iface: iface, input, output, tid: 0 }))
    }
}

pub struct UsbTransport {
    _iface: nusb::Interface,
    input: nusb::Endpoint<Bulk, In>,
    output: nusb::Endpoint<Bulk, Out>,
    tid: u32,
}

fn transfer_error(e: TE) -> TransportError {
    match e {
        TE::Disconnected => TransportError::Gone,
        TE::Cancelled => TransportError::Timeout,
        e => io(e),
    }
}

impl UsbTransport {
    fn write(&mut self, bytes: Vec<u8>) -> Result<(), TransportError> {
        let c = self.output.transfer_blocking(bytes.into(), TIMEOUT);
        c.status.map_err(transfer_error)
    }

    fn read_some(&mut self) -> Result<Vec<u8>, TransportError> {
        let buf = self.input.allocate(READ_CHUNK);
        let c = self.input.transfer_blocking(buf, TIMEOUT);
        c.status.map_err(transfer_error)?;
        Ok(c.buffer.into_vec())
    }

    /// One whole container. A read can end on a zero-length packet; skip those.
    fn read_container(&mut self) -> Result<(ptp::Header, Vec<u8>), TransportError> {
        let mut buf = Vec::new();
        while buf.len() < 12 {
            buf.extend(self.read_some()?);
        }
        let h = ptp::header(&buf).ok_or_else(|| io("short container"))?;
        while buf.len() < h.length as usize {
            buf.extend(self.read_some()?);
        }
        buf.truncate(h.length as usize);
        Ok((h, buf.split_off(12)))
    }
}

impl Transport for UsbTransport {
    fn op(&mut self, code: u16, params: &[u32], data: Option<&[u8]>) -> Result<Reply, TransportError> {
        self.tid = self.tid.wrapping_add(1);
        let tid = self.tid;
        self.write(ptp::command(code, tid, params))?;
        if let Some(d) = data {
            self.write(ptp::data(code, tid, d))?;
        }
        let mut payload = Vec::new();
        loop {
            let (h, body) = self.read_container()?;
            match h.kind {
                KIND_DATA => payload = body,
                KIND_RESPONSE => return Ok(Reply { rc: h.code, data: payload }),
                k => return Err(io(format!("unexpected container kind {k}"))),
            }
        }
    }
}
