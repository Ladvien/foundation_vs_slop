#!/usr/bin/env bash
#
# **Look at a guide card on the editor's own window.**
#
# The guide overlay is a UI panel, and Bevy draws a UI tree to one camera — so `bevy_debugger/screenshot`
# and its offscreen mirror cannot see it, and never will. `bevy_devshot` captures the whole frame
# including UI, which makes it the only way to answer "does the card actually look right".
#
# That capture reads the window surface, so the window has to be in front: an occluded one composites
# to black (measured on this editor at 1280x704 — a black frame is exactly 55,654 bytes, a real one
# over 1 MB).
#
# **You bring it to the front; this script will not.** The documented trick — `osascript ... set
# frontmost of (first process whose unix id is $PID)` — is accepted and does nothing on macOS 26.5,
# and a freshly launched window does not come up in front either. See `wait_for_front` for the
# measurements. So the script asks for your screen and waits, rather than taking it, which is the
# right shape for the one thing in this repo that needs it.
#
# Usage:
#
#   scripts/guide_devshot.sh <script.json> [verb ...]
#
#     shot:<name>   capture, save to debug_screenshots/guide_<name>.png
#     skip          advance the guide past the current step (records it as an attempt that failed)
#     beat:<name>   skip and capture inside the 2 s confirmation, to see the "OK <step>" card
#
#   Default verbs: shot:step1 skip shot:step2
#
# Example — the first card, a mid-script card, and the end:
#
#   scripts/guide_devshot.sh crates/emerge-mapper/guides/author_a_tile.json \
#     shot:first beat:confirm skip skip skip skip shot:person skip skip shot:done
#
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

SCRIPT_JSON="${1:-crates/emerge-mapper/guides/author_a_tile.json}"
shift || true
VERBS=("$@")
if [ ${#VERBS[@]} -eq 0 ]; then
    VERBS=(shot:step1 skip shot:step2)
fi

# A port of its own, so this never fights the game or an editor you already have open.
PORT="${BEVY_BRP_PORT:-15788}"
BRP="http://127.0.0.1:$PORT"
OUT="$REPO/debug_screenshots"
SENTINEL="$REPO/screenshot.request"
FRAME="$REPO/screenshot.png"
# Below this a PNG is an occluded window, not a frame. See the header.
MIN_BYTES=200000
# Seconds to wait for you to click the editor window before giving up on a shot.
FOCUS_WAIT="${FOCUS_WAIT:-120}"

if [ ! -f "$SCRIPT_JSON" ]; then
    echo "no such guide script: $SCRIPT_JSON" >&2
    exit 1
fi
mkdir -p "$OUT"
rm -f "$SENTINEL" "$FRAME"

post() {
    curl -s -X POST "$BRP" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"bevy_debugger/guide\",\"params\":$1}"
}

# **Build before the window goes up**, so a cold compile does not sit on your screen.
echo "building..."
cargo build -q -p emerge-mapper

# **Run the binary, never `cargo run`.** Cargo forks the editor as a child, so `$!` is cargo's pid and
# every process query about "the editor" answers about a command-line tool with no window instead.
# That sent a whole capture run down the wrong path: the frames were black, the pid was wrong, and it
# read as the screenshot path being broken. Killing the right process on exit needs this too.
TARGET="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
BIN="$TARGET/debug/emerge-mapper"
if [ ! -x "$BIN" ]; then
    echo "no editor binary at $BIN" >&2
    exit 1
fi

echo "starting the editor on port $PORT"
BEVY_BRP_PORT="$PORT" "$BIN" . untitled_map &
EDITOR=$!
cleanup() {
    kill "$EDITOR" 2>/dev/null || true
    wait "$EDITOR" 2>/dev/null || true
    rm -f "$SENTINEL" "$FRAME"
}
trap cleanup EXIT

# Wait for BRP rather than sleeping a guessed amount: asset loading dominates and varies.
for _ in $(seq 1 60); do
    if curl -s --max-time 1 -X POST "$BRP" -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"bevy_debugger/guide","params":{"read":true}}' \
        | grep -q '"result"'; then
        break
    fi
    sleep 1
done

echo
echo "  >>> CLICK THE emerge-mapper WINDOW. <<<"
echo "  The capture reads the window surface, so an occluded one comes back black, and macOS 26"
echo "  will not let a script raise it. Each shot waits up to ${FOCUS_WAIT}s for it to be frontmost."
echo
echo "posting $SCRIPT_JSON"
post "$(cat "$SCRIPT_JSON")" | head -c 200
echo

# **You raise the window, not this script — and that is the finding, not a shortcut.**
#
# The documented trick was `osascript ... set frontmost of (first process whose unix id is $PID)`.
# On macOS 26.5 it is accepted and does nothing: System Events resolves the process by unix id and
# answers `emerge-mapper`, the set command returns success, and a second later the frontmost process
# is still whatever it was. A freshly launched window does not come up in front either. Every frame
# taken that way is 55,654 bytes of black.
#
# So the script waits for the window to be yours instead of taking it. Which is the better shape
# anyway: the one thing in this repo that needs your screen now asks for it.
wait_for_front() {
    local front
    for _ in $(seq 1 "$FOCUS_WAIT"); do
        front=$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null || true)
        if [ "$front" = "emerge-mapper" ]; then
            return 0
        fi
        sleep 1
    done
    echo "  still not frontmost after ${FOCUS_WAIT}s (front: ${front:-unknown}) — the frame will be black" >&2
    return 1
}

capture() {
    local name="$1"
    local dest="$OUT/guide_$name.png"
    rm -f "$FRAME"
    touch "$SENTINEL"
    for _ in $(seq 1 40); do
        [ -f "$FRAME" ] && break
        sleep 0.25
    done
    if [ ! -f "$FRAME" ]; then
        echo "  $name: no frame written" >&2
        return 1
    fi
    # `stat -f%z` is the BSD spelling; this script is macOS-only anyway, because the raise is.
    local size
    size=$(stat -f%z "$FRAME")
    mv "$FRAME" "$dest"
    if [ "$size" -lt "$MIN_BYTES" ]; then
        echo "  $name: ${size} bytes — that is an occluded window, not a frame. $dest" >&2
        return 1
    fi
    echo "  $name: ${size} bytes -> $dest"
}

for verb in "${VERBS[@]}"; do
    case "$verb" in
        skip)
            post '{"skip": true}' >/dev/null
            echo "skipped a step"
            ;;
        beat:*)
            # The confirmation of a finished step holds for BEAT_SECONDS. Wait for focus FIRST, since
            # that is the slow part, then skip and shoot inside the two-second window.
            wait_for_front || true
            post '{"skip": true}' >/dev/null
            capture "${verb#beat:}" || true
            ;;
        shot:*)
            wait_for_front || true
            capture "${verb#shot:}" || true
            ;;
        *)
            echo "unknown verb: $verb" >&2
            exit 1
            ;;
    esac
done

echo
post '{"read": true}'
echo
