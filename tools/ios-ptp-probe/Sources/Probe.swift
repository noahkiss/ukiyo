import Foundation
@preconcurrency import ImageCaptureCore

/// One PTP exchange: what came back from ImageCaptureCore.
struct Exchange {
    var code: UInt16?
    var data: Data
    var response: Data
    var error: NSError?

    var ok: Bool { error == nil && code == PTP.ok }
}

/// Finds the camera, opens one session, and runs the probe steps.
///
/// Camera safety: one session per camera attach, no automatic reconnect, and the only writes
/// are D381 and D18C, each restored to the value read before.
@MainActor
final class Probe: NSObject, ObservableObject {
    @Published var log = ""
    @Published var status = "Searching for a USB camera..."
    @Published var mode = "unknown (run Read state)"
    @Published var cameraName: String?
    @Published var sessionOpen = false
    @Published var busy = false

    private let browser = ICDeviceBrowser()
    private var camera: ICCameraDevice?
    private var opening = false
    private var tid: UInt32 = 1

    // Signals for the mode line, from the latest definite answer in this session.
    private var d381Answers = false
    private var d18eAnswers = false
    private var descRefused = false

    static let notAuthorized = -21249 // ICReturnPTPNotAuthorizedToSendCommand

    override init() {
        super.init()
        browser.delegate = self
        let mask = ICDeviceTypeMask.camera.rawValue | ICDeviceLocationTypeMask.local.rawValue
        if let m = ICDeviceTypeMask(rawValue: mask) { browser.browsedDeviceTypeMask = m }
        browser.start()
        line("Browser started. Control auth: \(browser.controlAuthorizationStatus.rawValue), contents auth: \(browser.contentsAuthorizationStatus.rawValue)")
    }

    // MARK: Log

    func line(_ s: String) {
        let t = Date().formatted(.dateTime.hour(.twoDigits(amPM: .omitted)).minute().second())
        log += "[\(t)] \(s)\n"
    }

    func clear() { log = "" }

    // MARK: Session

    /// Asks for authorization, then opens one session. Never called automatically.
    func connect() {
        guard camera != nil, !sessionOpen, !opening else { return }
        opening = true
        status = "Requesting authorization..."
        browser.requestContentsAuthorization { [weak self] contents in
            Task { @MainActor in
                guard let self else { return }
                self.line("Contents authorization: \(contents.rawValue)")
                self.browser.requestControlAuthorization { control in
                    Task { @MainActor in
                        self.line("Control authorization: \(control.rawValue)")
                        if control != .authorized {
                            self.line("Control is not authorized. Raw PTP commands will fail with -21249. Allow camera control for PTP Probe in Settings, then reconnect the cable.")
                        }
                        self.status = "Opening session..."
                        self.camera?.requestOpenSession()
                    }
                }
            }
        }
    }

    func disconnect() {
        guard let camera, sessionOpen else { return }
        camera.requestCloseSession()
    }

    // MARK: PTP

    private func send(_ op: UInt16, _ params: [UInt32], out: Data? = nil) async -> Exchange {
        guard let camera else {
            return Exchange(code: nil, data: Data(), response: Data(),
                            error: NSError(domain: "PTPProbe", code: 0,
                                           userInfo: [NSLocalizedDescriptionKey: "no camera"]))
        }
        let cmd = PTP.command(op, tid: tid, params: params)
        tid &+= 1
        return await withCheckedContinuation { cont in
            camera.requestSendPTPCommand(cmd, outData: out) { data, response, error in
                let code = PTP.header(response).flatMap { $0.kind == PTP.kindResponse ? $0.code : nil }
                cont.resume(returning: Exchange(code: code, data: PTP.payload(data),
                                                response: response, error: error as NSError?))
            }
        }
    }

    /// Logs one exchange: the op, response code and bytes, and any known explanation.
    private func report(_ label: String, _ x: Exchange, dump: Data? = nil) {
        if let e = x.error {
            line("\(label): error \(e.domain) \(e.code) \(e.localizedDescription)")
            if e.code == Probe.notAuthorized {
                line("  -21249 ICReturnPTPNotAuthorizedToSendCommand: iOS refused to pass the raw PTP command. The app lacks camera control authorization, or iOS does not allow raw PTP for this device. Check Settings > PTP Probe, unplug and replug, and tap Connect again.")
            }
        }
        if let c = x.code {
            line(String(format: "\(label): response 0x%04X %@", c, PTP.codeName(c)))
            if let why = PTP.explain(c) { line("  \(why)") }
        } else if x.error == nil {
            line("\(label): no parsable response container")
        }
        line("  response bytes: \(PTP.hex(x.response))")
        let d = dump ?? x.data
        if !d.isEmpty { line("  data (\(d.count) bytes): \(PTP.hex(d))") }
    }

