#!/usr/bin/env bash
# Yank the crates.io releases that the fold into `bevy_carnage` 0.5.0 (2026-09-04) left behind: every version
# of the six `bevy_*` gore leaves, and the four `bevy_carnage` releases that cannot resolve without them.
# Both sets are named version by version below and nothing is chosen from what the registry happens to list,
# so a release published later can never be swept up by a rerun.
#
# Yanking is the strongest close an owner has here. A yanked version still resolves for a lockfile that
# already pins it, but no fresh resolution will select it. crates.io does let an owner delete a crate since
# RFC 3660 (this header once said it could not — wrong), but never one that any version of another crate
# depends on, yanked or not, and every leaf is a dependency of some `bevy_carnage` release.
#
# **Yanking a leaf breaks the releases that depend on it, and this script's first version said it did not.**
# As `scripts/yank_gore_leaves.sh` it yanked every version of the seven leaves and nothing else. Its one run
# (2026-09-04T23:55:47-51Z, measured from crates.io's version history) yanked the six `bevy_*` leaves and was
# refused for `bloodstain`. That left `bevy_carnage` 0.3.0-0.4.0 listed as live but unresolvable (measured
# 2026-10-02: `bevy_carnage = "=0.4.0"` fails on `bevy_cross_section = "^0.1.2"` ... "version 0.1.2 is
# yanked"), so they are in TARGETS now (decided 2026-10-02). The same run told the operator five times that
# the token lacked the yank scope. It cannot have: it yanked 18 versions. The likelier cause, inferred because
# that version discarded cargo's message, is a crate scope that covered `bevy_*` and not `bloodstain`.
#
# **`bloodstain` stays live, by decision (2026-10-02)** — so does `bevy_carnage` 0.2.0, which needs only
# `bloodstain ^0.1`. `bloodstain` is engine-free and `no_std`, and the module that replaced it is reachable only
# through `bevy_carnage`, which always pulls in Bevy. KEEP_LIVE names every version that must stay live, and
# the run fails if any of them is yanked.
#
# Needs a token with the `yank` endpoint scope and a crate scope that covers every crate still live in
# TARGETS — today only `bevy_carnage`. Leave `bloodstain` out of that scope, and no edit to this file can yank
# it. Hand the token to this one process, read without echo: typed after a command it lands in shell history,
# `cargo login` would overwrite the publish token in ~/.cargo/credentials.toml that `cargo publish -p
# bevy_carnage` relies on, and an exported CARGO_REGISTRY_TOKEN would outlive the run and override it too.
#
#   read -rs t && CARGO_REGISTRY_TOKEN=$t scripts/yank_folded_releases.sh; unset t      # bash
#   read -s t; and CARGO_REGISTRY_TOKEN=$t scripts/yank_folded_releases.sh; set -e t    # fish
#
# Idempotent: a version already yanked is skipped, so a rerun finishes the job once the reported cause is
# fixed. The verdict is read back from crates.io after the yanks, not taken from cargo's exit status.
set -euo pipefail

# Dependents first, so no release is ever live while a leaf it needs is yanked.
TARGETS=(
    bevy_carnage@0.4.0 bevy_carnage@0.3.2 bevy_carnage@0.3.1 bevy_carnage@0.3.0
    bevy_wetmap@0.2.3 bevy_wetmap@0.2.2 bevy_wetmap@0.2.1 bevy_wetmap@0.2.0 bevy_wetmap@0.1.0
    bevy_viscera@0.1.1 bevy_viscera@0.1.0
    bevy_cross_section@0.1.2 bevy_cross_section@0.1.1 bevy_cross_section@0.1.0
    bevy_flaymap@0.1.2 bevy_flaymap@0.1.1 bevy_flaymap@0.1.0
    bevy_laceration@0.1.2 bevy_laceration@0.1.1 bevy_laceration@0.1.0
    bevy_fracture_modes@0.1.1 bevy_fracture_modes@0.1.0
)
KEEP_LIVE=(
    bevy_carnage@0.5.0 bevy_carnage@0.2.0 bevy_carnage@0.1.1 bevy_carnage@0.1.0
    bloodstain@0.3.0 bloodstain@0.2.2 bloodstain@0.2.1 bloodstain@0.2.0 bloodstain@0.1.0
)

