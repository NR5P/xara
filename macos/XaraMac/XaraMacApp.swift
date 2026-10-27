import AppKit
import ServiceManagement
import SwiftUI

@main
struct XaraMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var modem = Modem.shared

    var body: some Scene {
        Window("xara", id: "main") {
            ContentView()
                .environmentObject(modem)
                .frame(minWidth: 620, minHeight: 560)
        }
        .defaultSize(width: 760, height: 720)

        MenuBarExtra {
            MenuBarMenu().environmentObject(modem)
        } label: {
            Image(systemName: modem.running ? "antenna.radiowaves.left.and.right" : "antenna.radiowaves.left.and.right.slash")
        }
    }
}

/// Keeps xara running with no window open, and takes xara:// URLs from other apps.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        if UserDefaults.standard.bool(forKey: "autoStart") {
            Task { @MainActor in await Modem.shared.start() }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    /// xara://start and xara://stop, e.g. from a Winlink client that wants the modem up.
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls where url.scheme == "xara" {
            let verb = url.host ?? url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
            Task { @MainActor in
                switch verb {
                case "start": await Modem.shared.start()
                case "stop": await Modem.shared.stop()
                default: break
                }
            }
        }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard Modem.shared.running else { return .terminateNow }
        // Release PTT and close the ports before quitting.
        Task { @MainActor in
            await Modem.shared.stop()
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}

struct MenuBarMenu: View {
    @EnvironmentObject private var modem: Modem
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(modem.status)
        Divider()
        Button(modem.running ? "Stop modem" : "Start modem") { Task { await modem.toggle() } }
            .disabled(modem.starting)
        Button("Show xara") {
            NSApp.activate(ignoringOtherApps: true)
            openWindow(id: "main")
        }
        Button("Open web UI in browser") {
            NSWorkspace.shared.open(URL(string: "http://127.0.0.1:\(ModemConfig().uiPort)/")!)
        }
        .disabled(!modem.running)
        Divider()
        Button("Quit xara") { NSApp.terminate(nil) }.keyboardShortcut("q")
    }
}

/// Opening xara at login, through the system's login items.
enum LoginItem {
    static var enabled: Bool { SMAppService.mainApp.status == .enabled }

    static func set(_ on: Bool) {
        try? on ? SMAppService.mainApp.register() : SMAppService.mainApp.unregister()
    }
}
