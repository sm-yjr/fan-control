import AppKit
import Darwin
import Observation

@main
enum FanControlMain {
    static func main() {
        switch FanControlLaunchMode.resolve(arguments: CommandLine.arguments) {
        case .helper:
            FanControlHelperDaemon.run()
        case .updaterCheck:
            exit(runUpdaterRuntimeCheck())
        case .menuBar:
            let application = NSApplication.shared
            let applicationDelegate = FanControlApplicationDelegate()
            application.delegate = applicationDelegate
            application.setActivationPolicy(.accessory)
            withExtendedLifetime(applicationDelegate) {
                application.run()
            }
        }
    }

    private static func runUpdaterRuntimeCheck() -> Int32 {
        let application = NSApplication.shared
        application.setActivationPolicy(.prohibited)
        let updateController = UpdateController()

        if updateController.isAvailable {
            print("Sparkle updater runtime is available")
            return EXIT_SUCCESS
        }

        fputs("Sparkle updater runtime is unavailable\n", stderr)
        return EXIT_FAILURE
    }
}

final class FanControlApplicationDelegate: NSObject, NSApplicationDelegate {
    private var appState: AppState?
    private var statusItemController: StatusItemController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.accessory)
        let appState = AppState()
        self.appState = appState
        self.statusItemController = StatusItemController(appState: appState)
    }
}

@Observable
final class AppState {
    let sensorManager: SensorManager
    let fanController: FanController
    let updateController: UpdateController
    let batteryMonitor: BatteryMonitor
    let isRunningAsRoot: Bool
    var helperAvailable: Bool = false
    var isInstallingHelper: Bool = false
    var helperMessage: String?
    var canWriteFans: Bool { isRunningAsRoot || helperAvailable }
    private var isPopoverPresented = false
    private var powerNotificationObservers: [NSObjectProtocol] = []
    private var powerEventObserver: PowerEventObserver?
    private var terminationRequested = false
    private var fanHandBackDone = false
    private var signalSources: [DispatchSourceSignal] = []
    private var lastSleepHandBackUptime: TimeInterval?

    init() {
        let sm = SensorManager()
        self.sensorManager = sm
        self.fanController = FanController(sensorManager: sm)
        self.updateController = UpdateController()
        self.batteryMonitor = BatteryMonitor()
        self.isRunningAsRoot = geteuid() == 0

        guard AppInstanceLock.shared.acquire() else {
            DispatchQueue.main.async {
                NSApplication.shared.terminate(nil)
            }
            return
        }

        NSApp?.setActivationPolicy(.accessory)

        sm.onSnapshotUpdated = { [weak self, weak sm] in
            guard let sm else { return }
            self?.fanController.syncFans(sm.fans)
            if self?.canWriteFans == true {
                self?.fanController.handleSensorUpdate()
            }
        }

        fanController.start()
        sm.startPolling { [weak self] in
            self?.preferredPollingInterval ?? 2
        }
        batteryMonitor.startPolling { [weak self] in
            self?.isPopoverPresented == true ? 2 : 10
        }
        refreshHelperStatus()

        setupPowerNotifications()
        setupCleanup()
    }