# The state is read from crates.io, so the yanks go to crates.io: without `--registry`, `cargo yank` follows
# `registry.default` and any `[source.crates-io]` replacement in the caller's config chain.
REGISTRY=crates-io
VERSIONS_API=https://crates.io/api/v1/crates
USER_AGENT="foundation_vs_slop yank_folded_releases"
# curl sets no transfer timeout of its own; a stalled read must fail the run, not hang it.
HTTP_CONNECT_TIMEOUT_S=10
HTTP_MAX_TIME_S=30
EX_USAGE=64

if [ "$#" -ne 0 ]; then
    echo "usage: ${0##*/} — takes no arguments, and has no dry run: every run yanks for real" >&2
    exit "$EX_USAGE"
fi

for target in "${TARGETS[@]}"; do
    for keep in "${KEEP_LIVE[@]}"; do
        if [ "$target" = "$keep" ]; then
            echo "$target is in both TARGETS and KEEP_LIVE — nothing was yanked" >&2
            exit 1
        fi
    done
done

# Prints `yanked` or `live` for one crate@version, as crates.io lists it now. `-S` makes curl name an HTTP
# or network failure, including the 404 for a version that was never published.
yank_state() {
    local crate=${1%@*} version=${1#*@} body
    body=$(curl -sSf --connect-timeout "$HTTP_CONNECT_TIMEOUT_S" --max-time "$HTTP_MAX_TIME_S" \
        -A "$USER_AGENT" "$VERSIONS_API/$crate/$version") || return 1
    python3 -c '
import json, sys
v = json.load(sys.stdin)["version"]
if (v["crate"], v["num"]) != (sys.argv[1], sys.argv[2]):
    sys.exit("crates.io answered for %s@%s, not the version asked for" % (v["crate"], v["num"]))
print("yanked" if v["yanked"] else "live")
' "$crate" "$version" <<<"$body"
}

failed=0
for target in "${TARGETS[@]}"; do
    if ! state=$(yank_state "$target"); then
        echo "$target: FAILED to read its state from crates.io (reason above)" >&2
        failed=1
        continue
    fi
    if [ "$state" = yanked ]; then
        echo "$target: already yanked"
        continue
    fi
    # cargo's own output is the diagnosis. Its exit status cannot tell an endpoint-scope 403 from a
    # crate-scope 403, a 429, a network error or a missing token; its message can.
    if cargo yank --registry "$REGISTRY" "$target"; then
        echo "$target: yanked"
    else
        echo "$target: FAILED (cargo's reason is above)" >&2
        failed=1
    fi
done

# The verdict is what crates.io lists now. It reads versions through `db_read()`, which prefers a read
# replica, so a yank made a moment ago can still show as live; a rerun reads it again.
expect() {
    local want=$1 target=$2 state
    if ! state=$(yank_state "$target"); then
        echo "$target: FAILED to re-read its state from crates.io (reason above)" >&2
        return 1
    fi
    if [ "$state" != "$want" ]; then
        echo "$target: crates.io lists it as $state, but it must be $want" >&2
        return 1
    fi
}
for target in "${TARGETS[@]}"; do expect yanked "$target" || failed=1; done
for keep in "${KEEP_LIVE[@]}"; do expect live "$keep" || failed=1; done

if [ "$failed" -ne 0 ]; then
    echo "crates.io does not match this script's lists — fix the cause reported above and rerun" >&2
    exit 1
fi
echo "crates.io matches: ${#TARGETS[@]} versions yanked, ${#KEEP_LIVE[@]} live"
