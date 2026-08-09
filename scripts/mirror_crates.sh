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
    # Vendored in with `git subtree add` rather than extracted from the game, so the flow here is the
    # same as every other crate but the direction of the FIRST move was inward. Its nested
    # `crates/bevy_debugger_bevy` travels with it — one mirror, both halves.
    bevy_debugger_mcp
    # Runtime mesh fracture. Extracted from `src/autogib.rs`; the game keeps the rifle tagger, the run
    # gate and the avian spawning, and this keeps the slicer.
    bevy_autogib
)

# Mirrors that are created PUBLIC. Everything absent from this list is created private — see the
# `gh repo create` call below for why the default sits here rather than in a flag.
#
# `bevy_autogib` is public because nothing in the Bevy ecosystem ships runtime plane-cut prefracture
# with watertight caps; `bevy_light_grid` because "illuminance the AI can read" is a question a renderer
# does not answer; `bevy_speech_bubbles` because a world-space balloon is not something Bevy's text
# stack does; `bevy_orca` because reciprocal avoidance is a solved algorithm most engines still make
# you write yourself; `bevy_stigmergy` because coordination-through-the-environment is a whole class
# of group behaviour that needs no messaging layer. None of the five needs any of the game to be useful. Both are standalone-buildable and
# both carry a `.github/workflows/ci.yml` that the split lifts to the mirror root, per the note above.
# `bevy_debugger_mcp` is also public on GitHub, but it was vendored in already-created, so this list
# never has to make it so.
#
# NB this list only drives repo CREATION. A repo that already exists is not touched here — flipping an
# existing mirror's visibility is `gh repo edit <name> --visibility public`, deliberately a hand action.
PUBLIC_CRATES=(
    bevy_autogib
    bevy_light_grid
    bevy_speech_bubbles
    bevy_orca
    bevy_stigmergy
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
    # **Examples are how a stranger judges the crate without building the game.** One runnable example
    # is the difference between "here is a library, read the source" and "here is what it does, run it".
    # A crate whose behaviour cannot be seen in isolation is one nobody adopts, so this is required
    # rather than encouraged.
    if ! ls "$prefix"/examples/*.rs >/dev/null 2>&1; then
        echo "$prefix needs examples/ with at least one .rs — a mirror nobody can run is a mirror nobody adopts." >&2
        exit 1
    fi
    # The honesty label. These crates were written by an agent; anyone landing on the repo cold is
    # entitled to know that before they depend on one.
    if ! grep -q 'Vibe Coded' "$prefix/README.md"; then
        echo "$prefix/README.md is missing the \"Vibe Coded\" warning label." >&2
        exit 1
    fi
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
        # Visibility is per-crate and DEFAULTS TO PRIVATE, so a crate nobody thought about cannot be
        # published by omission. It is read from PUBLIC_CRATES rather than passed as a flag because this
        # branch only runs when the repo does not exist yet — on a re-run against a deleted repo, a flag
        # someone forgot would silently recreate a public mirror as private, and the mirror would then
        # 404 for everyone who had the link.
        vis="--private"
        for pub_name in "${PUBLIC_CRATES[@]}"; do
            [[ "$name" == "$pub_name" ]] && vis="--public"
        done
        echo "== creating $ORG/$name (${vis#--})"
        gh repo create "$ORG/$name" "$vis" --description "$desc"
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
