#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${TMPDIR:-/tmp}/fan-control-status-item-checks-$$"
trap 'rm -f "$OUTPUT"' EXIT

swiftc \
  "$ROOT_DIR/Sources/FanControl/Views/FanStatusPresentation.swift" \
  "$ROOT_DIR/script/StatusItemPresentationChecks.swift" \
  -o "$OUTPUT"

"$OUTPUT"
