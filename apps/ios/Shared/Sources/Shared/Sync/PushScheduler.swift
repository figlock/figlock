import Foundation

/// Sends a local write to the remotes right away, without a sync per keystroke (FIG-261).
///
/// The first write runs `run` at once, so a saved item is on its way before the person looks away. Writes that
/// arrive while a run is going, or within `minGap` after one, are folded into one more run when the gap is over.
/// Nothing runs when nothing was written, so an idle phone wakes for nothing: the 15 second loop and the system's
/// background refresh stay the only timers.
@MainActor
public final class PushScheduler {
    private let minGap: Duration
    private let run: @MainActor () async -> Void
    private let pause: @Sendable (Duration) async -> Void
    private var task: Task<Void, Never>?
    private var again = false

    public init(
        minGap: Duration = .seconds(2),
        pause: @escaping @Sendable (Duration) async -> Void = { try? await Task.sleep(for: $0) },
        run: @escaping @MainActor () async -> Void
    ) {
        self.minGap = minGap
        self.pause = pause
        self.run = run
    }

    /// A push is running or waiting for its turn.
    public var isBusy: Bool { task != nil }

    /// Something was written: make sure a run follows it.
    public func noteWrite() {
        again = true
        guard task == nil else { return }
        task = Task { [weak self] in await self?.drain() }
    }

    /// Wait until every write noted so far has been run. The app calls this as it goes to the background, inside a
    /// background task, so the upload is not cut off.
    public func finish() async {
        await task?.value
    }

    private func drain() async {
        while again {
            again = false
            await run()
            if again { await pause(minGap) }
        }
        task = nil
    }
}
