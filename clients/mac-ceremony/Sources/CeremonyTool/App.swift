import SwiftUI

@main
struct CeremonyToolApp: App {
    var body: some Scene {
        WindowGroup("Onym Ceremony Tool") {
            ContentView()
                .frame(minWidth: 560, minHeight: 520)
        }
        .windowResizability(.contentSize)
    }
}
