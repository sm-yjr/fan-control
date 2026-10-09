#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT="$ROOT_DIR/.build/widget-tests"
mkdir -p "$OUTPUT/module-cache"
xcrun swiftc -parse-as-library -target arm64-apple-macosx14.0 \
  -sdk "$(xcrun --show-sdk-path)" -module-cache-path "$OUTPUT/module-cache" \
  "$ROOT_DIR/Widget/Models/Reading.swift" "$ROOT_DIR/Widget/Tests/ReadingTests.swift" \
  -o "$OUTPUT/ReadingTests"
"$OUTPUT/ReadingTests"