    private func prop(_ code: UInt16) -> String { String(format: "%04X", code) }

    /// GetDevicePropValue, logged. Returns the exchange.
    @discardableResult
    private func readProp(_ code: UInt16) async -> Exchange {
        let x = await send(PTP.getPropValue, [UInt32(code)])
        report("GetDevicePropValue \(prop(code))", x)
        if x.ok, let v = PTP.scalar(x.data) { line("  \(prop(code)) = \(v)\(meaning(code, v))") }
        // OK and DevicePropNotSupported say which mode this is. Busy, a menu or an error says nothing.
        let definite = x.error == nil && (x.code == PTP.ok || x.code == PTP.propNotSupported)
        if definite, code == 0xD381 { d381Answers = x.ok }
        if definite, code == 0xD18E { d18eAnswers = x.ok }
        updateMode()
        return x
    }

    /// Forgets what the last session said about the mode. The mode can change only while unplugged.
    private func resetMode() {
        d381Answers = false
        d18eAnswers = false
        descRefused = false
        mode = "unknown (run Read state)"
    }

    /// True when the camera answered that the property does not exist in this USB mode.
    private func notInMode(_ x: Exchange) -> Bool { x.error == nil && x.code == PTP.propNotSupported }

    private func writeProp(_ code: UInt16, _ value: Int, width: Int) async -> Exchange {
        let x = await send(PTP.setPropValue, [UInt32(code)], out: PTP.encode(value, width: width))
        report("SetDevicePropValue \(prop(code)) = \(value)", x)
        return x
    }

    private func meaning(_ code: UInt16, _ v: Int) -> String {
        switch (code, v) {
        case (0xD22E, 1): return " (shooting screen)"
        case (0xD22E, 2): return " (playback)"
        case (0xD22E, 3): return " (menu open)"
        case (0xD307, 1): return " (movie)"
        case (0xD307, 2): return " (stills)"
        case (0xD381, 1): return " (5 min)"
        case (0xD381, 2): return " (2 min)"
        case (0xD381, 3): return " (OFF)"
        case (0xD381, 4): return " (1 min)"
        case (0xD381, 5): return " (30 s)"
        case (0xD381, 6): return " (15 s)"
        default: return ""
        }
    }

    private func updateMode() {
        if d381Answers {
            mode = "USB TETHER SHOOTING FIXED (D381 answers)"
        } else if d18eAnswers {
            mode = descRefused
                ? "USB RAW CONV./BACKUP RESTORE (D18E answers, descriptors refused)"
                : "USB RAW CONV./BACKUP RESTORE (D18E answers)"
        } else {
            mode = "unknown (neither D381 nor D18E answered)"
        }
    }

    private func verdict(_ step: String, _ pass: Bool, _ detail: String = "") {
        line("\(pass ? "PASS" : "FAIL")  \(step)\(detail.isEmpty ? "" : " - \(detail)")")
    }

    /// A step that does not apply in this USB mode: expected, not a failure.
    private func skip(_ step: String, _ detail: String) {
        line("SKIP  \(step) - \(detail)")
    }

    // MARK: Steps

    func run(_ step: @escaping () async -> Void) {
        guard sessionOpen, !busy else { return }
        busy = true
        Task {
            await step()
            busy = false
        }
    }

    func deviceInfo() async {
        line("== Device info ==")
        let x = await send(PTP.getDeviceInfo, [])
        var info: DeviceInfo?
        var parseError: String?
        if x.ok {
            do { info = try DeviceInfo.parse(x.data) } catch { parseError = "\(error)" }
        }
        let shown = DeviceInfo.redacted(x.data, info)
        report("GetDeviceInfo", x, dump: shown)
        if x.ok, info == nil || info?.serialRange == nil {
            line("  raw data withheld: serial position unknown")
        }
        if let parseError { line("  parse failed: \(parseError)") }
        if let i = info {
            line("  manufacturer: \(i.manufacturer)")
            line("  model: \(i.model)")
            line("  firmware: \(i.firmware)")
            line("  serial: [redacted; bytes zeroed in the dump above]")
            line(String(format: "  vendor extension: id 0x%08X version %d \"%@\"",
                        i.vendorExtID, i.vendorExtVersion, i.vendorExtDesc))
            line("  operations: \(i.operations.count), properties: \(i.properties.count)")
            line("  operations: " + i.operations.map { String(format: "%04X", $0) }.joined(separator: " "))
        }
    }

