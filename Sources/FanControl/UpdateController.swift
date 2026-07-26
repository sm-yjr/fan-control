import Foundation
import SparkleRuntime

final class UpdateController {
    private(set) var isAvailable = false

    init() {
        precondition(Thread.isMainThread)
        guard !CommandLine.arguments.contains("--helper") else { return }
        isAvailable = FanControlInitializeUpdater()
        if !isAvailable {
            debugLog("[FanControl] updater unavailable")
        }
    }

    func checkForUpdates() {
        precondition(Thread.isMainThread)
        guard isAvailable, FanControlCanCheckForUpdates() else { return }
        FanControlCheckForUpdates()
    }
}
