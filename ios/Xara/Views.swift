import SwiftUI
import WebKit

struct ModemView: View {
    @EnvironmentObject private var modem: Modem
    @AppStorage("call") private var call = ""
    @AppStorage("mode") private var mode = ModemConfig.Mode.hf2300
    @AppStorage("ptt") private var ptt = ModemConfig.Ptt.vox
    @AppStorage("rigctld") private var rigctld = "192.168.1.10:4532"
    @AppStorage("driveDb") private var driveDb = -5.0
    @AppStorage("lan") private var lan = false
    @AppStorage("uiControl") private var uiControl = false
    @AppStorage("allowBuiltIn") private var allowBuiltIn = false
    @State private var addresses: [String] = []

    private var callValid: Bool {
        let c = call.uppercased()
        return c.range(of: #"^[A-Z0-9]{3,7}(-[0-9]{1,2})?$"#, options: .regularExpression) != nil
            && !c.hasPrefix("N0CALL")
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    HStack {
                        Circle().fill(modem.running ? .green : .secondary).frame(width: 10, height: 10)
                        Text(modem.status).font(.callout)
                    }
                    Text(modem.route).font(.caption).foregroundStyle(.secondary)
                    Button(modem.running ? "Stop" : "Start", role: modem.running ? .destructive : nil) {
                        Task {
                            if modem.running { await modem.stop() } else { await modem.start(config, allowBuiltIn: allowBuiltIn) }
                        }
                    }
                    .disabled(modem.starting || (!modem.running && !callValid))
                }
                Section("Station") {
                    TextField("Call sign", text: $call)
                        .textInputAutocapitalization(.characters)
                        .autocorrectionDisabled()
                    Picker("Mode", selection: $mode) {
                        ForEach(ModemConfig.Mode.allCases) { Text($0.label).tag($0) }
                    }
                    Stepper(value: $driveDb, in: -30...0, step: 1) {
                        LabeledContent("TX drive", value: "\(Int(driveDb)) dB")
                    }
                }
                .disabled(modem.running)
                Section {
                    Picker("PTT", selection: $ptt) {
                        Text("VOX").tag(ModemConfig.Ptt.vox)
                        Text("rigctld (network CAT)").tag(ModemConfig.Ptt.rigctld)
                    }
                    if ptt == .rigctld {
                        TextField("host:port", text: $rigctld)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                    }
                    Toggle("Allow the iPhone speaker and mic", isOn: $allowBuiltIn)
                } header: {
                    Text("Radio")
                } footer: {
                    Text("xara transmits into the current audio route. Connect a USB audio interface such as a "
                        + "Digirig; with the toggle off, xara will not start or keep running on the phone's own "
                        + "speaker. iOS has no serial or HID access, so key the radio with VOX or rigctld.")
                }
                .disabled(modem.running)
                Section {
                    Toggle("Accept clients on the local network", isOn: $lan)
                    Toggle("Web UI can control the radio", isOn: $uiControl)
                } header: {
                    Text("Clients")
                } footer: {
                    Text(clientsFooter)
                }
                .disabled(modem.running)
                Section {
                    LabeledContent("xara", value: Modem.version)
                }
            }
            .navigationTitle("xara")
            .onAppear { addresses = LocalNetwork.addresses() }
        }
    }

    private var clientsFooter: String {
        let p = ModemConfig.commandPort, k = ModemConfig.kissPort
        guard lan else {
            return "VARA command port \(p), data \(p + 1), KISS \(k), on this iPhone only."
        }
        let hosts = addresses.isEmpty ? ["(no Wi-Fi address)"] : addresses
        return "Point Winlink Express, Pat or another VARA client at "
            + hosts.map { "\($0):\(p)" }.joined(separator: ", ")
            + " (data \(p + 1), KISS \(k)). Anyone on this network can use the modem."
    }

    var config: ModemConfig {
        ModemConfig(
            call: call, mode: mode, ptt: ptt, rigctld: rigctld, driveDb: driveDb, lan: lan,
            uiControl: uiControl, uiToken: UiToken.value, stateDir: UiToken.stateDir)
    }
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

    static var stateDir: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("xara")
    }
}

struct WebUIView: View {
    @EnvironmentObject private var modem: Modem
    @AppStorage("uiControl") private var uiControl = false
    @State private var reload = 0

    var body: some View {
        NavigationStack {
            Group {
                if modem.running {
                    WebView(url: url, reload: reload)
                } else {
                    ContentUnavailableView("Modem stopped", systemImage: "waveform.slash",
                                           description: Text("Start the modem to see its waterfall and status."))
                }
            }
            .navigationTitle("Web UI")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                Button { reload += 1 } label: { Image(systemName: "arrow.clockwise") }.disabled(!modem.running)
            }
        }
    }

    private var url: URL {
        let token = uiControl ? "?token=\(UiToken.value)" : ""
        return URL(string: "http://127.0.0.1:\(ModemConfig.uiPort)/\(token)")!
    }
}

struct WebView: UIViewRepresentable {
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

    func makeUIView(context: Context) -> WKWebView {
        let w = WKWebView()
        w.isOpaque = false
        w.navigationDelegate = context.coordinator
        return w
    }

    func updateUIView(_ w: WKWebView, context: Context) {
        if context.coordinator.loaded.map({ $0 != url || $1 != reload }) ?? true {
            context.coordinator.loaded = (url, reload)
            w.load(URLRequest(url: url))
        }
    }
}

struct LogView: View {
    @EnvironmentObject private var modem: Modem

    var body: some View {
        NavigationStack {
            ScrollViewReader { proxy in
                List(modem.log.indices, id: \.self) { i in
                    Text(modem.log[i]).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                }
                .listStyle(.plain)
                .onChange(of: modem.log.count) { _, n in
                    if n > 0 { proxy.scrollTo(n - 1, anchor: .bottom) }
                }
            }
            .navigationTitle("Log")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ShareLink(item: modem.log.joined(separator: "\n"))
                Button("Clear") { modem.clearLog() }
            }
        }
    }
}
