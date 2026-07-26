import Foundation

@main
enum AppLaunchModeChecks {
    static func main() {
        require(
            FanControlLaunchMode.resolve(arguments: ["FanControl"]) == .menuBar,
            "normal launch did not select menu-bar mode"
        )
        require(
            FanControlLaunchMode.resolve(
                arguments: ["FanControl", "--helper"]
            ) == .helper,
            "--helper launch did not select helper mode"
        )
        require(
            FanControlLaunchMode.resolve(
                arguments: ["FanControl", "--check-updater-runtime"]
            ) == .updaterCheck,
            "updater runtime diagnostic did not select updater-check mode"
        )
        require(
            FanControlLaunchMode.resolve(
                arguments: [
                    "FanControl",
                    "--helper",
                    "--check-updater-runtime",
                ]
            ) == .helper,
            "helper mode must take precedence over diagnostics"
        )
        print("App launch mode checks passed")
    }

    private static func require(
        _ condition: @autoclosure () -> Bool,
        _ message: String
    ) {
        guard condition() else {
            fputs("App launch mode check failed: \(message)\n", stderr)
            exit(1)
        }
    }
}
