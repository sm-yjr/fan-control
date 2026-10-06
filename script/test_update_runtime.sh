#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/.build/update-runtime-check"
BUILD_LOG="$OUTPUT_DIR/package.log"
mkdir -p "$OUTPUT_DIR"

if ! APP_VERSION=0.0.3 \
  BUILD_NUMBER=3 \
  BUILD_CONFIGURATION=debug \
  OUTPUT_DIR="$OUTPUT_DIR" \
  "$ROOT_DIR/script/package_app.sh" >"$BUILD_LOG" 2>&1
then
  cat "$BUILD_LOG" >&2
  exit 1
fi

APP="$OUTPUT_DIR/FanControl.app"
HELPER="$APP/Contents/Library/LaunchServices/com.local.fan-control.helper"
STAGED_HELPER="$OUTPUT_DIR/com.local.fan-control.helper"

/usr/bin/install -m 755 "$HELPER" "$STAGED_HELPER"
codesign --verify --strict "$STAGED_HELPER"
if otool -L "$STAGED_HELPER" | grep -Eq 'Sparkle\.framework|@rpath/|@executable_path/|@loader_path/'; then
  echo "Standalone helper depends on an app-bundled library." >&2
  exit 1
fi
PACKAGE_JSON="$OUTPUT_DIR/package-info.json"
PACKAGE_INFO="$OUTPUT_DIR/package-info.plist"
"$APP/Contents/MacOS/FanControl" --package-info > "$PACKAGE_JSON"
/usr/bin/plutil -convert xml1 -o "$PACKAGE_INFO" "$PACKAGE_JSON"
/usr/bin/plutil -lint "$PACKAGE_INFO"
[[ "$(/usr/bin/plutil -extract app.version raw "$PACKAGE_INFO")" == "0.0.3" ]]
[[ "$(/usr/bin/plutil -extract app.build raw "$PACKAGE_INFO")" == "3" ]]
[[ "$(/usr/bin/plutil -extract helper_protocol raw "$PACKAGE_INFO")" == "8" ]]
[[ "$(/usr/bin/plutil -extract architecture raw "$PACKAGE_INFO")" == "aarch64" ]]

set +e
RUNTIME_OUTPUT="$(
  "$APP/Contents/MacOS/FanControl" \
    --check-updater-runtime 2>&1
)"
RUNTIME_STATUS=$?
set -e

if [[ $RUNTIME_STATUS -ne 0 ]]; then
  printf '%s\n' "$RUNTIME_OUTPUT" >&2
  echo "Sparkle runtime check failed for the packaged app." >&2
  exit 1
fi

grep -q "Sparkle updater runtime is available" <<<"$RUNTIME_OUTPUT"
codesign --verify --deep --strict "$APP"
echo "Sparkle runtime checks passed"
