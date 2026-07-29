import Foundation
import IOKit
import Observation

struct BatteryReading: Equatable {
    let levelPercent: Double
    let powerWatts: Double
    let isCharging: Bool
    let hasExternalPower: Bool
    let adapterWatts: Double?
}

@Observable
final class BatteryMonitor {
    private(set) var reading: BatteryReading?

    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private var pollingActive = false
    @ObservationIgnored private var defaultPollingInterval: TimeInterval = 10
    @ObservationIgnored private var pollingIntervalProvider: (() -> TimeInterval)?
    @ObservationIgnored private var isRefreshInFlight = false
    @ObservationIgnored private let queue = DispatchQueue(
        label: "FanControl.Battery",
        qos: .utility
    )

    func startPolling(
        interval: TimeInterval = 10,
        intervalProvider: (() -> TimeInterval)? = nil
    ) {
        timer?.invalidate()
        timer = nil
        pollingActive = true
        defaultPollingInterval = max(0.5, interval)
        pollingIntervalProvider = intervalProvider
        updateReading()
    }

    func stopPolling() {
        pollingActive = false
        timer?.invalidate()
        timer = nil
    }

    func reschedulePolling() {
        guard pollingActive else { return }
        scheduleNextPoll()
    }

    func updateReading() {
        guard !isRefreshInFlight else { return }
        isRefreshInFlight = true

        queue.async { [weak self] in
            let reading = Self.readBattery()
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.isRefreshInFlight = false
                if self.reading != reading {
                    self.reading = reading
                }
                self.scheduleNextPoll()
            }
        }
    }

    private func scheduleNextPoll() {
        guard pollingActive else { return }
        timer?.invalidate()

        let interval = max(
            0.5,
            pollingIntervalProvider?() ?? defaultPollingInterval
        )
        let timer = Timer(timeInterval: interval, repeats: false) { [weak self] _ in
            self?.updateReading()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private static func readBattery() -> BatteryReading? {
        let service = IOServiceGetMatchingService(
            kIOMainPortDefault,
            IOServiceMatching("AppleSmartBattery")
        )
        guard service != IO_OBJECT_NULL else { return nil }
        defer { IOObjectRelease(service) }

        var propertiesRef: Unmanaged<CFMutableDictionary>?
        guard IORegistryEntryCreateCFProperties(
            service,
            &propertiesRef,
            kCFAllocatorDefault,
            0
        ) == KERN_SUCCESS,
            let properties = propertiesRef?.takeRetainedValue() as? [String: Any]
        else { return nil }

        guard
            let currentCapacity = (properties["CurrentCapacity"] as? NSNumber)?.doubleValue,
            let maxCapacity = (properties["MaxCapacity"] as? NSNumber)?.doubleValue,
            maxCapacity > 0
        else { return nil }

        let voltageMillivolts = (properties["Voltage"] as? NSNumber)?.doubleValue ?? 0
        let amperageMilliamps = normalizedAmperage(properties["Amperage"])
        let hasExternalPower = (properties["ExternalConnected"] as? NSNumber)?.boolValue ?? false

        return BatteryReading(
            levelPercent: min(max(currentCapacity / maxCapacity * 100, 0), 100),
            powerWatts: voltageMillivolts * amperageMilliamps / 1_000_000,
            isCharging: (properties["IsCharging"] as? NSNumber)?.boolValue ?? false,
            hasExternalPower: hasExternalPower,
            adapterWatts: hasExternalPower
                ? adapterWatts(properties["AdapterDetails"])
                : nil
        )
    }

    private static func adapterWatts(_ value: Any?) -> Double? {
        guard let details = value as? [String: Any] else { return nil }

        if let watts = (details["Watts"] as? NSNumber)?.doubleValue, watts > 0 {
            return watts
        }

        guard
            let voltageMillivolts = (details["Voltage"] as? NSNumber)?.doubleValue,
            let currentMilliamps = (details["Current"] as? NSNumber)?.doubleValue,
            voltageMillivolts > 0, currentMilliamps > 0
        else { return nil }
        return voltageMillivolts * currentMilliamps / 1_000_000
    }

    private static func normalizedAmperage(_ value: Any?) -> Double {
        guard let number = value as? NSNumber else { return 0 }
        let raw = number.int64Value
        // Some firmware reports discharge current as unsigned 32-bit two's complement.
        if raw > Int64(Int32.max) || raw < Int64(Int32.min) {
            return Double(Int32(truncatingIfNeeded: raw))
        }
        return Double(raw)
    }
}
