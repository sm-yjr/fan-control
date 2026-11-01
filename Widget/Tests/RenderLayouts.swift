// Offline geometry review of production views. No system registration, shared
// snapshot writes, hardware, helper, or production application launch.
import SwiftUI
import WidgetKit
import AppKit

@main struct RenderLayouts {
    @MainActor static func main() throws {
        NSApplication.shared.setActivationPolicy(.prohibited)
        let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        let now = Date()
        let cases: [(String, Double?, Bool, TimeInterval)] = [
            ("reported", 1332, true, 65), ("four-digits", 9999, true, 10),
            ("hardware-maximum", 65535, true, 10), ("snapshot-maximum", 100000, true, 10),
            ("zero", 0, true, 10), ("missing", nil, false, 10),
            ("unknown-range", 1332, false, 10), ("stale", 1332, true, 180)
        ]
        func fixture(count: Int, rpm: Double?, range: Bool, age: TimeInterval) -> Reading {
            let time = now.timeIntervalSince1970
            let fans = (0..<count).map { i -> [String: Any] in
                ["id": i, "name": "Fan \(i + 1)", "rpm": rpm.map { min(100000, $0 == 1332 ? $0 - Double(i * 5) : $0) } as Any? ?? NSNull(), "min_rpm": range ? 1000 : NSNull(), "max_rpm": range ? max(6000, rpm ?? 0) : NSNull()]
            }
            let object: [String: Any] = ["schema_version": 1, "written_at": time, "sampled_at": time - age, "state": "running", "temperature": 50, "fan_count": count, "fans": fans, "history": []]
            return Reading.decode(try! JSONSerialization.data(withJSONObject: object), now: now)!
        }
        func save<V: View>(_ view: V, _ name: String) throws {
            let renderer = ImageRenderer(content: view); renderer.scale = 2
            guard let image = renderer.nsImage, let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff), let png = rep.representation(using: .png, properties: [:]) else { fatalError("No rendered image") }
            try png.write(to: output.appendingPathComponent(name + ".png"))
        }
        var reports: [[String: Any]] = []
        for language in ["en", "zh-Hans"] {
            for scheme in [ColorScheme.light, .dark] {
                for family in [WidgetFamily.systemSmall, .systemMedium] {
                    for count in [1, 2] {
                        for (scene, rpm, range, age) in cases {
                            let reading = fixture(count: count, rpm: rpm, range: range, age: age)
                            var view = ReadingView(familyOverride: family, date: now, reading: reading)
                            var measurements: [[String: Any]] = []
                            #if LAYOUT_AUDIT
                            view.layoutAudit = { geometry in
                                assert(geometry.textIsClear, "Arc/text collision in \(scene) / \(family) / \(count) / \(language)")
                                for rect in [geometry.value, geometry.unit, geometry.caption] {
                                    assert(rect.minX >= -0.01 && rect.maxX <= geometry.size.width + 0.01 && rect.minY >= -0.01 && rect.maxY <= geometry.size.height + 0.01, "Text escaped its dial in \(scene) / \(family) / \(count) / \(language)")
                                }
                                assert(geometry.radius == 0 || geometry.radius + geometry.stroke + geometry.clearance <= geometry.caption.minY + 0.01, "Arc escaped into caption/footer")
                                measurements.append(["width": geometry.size.width, "height": geometry.size.height, "radius": geometry.radius, "value_width": geometry.value.width, "value_height": geometry.value.height, "value_y": geometry.value.minY, "caption_height": geometry.caption.height, "clearance": geometry.clearance])
                            }
                            #endif
                            let tile = view.padding(16)
                                .frame(width: family == .systemSmall ? 164 : 344, height: 164)
                                .background(scheme == .light ? Color.white : Color(nsColor: .windowBackgroundColor))
                                .clipShape(RoundedRectangle(cornerRadius: 22))
                                .environment(\.locale, Locale(identifier: language))
                                .environment(\.colorScheme, scheme)
                            let name = "\(scene)-\(family == .systemSmall ? "small" : "medium")-\(count)-\(language)-\(scheme == .light ? "light" : "dark")"
                            try save(tile, name)
                            #if LAYOUT_AUDIT
                            assert(!measurements.isEmpty, "Measured-layout audit did not run")
                            #endif
                            reports.append(["scene": scene, "family": family == .systemSmall ? "small" : "medium", "fan_count": count, "language": language, "scheme": scheme == .light ? "light" : "dark", "image": name + ".png", "measurements": measurements])
                        }
                    }
                }
            }
        }
        try JSONSerialization.data(withJSONObject: ["offline_only": true, "fixture_data": true, "hardware_access": false, "system_widget_validation": false, "scenes": reports], options: [.prettyPrinted, .sortedKeys]).write(to: output.appendingPathComponent("report.json"))
        print("Widget offline review: \(reports.count) scenes rendered")
    }
}
