import Foundation

/// PTP containers and datasets, little-endian. Mirrors src-tauri/core/src/camera/ptp.rs.
enum PTP {
    static let getDeviceInfo: UInt16 = 0x1001
    static let getPropDesc: UInt16 = 0x1014
    static let getPropValue: UInt16 = 0x1015
    static let setPropValue: UInt16 = 0x1016

    static let ok: UInt16 = 0x2001
    static let generalError: UInt16 = 0x2002
    static let propNotSupported: UInt16 = 0x200A
    static let deviceBusy: UInt16 = 0x2019
    static let fujiMenu: UInt16 = 0xA001

    static let kindCommand: UInt16 = 1
    static let kindData: UInt16 = 2
    static let kindResponse: UInt16 = 3

    /// A command container: length, kind 1, code, transaction id, then the u32 params.
    static func command(_ code: UInt16, tid: UInt32, params: [UInt32]) -> Data {
        var d = Data()
        d.appendLE(UInt32(12 + 4 * params.count))
        d.appendLE(kindCommand)
        d.appendLE(code)
        d.appendLE(tid)
        for p in params { d.appendLE(p) }
        return d
    }

    /// A container header at the start of `d`, if it parses.
    static func header(_ d: Data) -> (length: UInt32, kind: UInt16, code: UInt16)? {
        guard d.count >= 8 else { return nil }
        var r = Reader(d)
        guard let len = try? r.u32(), let kind = try? r.u16(), let code = try? r.u16() else { return nil }
        return (len, kind, code)
    }

    /// The data-phase payload. Strips a data container header if the framework left one on.
    static func payload(_ d: Data) -> Data {
        if d.count >= 12, let h = header(d), h.kind == kindData, Int(h.length) == d.count {
            return d.subdata(in: d.startIndex + 12 ..< d.endIndex)
        }
        return d
    }

    static func hex(_ d: Data, limit: Int = 512) -> String {
        let bytes = d.prefix(limit).map { String(format: "%02x", $0) }
        var s = stride(from: 0, to: bytes.count, by: 16)
            .map { bytes[$0 ..< min($0 + 16, bytes.count)].joined(separator: " ") }
            .joined(separator: "\n    ")
        if d.count > limit { s += "\n    ... (\(d.count) bytes total)" }
        return s.isEmpty ? "(empty)" : s
    }

    static func codeName(_ c: UInt16) -> String {
        switch c {
        case 0x2001: return "OK"
        case 0x2002: return "GeneralError"
        case 0x2003: return "SessionNotOpen"
        case 0x2005: return "OperationNotSupported"
        case 0x200A: return "DevicePropNotSupported"
        case 0x200F: return "AccessDenied"
        case 0x2019: return "DeviceBusy"
        case 0x201C: return "InvalidDevicePropFormat"
        case 0x201D: return "InvalidDevicePropValue"
        case 0x201E: return "SessionAlreadyOpen"
        case 0xA001: return "Fuji menu open"
        default: return "unknown"
        }
    }

    /// What a response code means for this camera.
    static func explain(_ c: UInt16) -> String? {
        switch c {
        case deviceBusy:
            return "Camera busy. On the X-T5 this comes for about a second just before AUTO POWER OFF drops it from USB. Wait; do not retry in a loop."
        case fujiMenu:
            return "A menu is open on the camera. Every property answers 0xA001 until you close it."
        case generalError:
            return "GeneralError. In RAW CONV. mode the camera refuses every GetDevicePropDesc with this; values still read."
        case propNotSupported:
            return "The property is not available in this USB mode (tether mode answers this for recipe fields D18E+)."
        default:
            return nil
        }
    }

    /// The little-endian bytes of `v`, `width` bytes wide (1, 2 or 4).
    static func encode(_ v: Int, width: Int) -> Data {
        var d = Data()
        switch width {
        case 1: d.append(UInt8(truncatingIfNeeded: v))
        case 4: d.appendLE(UInt32(truncatingIfNeeded: v))
        default: d.appendLE(UInt16(truncatingIfNeeded: v))
        }
        return d
    }

    /// A value read with GetDevicePropValue, decoded by its length.
    static func scalar(_ d: Data) -> Int? {
        var r = Reader(d)
        switch d.count {
        case 1: return (try? r.u8()).map(Int.init)
        case 2: return (try? r.u16()).map(Int.init)
        case 4: return (try? r.u32()).map(Int.init)
        default: return nil
        }
    }
}

struct DecodeError: Error, CustomStringConvertible {
    let description: String
}

struct Reader {
    let d: Data
    var i: Int

    init(_ d: Data) {
        self.d = Data(d)
        i = 0
    }

    var offset: Int { i }

    mutating func take(_ n: Int) throws -> Data {
        guard i + n <= d.count else { throw DecodeError(description: "short dataset at byte \(i)") }
        defer { i += n }
        return d.subdata(in: i ..< i + n)
    }

