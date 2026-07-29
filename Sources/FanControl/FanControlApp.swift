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
            self?.fanController.prepareForSleep()
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
            debugLog("[FanControl] workspaceScreensDidSleep")
            self?.fanController.prepareForSleep()
        }

        powerNotificationObservers = [willSleep, didWake, screensDidWake, screensDidSleep]

        let powerObserver = PowerEventObserver(
            onWillSleep: { [weak self] in
                self?.fanController.prepareForSleep()
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

    private func setupCleanup() {
        signal(SIGINT) { _ in
            if geteuid() == 0 {
                _ = SMCKit.shared.resetFanControl()
            } else {
                _ = FanControlHelperClient.resetAll()
            }
            exit(0)
        }
        signal(SIGTERM) { _ in
            if geteuid() == 0 {
                _ = SMCKit.shared.resetFanControl()
            } else {
                _ = FanControlHelperClient.resetAll()
            }
            exit(0)
        }
        NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.sensorManager.stopPolling()
            self?.batteryMonitor.stopPolling()
            if self?.canWriteFans == true {
                self?.fanController.stop()
            }
            if let observers = self?.powerNotificationObservers {
                for observer in observers {
                    NSWorkspace.shared.notificationCenter.removeObserver(observer)
                }
            }
            self?.powerEventObserver?.stop()
        }
    }
}

final class AppInstanceLock {
    static let shared = AppInstanceLock()

    private var fd: Int32 = -1

    func acquire() -> Bool {
        if fd >= 0 { return true }

        let path = "/tmp/com.local.fan-control.lock"
        let opened = open(path, O_CREAT | O_RDWR, S_IRUSR | S_IWUSR)
        guard opened >= 0 else { return true }

        if flock(opened, LOCK_EX | LOCK_NB) == 0 {
            fd = opened
            return true
        }

        close(opened)
        return false
    }
}
