#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FOUNDATION_OUTPUT="${TMPDIR:-/tmp}/fan-control-fan-ui-foundation-checks-$$"
COMPONENT_OUTPUT="${TMPDIR:-/tmp}/fan-control-fan-ui-component-checks-$$"
trap 'rm -f "$FOUNDATION_OUTPUT" "$COMPONENT_OUTPUT"' EXIT

swiftc \
  "$ROOT_DIR/Sources/FanControl/FanUI/FanUIFoundation.swift" \
  "$ROOT_DIR/script/FanUIFoundationChecks.swift" \
  -o "$FOUNDATION_OUTPUT"

"$FOUNDATION_OUTPUT"

swiftc \
  "$ROOT_DIR/Sources/FanControl/FanUI/FanUIFoundation.swift" \
  "$ROOT_DIR/Sources/FanControl/FanUI/FanUIComponents.swift" \
  "$ROOT_DIR/script/FanUIComponentChecks.swift" \
  -o "$COMPONENT_OUTPUT"

"$COMPONENT_OUTPUT"
