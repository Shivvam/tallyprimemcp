#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

EXE=""
if [ -x "./target/release/tally-mcp" ]; then
    EXE="./target/release/tally-mcp"
elif [ -x "./target/debug/tally-mcp" ]; then
    EXE="./target/debug/tally-mcp"
else
    cargo build --quiet
    EXE="./target/debug/tally-mcp"
fi

DEST_DIR="$DIR/../tallymcp-web/src/content/generated"
DEST="$DEST_DIR/tools.json"
mkdir -p "$DEST_DIR"

if [[ "${1:-}" == "--check" ]]; then
    if [ ! -f "$DEST" ]; then
        echo "FAIL: $DEST does not exist"
        exit 1
    fi
    DIFF=$(diff -u <("$EXE" tools --json | jq -S .) <(jq -S . "$DEST") || true)
    if [ -n "$DIFF" ]; then
        echo "FAIL: tools.json has drifted"
        echo "$DIFF"
        exit 1
    fi
else
    "$EXE" tools --json > "$DEST"
fi
