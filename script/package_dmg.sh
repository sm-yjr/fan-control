#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?usage: package_dmg.sh <app> <output.dmg> [volume-name]}"
OUTPUT_DMG="${2:?usage: package_dmg.sh <app> <output.dmg> [volume-name]}"
VOLUME_NAME="${3:-Fan Control}"

if [[ ! -d "$APP_PATH" || "${APP_PATH##*.}" != "app" ]]; then
  echo "error: app bundle not found at $APP_PATH" >&2
  exit 1
fi

if [[ "${OUTPUT_DMG##*.}" != "dmg" ]]; then
  echo "error: output path must end in .dmg" >&2
  exit 2
fi

codesign --verify --deep --strict --verbose=2 "$APP_PATH"

APP_PARENT="$(cd "$(dirname "$APP_PATH")" && pwd)"
APP_NAME="$(basename "$APP_PATH")"
SOURCE_APP="$APP_PARENT/$APP_NAME"
OUTPUT_PARENT="$(dirname "$OUTPUT_DMG")"
mkdir -p "$OUTPUT_PARENT"
OUTPUT_PARENT="$(cd "$OUTPUT_PARENT" && pwd)"
OUTPUT_DMG="$OUTPUT_PARENT/$(basename "$OUTPUT_DMG")"

STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fan-control-dmg.XXXXXX")"
cleanup() {
  rm -rf "$STAGING_DIR"
}
trap cleanup EXIT

ditto "$SOURCE_APP" "$STAGING_DIR/$APP_NAME"

# A developer machine may have attached quarantine metadata to a locally
# supplied app. Do not bake that metadata into the disk image. Gatekeeper will
# still attach quarantine when the DMG is downloaded, so releases must also be
# notarized and stapled by the release workflow.
xattr -dr com.apple.quarantine "$STAGING_DIR/$APP_NAME" 2>/dev/null || true

ln -s /Applications "$STAGING_DIR/Applications"

rm -f "$OUTPUT_DMG"
hdiutil create \
  -quiet \
  -ov \
  -format UDZO \
  -fs HFS+ \
  -volname "$VOLUME_NAME" \
  -srcfolder "$STAGING_DIR" \
  "$OUTPUT_DMG"

hdiutil verify "$OUTPUT_DMG"
echo "Built $OUTPUT_DMG"
