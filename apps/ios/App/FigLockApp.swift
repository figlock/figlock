import Shared
import SwiftUI

@main
struct FigLockApp: App {
    var body: some Scene {
        WindowGroup { ContentView() }
    }
}

struct ContentView: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("FigLock").font(.largeTitle.bold())
            Text("Core linked: Argon2id \(FigLockCore.defaultKdf.memoryKiB / 1024) MiB")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .padding()
    }
}
