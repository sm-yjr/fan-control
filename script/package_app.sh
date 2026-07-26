#!/usr/bin/env bash
set -euo pipefail

APP_NAME="FanControl"
BUNDLE_ID="com.local.fan-control"
MIN_SYSTEM_VERSION="14.0"

SPARKLE_VERSION="2.9.2"
SPARKLE_ARCHIVE_CHECKSUM="b83e37436774556ed055e0244b297ef2c790e0737393bf65bf495fcbba6eed65"
SPARKLE_ARCHIVE_URL="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-for-Swift-Package-Manager.zip"
SPARKLE_FEED_URL="https://github.com/sm-yjr/fan-control/releases/latest/download/appcast.xml"
SPARKLE_PUBLIC_KEY="/hvZor9jnQzV8BJYBLdNoyEAps0epZ1MvtMu8wq5ikg="

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_CONFIGURATION="${BUILD_CONFIGURATION:-debug}"
APP_VERSION="${APP_VERSION:-1.0.0}"
BUILD_NUMBER="${BUILD_NUMBER:-1}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/dist}"
ARCHITECTURES="${ARCHITECTURES:-}"
CODE_SIGN_IDENTITY="${CODE_SIGN_IDENTITY:-}"
AD_HOC_CODE_SIGN="${AD_HOC_CODE_SIGN:-1}"

SPARKLE_CACHE_DIR="$ROOT_DIR/.build/sparkle/$SPARKLE_VERSION"
SPARKLE_ARCHIVE="$SPARKLE_CACHE_DIR/Sparkle.zip"
SPARKLE_FRAMEWORK="$SPARKLE_CACHE_DIR/Sparkle.xcframework/macos-arm64_x86_64/Sparkle.framework"

APP_BUNDLE="$OUTPUT_DIR/$APP_NAME.app"
APP_CONTENTS="$APP_BUNDLE/Contents"
APP_MACOS="$APP_CONTENTS/MacOS"
APP_FRAMEWORKS="$APP_CONTENTS/Frameworks"
APP_RESOURCES="$APP_CONTENTS/Resources"
APP_BINARY="$APP_MACOS/$APP_NAME"
INFO_PLIST="$APP_CONTENTS/Info.plist"

if [[ ! "$BUILD_CONFIGURATION" =~ ^(debug|release)$ ]]; then
  echo "error: BUILD_CONFIGURATION must be debug or release" >&2
  exit 2
fi

if [[ ! "$APP_VERSION" =~ ^[0-9]+(\.[0-9]+){1,2}([.-][A-Za-z0-9]+)*$ ]]; then
  echo "error: APP_VERSION must be a dotted release version such as 1.2.0" >&2
  exit 2
fi

if [[ ! "$BUILD_NUMBER" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
  echo "error: BUILD_NUMBER must contain only digits and dots" >&2
  exit 2
fi

build_arguments=(-c "$BUILD_CONFIGURATION")
if [[ -n "$ARCHITECTURES" ]]; then
  for architecture in $ARCHITECTURES; do
    build_arguments+=(--arch "$architecture")
  done
fi

fetch_sparkle() {
  if [[ -d "$SPARKLE_FRAMEWORK" && -x "$SPARKLE_CACHE_DIR/bin/generate_appcast" ]]; then
    return
  fi

  echo "Fetching Sparkle $SPARKLE_VERSION..."
  rm -rf "$SPARKLE_CACHE_DIR"
  mkdir -p "$SPARKLE_CACHE_DIR"
  curl --fail --location --silent --show-error \
    "$SPARKLE_ARCHIVE_URL" \
    --output "$SPARKLE_ARCHIVE"

  local actual_checksum
  actual_checksum="$(shasum -a 256 "$SPARKLE_ARCHIVE" | awk '{print $1}')"
  if [[ "$actual_checksum" != "$SPARKLE_ARCHIVE_CHECKSUM" ]]; then
    echo "error: Sparkle archive checksum mismatch" >&2
    rm -f "$SPARKLE_ARCHIVE"
    exit 1
  fi

  ditto -x -k "$SPARKLE_ARCHIVE" "$SPARKLE_CACHE_DIR"
  rm -f "$SPARKLE_ARCHIVE"
}

cd "$ROOT_DIR"
fetch_sparkle

echo "Building $APP_NAME ($BUILD_CONFIGURATION)..."
swift build "${build_arguments[@]}"
BUILD_BIN_PATH="$(swift build "${build_arguments[@]}" --show-bin-path)"
BUILD_BINARY="$BUILD_BIN_PATH/$APP_NAME"

if [[ ! -x "$BUILD_BINARY" ]]; then
  echo "error: expected executable was not found at $BUILD_BINARY" >&2
  exit 1
fi

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_MACOS" "$APP_FRAMEWORKS" "$APP_RESOURCES"
ditto "$BUILD_BINARY" "$APP_BINARY"
ditto "$SPARKLE_FRAMEWORK" "$APP_FRAMEWORKS/Sparkle.framework"
ditto "$ROOT_DIR/LICENSE" "$APP_RESOURCES/FanControl-LICENSE.txt"
ditto "$SPARKLE_CACHE_DIR/LICENSE" "$APP_RESOURCES/Sparkle-LICENSE.txt"
chmod +x "$APP_BINARY"

cat >"$INFO_PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
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
  <string>$APP_VERSION</string>
  <key>CFBundleVersion</key>
  <string>$BUILD_NUMBER</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_SYSTEM_VERSION</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>SUEnableAutomaticChecks</key>
  <true/>
  <key>SUFeedURL</key>
  <string>$SPARKLE_FEED_URL</string>
  <key>SUPublicEDKey</key>
  <string>$SPARKLE_PUBLIC_KEY</string>
</dict>
</plist>
PLIST

if [[ -n "$CODE_SIGN_IDENTITY" ]]; then
  echo "Signing with $CODE_SIGN_IDENTITY..."
  codesign --force --deep --options runtime --timestamp \
    --sign "$CODE_SIGN_IDENTITY" "$APP_BUNDLE"
elif [[ "$AD_HOC_CODE_SIGN" == "1" ]]; then
  echo "Applying an ad-hoc signature for local development..."
  codesign --force --deep --sign - "$APP_BUNDLE"
fi

plutil -lint "$INFO_PLIST" >/dev/null
codesign --verify --deep --strict "$APP_BUNDLE"

echo "Built $APP_BUNDLE"
