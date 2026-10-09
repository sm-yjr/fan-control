#!/usr/bin/env bash
set -euo pipefail

APP="${1:?usage: sign_app.sh <app> <identity> [timestamp]}"
IDENTITY="${2:?usage: sign_app.sh <app> <identity> [timestamp]}"
USE_TIMESTAMP="${3:-0}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPARKLE_FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
HELPER="$APP/Contents/Library/LaunchServices/com.local.fan-control.helper"

if [[ ! -d "$APP" ]]; then
  echo "error: app bundle not found at $APP" >&2
  exit 1
fi

if [[ ! -d "$SPARKLE_FRAMEWORK" ]]; then
  echo "error: Sparkle framework not found at $SPARKLE_FRAMEWORK" >&2
  exit 1
fi

if [[ ! -x "$HELPER" ]]; then
  echo "error: privileged helper not found at $HELPER" >&2
  exit 1
fi

sign_target() {
  local target="$1"
  shift
  local arguments=(--force)
  # Hardened Runtime enables Library Validation. Developer ID signs every
  # nested Sparkle component with the same Team ID, so release builds satisfy
  # that requirement. Ad-hoc signatures have no Team ID; enabling the runtime
  # there makes macOS reject Sparkle even though the bundle verifies cleanly.
  if [[ "$IDENTITY" != "-" ]]; then
    arguments+=(--options runtime)
  fi
  if [[ "$USE_TIMESTAMP" == "1" ]]; then
    arguments+=(--timestamp)
  fi
  codesign "${arguments[@]}" "$@" --sign "$IDENTITY" "$target" >/dev/null
}

# Sparkle's nested services must be signed from the inside out. Downloader.xpc
# needs its network entitlements preserved for update downloads.
sign_target "$SPARKLE_FRAMEWORK/Versions/Current/XPCServices/Installer.xpc"
sign_target "$SPARKLE_FRAMEWORK/Versions/Current/XPCServices/Downloader.xpc" \
  --preserve-metadata=entitlements
sign_target "$SPARKLE_FRAMEWORK/Versions/Current/Autoupdate"
sign_target "$SPARKLE_FRAMEWORK/Versions/Current/Updater.app"
sign_target "$SPARKLE_FRAMEWORK"
sign_target "$HELPER" --identifier "com.local.fan-control.helper"
# The privileged helper never receives App Group or sandbox entitlements.
sign_target "$APP/Contents/Frameworks/FanControlWidgetBridge.dylib"
sign_target "$APP/Contents/PlugIns/FanControlWidget.appex" --entitlements "$ROOT_DIR/Widget/Widget.entitlements"
sign_target "$APP" --entitlements "$ROOT_DIR/Widget/App.entitlements"

if [[ "$IDENTITY" != "-" ]]; then
  signature_metadata="$(codesign -dv --verbose=4 "$APP" 2>&1)"
  if [[ "$signature_metadata" != *"TeamIdentifier=JFC5CWT3V6"* ]]; then
    echo "error: Widget App Group requires the Fan Control signing team JFC5CWT3V6" >&2
    exit 1
  fi
fi

codesign --verify --strict --verbose=2 "$HELPER"
codesign --verify --deep --strict --verbose=2 "$APP"