    func readState() async {
        line("== Read state ==")
        for code: UInt16 in [0xD22E, 0xD307, 0xD381] { await readProp(code) }
        line("Mode probe (read only):")
        await readProp(0xD18E)
        line("Mode: \(mode)")
    }

    func describeD381() async {
        line("== Describe D381 ==")
        let x = await send(PTP.getPropDesc, [0xD381])
        report("GetDevicePropDesc D381", x)
        if x.error == nil, let c = x.code {
            // RAW CONV. mode refuses descriptors: 0x2002 on the Mac, 0x200A on an iPad.
            descRefused = c == PTP.generalError || c == PTP.propNotSupported
            updateMode()
        }
        guard x.ok else { return }
        do {
            let d = try PropDesc.parse(x.data)
            line(String(format: "  code %04X type 0x%04X %@ default %d current %d, form %@",
                        d.code, d.type, d.writable ? "rw" : "ro", d.factoryDefault, d.current, d.form))
        } catch {
            line("  parse failed: \(error)")
        }
    }

    /// Reads D381, writes OFF (or 5 min if it is already OFF), reads back, restores, reads back.
    func writeTest() async {
        line("== Write test (D381 AUTO POWER OFF) ==")
        let first = await readProp(0xD381)
        if notInMode(first) {
            skip("write test", "D381 is not available in this USB mode (not tether mode)")
            return
        }
        guard first.ok, let original = PTP.scalar(first.data) else {
            verdict("read D381", false, "no value; not writing")
            return
        }
        let width = first.data.count
        verdict("read D381", true, "\(original)\(meaning(0xD381, original))")
        let target = original == 3 ? 1 : 3
        if original == 3 { line("  D381 is already OFF; testing with 1 (5 min) instead") }

        let w = await writeProp(0xD381, target, width: width)
        verdict("write D381 = \(target)", w.ok)

        let back = await readProp(0xD381)
        let got = back.ok ? PTP.scalar(back.data) : nil
        verdict("read back D381 == \(target)", got == target, "read \(got.map(String.init) ?? "nothing")")

        if got != original {
            let r = await writeProp(0xD381, original, width: width)
            verdict("restore D381 = \(original)", r.ok)
        } else {
            line("  D381 unchanged; no restore needed")
        }
        let last = await readProp(0xD381)
        let final = last.ok ? PTP.scalar(last.data) : nil
        verdict("read back D381 == \(original) (restored)", final == original,
                "read \(final.map(String.init) ?? "nothing")")
        if final != original {
            line("  WARNING: D381 is not back to \(original). Set AUTO POWER OFF on the camera by hand.")
        }
    }

    /// Selects slot 1 with D18C, reads its name and first field, then restores D18C.
    func recipeRead() async {
        line("== Recipe read (RAW CONV. mode) ==")
        let first = await readProp(0xD18C)
        if notInMode(first) {
            skip("recipe read", "D18C is not available in this USB mode (not RAW CONV. mode)")
            return
        }
        guard first.ok, let original = PTP.scalar(first.data) else {
            verdict("read D18C", false, "no value; not writing (is the camera in RAW CONV. mode?)")
            return
        }
        let width = first.data.count
        verdict("read D18C", true, "slot \(original)")

        let w = await writeProp(0xD18C, 1, width: width)
        verdict("write D18C = 1", w.ok)

        let name = await readProp(0xD18D)
        if name.ok {
            var r = Reader(name.data)
            let s = (try? r.string()) ?? "(not a PTP string)"
            verdict("read D18D (slot name)", true, "\"\(s)\"")
        } else {
            verdict("read D18D (slot name)", false)
        }
        let field = await readProp(0xD18E)
        if notInMode(field) {
            skip("read D18E (first field)", "slot contents read only in RAW CONV. mode")
        } else {
            verdict("read D18E (first field)", field.ok,
                    field.ok ? "value \(PTP.scalar(field.data).map(String.init) ?? PTP.hex(field.data))" : "")
        }

        if original != 1 || !w.ok {
            let r = await writeProp(0xD18C, original, width: width)
            verdict("restore D18C = \(original)", r.ok)
        }
        let last = await readProp(0xD18C)
        let final = last.ok ? PTP.scalar(last.data) : nil
        verdict("read back D18C == \(original) (restored)", final == original,
                "read \(final.map(String.init) ?? "nothing")")
    }

    func runAll() async {
        line("== Run all ==")
        await deviceInfo()
        await readState()
        await describeD381()
        if d381Answers {
            await writeTest()
        } else {
            skip("write test", "D381 did not answer (not tether mode)")
        }
        if d18eAnswers {
            await recipeRead()
        } else {
            skip("recipe read", "D18E did not answer (not RAW CONV. mode)")
        }
        line("Mode: \(mode)")
        line("== Run all done ==")
    }
}

