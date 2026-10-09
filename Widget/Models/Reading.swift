import Foundation

let widgetGroup = "JFC5CWT3V6.com.local.fan-control.readings"
let freshness: TimeInterval = 120

struct FanReading: Decodable, Identifiable {
    let id: Int
    let name: String
    let rpm: Double?
    let minRpm: Double?
    let maxRpm: Double?
    var fraction: Double? {
        guard let rpm, let minRpm, let maxRpm, minRpm >= 0, maxRpm > minRpm else { return nil }
        return min(1, max(0, (rpm - minRpm) / (maxRpm - minRpm)))
    }
}
struct TemperaturePoint: Decodable { let sampledAt: Double; let temperature: Double? }
struct Reading: Decodable {
    let schemaVersion: Int
    let writtenAt: Double
    let sampledAt: Double?
    let state: String
    let temperature: Double?
    let fanCount: Int?
    let fans: [FanReading]
    let history: [TemperaturePoint]

    func status(at date: Date) -> ReadingStatus {
        guard ["running", "stopped", "sleeping", "starting"].contains(state) else { return .unknown }
        if state == "stopped" { return .stopped }
        if state == "sleeping" { return .sleeping }
        if state == "starting" { return .unknown }
        guard temperature != nil || fans.contains(where: { $0.rpm != nil }) || fanCount == 0 else { return .unknown }
        guard let sampledAt, sampledAt <= date.timeIntervalSince1970 + 5 else { return .unknown }
        return date.timeIntervalSince1970 - sampledAt > freshness ? .stale : .recent
    }
    static func decode(_ data: Data, now: Date) -> Reading? {
        guard data.count <= 128 * 1024 else { return nil }
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let reading = try? decoder.decode(Reading.self, from: data), reading.schemaVersion == 1,
              reading.writtenAt.isFinite, reading.writtenAt > 0, reading.writtenAt <= now.timeIntervalSince1970 + 5,
              reading.fans.count <= 8, reading.history.count <= 120,
              reading.fanCount.map({ (0...255).contains($0) }) ?? true,
              reading.sampledAt.map({ $0.isFinite && $0 > 0 && $0 <= reading.writtenAt }) ?? true,
              reading.fanCount.map({ reading.fans.count <= $0 }) ?? true,
              validTemperature(reading.temperature), Set(reading.fans.map(\.id)).count == reading.fans.count,
              reading.fans.allSatisfy({ (0...255).contains($0.id) && $0.name.count <= 64 && validRPM($0.rpm) && validRPM($0.minRpm) && validRPM($0.maxRpm) }),
              reading.history.allSatisfy({ $0.sampledAt.isFinite && $0.sampledAt > 0 && $0.sampledAt <= reading.writtenAt && validTemperature($0.temperature) }),
              zip(reading.history, reading.history.dropFirst()).allSatisfy({ $0.sampledAt < $1.sampledAt })
        else { return nil }
        return reading
    }
}
func validTemperature(_ value: Double?) -> Bool { value.map { $0.isFinite && $0 >= 0 && $0 <= 150 } ?? true }
func validRPM(_ value: Double?) -> Bool { value.map { $0.isFinite && $0 >= 0 && $0 <= 100_000 } ?? true }
enum ReadingStatus { case recent, stale, stopped, sleeping, unknown }

func loadReading(now: Date) -> Reading? {
    guard let group = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: widgetGroup) else { return nil }
    let url = group.appendingPathComponent("fan-control-widget/snapshot-v1.json")
    guard let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size <= 128 * 1024,
          let data = try? Data(contentsOf: url) else { return nil }
    return Reading.decode(data, now: now)
}
