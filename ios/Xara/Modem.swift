import AVFoundation
import Foundation
import XaraFFI

/// What the app asks xara to run as; turned into the same options the `xara` command takes.
struct ModemConfig {
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
        case vox, rigctld
        var id: String { rawValue }
    }

    static let commandPort = 8300
    static let kissPort = 8100
    static let uiPort = 8080

    var call: String
    var mode: Mode
    var ptt: Ptt
    var rigctld: String
    var driveDb: Double
    var lan: Bool
    var uiControl: Bool
    var uiToken: String
    var stateDir: URL
    var monitor = false
    var monitorCalls: [String] = []

    var arguments: [String] {
        var a = ["--call", call.uppercased()] + mode.options
        a += ["--capture", "route", "--playback", "route", "--radio"]
        a += ["--ptt", ptt == .vox ? "vox" : "rigctld:\(rigctld)"]
        a += ["--drive-db", String(format: "%.1f", driveDb)]
        a += ["--addr", lan ? "0.0.0.0" : "127.0.0.1"]
        a += ["--port", "\(Self.commandPort)", "--kiss-port", "\(Self.kissPort)"]
        a += ["--ui-port", "\(Self.uiPort)", "--ui-addr", lan ? "0.0.0.0" : "127.0.0.1"]
        if uiControl { a += ["--ui-control", "--ui-token", uiToken] }
        if monitor && mode.isHF {
            a += ["--monitor"]
            for c in monitorCalls { a += ["--monitor-call", c] }
        }
        a += ["--state-dir", stateDir.path]
        return a
    }
}

/// Runs the embedded Rust modem and keeps the app's view of it current.
@MainActor
final class Modem: ObservableObject {
    @Published private(set) var running = false
    @Published private(set) var starting = false
    @Published private(set) var status = "Stopped"
    @Published private(set) var log: [String] = []
    /// What the modem received, as readable text, appended as it arrives.
    @Published private(set) var received = ""
    @Published private(set) var route = AudioRoute.describe()

    private var handle: OpaquePointer?
    private var poll: Timer?
    private var lastConfig: ModemConfig?
    private var resumeAfterInterruption = false

    static var version: String { String(cString: xara_version()) }

    init() {
        let nc = NotificationCenter.default
        nc.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.routeChanged() }
        }
        nc.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] n in
            let info = n.userInfo ?? [:]
            let type = (info[AVAudioSessionInterruptionTypeKey] as? UInt).flatMap(AVAudioSession.InterruptionType.init)
            let opts = AVAudioSession.InterruptionOptions(rawValue: info[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0)
            MainActor.assumeIsolated { self?.interrupted(type, resume: opts.contains(.shouldResume)) }
        }
        poll = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    func start(_ config: ModemConfig, allowBuiltIn: Bool) async {
        guard handle == nil, !starting else { return }
        starting = true
        defer { starting = false }
        guard await AudioRoute.requestMicrophone() else {
            status = "Microphone access is off. Allow it in Settings › Xara."
            return
        }
        do {
            try AudioRoute.activate()
        } catch {
            status = "Audio session: \(error.localizedDescription)"
            return
        }
        route = AudioRoute.describe()
        if !allowBuiltIn, let why = AudioRoute.builtInReason() {
            status = "Not transmitting into \(why). Connect your radio's USB audio interface."
            return
        }
        try? FileManager.default.createDirectory(at: config.stateDir, withIntermediateDirectories: true)
        let args = config.arguments
        let owned = args.map { strdup($0) }
        defer { owned.forEach { free($0) } }
        let ptrs: [UnsafePointer<CChar>?] = owned.map { UnsafePointer($0) }
        handle = ptrs.withUnsafeBufferPointer { xara_start($0.baseAddress, $0.count) }
        guard handle != nil else {
            status = "The modem is already running."
            return
        }
        lastConfig = config
        running = true
        status = "Running \(config.mode.label) as \(config.call.uppercased())"
    }

    func stop() async {
        resumeAfterInterruption = false
        await finish()
    }

    @discardableResult
    private func finish() async -> UInt8 {
        guard let h = handle else { return 0 }
        handle = nil
        status = "Stopping…"
        let code = await Task.detached { xara_stop(h) }.value
        running = false
        status = code == 0 ? "Stopped" : "Stopped (exit \(code)): \(lastError ?? "see Log")"
        return code
    }

    private var lastError: String? {
        log.last { $0.hasPrefix("xara: ") }.map { String($0.dropFirst(6)) }
    }

    private func tick() {
        var buf = [CChar](repeating: 0, count: 2048)
        var got = false
        while xara_next_log(&buf, buf.count) {
            log.append(String(cString: buf))
            got = true
        }
        if got && log.count > 2000 { log.removeFirst(log.count - 2000) }
        var piece = ""
        while xara_next_received(&buf, buf.count) { piece += String(cString: buf) }
        if !piece.isEmpty {
            received += piece
            if received.count > Self.receivedLimit { received.removeFirst(received.count - Self.receivedLimit) }
        }
        if let h = handle, !xara_running(h) {
            Task { await finish() }
        }
    }

    private func routeChanged() {
        route = AudioRoute.describe()
        if running, let why = AudioRoute.builtInReason(), !(lastAllowBuiltIn) {
            status = "Audio moved to \(why); stopped so nothing transmits there."
            Task { await finish() }
        }
    }

    private var lastAllowBuiltIn: Bool { UserDefaults.standard.bool(forKey: "allowBuiltIn") }

    private func interrupted(_ type: AVAudioSession.InterruptionType?, resume: Bool) {
        switch type {
        case .began:
            resumeAfterInterruption = running
            Task { await finish() }
        case .ended:
            if resumeAfterInterruption && resume, let c = lastConfig {
                resumeAfterInterruption = false
                Task { await start(c, allowBuiltIn: lastAllowBuiltIn) }
            }
        default:
            break
        }
    }

    func clearLog() { log.removeAll() }
    func clearReceived() { received = "" }

    static let receivedLimit = 200_000
}
