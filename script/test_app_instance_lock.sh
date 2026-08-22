#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${TMPDIR:-/tmp}/fan-control-instance-lock-checks-$$"
trap 'rm -f "$OUTPUT"' EXIT

swiftc \
  "$ROOT_DIR/Sources/FanControl/DebugLog.swift" \
  "$ROOT_DIR/Sources/FanControl/AppInstanceLock.swift" \
  "$ROOT_DIR/script/AppInstanceLockChecks.swift" \
  -o "$OUTPUT"

"$OUTPUT"
