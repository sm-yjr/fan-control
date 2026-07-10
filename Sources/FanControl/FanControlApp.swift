import SwiftUI
import AppKit
import Darwin

@main
struct FanControlApp: App {
    @State private var appState: AppState

    init() {
        if CommandLine.arguments.contains("--helper") {
            FanControlHelperDaemon.run()
        }
        _appState = State(initialValue: AppState())
    }

    var body: some Scene {
        MenuBarExtra {
            ContentView(appState: appState)
        } label: {
            Image(systemName: "fan")
        }
        .menuBarExtraStyle(.window)
    }
}

@Observable
final class AppState {
    let sensorManager: SensorManager
    let fanController: FanController
    let isRunningAsRoot: Bool
    var helperAvailable: Bool = false
    var isInstallingHelper: Bool = false
    var helperMessage: String?
    var canWriteFans: Bool { isRunningAsRoot || helperAvailable }
    private var powerNotificationObservers: [NSObjectProtocol] = []
    private var powerEventObserver: PowerEventObserver?

    init() {
        let sm = SensorManager()
        self.sensorManager = sm
        self.fanController = FanController(sensorManager: sm)
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
        sm.startPolling()
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

        DispatchQueue.global(qos: .utility).async {
            let status = FanControlHelperClient.status()
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.helperAvailable = status.ok
                self.helperMessage = status.ok ? "Privileged helper is ready" : status.message
                if status.ok {
                    self.fanController.handleSensorUpdate()
                }
            }
        }
    }

    func installHelper() {
        guard !isRunningAsRoot, !isInstallingHelper else { return }
        isInstallingHelper = true
        helperMessage = "Waiting for administrator approval..."

        DispatchQueue.global(qos: .userInitiated).async {
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
                self.helperAvailable = status.ok
                self.helperMessage = status.ok ? "Privileged helper is ready" : status.message
                guard status.ok else {
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
