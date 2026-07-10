import Foundation
import IOKit
import IOKit.pwr_mgt

private let ioKitCommonMessageBase: UInt32 = 0xE0000000
private let ioMessageCanSystemSleep = ioKitCommonMessageBase | 0x270
private let ioMessageSystemWillSleep = ioKitCommonMessageBase | 0x280
private let ioMessageSystemHasPoweredOn = ioKitCommonMessageBase | 0x300

final class PowerEventObserver {
    private var rootPort: io_connect_t = 0
    private var notifier: io_object_t = 0
    private var notificationPort: IONotificationPortRef?
    private let onWillSleep: () -> Void
    private let onDidWake: () -> Void

    init(onWillSleep: @escaping () -> Void, onDidWake: @escaping () -> Void) {
        self.onWillSleep = onWillSleep
        self.onDidWake = onDidWake
    }

    func start() {
        guard rootPort == 0 else { return }

        let retainedSelf = Unmanaged.passUnretained(self).toOpaque()
        var port: IONotificationPortRef?
        var notifierObject: io_object_t = 0
        let powerPort = IORegisterForSystemPower(retainedSelf, &port, powerCallback, &notifierObject)
        guard powerPort != 0, let port else {
            debugLog("[FanControl] powerObserver start failed")
            return
        }

        rootPort = powerPort
        notificationPort = port
        notifier = notifierObject

        if let runLoopSource = IONotificationPortGetRunLoopSource(port)?.takeUnretainedValue() {
            CFRunLoopAddSource(CFRunLoopGetMain(), runLoopSource, .commonModes)
        }

        debugLog("[FanControl] powerObserver started")
    }

    func stop() {
        if notifier != 0 {
            IODeregisterForSystemPower(&notifier)
            notifier = 0
        }

        if rootPort != 0 {
            IOServiceClose(rootPort)
            rootPort = 0
        }

        if let notificationPort {
            IONotificationPortDestroy(notificationPort)
            self.notificationPort = nil
        }
    }

    deinit {
        stop()
    }

    fileprivate func handle(messageType: UInt32, messageArgument: UnsafeMutableRawPointer?) {
        switch messageType {
        case ioMessageCanSystemSleep:
            debugLog("[FanControl] powerObserverCanSystemSleep")
            IOAllowPowerChange(rootPort, intptr_t(bitPattern: messageArgument))
        case ioMessageSystemWillSleep:
            debugLog("[FanControl] powerObserverSystemWillSleep")
            onWillSleep()
            IOAllowPowerChange(rootPort, intptr_t(bitPattern: messageArgument))
        case ioMessageSystemHasPoweredOn:
            debugLog("[FanControl] powerObserverSystemHasPoweredOn")
            onDidWake()
        default:
            break
        }
    }
}

private let powerCallback: IOServiceInterestCallback = { refcon, _, messageType, messageArgument in
    guard let refcon else { return }
    let observer = Unmanaged<PowerEventObserver>.fromOpaque(refcon).takeUnretainedValue()
    DispatchQueue.main.async {
        observer.handle(messageType: messageType, messageArgument: messageArgument)
    }
}
