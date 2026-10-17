//! PTP containers and datasets: the byte layouts, with no I/O. Ported from
//! `tools/ptp/read_presets.py` and `dump_props.py`.

use serde::{Deserialize, Serialize};
use specta::Type;

pub const OP_GET_DEVICE_INFO: u16 = 0x1001;
pub const OP_OPEN_SESSION: u16 = 0x1002;
pub const OP_CLOSE_SESSION: u16 = 0x1003;
pub const OP_GET_PROP_DESC: u16 = 0x1014;
pub const OP_GET_PROP_VALUE: u16 = 0x1015;
pub const OP_SET_PROP_VALUE: u16 = 0x1016;
/// InitiateCapture(0, 0): performs the gesture written to D208 first.
pub const OP_INITIATE_CAPTURE: u16 = 0x100E;

pub const RC_OK: u16 = 0x2001;
pub const RC_OPERATION_NOT_SUPPORTED: u16 = 0x2005;
pub const RC_PROP_NOT_SUPPORTED: u16 = 0x200A;
pub const RC_ACCESS_DENIED: u16 = 0x200F;
pub const RC_DEVICE_BUSY: u16 = 0x2019;
pub const RC_SESSION_ALREADY_OPEN: u16 = 0x201E;
/// Fujifilm: every property answers this while a menu is open on the camera.
pub const RC_FUJI_MENU: u16 = 0xA001;
pub const RC_GENERAL_ERROR: u16 = 0x2002;

pub const KIND_COMMAND: u16 = 1;
pub const KIND_DATA: u16 = 2;
pub const KIND_RESPONSE: u16 = 3;

/// A command container: length, kind 1, code, transaction id, then the u32 params.
pub fn command(code: u16, tid: u32, params: &[u32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(12 + 4 * params.len());
    b.extend_from_slice(&(12 + 4 * params.len() as u32).to_le_bytes());
    b.extend_from_slice(&KIND_COMMAND.to_le_bytes());
    b.extend_from_slice(&code.to_le_bytes());
    b.extend_from_slice(&tid.to_le_bytes());
    for p in params {
        b.extend_from_slice(&p.to_le_bytes());
    }
    b
}

/// A data container for the host-to-device phase.
pub fn data(code: u16, tid: u32, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(12 + payload.len());
    b.extend_from_slice(&(12 + payload.len() as u32).to_le_bytes());
    b.extend_from_slice(&KIND_DATA.to_le_bytes());
    b.extend_from_slice(&code.to_le_bytes());
    b.extend_from_slice(&tid.to_le_bytes());
    b.extend_from_slice(payload);
    b
}

/// A container header: total length, kind, code (op or response code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub length: u32,
    pub kind: u16,
    pub code: u16,
}

pub fn header(b: &[u8]) -> Option<Header> {
    if b.len() < 12 {
        return None;
    }
    Some(Header {
        length: u32::from_le_bytes(b[0..4].try_into().ok()?),
        kind: u16::from_le_bytes(b[4..6].try_into().ok()?),
        code: u16::from_le_bytes(b[6..8].try_into().ok()?),
    })
}

/// The PTP datatype codes this camera uses.
pub fn type_name(t: u16) -> &'static str {
    match t {
        0x0000 => "undefined",
        0x0001 => "int8",
        0x0002 => "uint8",
        0x0003 => "int16",
        0x0004 => "uint16",
        0x0005 => "int32",
        0x0006 => "uint32",
        0x0007 => "int64",
        0x0008 => "uint64",
        0xFFFF => "string",
        t if t & 0x4000 != 0 => "array",
        _ => "unknown",
    }
}

fn scalar_size(t: u16) -> Option<usize> {
    match t & 0xFF {
        1 | 2 => Some(1),
        3 | 4 => Some(2),
        5 | 6 => Some(4),
        7 | 8 => Some(8),
        _ => None,
    }
}

/// One decoded property value. Numbers widen to i64 (no camera value needs more).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "v", rename_all = "lowercase")]
pub enum Value {
    Int(i64),
    Str(String),
    Array(Vec<i64>),
    /// Bytes of a datatype this code cannot decode (the camera reports datatype 0 for some
    /// properties), as lowercase hex.
    Raw(String),
}

