import Foundation

/// Bounded wait used when handing fans back to system control during
/// termination.
///
/// Write completions are delivered on `completionQueue` — deliberately never
/// the main queue — because termination blocks the main thread while this
/// wait runs (system-initiated quit waits on a semaphore from
/// `willTerminate`). A completion dispatched onto the blocked main queue
/// would never execute, so every hand-back would silently burn its full
/// timeout instead of returning as soon as the writes land.
final class FanHandBackWaiter {
    let completionQueue: DispatchQueue
    private let group = DispatchGroup()

    init(label: String = "FanControl.FanHandBack") {
        completionQueue = DispatchQueue(label: label)
    }

    func enter() {
        group.enter()
    }

    func leave() {
        group.leave()
    }

    /// Returns true when every pending completion lands within the timeout;
    /// false only on a real timeout.
    func waitForCompletion(timeout: TimeInterval) -> Bool {
        group.wait(timeout: .now() + timeout) == .success
    }
}

/// Scheduling policy for the pre-sleep hand-back.
///
/// The hand-back must run synchronously on the will-sleep notification and
/// finish (or time out) before the caller acknowledges the sleep: once the
/// system is allowed to sleep, a dispatched-but-not-yet-executed hand-back
/// may never run. The wait is short and bounded; anything that does not
/// land is recovered by the multi-stage wake reapply.
enum SleepHandBackPolicy {
    static let timeout: TimeInterval = 2

    /// Two will-sleep sources (IOKit and NSWorkspace) fire within the same
    /// sleep cycle; only the first may run the hand-back. Cycles are far
    /// apart, so a short dedup window is safe and self-clears even when a
    /// sleep attempt is aborted without any wake notification.
    static let dedupWindow: TimeInterval = 5

    static func shouldHandBack(canWriteFans: Bool, fanCount: Int) -> Bool {
        canWriteFans && fanCount > 0
    }

    static func shouldPerformHandBack(
        now: TimeInterval,
        lastHandBackAt: TimeInterval?
    ) -> Bool {
        guard let lastHandBackAt else { return true }
        return now - lastHandBackAt >= dedupWindow
    }
}
