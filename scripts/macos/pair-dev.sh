#!/usr/bin/env bash
set -euo pipefail

LISTEN="${LISTEN:-0.0.0.0:9012}"
URL="${URL:-}"
BUILD="${BUILD:-0}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG_PATH="$REPO_ROOT/tmp/dev/system-wgo.yaml"
SYSTEM_EXE="$REPO_ROOT/target/debug/wgo-macos-system"

if [[ "$BUILD" == "1" || ! -x "$SYSTEM_EXE" ]]; then
  cargo build -p wgo-macos-daemon --bin wgo-macos-system
fi

ARGS=(pair --listen "$LISTEN" --config "$CONFIG_PATH")
if [[ -n "$URL" ]]; then
  ARGS+=(--url "$URL")
fi

sudo "$SYSTEM_EXE" "${ARGS[@]}"