impl Value {
    /// The value as the catalog keys its labels: decimal for numbers, the text for strings.
    pub fn key(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Str(s) => s.clone(),
            Value::Array(a) => a.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(","),
            Value::Raw(h) => format!("0x{h}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A little-endian cursor over a dataset.
pub struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let s = self
            .b
            .get(self.i..self.i + n)
            .ok_or_else(|| DecodeError(format!("short dataset at byte {}", self.i)))?;
        self.i += n;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn scalar(&mut self, t: u16) -> Result<i64, DecodeError> {
        let n = scalar_size(t).ok_or_else(|| DecodeError(format!("type {t:#06x}")))?;
        let s = self.take(n)?;
        let mut le = [0u8; 8];
        le[..n].copy_from_slice(s);
        let u = u64::from_le_bytes(le);
        // Odd low byte = signed type: sign-extend.
        Ok(if (t & 0xFF) % 2 == 1 && n < 8 {
            let shift = 64 - 8 * n as u32;
            ((u << shift) as i64) >> shift
        } else {
            u as i64
        })
    }

    /// A PTP string: a u8 count of UTF-16 units, the units, a trailing NUL counted.
    pub fn string(&mut self) -> Result<String, DecodeError> {
        let n = self.u8()? as usize;
        let s = self.take(2 * n)?;
        let units: Vec<u16> = s.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
    }

    pub fn array_u16(&mut self) -> Result<Vec<u16>, DecodeError> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.u16()).collect()
    }

    pub fn value(&mut self, t: u16) -> Result<Value, DecodeError> {
        if t == 0xFFFF {
            return Ok(Value::Str(self.string()?));
        }
        if t & 0x4000 != 0 && scalar_size(t).is_some() {
            let n = self.u32()? as usize;
            return Ok(Value::Array((0..n).map(|_| self.scalar(t)).collect::<Result<_, _>>()?));
        }
        Ok(Value::Int(self.scalar(t)?))
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A GetDevicePropValue dataset in the property's datatype; anything that does not parse
/// as that type comes back as `Raw`.
pub fn decode_value(t: u16, b: &[u8]) -> Value {
    if t == 0 {
        return Value::Raw(hex(b));
    }
    let mut r = Reader::new(b);
    match r.value(t) {
        Ok(v) if r.i == b.len() => v,
        _ => Value::Raw(hex(b)),
    }
}

/// Encodes a value for SetDevicePropValue in the property's datatype.
pub fn encode(t: u16, v: &Value) -> Result<Vec<u8>, DecodeError> {
    match (t, v) {
        (0xFFFF, Value::Str(s)) => {
            let mut units: Vec<u16> = s.encode_utf16().collect();
            if units.len() >= 255 {
                return Err(DecodeError("string too long".into()));
            }
            let mut b = Vec::new();
            if units.is_empty() {
                b.push(0);
                return Ok(b);
            }
            units.push(0);
            b.push(units.len() as u8);
            for u in units {
                b.extend_from_slice(&u.to_le_bytes());
            }
            Ok(b)
        }
        (t, Value::Int(n)) if t & 0x4000 == 0 => {
            let size = scalar_size(t).ok_or_else(|| DecodeError(format!("type {t:#06x}")))?;
            let signed = (t & 0xFF) % 2 == 1;
            let bits = 8 * size as u32;
            let ok = if signed {
                bits == 64 || (*n >= -(1i64 << (bits - 1)) && *n < (1i64 << (bits - 1)))
            } else {
                bits == 64 || (*n >= 0 && *n < (1i64 << bits))
            };
            if !ok {
                return Err(DecodeError(format!("{n} does not fit {}", type_name(t))));
            }
            Ok(n.to_le_bytes()[..size].to_vec())
        }
        _ => Err(DecodeError(format!("cannot write {v:?} as {}", type_name(t)))),
    }
}

/// The value's allowed set, as the descriptor's form says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "form", rename_all = "lowercase")]
pub enum Form {
    None,
    Range { min: Value, max: Value, step: Value },
    Enum { values: Vec<Value> },
}

/// A GetDevicePropDesc dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PropDesc {
    pub code: u16,
    pub datatype: u16,
    pub writable: bool,
    pub default: Value,
    pub current: Value,
    pub form: Form,
}

pub fn prop_desc(b: &[u8]) -> Result<PropDesc, DecodeError> {
    let mut r = Reader::new(b);
    let code = r.u16()?;
    let datatype = r.u16()?;
    let writable = r.u8()? != 0;
    if datatype == 0 {
        // Datatype 0 carries no size, so nothing after the flag can be read.
        let raw = Value::Raw(String::new());
        return Ok(PropDesc { code, datatype, writable, default: raw.clone(), current: raw, form: Form::None });
    }
    let default = r.value(datatype)?;
    let current = r.value(datatype)?;
    let form = match r.u8().unwrap_or(0) {
        1 => Form::Range {
            min: r.value(datatype)?,
            max: r.value(datatype)?,
            step: r.value(datatype)?,
        },
        2 => {
            let n = r.u16()?;
            Form::Enum {
                values: (0..n).map(|_| r.value(datatype)).collect::<Result<_, _>>()?,
            }
        }
        _ => Form::None,
    };
    Ok(PropDesc { code, datatype, writable, default, current, form })
}

/// The parts of GetDeviceInfo the app uses. The serial number is never read out, except as
/// a digest ([`serial_digest`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DeviceInfo {
    pub vendor_ext: String,
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub operations: Vec<u16>,
    pub properties: Vec<u16>,
}

