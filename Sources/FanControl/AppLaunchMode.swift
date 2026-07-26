enum FanControlLaunchMode: Equatable {
    case menuBar
    case helper

    static func resolve(arguments: [String]) -> FanControlLaunchMode {
        arguments.contains("--helper") ? .helper : .menuBar
    }
}
