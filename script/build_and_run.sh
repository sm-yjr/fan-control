#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_NAME="FanControl"
BUNDLE_ID="com.local.fan-control"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_BUNDLE="$ROOT_DIR/dist/$APP_NAME.app"
APP_BINARY="$APP_BUNDLE/Contents/MacOS/$APP_NAME"

usage() {
  echo "usage: $0 [run|build|--debug|--logs|--telemetry|--verify]" >&2
}

kill_running_app() {
  pkill -x "$APP_NAME" >/dev/null 2>&1 || true
}

build_bundle() {
  OUTPUT_DIR="$ROOT_DIR/dist" \
    BUILD_CONFIGURATION=debug \
    "$ROOT_DIR/script/package_app.sh"
}

open_app() {
  /usr/bin/open -n "$APP_BUNDLE"
}

case "$MODE" in
  run)
    kill_running_app
    build_bundle
    open_app
    ;;
  build)
    build_bundle
    ;;
  --debug|debug)
    kill_running_app
    build_bundle
    lldb -- "$APP_BINARY"
    ;;
  --logs|logs)
    kill_running_app
    build_bundle
    open_app
    /usr/bin/log stream --info --style compact --predicate "process == \"$APP_NAME\""
    ;;
  --telemetry|telemetry)
    kill_running_app
    build_bundle
    open_app
    /usr/bin/log stream --info --style compact --predicate "subsystem == \"$BUNDLE_ID\""
    ;;
  --verify|verify)
    kill_running_app
    build_bundle
    open_app
    sleep 2
    if pgrep -x "$APP_NAME" >/dev/null; then
      echo "Verified $APP_NAME is running."
    else
      echo "error: $APP_NAME did not start." >&2
      exit 1
    fi
    ;;
  *)
    usage
    exit 2
    ;;
esac
