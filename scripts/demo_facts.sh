#!/usr/bin/env bash
#
# Cross-check the demo roster against the page that serves it.
#
#   ./scripts/demo_facts.sh
#
# **Both drift modes are invisible in a green build**, which is the whole reason
# this exists:
#
# - A module built but **not allow-listed** in `web/play.html` is tens of
#   megabytes of wasm nothing on the site can reach.
# - A module allow-listed but **not built** is a link to a 404.
#
# Neither shows up in a compile, a test, or the build script's own output. So the
# roster is stated exactly twice — `DEMOS` in `scripts/build_web.sh` and `DEMOS`
# in `web/play.html` — and this holds the two against each other, plus the
# per-demo key block each entry promises.
#
# Called by `build_web.sh` before it cleans anything, and by CI as its own step.

set -euo pipefail
cd "$(dirname "$0")/.."

BUILD=scripts/build_web.sh
PLAY=web/play.html

# The roster: the `pkg:example` lines between `DEMOS=(` and its closing paren.
roster=$(awk '/^DEMOS=\(/{f=1;next} f&&/^\)/{exit} f{gsub(/[ \t]/,""); if ($0 !~ /^#/ && $0 != "") print}' "$BUILD" |
    sed 's/.*://')

# The allow-list: the keys of the `const DEMOS = { … }` object literal.
allow=$(awk '/const DEMOS = \{/{f=1;next} f&&/^  \};?$/{exit} f{print}' "$PLAY" |
    sed -n 's/^ *\([a-z0-9_]*\):.*/\1/p')

fail=0

n_roster=$(printf '%s\n' "$roster" | grep -c . || true)
n_allow=$(printf '%s\n' "$allow" | grep -c . || true)

if [ "$n_roster" -eq 0 ]; then
    echo "::error::could not read the DEMOS array out of $BUILD" >&2
    exit 1
fi
if [ "$n_allow" -eq 0 ]; then
    echo "::error::could not read the DEMOS allow-list out of $PLAY" >&2
    exit 1
fi

if [ "$n_roster" -ne "$n_allow" ]; then
    echo "::error::$BUILD builds $n_roster demos, $PLAY allow-lists $n_allow" >&2
    fail=1
fi

# Named both ways round, because "which side is missing it" is the whole content
# of the failure.
for demo in $roster; do
    if ! printf '%s\n' "$allow" | grep -qx "$demo"; then
        echo "::error::$demo is built by $BUILD but not allow-listed in $PLAY — that module is unreachable" >&2
        fail=1
    fi
    if ! grep -q "id=\"notes-$demo\"" "$PLAY"; then
        echo "::error::$demo has no id=\"notes-$demo\" key block in $PLAY — the page would show a demo with no legend" >&2
        fail=1
    fi
done

for demo in $allow; do
    if ! printf '%s\n' "$roster" | grep -qx "$demo"; then
        echo "::error::$demo is allow-listed in $PLAY but not built by $BUILD — that link is a 404" >&2
        fail=1
    fi
done

# Every roster entry must name a real example target, or the build fails
# twenty-five minutes in on a typo.
for entry in $(awk '/^DEMOS=\(/{f=1;next} f&&/^\)/{exit} f{gsub(/[ \t]/,""); if ($0 !~ /^#/ && $0 != "") print}' "$BUILD"); do
    pkg="${entry%%:*}"
    demo="${entry##*:}"
    if [ ! -f "crates/$pkg/examples/$demo.rs" ]; then
        echo "::error::crates/$pkg/examples/$demo.rs does not exist" >&2
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    exit 1
fi

echo "demo roster: $n_roster demos, built and allow-listed and legended, all agreeing"
