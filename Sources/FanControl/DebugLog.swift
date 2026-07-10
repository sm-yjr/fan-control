import OSLog

private let fanControlLogger = Logger(subsystem: "com.local.fan-control", category: "debug")

func debugLog(_ message: String) {
    fanControlLogger.info("\(message, privacy: .public)")
}
