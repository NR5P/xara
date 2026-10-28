import AVFoundation
import Foundation
import XaraFFI

enum Mode: String, CaseIterable, Identifiable {
    case hf500, hf2300, hf2750, fmNarrow, fmWide
    var id: String { rawValue }
    var label: String {
        switch self {
        case .hf500: "HF 500 Hz"
        case .hf2300: "HF 2300 Hz"
        case .hf2750: "HF 2750 Hz"
        case .fmNarrow: "FM narrow"
        case .fmWide: "FM wide"
        }
    }
    var isHF: Bool { self == .hf500 || self == .hf2300 || self == .hf2750 }
    var options: [String] {
        switch self {
        case .hf500: ["--hf", "500"]
        case .hf2300: ["--hf", "2300"]
        case .hf2750: ["--hf", "2750"]
        case .fmNarrow: ["--fm", "narrow"]
        case .fmWide: ["--fm", "wide"]
        }
    }
}

enum Ptt: String, CaseIterable, Identifiable {
    case vox, rts, dtr, cm108, rigctld
    var id: String { rawValue }
    var label: String {
        switch self {
        case .vox: "VOX"
        case .rts: "Serial RTS"
        case .dtr: "Serial DTR"
        case .cm108: "CM108 / AIOC (USB HID)"
        case .rigctld: "rigctld (hamlib CAT)"
        }
    }
}

/// The saved settings, under the keys the views' @AppStorage use, turned into xara's options.
struct ModemConfig {
    static let defaults = UserDefaults.standard

    var call: String { Self.defaults.string(forKey: "call") ?? "" }
    var mode: Mode { Mode(rawValue: Self.defaults.string(forKey: "mode") ?? "") ?? .hf2300 }
    var capture: String { Self.defaults.string(forKey: "capture") ?? "" }
    var playback: String { Self.defaults.string(forKey: "playback") ?? "" }
    var radio: Bool { Self.defaults.bool(forKey: "radio") }
    var ptt: Ptt { Ptt(rawValue: Self.defaults.string(forKey: "ptt") ?? "") ?? .vox }
    var serial: String { Self.defaults.string(forKey: "serial") ?? "" }
    var cm108: String { Self.defaults.string(forKey: "cm108") ?? "0d8c:0012" }
    var rigctld: String { Self.defaults.string(forKey: "rigctld") ?? "127.0.0.1:4532" }
    var driveDb: Double { Self.defaults.object(forKey: "driveDb") as? Double ?? -5 }
    var leadMs: Int { Self.defaults.integer(forKey: "leadMs") }
    var tailMs: Int { Self.defaults.object(forKey: "tailMs") as? Int ?? 100 }
    var port: Int { Self.defaults.object(forKey: "port") as? Int ?? 8300 }
    var kissPort: Int { Self.defaults.object(forKey: "kissPort") as? Int ?? 8100 }
    var uiPort: Int { Self.defaults.object(forKey: "uiPort") as? Int ?? 8080 }
    var lan: Bool { Self.defaults.bool(forKey: "lan") }
    var uiControl: Bool { Self.defaults.bool(forKey: "uiControl") }
    var monitor: Bool { Self.defaults.bool(forKey: "monitor") }
    var monitorCalls: [String] { calls(Self.defaults.string(forKey: "monitorCalls") ?? "") }
    var monitorFrames: Bool { Self.defaults.bool(forKey: "monitorFrames") }
    /// Registered VARA speeds, for a user with a VARA licence.
    var registered: Bool { Self.defaults.bool(forKey: "registered") }

    static var callValid: Bool {
        let c = ModemConfig().call.uppercased()
        return c.range(of: #"^[A-Z0-9]{3,7}(-[0-9]{1,2})?$"#, options: .regularExpression) != nil && !c.hasPrefix("N0CALL")
    }

    var pttOption: String {
        switch ptt {
        case .vox: "vox"
        case .rts: "rts:\(serial)"
        case .dtr: "dtr:\(serial)"
        case .cm108: "cm108:\(cm108)"
        case .rigctld: "rigctld:\(rigctld)"
        }
    }

    var arguments: [String] {
        let bind = lan ? "0.0.0.0" : "127.0.0.1"
        var a = ["--call", call.uppercased()] + mode.options
        a += ["--audio", "coreaudio", "--capture", capture, "--playback", playback]
        if radio {
            a += ["--radio", "--ptt", pttOption, "--ptt-lead-ms", "\(leadMs)", "--ptt-tail-ms", "\(tailMs)"]
        }
        a += ["--drive-db", String(format: "%.1f", driveDb)]
        a += ["--addr", bind, "--port", "\(port)", "--kiss-port", "\(kissPort)"]
        a += ["--ui-port", "\(uiPort)", "--ui-addr", bind]
        if uiControl { a += ["--ui-control", "--ui-token", UiToken.value] }
        if monitor && mode.isHF {
            a += ["--monitor"]
            if monitorFrames { a += ["--monitor-frames"] }
            for c in monitorCalls { a += ["--monitor-call", c] }
        }
        if registered { a += ["--registered"] }
        a += ["--state-dir", Self.stateDir.path]
        return a
    }

    static var stateDir: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("xara")
    }

