import AppKit
import SwiftUI
import WebKit

struct ContentView: View {
    var body: some View {
        TabView {
            ModemView().tabItem { Label("Modem", systemImage: "antenna.radiowaves.left.and.right") }
            WebUIView().tabItem { Label("Web UI", systemImage: "waveform") }
            ReceivedView().tabItem { Label("Received", systemImage: "text.bubble") }
            LogView().tabItem { Label("Log", systemImage: "text.alignleft") }
        }
        .padding(.top, 8)
    }
}

struct ModemView: View {
    @EnvironmentObject private var modem: Modem
    @AppStorage("call") private var call = ""
    @AppStorage("mode") private var mode = Mode.hf2300
    @AppStorage("capture") private var capture = ""
    @AppStorage("playback") private var playback = ""
    @AppStorage("radio") private var radio = false
    @AppStorage("ptt") private var ptt = Ptt.vox
    @AppStorage("serial") private var serial = ""
    @AppStorage("cm108") private var cm108 = "0d8c:0012"
    @AppStorage("rigctld") private var rigctld = "127.0.0.1:4532"
    @AppStorage("driveDb") private var driveDb = -5.0
    @AppStorage("leadMs") private var leadMs = 0
    @AppStorage("tailMs") private var tailMs = 100
    @AppStorage("port") private var port = 8300
    @AppStorage("kissPort") private var kissPort = 8100
    @AppStorage("uiPort") private var uiPort = 8080
    @AppStorage("lan") private var lan = false
    @AppStorage("uiControl") private var uiControl = false
    @AppStorage("monitor") private var monitor = false
    @AppStorage("monitorCalls") private var monitorCalls = ""
    @AppStorage("monitorFrames") private var monitorFrames = false
    @AppStorage("autoStart") private var autoStart = false
    @State private var loginItem = LoginItem.enabled
    @State private var devices = Devices.audio()
    @State private var serialPorts = Devices.serialPorts()

    private var inputs: [AudioDevice] { devices.filter(\.inputs) }
    /// Without radio mode xara only plays into a virtual device, so that is all it offers.
    private var outputs: [AudioDevice] { devices.filter { $0.outputs && (radio || $0.virtual) } }