// MARK: - ICDeviceBrowserDelegate

extension Probe: ICDeviceBrowserDelegate {
    nonisolated func deviceBrowser(_ browser: ICDeviceBrowser, didAdd device: ICDevice, moreComing: Bool) {
        Task { @MainActor in
            guard let cam = device as? ICCameraDevice else {
                self.line("Ignoring non-camera device: \(device.name ?? "?")")
                return
            }
            guard self.camera == nil else {
                self.line("Ignoring a second camera: \(cam.name ?? "?")")
                return
            }
            self.camera = cam
            cam.delegate = self
            self.cameraName = cam.name
            self.status = "Camera found. Tap Connect."
            let ptp = cam.capabilities.contains(ICDeviceCapability.cameraDeviceCanAcceptPTPCommands.rawValue)
            self.line("Camera found: \(cam.name ?? "?"), USB vendor \(String(format: "0x%04X", cam.usbVendorID)) product \(String(format: "0x%04X", cam.usbProductID)), accepts PTP commands: \(ptp)")
        }
    }

    nonisolated func deviceBrowser(_ browser: ICDeviceBrowser, didRemove device: ICDevice, moreGoing: Bool) {
        Task { @MainActor in self.dropCamera(device, why: "removed by browser") }
    }

    func dropCamera(_ device: ICDevice, why: String) {
        guard device == camera else { return }
        line("Camera gone (\(why)). Not reconnecting; replug and tap Connect.")
        camera = nil
        cameraName = nil
        sessionOpen = false
        opening = false
        resetMode()
        status = "No camera. Plug one in."
    }
}

// MARK: - ICCameraDeviceDelegate

extension Probe: ICCameraDeviceDelegate {
    nonisolated func device(_ device: ICDevice, didOpenSessionWithError error: Error?) {
        Task { @MainActor in
            self.opening = false
            if let e = error as NSError? {
                self.line("Open session failed: \(e.domain) \(e.code) \(e.localizedDescription)")
                self.status = "Session failed. Replug and tap Connect."
            } else {
                self.sessionOpen = true
                self.resetMode()
                self.status = "Session open."
                self.line("Session open.")
            }
        }
    }

    nonisolated func device(_ device: ICDevice, didCloseSessionWithError error: Error?) {
        Task { @MainActor in
            self.sessionOpen = false
            self.status = "Session closed."
            self.line("Session closed\(error.map { ": \($0.localizedDescription)" } ?? ".")")
        }
    }

    nonisolated func didRemove(_ device: ICDevice) {
        Task { @MainActor in self.dropCamera(device, why: "device removed") }
    }

    nonisolated func deviceDidBecomeReady(_ device: ICDevice) {
        Task { @MainActor in self.line("Device ready.") }
    }

    nonisolated func device(_ device: ICDevice, didEncounterError error: Error?) {
        Task { @MainActor in self.line("Device error: \(error?.localizedDescription ?? "unknown")") }
    }

    nonisolated func cameraDevice(_ camera: ICCameraDevice, didAdd items: [ICCameraItem]) {}
    nonisolated func cameraDevice(_ camera: ICCameraDevice, didRemove items: [ICCameraItem]) {}
    nonisolated func cameraDevice(_ camera: ICCameraDevice, didReceiveThumbnail thumbnail: CGImage?, for item: ICCameraItem, error: Error?) {}
    nonisolated func cameraDevice(_ camera: ICCameraDevice, didReceiveMetadata metadata: [AnyHashable: Any]?, for item: ICCameraItem, error: Error?) {}
    nonisolated func cameraDevice(_ camera: ICCameraDevice, didRenameItems items: [ICCameraItem]) {}
    nonisolated func cameraDeviceDidChangeCapability(_ camera: ICCameraDevice) {}
    nonisolated func cameraDevice(_ camera: ICCameraDevice, didReceivePTPEvent eventData: Data) {
        let hex = PTP.hex(eventData)
        Task { @MainActor in self.line("PTP event: \(hex)") }
    }
    nonisolated func deviceDidBecomeReady(withCompleteContentCatalog device: ICCameraDevice) {
        Task { @MainActor in self.line("Content catalog ready.") }
    }
    nonisolated func cameraDeviceDidRemoveAccessRestriction(_ device: ICDevice) {
        Task { @MainActor in self.line("Access restriction removed (camera unlocked).") }
    }
    nonisolated func cameraDeviceDidEnableAccessRestriction(_ device: ICDevice) {
        Task { @MainActor in self.line("Access restriction enabled (camera locked).") }
    }
}
