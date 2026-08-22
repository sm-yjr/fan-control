import Foundation

/// Single-instance guard backed by an exclusive `flock` on a file that is
/// private to the current user.
///
/// The previous implementation kept the lock at a predictable path in
/// world-writable `/tmp`: any local account could pre-create the file and
/// hold the lock to block every launch, and an `open()` failure was treated
/// as success, silently disabling the guard. The lock now lives in the
/// user's Application Support directory, refuses symlinks, and fails closed:
/// any setup problem is reported as "another instance owns the session"
/// instead of letting a second controller race the first one.
final class AppInstanceLock {
    static let shared = AppInstanceLock()
    static let lockFileName = "instance.lock"

    private var fd: Int32 = -1

    static var defaultLockDirectory: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/FanControl", isDirectory: true)
    }

    func acquire() -> Bool {
        acquire(in: Self.defaultLockDirectory)
    }

    func acquire(in directory: URL) -> Bool {
        if fd >= 0 { return true }

        let fileManager = FileManager.default
        do {
            try fileManager.createDirectory(
                at: directory,
                withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700]
            )
            // createDirectory does not apply permissions to an existing
            // directory; enforce them either way.
            try fileManager.setAttributes(
                [.posixPermissions: 0o700],
                ofItemAtPath: directory.path
            )
        } catch {
            debugLog("[FanControl] instanceLock directory failed error=\(error.localizedDescription)")
            return false
        }

        let lockPath = directory.appendingPathComponent(Self.lockFileName).path
        // O_NOFOLLOW: a pre-planted symlink must fail closed instead of
        // locking an attacker-chosen file.
        let opened = open(lockPath, O_CREAT | O_RDWR | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard opened >= 0 else {
            debugLog("[FanControl] instanceLock open failed errno=\(errno)")
            return false
        }

        if flock(opened, LOCK_EX | LOCK_NB) == 0 {
            fd = opened
            debugLog("[FanControl] instanceLock acquired path=\(lockPath)")
            return true
        }

        close(opened)
        debugLog("[FanControl] instanceLock busy path=\(lockPath)")
        return false
    }

    func release() {
        guard fd >= 0 else { return }
        close(fd)
        fd = -1
    }

    deinit {
        release()
    }
}
