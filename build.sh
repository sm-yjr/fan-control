#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

OUTPUT_DIR="$ROOT_DIR/.build/debug" \
  BUILD_CONFIGURATION=debug \
  "$ROOT_DIR/script/package_app.sh"

echo
echo "To run:"
echo "  open $ROOT_DIR/.build/debug/FanControl.app"
echo
echo "On first launch, click Enable to install the privileged helper for fan control."
