import Foundation
@main struct ReadingTests {
    static func main() {
        let now = Date(timeIntervalSince1970: 10_000)
        var object: [String: Any] = ["schema_version": 1, "written_at": 10_000, "sampled_at": 9_990, "state": "running", "temperature": 54.0, "fan_count": 1,
            "fans": [["id": 0, "name": "Fan", "rpm": 1500.0, "min_rpm": 1000.0, "max_rpm": 6000.0]],
            "history": [["sampled_at": 9_960, "temperature": 53.0], ["sampled_at": 9_990, "temperature": 54.0]]]
        func decode(_ value: [String: Any]) -> Reading? { Reading.decode(try! JSONSerialization.data(withJSONObject: value), now: now) }
        let good = decode(object)!
        assert(good.status(at: now) == .recent)
        assert(good.status(at: Date(timeIntervalSince1970: 10_111)) == .stale)
        assert(good.fans[0].fraction == 0.1)
        for (state, status) in [("stopped", ReadingStatus.stopped), ("sleeping", .sleeping), ("starting", .unknown), ("unsupported", .unknown)] {
            object["state"] = state; assert(decode(object)!.status(at: now) == status)
        }
        object["state"] = "running"
        object["fan_count"] = 0; object["fans"] = []; object["temperature"] = NSNull(); object["sampled_at"] = NSNull()
        let empty = decode(object)!; assert(empty.temperature == nil && empty.fanCount == 0 && empty.status(at: now) == .unknown)
        object["fan_count"] = NSNull()
        assert(decode(object)!.fanCount == nil)
        object["fans"] = [["id": 0, "name": "Fan", "rpm": NSNull(), "min_rpm": NSNull(), "max_rpm": NSNull()]]
        assert(decode(object)!.fans[0].rpm == nil && decode(object)!.fans[0].fraction == nil)
        object["fans"] = [["id": 0, "name": "Fan", "rpm": 0, "min_rpm": 1000, "max_rpm": 6000]]
        assert(decode(object)!.fans[0].rpm == 0)
        object["schema_version"] = 0; assert(decode(object) == nil)
        object["schema_version"] = 1; object["written_at"] = 10_010; assert(decode(object) == nil)
        object["written_at"] = 10_000; object["history"] = [["sampled_at": 10_001, "temperature": 53]]; assert(decode(object) == nil)
        object["history"] = [["sampled_at": 9_990, "temperature": 53], ["sampled_at": 9_960, "temperature": 52]]; assert(decode(object) == nil)
        assert(Reading.decode(Data("broken".utf8), now: now) == nil)
        assert(Reading.decode(Data(repeating: 32, count: 128 * 1024 + 1), now: now) == nil)
        print("Widget decoding, expiry, lifecycle, null readings and corrupt/old snapshot tests passed")
    }
}
