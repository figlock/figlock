import BackgroundTasks
import Foundation

/// The periodic refresh iOS gives the app (ADR-0026): every remote is synced, each on its own. The system decides
/// when it runs; a person who never opens the app still gets a sync now and then, and the foreground loop of
/// `SyncModel` does the rest while the app is in front.
@MainActor
enum BackgroundSync {
    static let identifier = "com.figlock.ios.sync"

    /// Set by `AppEnvironment` once the model exists. The task handler is registered before that, at launch.
    static var model: SyncModel?

    /// Must run before the app finishes launching.
    static func register() {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: nil) { task in
            guard let task = task as? BGAppRefreshTask else { return }
            Task { @MainActor in handle(task) }
        }
    }

    /// Ask for the next refresh, no sooner than 15 minutes from now.
    static func schedule() {
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        try? BGTaskScheduler.shared.submit(request)
    }

    private static func handle(_ task: BGAppRefreshTask) {
        schedule()
        guard let model else {
            task.setTaskCompleted(success: false)
            return
        }
        let work = Task {
            await model.syncNow(force: false)
            task.setTaskCompleted(success: true)
        }
        task.expirationHandler = {
            work.cancel()
            task.setTaskCompleted(success: false)
        }
    }
}
