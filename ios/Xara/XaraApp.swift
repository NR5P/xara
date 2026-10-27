import SwiftUI

@main
struct XaraApp: App {
    @StateObject private var modem = Modem()

    var body: some Scene {
        WindowGroup {
            TabView {
                ModemView().tabItem { Label("Modem", systemImage: "antenna.radiowaves.left.and.right") }
                WebUIView().tabItem { Label("Web UI", systemImage: "waveform") }
                LogView().tabItem { Label("Log", systemImage: "text.alignleft") }
            }
            .environmentObject(modem)
        }
    }
}
