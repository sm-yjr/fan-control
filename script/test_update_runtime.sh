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
