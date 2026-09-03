#!/usr/bin/env bash
#
# Build the carnage demo site into web/dist: ten WebAssembly demos and three
# hand-written pages.
#
#   ./scripts/build_web.sh
#
# # Why this is built from the monorepo
#
# It is the only tree where all four crates — `bevy_carnage`, `bloodstain`,
# `bevy_wetmap`, `bevy_viscera` — are visible to one cargo workspace, so it is
# the only place the flagship cross-crate demo can be compiled at all. Each
# crate's own public mirror contains that crate alone.
#
# # Four things this script learned from `~/isomesh/scripts/build_web.sh`, which
# # is its model, and each one cost a real failure there
#
# **1. Every check that can fail without compiling anything runs ABOVE
# `rm -rf web/dist`.** A gate that fires after the clean has already destroyed
# the previous build makes a local iteration loop pay the whole build for a typo.
#
# **2. The `wasm-bindgen` CLI version is read out of the lockfile, never written
# here.** A CLI that differs from the crate emits glue for a different ABI, and
# the module then fails to instantiate in the browser with an error naming
# neither tool.
#
# **3. There is no `wasm-opt`.** Measured on the reference site: `-Oz` took a
# Bevy module from 37.4 MB to 29.1 MB raw and from 8.73 MB to **9.31 MB
# gzipped** — 6.7 % the wrong way on the number a site actually pays — at 23 s
# per module. Each crate's own profile is doing this work already.
#
# **4. There is no `CNAME` and no custom domain.** A custom domain without a TLS
# certificate 301-redirects HTTPS to HTTP, and **WebGPU is a secure-context-only
# API**, so `navigator.gpu` goes `undefined` and every demo on the site dies. The
# reference site lost its whole demo page that way. A `CNAME` file in the
# artifact *re-sets* the domain on every deploy, so adding one here would
# silently undo the removal on the next push.
#
# # And one that is this project's own
#
# **Wasm demos never enable `vfx`.** `bevy_hanabi`'s wasm support is
# WebGPU-compute-only, and every *new* visual in this plan is CPU-side anyway —
# so `bevy_carnage` is built `--no-default-features --features serde` and GPU
# particles stay a native extra. Decided here rather than discovered at build
# time.

set -euo pipefail
cd "$(dirname "$0")/.."

# `pkg:example`, one per line. **The roster lives here and in `web/play.html`'s
# allow-list, and `scripts/demo_facts.sh` holds the two against each other** —
# because both drift modes are invisible in a green build: a module built but not
# listed is tens of megabytes nothing can reach, and one listed but not built is
# a link to a 404.
#
# Ordered cheapest-and-most-visual first, so a break in the wasm build in general
# surfaces on an early module rather than after twenty-five minutes. The flagship
# is last because it is the one that needs every crate.
DEMOS=(
    bevy_carnage:fault_modes
    bevy_carnage:stain_morphology
    bevy_carnage:pattern_classes
    bevy_carnage:rheology
    bevy_carnage:drying
    bevy_carnage:fragment_energy
    bevy_carnage:gore_tier
    bevy_wetmap:wetmap_paint
    bevy_viscera:viscera_spill
    bevy_carnage:carnage_web
)

OUT=web/dist
TARGET_DIR="${CARGO_TARGET_DIR:-$(cargo metadata --format-version 1 --no-deps 2>/dev/null |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"

# ---------------------------------------------------------------------------
# Refusals. Everything here can fail without compiling a line, so it all sits
# above the clean.
# ---------------------------------------------------------------------------

WANT=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[^0-9.]/,""); print; exit}' Cargo.lock)
if [ -z "$WANT" ]; then
    echo "cannot read the wasm-bindgen version from Cargo.lock" >&2
    exit 1
fi

if ! command -v wasm-bindgen >/dev/null 2>&1 ||
    ! wasm-bindgen --version 2>/dev/null | grep -qF "$WANT"; then
    echo "wasm-bindgen $WANT is required and is not what is on PATH." >&2
    echo "  cargo install wasm-bindgen-cli --version $WANT --locked" >&2
    exit 1
fi

# The roster and the allow-list must already agree. Running this here rather than
# only in CI means a mismatch costs a second instead of twenty-five minutes.
./scripts/demo_facts.sh

for page in web/index.html web/play.html web/style.css; do
    if [ ! -f "$page" ]; then
        echo "missing $page — the site is three hand-written files and this is one" >&2
        exit 1
    fi
done

if [ -e web/CNAME ]; then
    echo "web/CNAME exists. Read this script's header: a custom domain without a" >&2
    echo "certificate kills every WebGPU demo on the site." >&2
    exit 1
fi

rustup target add wasm32-unknown-unknown

# ---------------------------------------------------------------------------
# Build.
# ---------------------------------------------------------------------------

rm -rf "$OUT"
mkdir -p "$OUT/play/pkg"

for entry in "${DEMOS[@]}"; do
    pkg="${entry%%:*}"
    demo="${entry##*:}"
    echo "==> $pkg / $demo"
    # `--no-default-features --features serde` on `bevy_carnage`: see the header for why Hanabi is
    # absent from every wasm module. The other two crates have no such feature to turn off.
    #
    # **`${features[@]+"${features[@]}"}`, not `"${features[@]}"`.** Under `set -u` an EMPTY array
    # expands to an unbound-variable error on bash 3.2, which is what macOS ships — so the plain form
    # built the seven `bevy_carnage` demos and then died on the first crate with no features to pass.
    features=()
    if [ "$pkg" = "bevy_carnage" ]; then
        features=(--no-default-features --features serde)
    fi
    cargo build -p "$pkg" --profile wasm-release --target wasm32-unknown-unknown \
        --example "$demo" ${features[@]+"${features[@]}"}
    wasm-bindgen --target web --no-typescript \
        --remove-name-section --remove-producers-section \
        --out-dir "$OUT/play/pkg/$demo" --out-name "$demo" \
        "$TARGET_DIR/wasm32-unknown-unknown/wasm-release/examples/$demo.wasm"
done

echo "==> pages"
cp web/index.html web/play.html web/style.css "$OUT/"

echo
echo "wasm modules:"
for entry in "${DEMOS[@]}"; do
    demo="${entry##*:}"
    du -h "$OUT/play/pkg/$demo/${demo}_bg.wasm" | sed 's/^/  /'
done
echo "site total:"
du -sh "$OUT" | sed 's/^/  /'
