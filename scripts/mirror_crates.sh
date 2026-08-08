#!/usr/bin/env bash
# Push each workspace crate to its own GitHub repo, history intact.
#
# WHAT THIS IS: history MIRRORS, not forks. The monorepo is the source of truth; nothing is ever
# edited on the far side and nothing is ever pulled back. `git subtree split` re-derives the same
# synthetic commits from the same monorepo history on every run (it caches in `.git/subtree-cache`),
# so a re-sync is a fast-forward and re-running this is a no-op.
#
# WHY NO --force: when the push is NOT a fast-forward, the monorepo's history was rewritten under the
# mirror. That is a human decision — rebase, amend, filter-branch — and a sync script that quietly
# force-pushed would turn "I amended a commit message" into "the mirror's history silently diverged".
# The push failing IS the correct outcome. To resolve deliberately:
#
#     git push --force-with-lease=main:<remote-sha> git@github.com:Ladvien/<name>.git <sha>:main
#
# WHY SIBLING PATH DEPS ARE LEFT ALONE: four of these crates depend on siblings by workspace path, so
# their mirrors do not build standalone, and their READMEs say so. The alternative — rewriting path
# deps to git deps in a generated fixup commit — would put a DIFFERENT Cargo.toml on the mirror than
# the one the monorepo builds and tests, which is a second differently-configured copy of the crate.
# It would also have to be re-applied after every split, making every sync a force-push. One manifest,
# one build, one path; the cost is a line in the README, and that is the honest cost.
#
# MIRROR CI: none, on purpose. Most of these cannot build outside the workspace, so a `cargo test`
# workflow would be red by construction, and a red badge nobody can fix is worse than no badge. When a
# mirror becomes public and standalone-buildable, put its workflow at
# `crates/<name>/.github/workflows/ci.yml` — GitHub ignores that path in the monorepo (only the root
# `.github/workflows` runs), and the subtree split lifts it to the mirror's root where it does run.
#
# Usage:  scripts/mirror_crates.sh [crate ...]     (default: every crate in CRATES)
set -euo pipefail

ORG=Ladvien

# Every crate that gets a mirror. Order is cosmetic; each split is independent.
CRATES=(
    bevy_orca
    map_elites
    bevy_devshot
    bevy_stigmergy
    bevy_light_grid
    bevy_speech_bubbles
    emerge-core
    emerge-anim
    emerge-bevy
    emerge-mapper
)

cd "$(git rev-parse --show-toplevel)"

if [ -n "$(git status --porcelain)" ]; then
    echo "working tree is dirty. A mirror reflects committed history; commit or stash first." >&2
    exit 1
fi

command -v gh >/dev/null || { echo "gh CLI not found — needed to create the repos." >&2; exit 1; }

# Explicit arguments override the list, so a single crate can be re-synced on its own.
if [ "$#" -gt 0 ]; then
    CRATES=("$@")
fi

for name in "${CRATES[@]}"; do
    prefix="crates/$name"
    if [ ! -d "$prefix" ]; then
        echo "$prefix does not exist — fix CRATES rather than skipping, or a mirror goes stale in silence." >&2
        exit 1
    fi
    # A mirror's root IS this directory, so anything a reader needs must live in it — and the reader is
    # no longer only a human. `CLAUDE.md` is required for the same reason `README.md` is: an agent that
    # clones one of these repos standalone has no monorepo to consult, so the crate's own
    # non-negotiables (the engine-free ratchets, "the caller owns the schedule", no-transitions, the
    # single spawner) have to travel with it or they are simply absent at the moment they are needed.
    for required in README.md Cargo.toml CLAUDE.md; do
        [ -f "$prefix/$required" ] || { echo "$prefix/$required is missing; a mirror needs it at its root." >&2; exit 1; }
    done
    # Two licensing arrangements exist here on purpose: the crates extracted as reusable libraries are
    # MIT OR Apache-2.0 (the Bevy-ecosystem norm, so they can actually be adopted), and the `emerge-*`
    # family stays GPL-3.0 with the game it was carved out of. Either is fine; NO license file is not,
    # because a repo without one is "all rights reserved" no matter what the manifest claims.
    if [ ! -f "$prefix/LICENSE" ] && { [ ! -f "$prefix/LICENSE-MIT" ] || [ ! -f "$prefix/LICENSE-APACHE" ]; }; then
        echo "$prefix needs either LICENSE, or both LICENSE-MIT and LICENSE-APACHE." >&2
        exit 1
    fi

    url="git@github.com:$ORG/$name.git"
    if ! gh repo view "$ORG/$name" >/dev/null 2>&1; then
        desc=$(sed -n 's/^description = "\(.*\)"$/\1/p' "$prefix/Cargo.toml" | head -1)
        echo "== creating $ORG/$name (private)"
        gh repo create "$ORG/$name" --private --description "$desc"
    fi

    echo "== $name"
    # The sha is the last line of stdout; `git subtree split` also prints progress there, so it is
    # verified to BE a commit before anything is pushed.
    sha=$(git subtree split --prefix="$prefix" HEAD | tail -n 1)
    git rev-parse --verify "$sha^{commit}" >/dev/null 2>&1 \
        || { echo "subtree split produced no commit for $name (got: $sha)" >&2; exit 1; }
    git push "$url" "$sha:refs/heads/main"
done

echo "done — $(( ${#CRATES[@]} )) mirror(s) in sync."
