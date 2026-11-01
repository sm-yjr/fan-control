import Foundation
import CoreGraphics

@main struct DialGeometryTests {
    static func main() {
        var checked = 0
        for width in [54.0, 62, 88, 150] {
            for height in [44.0, 58, 90] {
                for textWidth in [18.0, 42, width - 4] {
                    for captionHeight in [10.0, 19] {
                        for knownRange in [false, true] {
                            let geometry = DialGeometry(size: CGSize(width: width, height: height), value: CGSize(width: textWidth, height: min(17, height * 0.3)), unit: CGSize(width: 18, height: 8), caption: CGSize(width: width - 4, height: captionHeight), symbol: CGSize(width: 9, height: 9), hasRange: knownRange)
                            assert(geometry.textIsClear, "Stroked arc intersects text")
                            assert(geometry.value.maxY <= geometry.unit.minY)
                            assert(geometry.unit.maxY + 1 <= geometry.caption.minY + 0.01)
                            assert(geometry.caption.maxY <= height + 0.01)
                            assert(geometry.value.minY >= 0)
                            assert(geometry.radius + geometry.stroke <= geometry.caption.minY - geometry.clearance + 0.01 || geometry.radius == 0)
                            if let symbol = geometry.symbol { assert(geometry.clearsTrack(symbol)) }
                            if !knownRange { assert(geometry.radius == 0) }
                            if geometry.radius > 0 {
                                // Independently sample the drawn full semicircle, including
                                // both round caps, against the measured text rectangles.
                                for degree in 0...180 {
                                    let angle = Double(degree) * .pi / 180
                                    let point = CGPoint(x: geometry.center.x + geometry.radius * cos(angle), y: geometry.center.y - geometry.radius * sin(angle))
                                    for rect in [geometry.value, geometry.unit, geometry.caption] {
                                        let dx = max(rect.minX - point.x, 0, point.x - rect.maxX)
                                        let dy = max(rect.minY - point.y, 0, point.y - rect.maxY)
                                        assert(hypot(dx, dy) + 0.01 >= geometry.stroke / 2 + geometry.clearance, "Sampled stroke touched text")
                                    }
                                }
                            }
                            checked += 1
                        }
                    }
                }
            }
        }
        print("Widget measured-text/arc clearance: \(checked) geometry cases passed")
    }
}