/// The SHA-256 of the serial number in a GetDeviceInfo dataset, as lowercase hex; `None`
/// when the dataset has no serial. The serial text goes no further than this function. The
/// library salts the digest again before it names a camera's history (`library::history`).
pub fn serial_digest(b: &[u8]) -> Option<String> {
    use sha2::Digest as _;
    let mut r = Reader::new(b);
    r.u16().ok()?;
    r.u32().ok()?;
    r.u16().ok()?;
    r.string().ok()?;
    r.u16().ok()?;
    for _ in 0..5 {
        r.array_u16().ok()?;
    }
    for _ in 0..3 {
        r.string().ok()?; // manufacturer, model, firmware
    }
    let serial = r.string().ok()?;
    let serial = serial.trim();
    (!serial.is_empty()).then(|| hex(&sha2::Sha256::digest(format!("ukiyo-serial:{serial}").as_bytes())))
}

pub fn device_info(b: &[u8]) -> Result<DeviceInfo, DecodeError> {
    let mut r = Reader::new(b);
    r.u16()?; // standard version
    r.u32()?; // vendor extension id
    r.u16()?; // vendor extension version
    let vendor_ext = r.string()?;
    r.u16()?; // functional mode
    let operations = r.array_u16()?;
    r.array_u16()?; // events
    let properties = r.array_u16()?;
    r.array_u16()?; // capture formats
    r.array_u16()?; // image formats
    let manufacturer = r.string()?;
    let model = r.string()?;
    let firmware = r.string()?;
    Ok(DeviceInfo { vendor_ext, manufacturer, model, firmware, operations, properties })
}

/// Bytes for a device-info dataset, for the fake camera and tests.
pub fn encode_device_info(i: &DeviceInfo) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&100u16.to_le_bytes());
    b.extend_from_slice(&6u32.to_le_bytes());
    b.extend_from_slice(&100u16.to_le_bytes());
    b.extend(encode(0xFFFF, &Value::Str(i.vendor_ext.clone())).unwrap());
    b.extend_from_slice(&0u16.to_le_bytes());
    let arr = |b: &mut Vec<u8>, a: &[u16]| {
        b.extend_from_slice(&(a.len() as u32).to_le_bytes());
        for x in a {
            b.extend_from_slice(&x.to_le_bytes());
        }
    };
    arr(&mut b, &i.operations);
    arr(&mut b, &[]);
    arr(&mut b, &i.properties);
    arr(&mut b, &[]);
    arr(&mut b, &[]);
    for s in [&i.manufacturer, &i.model, &i.firmware, &"FAKE".to_string()] {
        b.extend(encode(0xFFFF, &Value::Str(s.clone())).unwrap());
    }
    b
}

/// Bytes for a value dataset, for the fake camera and tests.
pub fn encode_any(t: u16, v: &Value) -> Vec<u8> {
    match v {
        Value::Raw(h) => (0..h.len() / 2).filter_map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()).collect(),
        Value::Array(a) => {
            let mut b = (a.len() as u32).to_le_bytes().to_vec();
            for n in a {
                b.extend(encode(t & !0x4000, &Value::Int(*n)).unwrap_or_default());
            }
            b
        }
        v => encode(t, v).unwrap_or_default(),
    }
}

