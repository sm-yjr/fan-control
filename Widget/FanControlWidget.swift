import SwiftUI
import WidgetKit

struct ReadingEntry: TimelineEntry { let date: Date; let reading: Reading? }
struct ReadingProvider: TimelineProvider {
    func placeholder(in context: Context) -> ReadingEntry { ReadingEntry(date: Date(), reading: nil) }
    func getSnapshot(in context: Context, completion: @escaping (ReadingEntry) -> Void) {
        let now = Date(); completion(ReadingEntry(date: now, reading: context.isPreview ? nil : loadReading(now: now)))
    }
    func getTimeline(in context: Context, completion: @escaping (Timeline<ReadingEntry>) -> Void) {
        let now = Date(); let reading = loadReading(now: now)
        var entries = [ReadingEntry(date: now, reading: reading)]
        if let sampled = reading?.sampledAt {
            let expiry = Date(timeIntervalSince1970: sampled + freshness + 1)
            if expiry > now { entries.append(ReadingEntry(date: expiry, reading: reading)) }
        }
        // This is a request to WidgetKit, not a realtime guarantee. Expiry entries
        // are computed from existing data; they never invent future measurements.
        completion(Timeline(entries: entries, policy: .after(now.addingTimeInterval(900))))
    }
}
@main struct FanControlWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "FanControlReadings", provider: ReadingProvider()) { entry in
            ReadingView(date: entry.date, reading: entry.reading)
                .containerBackground(.thinMaterial, for: .widget)
                .widgetURL(URL(string: "fancontrol://dashboard"))
        }
        .configurationDisplayName("Fan Control")
        .description("Temperature and measured fan speeds. Open Fan Control to collect readings; refresh is managed by macOS.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}
