#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

OUTPUT_DIR="$ROOT_DIR/dist" \
  BUILD_CONFIGURATION=debug \
  "$ROOT_DIR/script/package_app.sh"

echo
echo "To run:"
echo "  open \"$ROOT_DIR/dist/FanControl.app\""
echo
echo "首次控制风扇前，应用会要求管理员授权安装 helper。"