    mutating func u8() throws -> UInt8 { try take(1)[0] }

    mutating func u16() throws -> UInt16 {
        let b = try take(2)
        return UInt16(b[0]) | UInt16(b[1]) << 8
    }

    mutating func u32() throws -> UInt32 {
        let b = try take(4)
        return b.enumerated().reduce(0) { $0 | UInt32($1.element) << (8 * UInt32($1.offset)) }
    }

    /// A PTP string: a u8 count of UTF-16 units (trailing NUL counted), then the units.
    mutating func string() throws -> String {
        let n = Int(try u8())
        let b = try take(2 * n)
        let units = stride(from: 0, to: b.count, by: 2).map { UInt16(b[$0]) | UInt16(b[$0 + 1]) << 8 }
        return String(decoding: units, as: UTF16.self).trimmingCharacters(in: CharacterSet(["\0"]))
    }

    mutating func arrayU16() throws -> [UInt16] {
        let n = Int(try u32())
        return try (0 ..< n).map { _ in try u16() }
    }

    /// A scalar of PTP datatype `t`, widened to Int. Odd low byte = signed.
    mutating func scalar(_ t: UInt16) throws -> Int {
        let size: Int
        switch t & 0xFF {
        case 1, 2: size = 1
        case 3, 4: size = 2
        case 5, 6: size = 4
        case 7, 8: size = 8
        default: throw DecodeError(description: String(format: "type 0x%04x has no fixed size", t))
        }
        let b = try take(size)
        var u: UInt64 = 0
        for (k, byte) in b.enumerated() { u |= UInt64(byte) << (8 * UInt64(k)) }
        if (t & 0xFF) % 2 == 1 && size < 8 {
            let shift = UInt64(64 - 8 * size)
            return Int(Int64(bitPattern: u << shift) >> Int64(shift))
        }
        return Int(truncatingIfNeeded: u)
    }
}

/// The parts of GetDeviceInfo the probe shows. The serial number is located but never decoded.
struct DeviceInfo {
    var vendorExtID: UInt32
    var vendorExtVersion: UInt16
    var vendorExtDesc: String
    var operations: [UInt16]
    var properties: [UInt16]
    var manufacturer: String
    var model: String
    var firmware: String
    /// Byte range of the serial-number string, for redacting the raw dump.
    var serialRange: Range<Int>?

    static func parse(_ d: Data) throws -> DeviceInfo {
        var r = Reader(d)
        _ = try r.u16() // standard version
        let extID = try r.u32()
        let extVer = try r.u16()
        let extDesc = try r.string()
        _ = try r.u16() // functional mode
        let ops = try r.arrayU16()
        _ = try r.arrayU16() // events
        let props = try r.arrayU16()
        _ = try r.arrayU16() // capture formats
        _ = try r.arrayU16() // image formats
        let manufacturer = try r.string()
        let model = try r.string()
        let firmware = try r.string()
        var serial: Range<Int>?
        let start = r.offset
        if let n = try? r.u8(), (try? r.take(2 * Int(n))) != nil {
            serial = start ..< r.offset
        }
        return DeviceInfo(vendorExtID: extID, vendorExtVersion: extVer, vendorExtDesc: extDesc,
                          operations: ops, properties: props, manufacturer: manufacturer,
                          model: model, firmware: firmware, serialRange: serial)
    }

    /// `d` with the serial string's bytes zeroed, or with everything from the serial on cut
    /// when its position is unknown.
    static func redacted(_ d: Data, _ info: DeviceInfo?) -> Data {
        var out = Data(d)
        if let range = info?.serialRange {
            out.replaceSubrange(range, with: Data(count: range.count))
            return out
        }
        return Data()
    }
}

/// A GetDevicePropDesc dataset for a scalar property.
struct PropDesc {
    var code: UInt16
    var type: UInt16
    var writable: Bool
    var factoryDefault: Int
    var current: Int
    var form: String

    static func parse(_ d: Data) throws -> PropDesc {
        var r = Reader(d)
        let code = try r.u16()
        let type = try r.u16()
        let rw = try r.u8()
        let def = try r.scalar(type)
        let cur = try r.scalar(type)
        var form = "none"
        if let flag = try? r.u8() {
            switch flag {
            case 1:
                let lo = try r.scalar(type), hi = try r.scalar(type), step = try r.scalar(type)
                form = "range \(lo)...\(hi) step \(step)"
            case 2:
                let n = Int(try r.u16())
                let vals = try (0 ..< n).map { _ in try r.scalar(type) }
                form = "enum [" + vals.map(String.init).joined(separator: ", ") + "]"
            default:
                form = "none"
            }
        }
        return PropDesc(code: code, type: type, writable: rw == 1, factoryDefault: def,
                        current: cur, form: form)
    }
}

extension Data {
    mutating func appendLE<T: FixedWidthInteger>(_ v: T) {
        Swift.withUnsafeBytes(of: v.littleEndian) { append(contentsOf: $0) }
    }
}
