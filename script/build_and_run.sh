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

gui_pids() {
  local gui_user_id
  gui_user_id="$(id -u)"
  /bin/ps -ww -axo pid=,uid=,command= | while read -r process_id process_uid process_command; do
    # Match the complete GUI executable path and the invoking user. The helper
    # has its own path and --helper argument and must remain running.
    if [[ "$process_uid" == "$gui_user_id" && ( "$process_command" == "$APP_BINARY" || "$process_command" == "$APP_BINARY --demo" ) ]]; then
      printf '%s\n' "$process_id"
    fi
  done
}

kill_running_app() {
  local process_id attempt
  while read -r process_id; do
    [[ -n "$process_id" ]] || continue
    /bin/kill -TERM "$process_id" 2>/dev/null || true
  done < <(gui_pids)

  # Allow the GUI to finish its bounded fan hand-back. Do not force-kill a
  # stalled controller or target root helpers by their shared executable name.
  for attempt in {1..7}; do
    if [[ -z "$(gui_pids)" ]]; then
      return
    fi
    sleep 1
  done
  echo "error: the existing GUI did not finish shutting down; inspect its status before retrying" >&2
  exit 1
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
    if [[ -n "$(gui_pids)" ]]; then
      echo "Verified the packaged $APP_NAME GUI process is running."
      echo "Hardware control, helper installation and sleep recovery require separate acceptance checks."
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