    /// The equivalent `xara` command, for running it from Terminal or a launchd agent instead.
    var commandLine: String {
        (["xara"] + arguments).map { $0.contains(" ") || $0.isEmpty ? "'\($0)'" : $0 }.joined(separator: " ")
    }
}

/// Calls from a free-form list: spaces or commas between them.
func calls(_ s: String) -> [String] {
    s.uppercased().split(whereSeparator: { $0 == " " || $0 == "," }).map(String.init)
}

enum UiToken {
    /// A token kept across launches, so a bookmarked controlling web UI keeps working.
    static var value: String {
        if let t = UserDefaults.standard.string(forKey: "uiToken") { return t }
        let chars = Array("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789")
        let t = String((0..<32).map { _ in chars.randomElement()! })
        UserDefaults.standard.set(t, forKey: "uiToken")
        return t
    }
}

/// Runs the embedded Rust modem and keeps the app's view of it current.
@MainActor
final class Modem: ObservableObject {
    static let shared = Modem()

    @Published private(set) var running = false
    @Published private(set) var starting = false
    @Published private(set) var status = "Stopped"
    @Published private(set) var log: [String] = []
    /// What the modem received, as readable text, appended as it arrives.
    @Published private(set) var received = ""

    private var handle: OpaquePointer?
    private var poll: Timer?

    static var version: String { String(cString: xara_version()) }

    private init() {
        poll = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    func start() async {
        guard handle == nil, !starting else { return }
        guard ModemConfig.callValid else {
            status = "Set your call sign first."
            return
        }
        starting = true
        defer { starting = false }
        guard await AVCaptureDevice.requestAccess(for: .audio) else {
            status = "Microphone access is off. Allow Xara in System Settings › Privacy & Security › Microphone."
            return
        }
        let config = ModemConfig()
        try? FileManager.default.createDirectory(at: ModemConfig.stateDir, withIntermediateDirectories: true)
        let owned = config.arguments.map { strdup($0) }
        defer { owned.forEach { free($0) } }
        let ptrs: [UnsafePointer<CChar>?] = owned.map { UnsafePointer($0) }
        handle = ptrs.withUnsafeBufferPointer { xara_start($0.baseAddress, $0.count) }
        guard handle != nil else {
            status = "The modem is already running."
            return
        }
        running = true
        status = "Running \(config.mode.label) as \(config.call.uppercased())"
    }

    func stop() async {
        guard let h = handle else { return }
        handle = nil
        status = "Stopping…"
        let code = await Task.detached { xara_stop(h) }.value
        tick()
        running = false
        let why = log.last { $0.hasPrefix("xara: ") }.map { String($0.dropFirst(6)) } ?? "see Log"
        status = code == 0 ? "Stopped" : "Stopped (exit \(code)): \(why)"
    }

    func toggle() async {
        if running { await stop() } else { await start() }
    }

    private func tick() {
        var buf = [CChar](repeating: 0, count: 2048)
        var got = false
        while xara_next_log(&buf, buf.count) {
            log.append(String(cString: buf))
            got = true
        }
        if got && log.count > 5000 { log.removeFirst(log.count - 5000) }
        var piece = ""
        while xara_next_received(&buf, buf.count) { piece += String(cString: buf) }
        if !piece.isEmpty {
            received += piece
            if received.count > Self.receivedLimit { received.removeFirst(received.count - Self.receivedLimit) }
        }
        if let h = handle, !xara_running(h) {
            Task { await stop() }
        }
    }

    func clearLog() { log.removeAll() }
    func clearReceived() { received = "" }

    static let receivedLimit = 500_000
}