    func refreshHelperStatus() {
        if isRunningAsRoot {
            helperAvailable = true
            helperMessage = "Running as root"
            return
        }

        DispatchQueue.global(qos: .utility).async { [weak self] in
            let status = FanControlHelperClient.status()
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                let compatible = status.ok
                    && status.protocolVersion == FanHelperConstants.protocolVersion
                self.helperAvailable = compatible
                if status.ok && !compatible {
                    self.helperMessage = "Privileged helper update required"
                } else {
                    self.helperMessage = compatible ? "Privileged helper is ready" : status.message
                }
                if compatible {
                    self.fanController.handleSensorUpdate()
                }
            }
        }
    }

    func installHelper() {
        guard !isRunningAsRoot, !isInstallingHelper else { return }
        isInstallingHelper = true
        helperMessage = "Waiting for administrator approval..."

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let response = PrivilegedHelperManager.installCurrentAppHelper()
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.isInstallingHelper = false
                self.helperAvailable = response.ok
                self.helperMessage = response.message
                if response.ok {
                    self.fanController.handleSensorUpdate()
                }
            }
        }
    }

    func setPopoverPresented(_ isPresented: Bool) {
        guard isPopoverPresented != isPresented else { return }
        isPopoverPresented = isPresented
        sensorManager.reschedulePolling()
        batteryMonitor.reschedulePolling()
    }

    private var preferredPollingInterval: TimeInterval {
        SensorPollingPolicy.interval(
            isPopoverPresented: isPopoverPresented,
            thermalPressure: sensorManager.systemThermalPressure,
            hottestSiliconTemperature: sensorManager.hottestSiliconTemperature,
            activity: fanController.pollingActivity
        )
    }

    private func setupPowerNotifications() {
        let notificationCenter = NSWorkspace.shared.notificationCenter
        let willSleep = notificationCenter.addObserver(
            forName: NSWorkspace.willSleepNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            debugLog("[FanControl] workspaceWillSleep")
            self?.performSleepHandBackIfNeeded()
        }

        let didWake = notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            debugLog("[FanControl] workspaceDidWake")
            self?.handleWake()
        }

        let screensDidWake = notificationCenter.addObserver(
            forName: NSWorkspace.screensDidWakeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            debugLog("[FanControl] workspaceScreensDidWake")
            self?.handleWake()
        }

        let screensDidSleep = notificationCenter.addObserver(
            forName: NSWorkspace.screensDidSleepNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            // Display-only sleep: the system keeps running, so clear pending
            // controller state without handing the fans back.
            debugLog("[FanControl] workspaceScreensDidSleep")
            _ = self?.fanController.prepareForSleep()
        }

        powerNotificationObservers = [willSleep, didWake, screensDidWake, screensDidSleep]

        // The IOKit callback invokes IOAllowPowerChange only after this
        // closure returns, so the synchronous hand-back is guaranteed to
        // finish (or time out) before the system is allowed to sleep.
        let powerObserver = PowerEventObserver(
            onWillSleep: { [weak self] in
                self?.performSleepHandBackIfNeeded()
            },
            onDidWake: { [weak self] in
                self?.handleWake()
            }
        )
        powerObserver.start()
        powerEventObserver = powerObserver
    }

    private func handleWake() {
        debugLog("[FanControl] handleWake")
        // A new sleep cycle may begin; clear the will-sleep dedup marker.
        lastSleepHandBackUptime = nil
        sensorManager.updateReadings()
        reapplyAfterWake(delay: 0.5)
        reapplyAfterWake(delay: 2.0)
        reapplyAfterWake(delay: 8.0)
        reapplyAfterWake(delay: 20.0)
        reapplyAfterWake(delay: 60.0)
    }

    private func reapplyAfterWake(delay: TimeInterval) {
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self else { return }
            self.sensorManager.updateReadings()

            if !self.isRunningAsRoot {
                let status = FanControlHelperClient.status(timeout: 2.0)
                let compatible = status.ok
                    && status.protocolVersion == FanHelperConstants.protocolVersion
                self.helperAvailable = compatible
                if status.ok && !compatible {
                    self.helperMessage = "Privileged helper update required"
                } else {
                    self.helperMessage = compatible ? "Privileged helper is ready" : status.message
                }
                guard compatible else {
                    debugLog("[FanControl] wakeReapply skipped reason=helperUnavailable delay=\(delay) message=\(status.message)")
                    return
                }
            }

            guard self.canWriteFans else { return }
            self.fanController.reapplyConfiguredModes(reason: "wake+\(String(format: "%.1f", delay))s")
        }
    }

    /// Real system sleep: hand the fans back to system control and finish
    /// that hand-back BEFORE the caller acknowledges the sleep.
    ///
    /// Runs synchronously on the will-sleep path (bounded by
    /// `SleepHandBackPolicy.timeout`): the IOKit callback only calls
    /// `IOAllowPowerChange` after this returns, and the NSWorkspace handler
    /// gets the same guarantee. Waiting here is safe even on the main
    /// thread because write completions are delivered on the hand-back
    /// waiter's own queue, never on main. A per-sleep-cycle dedup window
    /// keeps the two will-sleep sources from handing back twice.
    private func performSleepHandBackIfNeeded() {
        let now = ProcessInfo.processInfo.systemUptime
        guard SleepHandBackPolicy.shouldPerformHandBack(
            now: now,
            lastHandBackAt: lastSleepHandBackUptime
        ) else {
            debugLog("[FanControl] sleepHandBack skipped reason=dedup")
            return
        }
        lastSleepHandBackUptime = now

        let fanIds = fanController.prepareForSleep()
        guard SleepHandBackPolicy.shouldHandBack(
            canWriteFans: canWriteFans,
            fanCount: fanIds.count
        ) else { return }

        debugLog("[FanControl] sleepHandBack start fans=\(fanIds.count)")
        fanController.handBackFans(fanIds, timeout: SleepHandBackPolicy.timeout)
        debugLog("[FanControl] sleepHandBack finished")
    }

    private func setupCleanup() {
        installSignalSources()
        NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.prepareForTermination()
        }
    }

    /// Signals must not run Swift runtime, SMC, socket, or logging code from
    /// the handler itself. Ignore the default disposition and move the
    /// cleanup onto the main queue through a dispatch source.
    private func installSignalSources() {
        signal(SIGINT, SIG_IGN)
        signal(SIGTERM, SIG_IGN)
        for signalNumber in [SIGINT, SIGTERM] {
            let source = DispatchSource.makeSignalSource(signal: signalNumber, queue: .main)
            source.setEventHandler { [weak self] in
                self?.requestTermination()
            }
            source.resume()
            signalSources.append(source)
        }
    }

    /// User-initiated quit (Quit button, Ctrl-C, SIGTERM): hand the fans back
    /// on a background queue with a bounded wait, then terminate. The
    /// willTerminate observer sees `fanHandBackDone` and does not repeat the
    /// work.
    func requestTermination() {
        guard !terminationRequested else { return }
        terminationRequested = true
        debugLog("[FanControl] terminationRequested canWriteFans=\(canWriteFans)")

        guard canWriteFans else {
            NSApplication.shared.terminate(nil)
            return
        }

        let fanIds = fanController.prepareForHandBack()
        let controller = fanController
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            controller.handBackFans(fanIds, timeout: 5)
            DispatchQueue.main.async {
                self?.fanHandBackDone = true
                NSApplication.shared.terminate(nil)
            }
        }
    }

    /// Runs from willTerminate on the main thread. Covers system-initiated
    /// termination (log out, restart, forced quit) where requestTermination
    /// never ran. The wait is bounded so a stalled helper cannot hang
    /// termination indefinitely.
    private func prepareForTermination() {
        sensorManager.stopPolling()
        batteryMonitor.stopPolling()
        for observer in powerNotificationObservers {
            NSWorkspace.shared.notificationCenter.removeObserver(observer)
        }
        powerNotificationObservers = []
        powerEventObserver?.stop()

        guard !fanHandBackDone, canWriteFans else {
            fanHandBackDone = true
            return
        }

        let fanIds = fanController.prepareForHandBack()
        let controller = fanController
        let finished = DispatchSemaphore(value: 0)
        DispatchQueue.global(qos: .userInitiated).async {
            controller.handBackFans(fanIds, timeout: 4)
            finished.signal()
        }
        _ = finished.wait(timeout: .now() + 5)
        fanHandBackDone = true
    }
}
