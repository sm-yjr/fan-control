#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${TMPDIR:-/tmp}/fan-control-app-launch-checks-$$"
trap 'rm -f "$OUTPUT"' EXIT

swiftc \
  "$ROOT_DIR/Sources/FanControl/AppLaunchMode.swift" \
  "$ROOT_DIR/script/AppLaunchModeChecks.swift" \
  -o "$OUTPUT"

"$OUTPUT"
