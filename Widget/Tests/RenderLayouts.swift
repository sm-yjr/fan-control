// Offline layout review only. This does not register or validate a system widget.
import SwiftUI
import WidgetKit
import AppKit

@main struct RenderLayouts {
    @MainActor static func main() throws {
        NSApplication.shared.setActivationPolicy(.prohibited)
        let now = Date()
        func fixture(count: Int, state: String = "running", missing: Bool = false) -> Reading {
            let time = now.timeIntervalSince1970
            let fans = (0..<count).map { i -> [String: Any] in ["id": i, "name": "Fan \(i+1)", "rpm": missing ? NSNull() : 1430 + i * 60, "min_rpm": missing ? NSNull() : 1000, "max_rpm": missing ? NSNull() : 6000] }
            let points = (0..<100).map { i in ["sampled_at": time - Double(100-i)*30, "temperature": 48 + sin(Double(i)/14)*5] }
            let object: [String: Any] = ["schema_version": 1, "written_at": time, "sampled_at": time-10, "state": state, "temperature": missing ? NSNull() : 54, "fan_count": count, "fans": fans, "history": missing ? [] : points]
            return Reading.decode(try! JSONSerialization.data(withJSONObject: object), now: now)!
        }
        let one = fixture(count: 1), two = fixture(count: 2), missing = fixture(count: 2, missing: true), stopped = fixture(count: 0, state: "stopped")
        func tile(_ reading: Reading, family: WidgetFamily, scheme: ColorScheme) -> some View {
            ReadingView(familyOverride: family, date: now, reading: reading)
                .padding(16).frame(width: family == .systemSmall ? 170 : 360, height: 170)
                .background(.thinMaterial).clipShape(RoundedRectangle(cornerRadius: 22))
                .environment(\.colorScheme, scheme)
        }
        let sheet = VStack(alignment: .leading, spacing: 18) {
            Text("Fan Control · Offline layout review").font(.title2.weight(.semibold))
            Text("Fixture data · system Gallery and desktop validation pending").font(.caption).foregroundStyle(.secondary)
            HStack(spacing: 18) { tile(one, family: .systemSmall, scheme: .light); tile(two, family: .systemSmall, scheme: .light); tile(two, family: .systemMedium, scheme: .light) }
            HStack(spacing: 18) { tile(one, family: .systemSmall, scheme: .dark); tile(two, family: .systemSmall, scheme: .dark); tile(two, family: .systemMedium, scheme: .dark) }
            HStack(spacing: 18) { tile(stopped, family: .systemSmall, scheme: .light); tile(missing, family: .systemSmall, scheme: .light); tile(one, family: .systemMedium, scheme: .light) }
        }.padding(28).background(Color(nsColor: .windowBackgroundColor))
        let renderer = ImageRenderer(content: sheet); renderer.scale = 2
        guard let image = renderer.nsImage, let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff), let png = rep.representation(using: .png, properties: [:]) else { fatalError("No rendered image") }
        try png.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
    }
}
