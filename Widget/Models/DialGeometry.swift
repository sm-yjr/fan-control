import Foundation
import CoreGraphics

/// Layout uses measured text, including grouping separators and localized captions.
/// Coordinates are top-down, matching SwiftUI Canvas. The full track is protected,
/// so a low reading cannot accidentally look correct only because its fill is short.
struct DialGeometry {
    static let minimumStroke: CGFloat = 4
    static let strokeToDiameter: CGFloat = 0.055
    static func stroke(forRadius radius: CGFloat) -> CGFloat { max(minimumStroke, radius * 2 * strokeToDiameter) }
    static func radius(fittingWidth width: CGFloat) -> CGFloat {
        max(0, min((width - minimumStroke) / 2, width / (2 + 2 * strokeToDiameter)))
    }
    let size: CGSize
    let value: CGRect
    let unit: CGRect
    let caption: CGRect
    let symbol: CGRect?
    let center: CGPoint
    let radius: CGFloat
    let stroke: CGFloat
    let clearance: CGFloat = 2

    init(size: CGSize, value: CGSize, unit: CGSize, caption: CGSize, symbol: CGSize, hasRange: Bool) {
        self.size = size
        func centered(_ measured: CGSize, y: CGFloat) -> CGRect {
            CGRect(x: (size.width - measured.width) / 2, y: y, width: measured.width, height: measured.height)
        }
        self.caption = centered(caption, y: size.height - caption.height)
        self.unit = centered(unit, y: self.caption.minY - 1 - unit.height)
        self.value = centered(value, y: self.unit.minY - value.height)
        let arcHeight = max(0, self.caption.minY - clearance)
        let arcWidth = max(0, size.width - 1)
        let maximumRadius = max(0, min(Self.radius(fittingWidth: arcWidth), arcHeight / (1 + 2 * Self.strokeToDiameter), arcHeight - Self.minimumStroke))
        let maximumStroke = Self.stroke(forRadius: maximumRadius)
        let inner = maximumRadius - maximumStroke / 2 - clearance
        let halfValue = value.width / 2
        let centerY = maximumRadius + maximumStroke / 2
        let fitsInside = inner > halfValue && self.value.minY >= centerY - sqrt(inner * inner - halfValue * halfValue)
        // Wide/long readings keep their font and move below the arc opening.
        // Reserve the complete stroke, its round caps and a visible clearance.
        self.radius = hasRange ? (fitsInside ? maximumRadius : min(maximumRadius, max(0, self.value.minY - clearance - maximumStroke))) : 0
        self.stroke = Self.stroke(forRadius: self.radius)
        self.center = CGPoint(x: size.width / 2, y: self.radius + self.stroke / 2)
        let symbolBottom = min(self.value.minY - 2, hasRange ? self.center.y : self.value.minY - 2)
        let symbolRect = centered(symbol, y: symbolBottom - symbol.height)
        let symbolInner = self.radius - self.stroke / 2 - clearance
        let symbolFits = symbolRect.minY >= 0 && (!hasRange || (symbolInner > symbol.width / 2 && symbolRect.minY >= self.center.y - sqrt(symbolInner * symbolInner - symbol.width * symbol.width / 4)))
        self.symbol = symbolFits ? symbolRect : nil
    }

    func clearsTrack(_ rect: CGRect) -> Bool {
        guard radius > 0 else { return true }
        if rect.minY >= center.y + stroke / 2 + clearance - 0.01 { return true }
        let inner = radius - stroke / 2 - clearance
        let farX = max(abs(rect.minX - center.x), abs(rect.maxX - center.x))
        return inner > farX && rect.minY >= center.y - sqrt(inner * inner - farX * farX) - 0.01
    }

    var textIsClear: Bool { [value, unit, caption].allSatisfy(clearsTrack) }
}