    var body: some View {
        Form {
            Section {
                HStack(spacing: 10) {
                    Circle().fill(modem.running ? Color.green : Color.secondary).frame(width: 10, height: 10)
                    Text(modem.status)
                    Spacer()
                    Button(modem.running ? "Stop" : "Start") { Task { await modem.toggle() } }
                        .keyboardShortcut(.return, modifiers: .command)
                        .controlSize(.large)
                        .disabled(modem.starting || (!modem.running && !ModemConfig.callValid))
                }
                if modem.running {
                    Text(verbatim: "VARA command \(port), data \(port + 1), KISS \(kissPort), web UI \(uiPort)")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }

            Section("Station") {
                TextField("Call sign", text: $call).onChange(of: call) { _, v in call = v.uppercased() }
                Picker("Mode", selection: $mode) {
                    ForEach(Mode.allCases) { Text($0.label).tag($0) }
                }
                LabeledContent("TX drive") {
                    HStack {
                        Slider(value: $driveDb, in: -30...0, step: 1)
                        Text("\(Int(driveDb)) dB").monospacedDigit().frame(width: 50, alignment: .trailing)
                    }
                }
            }
            .disabled(modem.running)

            Section {
                Picker("Audio in", selection: $capture) {
                    Text("System default").tag("default")
                    ForEach(inputs) { Text($0.label).tag($0.uid) }
                }
                Picker("Audio out", selection: $playback) {
                    if outputs.isEmpty { Text("No virtual device (install BlackHole)").tag("") }
                    ForEach(outputs) { Text($0.label).tag($0.uid) }
                }
                Toggle("Radio mode: transmit through a real radio", isOn: $radio)
                if radio {
                    Picker("PTT", selection: $ptt) {
                        ForEach(Ptt.allCases) { Text($0.label).tag($0) }
                    }
                    switch ptt {
                    case .rts, .dtr:
                        Picker("Serial port", selection: $serial) {
                            if serialPorts.isEmpty { Text("No serial ports found").tag("") }
                            ForEach(serialPorts, id: \.self) { Text($0).tag($0) }
                        }
                    case .cm108:
                        TextField("USB id (vendor:product)", text: $cm108)
                    case .rigctld:
                        TextField("rigctld host:port", text: $rigctld)
                    case .vox:
                        EmptyView()
                    }
                    Stepper("PTT lead \(leadMs) ms", value: $leadMs, in: 0...1000, step: 10)
                    Stepper("PTT tail \(tailMs) ms", value: $tailMs, in: 0...1000, step: 10)
                }
                Button("Refresh devices") {
                    devices = Devices.audio()
                    serialPorts = Devices.serialPorts()
                }
            } header: {
                Text("Audio and radio")
            } footer: {
                Text(radio
                    ? "xara keys the radio and transmits into the chosen output. Pick your radio's USB audio interface."
                    : "Without radio mode xara only plays into a virtual device such as BlackHole, so nothing reaches a transmitter.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .disabled(modem.running)

            Section {
                Toggle("Show other stations' traffic", isOn: $monitor).disabled(!mode.isHF)
                if monitor && mode.isHF {
                    Picker("Show", selection: $monitorFrames) {
                        Text("Text only").tag(false)
                        Text("Every packet").tag(true)
                    }
                    .pickerStyle(.segmented)
                    TextField("Calls to name (optional)", text: $monitorCalls)
                }
            } header: {
                Text("Receive")
            } footer: {
                Text(mode.isHF
                    ? "The Received tab always shows your own links. With this on it also decodes VARA HF traffic "
                        + "between other stations, like VARA's monitor. Answering stations show as #hash unless "
                        + "their call is listed, separated by spaces. Every packet lists each frame heard, as "
                        + "VARA's monitor does."
                    : "Showing other stations' traffic is for VARA HF; FM shows your own links only.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .disabled(modem.running)

            Section {
                Toggle("Accept clients on the local network", isOn: $lan)
                Toggle("Web UI can control the radio", isOn: $uiControl)
                TextField("VARA command port (data is +1)", value: $port, format: .number.grouping(.never))
                TextField("KISS port", value: $kissPort, format: .number.grouping(.never))
                TextField("Web UI port", value: $uiPort, format: .number.grouping(.never))
            } header: {
                Text("Clients")
            } footer: {
                Text(clientsFooter).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            .disabled(modem.running)

            Section("App") {
                Toggle("Start the modem when xara opens", isOn: $autoStart)
                Toggle("Open xara at login", isOn: $loginItem).onChange(of: loginItem) { _, on in
                    LoginItem.set(on)
                    loginItem = LoginItem.enabled
                }
                LabeledContent("Command line") {
                    Button("Copy") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(ModemConfig().commandLine, forType: .string)
                    }
                }
                LabeledContent("Version", value: "xara \(Modem.version)")
            }
        }
        .formStyle(.grouped)
        .onAppear {
            if capture.isEmpty { capture = inputs.first(where: { !$0.virtual })?.uid ?? "default" }
            if playback.isEmpty || !outputs.contains(where: { $0.uid == playback }) {
                playback = outputs.first?.uid ?? ""
            }
        }
        .onChange(of: radio) { _, _ in
            if !outputs.contains(where: { $0.uid == playback }) { playback = outputs.first?.uid ?? "" }
        }
    }

    private var clientsFooter: String {
        guard lan else {
            return "Winlink Express, Pat and other VARA clients on this Mac connect to 127.0.0.1:\(port) "
                + "(data \(port + 1)). Other apps can start the modem with xara://start."
        }
        let hosts = LocalNetwork.addresses()
        return "Clients elsewhere on the network connect to "
            + (hosts.isEmpty ? "this Mac" : hosts.map { "\($0):\(port)" }.joined(separator: ", "))
            + ". Anyone on this network can use the modem."
    }
}

struct WebUIView: View {
    @EnvironmentObject private var modem: Modem
    @AppStorage("uiControl") private var uiControl = false
    @AppStorage("uiPort") private var uiPort = 8080
    @State private var reload = 0

    var body: some View {
        Group {
            if modem.running {
                VStack(spacing: 0) {
                    HStack {
                        Spacer()
                        Button("Open in browser") { NSWorkspace.shared.open(url) }
                        Button { reload += 1 } label: { Image(systemName: "arrow.clockwise") }
                    }
                    .padding(8)
                    WebView(url: url, reload: reload)
                }
            } else {
                ContentUnavailableView("Modem stopped", systemImage: "waveform.slash",
                                       description: Text("Start the modem to see its waterfall and status."))
            }
        }
    }

    private var url: URL {
        URL(string: "http://127.0.0.1:\(uiPort)/" + (uiControl ? "?token=\(UiToken.value)" : ""))!
    }
}

struct WebView: NSViewRepresentable {
    let url: URL
    let reload: Int

    /// Retries until the UI server answers; it comes up a moment after Start.
    final class Coordinator: NSObject, WKNavigationDelegate {
        var loaded: (URL, Int)?

        func webView(_ w: WKWebView, didFailProvisionalNavigation _: WKNavigation!, withError _: Error) {
            guard let url = loaded?.0 else { return }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak w] in w?.load(URLRequest(url: url)) }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let w = WKWebView()
        w.navigationDelegate = context.coordinator
        return w
    }

    func updateNSView(_ w: WKWebView, context: Context) {
        if context.coordinator.loaded.map({ $0 != url || $1 != reload }) ?? true {
            context.coordinator.loaded = (url, reload)
            w.load(URLRequest(url: url))
        }
    }
}

struct ReceivedView: View {
    @EnvironmentObject private var modem: Modem

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(modem.received.isEmpty ? "" : "\(modem.received.count) characters")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Copy") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(modem.received, forType: .string)
                }
                Button("Clear") { modem.clearReceived() }
            }
            .padding(8)
            ScrollViewReader { proxy in
                ScrollView {
                    Text(modem.received.isEmpty ? "Nothing received yet." : modem.received)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(modem.received.isEmpty ? .secondary : .primary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding()
                    Color.clear.frame(height: 1).id("end")
                }
                .onChange(of: modem.received) { _, _ in proxy.scrollTo("end", anchor: .bottom) }
            }
        }
    }
}

struct LogView: View {
    @EnvironmentObject private var modem: Modem

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Spacer()
                Button("Copy") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(modem.log.joined(separator: "\n"), forType: .string)
                }
                Button("Clear") { modem.clearLog() }
            }
            .padding(8)
            ScrollViewReader { proxy in
                List(modem.log.indices, id: \.self) { i in
                    Text(modem.log[i]).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                }
                .onChange(of: modem.log.count) { _, n in
                    if n > 0 { proxy.scrollTo(n - 1, anchor: .bottom) }
                }
            }
        }
    }
}
