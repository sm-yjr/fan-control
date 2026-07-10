#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_NAME="FanControl"
BUNDLE_ID="com.local.fan-control"
MIN_SYSTEM_VERSION="14.0"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
APP_CONTENTS="$APP_BUNDLE/Contents"
APP_MACOS="$APP_CONTENTS/MacOS"
APP_RESOURCES="$APP_CONTENTS/Resources"
APP_BINARY="$APP_MACOS/$APP_NAME"
INFO_PLIST="$APP_CONTENTS/Info.plist"

usage() {
  echo "usage: $0 [run|build|--debug|--logs|--telemetry|--verify]" >&2
}

kill_running_app() {
  pkill -x "$APP_NAME" >/dev/null 2>&1 || true
}

ensure_swiftui_macros_available() {
  local swift_path
  swift_path="$(xcrun --find swift 2>/dev/null || command -v swift)"
  local toolchain_root
  toolchain_root="${swift_path%/usr/bin/swift}"
  local selected_developer_dir
  selected_developer_dir="$(xcode-select -p 2>/dev/null || true)"

  local candidate
  local candidates=(
    "$toolchain_root/usr/lib/swift/host/plugins/libSwiftUIMacros.dylib"
    "$selected_developer_dir/usr/lib/swift/host/plugins/libSwiftUIMacros.dylib"
    "$selected_developer_dir/Platforms/MacOSX.platform/Developer/usr/lib/swift/host/plugins/libSwiftUIMacros.dylib"
  )

  for candidate in "${candidates[@]}"; do
    if [[ -f "$candidate" ]]; then
      return
    fi
  done

  if [[ ! -f "$candidate" ]]; then
    echo "error: SwiftUI macro plugin was not found for the selected Swift toolchain." >&2
    echo "selected developer directory: ${selected_developer_dir:-unknown}" >&2
    echo "swift executable: $swift_path" >&2
    echo "checked plugin paths:" >&2
    printf '  %s\n' "${candidates[@]}" >&2
    echo "" >&2
    echo "Install or select Apple developer tools that include SwiftUI macros." >&2
    echo "For full Xcode, use:" >&2
    echo "  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer" >&2
    exit 1
  fi
}

build_bundle() {
  cd "$ROOT_DIR"

  ensure_swiftui_macros_available
  swift build
  local build_bin_path
  build_bin_path="$(swift build --show-bin-path)"
  local build_binary="$build_bin_path/$APP_NAME"

  if [[ ! -x "$build_binary" ]]; then
    echo "error: expected executable was not found at $build_binary" >&2
    exit 1
  fi

  rm -rf "$APP_BUNDLE"
  mkdir -p "$APP_MACOS" "$APP_RESOURCES"
  cp "$build_binary" "$APP_BINARY"
  chmod +x "$APP_BINARY"

  cat >"$INFO_PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleName</key>
  <string>Fan Control</string>
  <key>CFBundleDisplayName</key>
  <string>Fan Control</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_SYSTEM_VERSION</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST

  echo "Built $APP_BUNDLE"
}

open_app() {
  /usr/bin/open -n "$APP_BUNDLE"
}

case "$MODE" in
  run)
    kill_running_app
    build_bundle
    open_app
    ;;
  build)
    build_bundle
    ;;
  --debug|debug)
    kill_running_app
    build_bundle
    lldb -- "$APP_BINARY"
    ;;
  --logs|logs)
    kill_running_app
    build_bundle
    open_app
    /usr/bin/log stream --info --style compact --predicate "process == \"$APP_NAME\""
    ;;
  --telemetry|telemetry)
    kill_running_app
    build_bundle
    open_app
    /usr/bin/log stream --info --style compact --predicate "subsystem == \"$BUNDLE_ID\""
    ;;
  --verify|verify)
    kill_running_app
    build_bundle
    open_app
    sleep 2
    if pgrep -x "$APP_NAME" >/dev/null; then
      echo "Verified $APP_NAME is running."
    else
      echo "error: $APP_NAME did not start." >&2
      exit 1
    fi
    ;;
  *)
    usage
    exit 2
    ;;
esac
