#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:?usage: build_widget.sh <app bundle>}"
APP_VERSION="${APP_VERSION:-0.5.0}"
BUILD_NUMBER="${BUILD_NUMBER:-1}"
SDK="$(xcrun --show-sdk-path)"
CACHE="$ROOT_DIR/.build/widget-module-cache"
EXT="$APP/Contents/PlugIns/FanControlWidget.appex"
mkdir -p "$EXT/Contents/MacOS" "$EXT/Contents/Resources" "$CACHE" "$APP/Contents/Frameworks"
common=(-sdk "$SDK" -target arm64-apple-macosx14.0 -module-cache-path "$CACHE" -O)
xcrun swiftc "${common[@]}" -parse-as-library -application-extension \
  "$ROOT_DIR/Widget/Models/Reading.swift" "$ROOT_DIR/Widget/Views/ReadingView.swift" \
  "$ROOT_DIR/Widget/Views/TemperatureTrend.swift" "$ROOT_DIR/Widget/FanControlWidget.swift" \
  -o "$EXT/Contents/MacOS/FanControlWidget"
xcrun swiftc "${common[@]}" -emit-library "$ROOT_DIR/Widget/WidgetBridge.swift" \
  -Xlinker -install_name -Xlinker @rpath/FanControlWidgetBridge.dylib \
  -o "$APP/Contents/Frameworks/FanControlWidgetBridge.dylib"
cat > "$EXT/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.local.fan-control.widget</string>
<key>CFBundleExecutable</key><string>FanControlWidget</string>
<key>CFBundleName</key><string>Fan Control</string>
<key>CFBundleDisplayName</key><string>Fan Control</string>
<key>CFBundlePackageType</key><string>XPC!</string>
<key>CFBundleShortVersionString</key><string>$APP_VERSION</string>
<key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleLocalizations</key><array><string>en</string><string>zh-Hans</string></array>
<key>NSExtension</key><dict><key>NSExtensionPointIdentifier</key><string>com.apple.widgetkit-extension</string></dict>
</dict></plist>
PLIST
for lang in en zh-Hans; do
  mkdir -p "$EXT/Contents/Resources/$lang.lproj"
  ditto "$ROOT_DIR/Widget/Resources/$lang.lproj/Localizable.strings" "$EXT/Contents/Resources/$lang.lproj/Localizable.strings"
done
plutil -lint "$EXT/Contents/Info.plist"
[[ "$(lipo -archs "$EXT/Contents/MacOS/FanControlWidget")" == arm64 ]]
