#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT="${1:-$ROOT_DIR/.build/widget-layout-review}"
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
APP="$OUTPUT/RenderWidget.app"
CACHE="$ROOT_DIR/.build/widget-tests/module-cache"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$CACHE"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.local.fan-control.layout-review</string>
<key>CFBundleExecutable</key><string>RenderWidget</string>
<key>CFBundleName</key><string>Widget Layout Review</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleLocalizations</key><array><string>en</string><string>zh-Hans</string></array>
<key>LSUIElement</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
for lang in en zh-Hans; do
  ditto "$ROOT_DIR/Widget/Resources/$lang.lproj" "$APP/Contents/Resources/$lang.lproj"
done
xcrun swiftc -D LAYOUT_AUDIT -parse-as-library -target arm64-apple-macosx14.0 \
  -sdk "$(xcrun --show-sdk-path)" -module-cache-path "$CACHE" \
  "$ROOT_DIR/Widget/Models/Reading.swift" "$ROOT_DIR/Widget/Models/DialGeometry.swift" \
  "$ROOT_DIR/Widget/Views/ReadingView.swift" "$ROOT_DIR/Widget/Views/TemperatureTrend.swift" \
  "$ROOT_DIR/Widget/Tests/RenderLayouts.swift" -o "$APP/Contents/MacOS/RenderWidget"
# This separate diagnostic renders fixture data offscreen. It never registers
# a system Widget or invokes the production app, hardware, helper or snapshot writer.
"$APP/Contents/MacOS/RenderWidget" "$OUTPUT/scenes"
