#!/usr/bin/env bash
# Bring the pinned `bevy_debugger_bevy` rev up to date with its upstream repo.
#
# WHY A PIN AT ALL: `Cargo.toml` names an exact rev rather than a branch, so a fresh clone and CI build
# the same debugger this machine did. A branch would always be current and never be reproducible, which
# is the wrong trade in a repo that pins goldens for a living.
#
# The cost of a pin is drift — nothing tells you upstream moved. This is that thing. It shows the
# commits between the pin and upstream `main`, bumps the manifest, and then BUILDS, because a bump that
# does not compile is worse than a stale pin: the stale one works.
#
# Usage:
#   scripts/sync_debugger.sh            # report only — what would change
#   scripts/sync_debugger.sh --apply    # bump the rev and verify it builds
set -euo pipefail

REPO="https://github.com/Ladvien/bevy_debugger_mcp"
MANIFEST="Cargo.toml"
APPLY=0
[ "${1:-}" = "--apply" ] && APPLY=1

cd "$(git rev-parse --show-toplevel)"

# The pin, read from the one place that decides it. Parsed rather than passed in, so this cannot drift
# from the manifest the build actually uses.
pinned=$(sed -n 's/.*bevy_debugger_bevy = .*rev = "\([0-9a-f]*\)".*/\1/p' "$MANIFEST" | head -1)
if [ -z "$pinned" ]; then
    echo "no pinned bevy_debugger_bevy rev found in $MANIFEST — has the dependency moved or gone?" >&2
    exit 1
fi

upstream=$(git ls-remote "$REPO" refs/heads/main | cut -f1)
if [ -z "$upstream" ]; then
    echo "could not reach $REPO to read main" >&2
    exit 1
fi

echo "pinned:   $pinned"
echo "upstream: ${upstream:0:7}  (main)"

if [ "${upstream:0:${#pinned}}" = "$pinned" ]; then
    echo "already current — nothing to do."
    exit 0
fi

# What actually changed. Fetched into this repo's object store under a throwaway ref, so no sibling
# checkout is required and nothing is left behind pointing at another project's history.
tmpref="refs/debugger-sync/main"
git fetch --quiet "$REPO" "main:$tmpref" --force
echo
echo "commits between the pin and upstream main:"
git log --oneline --no-decorate "$pinned..$tmpref" | sed 's/^/  /'
git update-ref -d "$tmpref"

if [ "$APPLY" -eq 0 ]; then
    echo
    echo "report only. Re-run with --apply to bump the pin and verify it builds."
    exit 0
fi

short=${upstream:0:7}
echo
echo "== bumping the pin to $short"
# In-place, matching only the rev on the dependency's own line.
perl -pi -e "s/(bevy_debugger_bevy = .*rev = \")[0-9a-f]+(\")/\${1}$short\${2}/" "$MANIFEST"
grep -n 'bevy_debugger_bevy = ' "$MANIFEST"

echo
echo "== verifying it builds (a bump that does not compile is worse than a stale pin)"
if cargo check --features debugger; then
    echo
    echo "done. The debugger is at $short and the game builds against it."
    echo "Commit \`$MANIFEST\` and \`Cargo.lock\` together."
else
    echo
    echo "the bumped rev does NOT build. The pin has been edited in your working tree; revert it with" >&2
    echo "  git checkout -- $MANIFEST" >&2
    echo "or fix the debugger upstream first. It is not committed, so nothing is broken yet." >&2
    exit 1
fi
