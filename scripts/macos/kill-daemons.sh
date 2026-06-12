#!/usr/bin/env bash
set -euo pipefail

LISTEN="${LISTEN:-0.0.0.0:9012}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TMP_DIR="$REPO_ROOT/tmp/dev"
CONFIG_PATH="$TMP_DIR/system-wgo.yaml"
SYSTEM_EXE="$REPO_ROOT/target/debug/wgo-macos-system"
USER_EXE="$REPO_ROOT/target/debug/wgo-macos-user"
PORT="${LISTEN##*:}"

kill_pid() {
  local label="$1"
  local pid="$2"
  local use_sudo="$3"

  if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
    return
  fi

  echo "Stopping $label pid=$pid"
  if [[ "$use_sudo" == "1" ]]; then
    sudo kill "$pid" 2>/dev/null || true
  else
    kill "$pid" 2>/dev/null || true
  fi

  for _ in {1..20}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      return
    fi
    sleep 0.1
  done

  echo "Force stopping $label pid=$pid"
  if [[ "$use_sudo" == "1" ]]; then
    sudo kill -9 "$pid" 2>/dev/null || true
  else
    kill -9 "$pid" 2>/dev/null || true
  fi
}

stop_pid_file() {
  local label="$1"
  local pid_file="$2"
  local use_sudo="$3"

  if [[ ! -f "$pid_file" ]]; then
    return
  fi
  local pid
  pid="$(head -n 1 "$pid_file" || true)"
  rm -f "$pid_file"
  kill_pid "$label" "$pid" "$use_sudo"
}

stop_pgrep_matches() {
  local label="$1"
  local pattern="$2"
  local use_sudo="$3"

  if ! command -v pgrep >/dev/null 2>&1; then
    return
  fi
  while IFS= read -r pid; do
    kill_pid "$label" "$pid" "$use_sudo"
  done < <(pgrep -f "$pattern" 2>/dev/null || true)
}

stop_system_port_matches() {
  if ! command -v lsof >/dev/null 2>&1; then
    return
  fi
  while IFS= read -r pid; do
    kill_pid "system daemon" "$pid" 1
  done < <(
    sudo lsof -nP -iUDP:"$PORT" 2>/dev/null |
      awk 'NR > 1 && $1 ~ /^wgo-macos/ { print $2 }' |
      sort -u
  )
}

stop_pid_file "system daemon" "$TMP_DIR/macos-system.pid" 1
stop_pid_file "user daemon" "$TMP_DIR/macos-user.pid" 0

stop_pgrep_matches "system daemon" "$SYSTEM_EXE.*run.*--config $CONFIG_PATH" 1
stop_pgrep_matches "user daemon" "$USER_EXE.*run" 0
stop_system_port_matches
