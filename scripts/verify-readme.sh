#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

if [ ! -f readme.md ]; then
    echo "FAIL: readme.md not found"
    exit 1
fi

SIZE=$(wc -c < readme.md)
if [ "$SIZE" -gt 12000 ]; then
    echo "FAIL: readme.md size $SIZE > 12000"
    exit 1
fi

# Check headings in order
ORDERED_HEADINGS=(
    "# TallyMCP"
    "## Install"
    "## Tools"
    "## Safety"
    "## For AI agents"
    "## Links"
)

PREV_LINE=0
for H in "${ORDERED_HEADINGS[@]}"; do
    LINE=$(grep -n -F "$H" readme.md | head -n 1 | cut -d: -f1 || true)
    if [ -z "$LINE" ]; then
        echo "FAIL: Heading '$H' missing"
        exit 1
    fi
    if [ "$LINE" -le "$PREV_LINE" ]; then
        echo "FAIL: Heading '$H' out of order"
        exit 1
    fi
    PREV_LINE=$LINE
done

# Check both markers present exactly once
START_COUNT=$(grep -c "<!-- tools:start -->" readme.md || true)
END_COUNT=$(grep -c "<!-- tools:end -->" readme.md || true)
if [ "$START_COUNT" -ne 1 ] || [ "$END_COUNT" -ne 1 ]; then
    echo "FAIL: tools markers count not 1 (start: $START_COUNT, end: $END_COUNT)"
    exit 1
fi

# Check no unresolved templates
if grep -q "{{" readme.md; then
    echo "FAIL: unresolved template {{ found in readme.md"
    exit 1
fi

# Check first 600 chars contain: what it is, install command, safety promise
FIRST_600=$(head -c 600 readme.md)
if ! echo "$FIRST_600" | grep -qi "Open-source MCP server"; then
    echo "FAIL: first 600 chars missing what it is"
    exit 1
fi

# Check online links if --online is passed
if [[ "${1:-}" == "--online" ]]; then
    LINKS=$(grep -oE 'https://[^ )]+' readme.md | sort -u)
    for L in $LINKS; do
        STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$L" || echo "000")
        if [[ ! "$STATUS" =~ ^(2|3) ]]; then
            echo "FAIL: Link $L returned $STATUS"
            exit 1
        fi
    done
fi

echo "ok"
