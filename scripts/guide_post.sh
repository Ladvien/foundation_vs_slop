#!/usr/bin/env bash
#
# **Post a guide script, once the app is actually listening.**
#
# `cargo run -p emerge-mapper & curl ...` is the obvious thing to type and it does not work: the
# editor takes ten seconds to compile-check, open a window and bake its palette thumbnails, and the
# curl runs immediately. It fails with an empty reply, which looks like nothing happening at all --
# three separate attempts in one session died this way, and each time the editor came up with no
# script and no sign that one had been asked for.
#
# So this waits for BRP to answer before posting, and says which it did.
#
# Usage:
#
#   scripts/guide_post.sh                         # the tile script, default port
#   scripts/guide_post.sh <script.json>
#   BEVY_BRP_PORT=15788 scripts/guide_post.sh     # a second editor
#
# The app has to be started separately -- this does not launch anything, so it works against an
# editor you already have open, which is the usual case.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

SCRIPT_JSON="${1:-crates/emerge-mapper/guides/author_a_tile.json}"
PORT="${BEVY_BRP_PORT:-15702}"
BRP="http://127.0.0.1:$PORT"
WAIT="${WAIT:-90}"

if [ ! -f "$SCRIPT_JSON" ]; then
    echo "no such guide script: $SCRIPT_JSON" >&2
    exit 1
fi

ask() {
    curl -s --max-time 2 -X POST "$BRP" -H 'Content-Type: application/json' -d "$1"
}

printf 'waiting for BRP on %s' "$PORT"
for _ in $(seq 1 "$WAIT"); do
    if ask '{"jsonrpc":"2.0","id":1,"method":"bevy_debugger/guide","params":{"read":true}}' \
        | grep -q '"result"'; then
        echo " -- up"
        ask "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"bevy_debugger/guide\",\"params\":$(cat "$SCRIPT_JSON")}"
        echo
        echo "watch it with:"
        echo "  curl -N -s -X POST $BRP -H 'Content-Type: application/json' \\"
        echo "    -d '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"bevy_debugger/guide+watch\"}'"
        exit 0
    fi
    printf '.'
    sleep 1
done

echo
echo "nothing answered on $PORT after ${WAIT}s." >&2
echo "Is the editor running, and was it built with the debugger feature (it is on by default)?" >&2
exit 1
