#!/usr/bin/env bash
# Yank every version of the seven gore leaves that folded into `bevy_carnage` 0.5.0 (2026-09-04).
#
# crates.io cannot delete a crate; yanking is what "closed" means there. A yanked version still
# resolves for a lockfile that already pins it, so nothing shipped breaks — but no new dependency
# can pick one up, which is the point: `bevy_carnage` is the one crate a game depends on.
#
# Needs a token with the **yank** scope. The publish token this workspace usually carries answers
# `403 Forbidden` to every line below, which is how this script came to exist.
#
#   cargo login <token-with-yank-scope>
#   scripts/yank_gore_leaves.sh
#
# Idempotent: already-yanked versions are skipped, so a rerun after a partial failure finishes the job.
set -euo pipefail

CRATES=(bloodstain bevy_wetmap bevy_viscera bevy_cross_section bevy_flaymap bevy_laceration bevy_fracture_modes)

live_versions() {
    curl -sf -A "foundation_vs_slop yank_gore_leaves" "https://crates.io/api/v1/crates/$1/versions" |
        python3 -c 'import json,sys; print(" ".join(v["num"] for v in json.load(sys.stdin)["versions"] if not v["yanked"]))'
}

failed=0
for crate in "${CRATES[@]}"; do
    versions=$(live_versions "$crate")
    if [ -z "$versions" ]; then
        echo "$crate: nothing live"
        continue
    fi
    for v in $versions; do
        if cargo yank "$crate@$v" >/dev/null 2>&1; then
            echo "$crate@$v: yanked"
        else
            echo "$crate@$v: FAILED (token lacks the yank scope?)" >&2
            failed=1
        fi
    done
done

if [ "$failed" -ne 0 ]; then
    echo "some versions are still live — log in with a token carrying the yank scope and rerun" >&2
    exit 1
fi
echo "every version of the seven is yanked"
