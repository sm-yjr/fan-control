import Foundation

@main
enum AppInstanceLockChecks {
    static func main() {
        checkDefaultLocationIsUserPrivate()
        checkAcquireAndContention()
        checkReleaseAllowsReacquire()
        checkSymlinkFailsClosed()
        checkUnavailableDirectoryFailsClosed()
        print("App instance lock checks passed")
    }

    private static func temporaryDirectory(_ name: String) -> URL {
        let base = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "fan-control-lock-checks-\(ProcessInfo.processInfo.processIdentifier)-\(name)",
                isDirectory: true
            )
        try? FileManager.default.removeItem(at: base)
        return base
    }

    private static func checkDefaultLocationIsUserPrivate() {
        let path = AppInstanceLock.defaultLockDirectory.path
        require(!path.hasPrefix("/tmp"), "lock directory must not live in shared /tmp")
        require(!path.hasPrefix("/private/tmp"), "lock directory must not live in shared /tmp")
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        require(path.hasPrefix(home), "lock directory must be private to the current user")
        require(
            path.contains("Library/Application Support/FanControl"),
            "lock directory moved unexpectedly"
        )
    }

    private static func checkAcquireAndContention() {
        let dir = temporaryDirectory("contention")
        let first = AppInstanceLock()
        require(first.acquire(in: dir), "first instance failed to acquire the lock")
        require(first.acquire(in: dir), "re-acquire by the holder must succeed")

        let second = AppInstanceLock()
        require(!second.acquire(in: dir), "second instance acquired a held lock")

        let attributes = try? FileManager.default.attributesOfItem(atPath: dir.path)
        let permissions = (attributes?[.posixPermissions] as? NSNumber)?.intValue
        require(permissions == 0o700, "lock directory permissions are not 0700")

        first.release()
        try? FileManager.default.removeItem(at: dir)
    }

    private static func checkReleaseAllowsReacquire() {
        let dir = temporaryDirectory("release")
        let first = AppInstanceLock()
        require(first.acquire(in: dir), "acquire before release failed")
        first.release()

        let second = AppInstanceLock()
        require(second.acquire(in: dir), "released lock could not be reacquired")
        second.release()
        try? FileManager.default.removeItem(at: dir)
    }

    private static func checkSymlinkFailsClosed() {
        let dir = temporaryDirectory("symlink")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let lockURL = dir.appendingPathComponent(AppInstanceLock.lockFileName)
        let target = dir.appendingPathComponent("some-other-file")
        FileManager.default.createFile(atPath: target.path, contents: Data())
        try? FileManager.default.createSymbolicLink(at: lockURL, withDestinationURL: target)

        let lock = AppInstanceLock()
        require(!lock.acquire(in: dir), "lock followed a pre-planted symlink")
        try? FileManager.default.removeItem(at: dir)
    }

    private static func checkUnavailableDirectoryFailsClosed() {
        let base = temporaryDirectory("unavailable")
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        let blockingFile = base.appendingPathComponent("blocker")
        FileManager.default.createFile(atPath: blockingFile.path, contents: Data())
        // A directory cannot be created underneath a regular file; the guard
        // must fail closed instead of reporting success.
        let impossible = blockingFile.appendingPathComponent("FanControl", isDirectory: true)

        let lock = AppInstanceLock()
        require(!lock.acquire(in: impossible), "open/create failure was treated as success")
        try? FileManager.default.removeItem(at: base)
    }

    private static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("App instance lock check failed: \(message)\n", stderr)
            exit(1)
        }
    }
}
