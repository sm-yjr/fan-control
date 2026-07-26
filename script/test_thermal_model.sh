#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${TMPDIR:-/tmp}/fan-control-thermal-model-checks-$$"
trap 'rm -f "$OUTPUT"' EXIT

swiftc \
  "$ROOT_DIR/Sources/FanControl/Controller/ThermalDemandEstimator.swift" \
  "$ROOT_DIR/Sources/FanControl/Controller/FanCurve.swift" \
  "$ROOT_DIR/script/ThermalModelChecks.swift" \
  -o "$OUTPUT"

"$OUTPUT"
