#!/usr/bin/env bash
set -euo pipefail

APP="${1:?usage: sign_app.sh <app> <identity> [timestamp]}"
IDENTITY="${2:?usage: sign_app.sh <app> <identity> [timestamp]}"
USE_TIMESTAMP="${3:-0}"
SPARKLE_FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"

if [[ ! -d "$APP" ]]; then
  echo "error: app bundle not found at $APP" >&2
  exit 1
fi

if [[ ! -d "$SPARKLE_FRAMEWORK" ]]; then
  echo "error: Sparkle framework not found at $SPARKLE_FRAMEWORK" >&2
  exit 1
fi

sign_target() {
  local target="$1"
  shift
  local arguments=(--force --options runtime)
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
sign_target "$APP"

codesign --verify --deep --strict --verbose=2 "$APP"
