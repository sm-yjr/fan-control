import AppKit
import Observation
import QuartzCore
import SwiftUI

/// Observation change callbacks are `@Sendable`; AppKit state remains confined to
/// the main thread because callbacks immediately marshal back to the main queue.
final class StatusItemController: NSObject, NSPopoverDelegate, @unchecked Sendable {
    private let appState: AppState
    private let statusItem: NSStatusItem
    private let iconView = StatusFanImageView(frame: .zero)
    private let popover = NSPopover()
    private var reduceMotionObserver: NSObjectProtocol?

    init(appState: AppState) {
        self.appState = appState
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        super.init()

        configureStatusItem()
        configurePopover()
        observeFans()
        updatePresentation()

        reduceMotionObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.updatePresentation()
        }
    }

    deinit {
        if let reduceMotionObserver {
            NSWorkspace.shared.notificationCenter.removeObserver(reduceMotionObserver)
        }
        NSStatusBar.system.removeStatusItem(statusItem)
    }

    private func configureStatusItem() {
        statusItem.autosaveName = "FanControl"
        statusItem.isVisible = true

        guard let button = statusItem.button else { return }
        button.image = nil
        button.imagePosition = .imageOnly
        button.target = self
        button.action = #selector(togglePopover)
        button.sendAction(on: [.leftMouseUp])
        button.setAccessibilityLabel("Fan Control")

        let iconSide: CGFloat = 18
        layoutStatusIcon(in: button, iconSide: iconSide)
        iconView.autoresizingMask = [
            .minXMargin,
            .maxXMargin,
            .minYMargin,
            .maxYMargin,
        ]
        button.addSubview(iconView)

        // The status bar assigns the button's real bounds on the next run-loop
        // pass. Recenter once those bounds are available instead of preserving
        // a frame derived from the transient zero-sized button.
        DispatchQueue.main.async { [weak self, weak button] in
            guard let self, let button else { return }
            self.layoutStatusIcon(in: button, iconSide: iconSide)
        }
    }

    private func layoutStatusIcon(
        in button: NSStatusBarButton,
        iconSide: CGFloat
    ) {
        let frame = StatusItemIconLayout.frame(
            containerSize: button.bounds.size,
            iconSide: iconSide
        )
        iconView.applyCenteredFrame(NSRect(
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height
        ))
    }

    private func configurePopover() {
        let hostingController = NSHostingController(
            rootView: ContentView(appState: appState)
        )
        hostingController.sizingOptions = [.preferredContentSize]
        popover.contentViewController = hostingController
        popover.behavior = .transient
        popover.animates = true
        popover.delegate = self
    }

    private func observeFans() {
        withObservationTracking {
            _ = appState.sensorManager.fans
        } onChange: { [weak self] in
            DispatchQueue.main.async {
                guard let self else { return }
                self.updatePresentation()
                self.observeFans()
            }
        }
    }

    private func updatePresentation() {
        let presentation = FanStatusPresentation.resolve(
            samples: appState.sensorManager.fans.map {
                FanRotationSample(
                    currentRPM: $0.currentSpeed,
                    minimumRPM: $0.minSpeed,
                    maximumRPM: $0.maxSpeed
                )
            }
        )
        if let button = statusItem.button {
            button.toolTip = "Fan Control — \(presentation.accessibilityValue)"
            button.setAccessibilityValue(presentation.accessibilityValue)
        }

        let reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        iconView.configure(
            presentation: presentation,
            reduceMotion: reduceMotion
        )
    }

    @objc private func togglePopover() {
        guard let button = statusItem.button else { return }

        if popover.isShown {
            popover.performClose(nil)
        } else {
            NSApplication.shared.activate(ignoringOtherApps: true)
            popover.show(
                relativeTo: button.bounds,
                of: button,
                preferredEdge: .minY
            )
            iconView.setHighlighted(true)
            button.highlight(true)
        }
    }

    func popoverDidClose(_ notification: Notification) {
        iconView.setHighlighted(false)
        statusItem.button?.highlight(false)
    }
}

private final class StatusFanImageView: NSImageView {
    private var appliedLevel = FanRotationLevel.stopped
    private var appliedRotationPeriod: TimeInterval?
    private var isAnimating = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        imageScaling = .scaleProportionallyDown
        contentTintColor = .labelColor
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        nil
    }

    func applyCenteredFrame(_ frame: NSRect) {
        self.frame = frame

        // AppKit-backed layers use a corner anchor by default. Preserve the
        // frame while moving the anchor to the symbol's geometric center.
        guard let layer else { return }
        let stableFrame = layer.frame
        layer.anchorPoint = CGPoint(
            x: StatusItemIconLayout.rotationAnchorX,
            y: StatusItemIconLayout.rotationAnchorY
        )
        layer.frame = stableFrame
    }

    func configure(
        presentation: FanStatusPresentation,
        reduceMotion: Bool
    ) {
        if appliedLevel != presentation.level || image == nil {
            let weight: NSFont.Weight = presentation.level == .high
                ? .semibold
                : .regular
            let configuration = NSImage.SymbolConfiguration(
                pointSize: 13,
                weight: weight
            )
            let image = NSImage(
                systemSymbolName: "fan",
                accessibilityDescription: nil
            )?.withSymbolConfiguration(configuration)
            image?.isTemplate = true
            self.image = image
            alphaValue = presentation.level == .stopped ? 0.55 : 1
            appliedLevel = presentation.level
        }

        guard presentation.shouldAnimate(reduceMotion: reduceMotion),
              let rotationPeriod = presentation.rotationPeriod else {
            stopRotation()
            return
        }
        startRotation(period: rotationPeriod)
    }

    func setHighlighted(_ highlighted: Bool) {
        contentTintColor = highlighted ? .selectedMenuItemTextColor : .labelColor
    }

    private func startRotation(period: TimeInterval) {
        guard !isAnimating || appliedRotationPeriod != period else { return }

        let currentAngle = (
            layer?.presentation()?.value(forKeyPath: "transform.rotation.z")
                as? NSNumber
        )?.doubleValue ?? 0
        layer?.removeAnimation(forKey: "fanRotation")
        layer?.setValue(currentAngle, forKeyPath: "transform.rotation.z")

        let animation = CABasicAnimation(keyPath: "transform.rotation.z")
        animation.fromValue = currentAngle
        animation.toValue = currentAngle + 2 * Double.pi
        animation.duration = period
        animation.repeatCount = .infinity
        animation.timingFunction = CAMediaTimingFunction(name: .linear)
        animation.isRemovedOnCompletion = false
        layer?.add(animation, forKey: "fanRotation")

        appliedRotationPeriod = period
        isAnimating = true
    }

    private func stopRotation() {
        guard isAnimating || appliedRotationPeriod != nil else { return }
        layer?.removeAnimation(forKey: "fanRotation")
        layer?.setValue(0, forKeyPath: "transform.rotation.z")
        appliedRotationPeriod = nil
        isAnimating = false
    }
}
