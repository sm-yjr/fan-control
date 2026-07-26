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
