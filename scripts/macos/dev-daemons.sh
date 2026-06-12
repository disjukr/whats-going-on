#!/usr/bin/env bash
set -euo pipefail

LISTEN="${LISTEN:-0.0.0.0:9012}"
SKIP_BUILD="${SKIP_BUILD:-0}"
RUST_LOG="${RUST_LOG:-info}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TMP_DIR="$REPO_ROOT/tmp/dev"
LOG_DIR="$REPO_ROOT/tmp/log"
CONFIG_PATH="$TMP_DIR/system-wgo.yaml"
SYSTEM_PID_FILE="$TMP_DIR/macos-system.pid"
USER_PID_FILE="$TMP_DIR/macos-user.pid"
SYSTEM_OUT_LOG="$LOG_DIR/macos-system.out.log"
SYSTEM_ERR_LOG="$LOG_DIR/macos-system.err.log"
USER_OUT_LOG="$LOG_DIR/macos-user.out.log"
USER_ERR_LOG="$LOG_DIR/macos-user.err.log"
SYSTEM_EXE="$REPO_ROOT/target/debug/wgo-macos-system"
USER_EXE="$REPO_ROOT/target/debug/wgo-macos-user"

mkdir -p "$TMP_DIR" "$LOG_DIR"

stop_pid_file() {
  local label="$1"
  local pid_file="$2"
  if [[ ! -f "$pid_file" ]]; then
    return
  fi
  local pid
  pid="$(head -n 1 "$pid_file" || true)"
  rm -f "$pid_file"
  if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
    return
  fi
  echo "Stopping previous $label pid=$pid"
  if [[ "$label" == "system daemon" ]]; then
    sudo kill "$pid" 2>/dev/null || true
  else
    kill "$pid" 2>/dev/null || true
  fi
}

show_log_tail() {
  local label="$1"
  local path="$2"
  echo
  echo "[$label] last 80 lines: $path"
  if [[ -f "$path" ]]; then
    tail -n 80 "$path" || true
  else
    echo "(missing)"
  fi
}

cleanup() {
  stop_pid_file "system daemon" "$SYSTEM_PID_FILE"
  stop_pid_file "user daemon" "$USER_PID_FILE"
}

trap cleanup EXIT INT TERM

"$SCRIPT_DIR/kill-daemons.sh"

if [[ "$SKIP_BUILD" != "1" ]]; then
  cargo build -p wgo-macos-daemon --bins
fi

if [[ ! -x "$SYSTEM_EXE" ]]; then
  echo "Missing $SYSTEM_EXE. Run without SKIP_BUILD=1 first." >&2
  exit 1
fi
if [[ ! -x "$USER_EXE" ]]; then
  echo "Missing $USER_EXE. Run without SKIP_BUILD=1 first." >&2
  exit 1
fi

sudo -v

echo "Starting wgo macOS system daemon on $LISTEN"
sudo env RUST_LOG="$RUST_LOG" "$SYSTEM_EXE" run --listen "$LISTEN" --config "$CONFIG_PATH" \
  >"$SYSTEM_OUT_LOG" 2>"$SYSTEM_ERR_LOG" &
SYSTEM_PID="$!"
echo "$SYSTEM_PID" >"$SYSTEM_PID_FILE"

echo "Starting wgo macOS user daemon"
RUST_LOG="$RUST_LOG" "$USER_EXE" run >"$USER_OUT_LOG" 2>"$USER_ERR_LOG" &
USER_PID="$!"
echo "$USER_PID" >"$USER_PID_FILE"

echo
echo "System daemon pid=$SYSTEM_PID"
echo "User daemon pid=$USER_PID"
echo "Dev config: $CONFIG_PATH"
echo "Logs: $LOG_DIR"
echo "WebTransport endpoints: https://$LISTEN/rpc and https://$LISTEN/moqt"
echo "Press Ctrl+C to stop both daemons."

while true; do
  if ! kill -0 "$SYSTEM_PID" 2>/dev/null; then
    show_log_tail "system stdout" "$SYSTEM_OUT_LOG"
    show_log_tail "system stderr" "$SYSTEM_ERR_LOG"
    exit 1
  fi
  if ! kill -0 "$USER_PID" 2>/dev/null; then
    show_log_tail "user stdout" "$USER_OUT_LOG"
    show_log_tail "user stderr" "$USER_ERR_LOG"
    exit 1
  fi
  sleep 1
done
