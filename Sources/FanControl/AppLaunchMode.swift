enum FanControlLaunchMode: Equatable {
    case menuBar
    case helper
    case updaterCheck

    static func resolve(arguments: [String]) -> FanControlLaunchMode {
        if arguments.contains("--helper") {
            return .helper
        }
        if arguments.contains("--check-updater-runtime") {
            return .updaterCheck
        }
        return .menuBar
    }
}