/// Bytes for a descriptor dataset, for the fake camera and tests.
pub fn encode_prop_desc(d: &PropDesc) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&d.code.to_le_bytes());
    b.extend_from_slice(&d.datatype.to_le_bytes());
    b.push(d.writable as u8);
    b.extend(encode_any(d.datatype, &d.default));
    b.extend(encode_any(d.datatype, &d.current));
    match &d.form {
        Form::None => b.push(0),
        Form::Range { min, max, step } => {
            b.push(1);
            for v in [min, max, step] {
                b.extend(encode_any(d.datatype, v));
            }
        }
        Form::Enum { values } => {
            b.push(2);
            b.extend_from_slice(&(values.len() as u16).to_le_bytes());
            for v in values {
                b.extend(encode_any(d.datatype, v));
            }
        }
    }
    b
}

/// A name for a response code, for logs and the UI.
pub fn rc_name(rc: u16) -> &'static str {
    match rc {
        RC_OK => "OK",
        RC_GENERAL_ERROR => "GeneralError",
        RC_OPERATION_NOT_SUPPORTED => "OperationNotSupported",
        RC_PROP_NOT_SUPPORTED => "DevicePropNotSupported",
        RC_ACCESS_DENIED => "AccessDenied",
        RC_DEVICE_BUSY => "DeviceBusy",
        RC_SESSION_ALREADY_OPEN => "SessionAlreadyOpen",
        0x201C => "InvalidDevicePropFormat",
        0x201D => "InvalidDevicePropValue",
        RC_FUJI_MENU => "CameraInMenu",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_layout_matches_the_python_scripts() {
        // struct.pack("<IHHI", 16, 1, 0x1015, 7) + pack("<I", 0xD381)
        assert_eq!(
            command(OP_GET_PROP_VALUE, 7, &[0xD381]),
            vec![16, 0, 0, 0, 1, 0, 0x15, 0x10, 7, 0, 0, 0, 0x81, 0xD3, 0, 0]
        );
    }

    #[test]
    fn signed_values_sign_extend() {
        let mut r = Reader::new(&[0xFF, 0xFF]);
        assert_eq!(r.value(0x0003).unwrap(), Value::Int(-1));
        let mut r = Reader::new(&[0xFD, 0xFF, 0xFF, 0xFF]);
        assert_eq!(r.value(0x0006).unwrap(), Value::Int(0xFFFF_FFFD));
    }

    #[test]
    fn descriptors_round_trip() {
        let d = PropDesc {
            code: 0xD381,
            datatype: 0x0004,
            writable: true,
            default: Value::Int(1),
            current: Value::Int(3),
            form: Form::Enum { values: (1..=6).map(Value::Int).collect() },
        };
        assert_eq!(prop_desc(&encode_prop_desc(&d)).unwrap(), d);
        let s = PropDesc {
            code: 0x5003,
            datatype: 0xFFFF,
            writable: true,
            default: Value::Str("3888x2592".into()),
            current: Value::Str("".into()),
            form: Form::Range { min: Value::Str("a".into()), max: Value::Str("b".into()), step: Value::Str("".into()) },
        };
        assert_eq!(prop_desc(&encode_prop_desc(&s)).unwrap(), s);
    }

    #[test]
    fn device_info_round_trips() {
        let i = DeviceInfo {
            vendor_ext: "fujifilm.co.jp: 1.0;".into(),
            manufacturer: "FUJIFILM".into(),
            model: "X-T5".into(),
            firmware: "4.31".into(),
            operations: vec![0x1001, 0x1002],
            properties: vec![0xD22E, 0xD381],
        };
        assert_eq!(device_info(&encode_device_info(&i)).unwrap(), i);
    }

    #[test]
    fn encode_rejects_out_of_range() {
        assert!(encode(0x0004, &Value::Int(70000)).is_err());
        assert_eq!(encode(0x0004, &Value::Int(3)).unwrap(), vec![3, 0]);
        assert_eq!(encode(0x0003, &Value::Int(-1)).unwrap(), vec![0xFF, 0xFF]);
    }
}
