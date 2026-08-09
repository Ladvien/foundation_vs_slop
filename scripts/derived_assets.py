#!/usr/bin/env python3
"""**Derived assets are build output, so git keeps the recipe and not the bytes.**

`assets/ozea/*.glb` is not a source file. It is what `scripts/fbx_to_glb.py` produces from an `.fbx`
on the asset library share — resource-compiler output, in the sense Gregory means in *Game Engine
Architecture* 3e §7.2.1: source assets in native DCC formats pass through exporters and resource
compilers on their way to the engine. Committing that output is committing build artifacts, and git
keeps every version of every binary forever. Measured on this repo before this existed:
`assets/scp610/scp-610.glb` is in history three times, at 27 MB, 5 MB and 4 MB.

So the manifest goes in git and the bytes do not:

    cargo fvs assets verify    # do the files on disk match what the manifest says?
    cargo fvs assets sync      # fetch them from the cache on the library share
    cargo fvs assets build     # regenerate from source (needs Blender), then fill the cache

# Why a hash per output rather than just a rule

Because a rebuild that silently differs is worse than no rebuild. Lamb & Zacchiroli,
*Reproducible Builds: Increasing the Integrity of Software Supply Chains* (`10.1109/ms.2021.3073045`)
make the general argument; here it is concrete — Blender's exporter is not promised to be
byte-identical across versions, and an asset that quietly changed shape is the kind of thing that
surfaces as a placement looking wrong three weeks later. `verify` compares sha256 against the
manifest, so a drift is a named failure rather than a mystery.

# Why a cache, and why on the share

`build` needs Blender and the raw library; a fresh clone or a CI runner has neither. So a build also
**stages** its outputs into a content-addressed cache under the library share, and `sync` copies from
there. One machine with Blender pays the conversion; everything else copies. The cache is keyed by
the sha256 the manifest records, so a stale entry cannot be served for a changed recipe.

# The library root is per-machine

`FVS_ASSET_LIBRARY` names it, defaulting to the NFS export this project already documents. The
manifest stores paths *relative* to that root, so it says nothing about any one machine's mounts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MANIFEST = REPO / "assets" / "derived.json"
DEFAULT_LIBRARY = "/mnt/codex_fs/game_assets"
CACHE_DIRNAME = "fvs_derived_cache"


def library_root() -> Path:
    return Path(os.environ.get("FVS_ASSET_LIBRARY", DEFAULT_LIBRARY))


def cache_root() -> Path:
    return library_root() / CACHE_DIRNAME


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def load() -> dict:
    if not MANIFEST.exists():
        sys.exit(f"no manifest at {MANIFEST} — nothing is declared derived yet")
    return json.loads(MANIFEST.read_text())


def outputs(manifest: dict):
    """Every declared output as `(recipe, entry, absolute path)`."""
    for recipe in manifest["recipes"]:
        out_dir = REPO / recipe["out_dir"]
        for entry in recipe["outputs"]:
            yield recipe, entry, out_dir / entry["file"]


def cmd_verify(_args) -> int:
    manifest = load()
    missing, wrong, ok = [], [], 0
    for _recipe, entry, path in outputs(manifest):
        if not path.exists():
            missing.append(entry["file"])
        elif sha256(path) != entry["sha256"]:
            wrong.append(entry["file"])
        else:
            ok += 1
    print(f"derived assets: {ok} match, {len(missing)} missing, {len(wrong)} differ")
    for f in missing[:10]:
        print(f"  missing: {f}")
    for f in wrong[:10]:
        print(f"  DIFFERS: {f}")
    if len(missing) > 10 or len(wrong) > 10:
        print(f"  ... and {max(0, len(missing) - 10) + max(0, len(wrong) - 10)} more")
    # A file that DIFFERS is the loud case: something rebuilt it and nobody said so.
    return 1 if wrong or missing else 0


def cmd_sync(_args) -> int:
    """Fill `assets/` from the cache. Never runs Blender; never invents a file."""
    manifest = load()
    cache = cache_root()
    if not cache.exists():
        sys.exit(
            f"no cache at {cache}. Either the library share is not mounted, or nobody has run "
            f"`assets build` yet on a machine that has Blender."
        )
    copied, already, absent = 0, 0, []
    for _recipe, entry, path in outputs(manifest):
        if path.exists() and sha256(path) == entry["sha256"]:
            already += 1
            continue
        blob = cache / entry["sha256"][:2] / entry["sha256"]
        if not blob.exists():
            absent.append(entry["file"])
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(blob, path)
        copied += 1
    print(f"derived assets: {copied} copied, {already} already current, {len(absent)} not in cache")
    for f in absent[:10]:
        print(f"  not cached: {f}")
    # Absent from the cache is a refusal, not a shrug: the alternative is a half-populated tree
    # that looks like a working checkout.
    return 1 if absent else 0


def cmd_stage(_args) -> int:
    """Put what is on disk into the cache, keyed by the hash the manifest declares."""
    manifest = load()
    cache = cache_root()
    staged, skipped = 0, 0
    for _recipe, entry, path in outputs(manifest):
        if not path.exists():
            skipped += 1
            continue
        have = sha256(path)
        if have != entry["sha256"]:
            print(f"  NOT STAGED {entry['file']}: on disk it hashes {have[:12]}, "
                  f"the manifest says {entry['sha256'][:12]}")
            skipped += 1
            continue
        blob = cache / have[:2] / have
        if blob.exists():
            continue
        blob.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, blob)
        staged += 1
    print(f"derived assets: {staged} staged into {cache}, {skipped} skipped")
    return 1 if skipped else 0


def cmd_build(args) -> int:
    """Regenerate from source with the recipe's own tool, then verify and stage."""
    manifest = load()
    blender = os.environ.get("FVS_BLENDER", "/Applications/Blender.app/Contents/MacOS/Blender")
    if not Path(blender).exists() and not shutil.which(blender):
        sys.exit(f"no Blender at {blender} — set FVS_BLENDER, or use `assets sync` instead")
    for recipe in manifest["recipes"]:
        src = library_root() / recipe["source"]
        if not src.exists():
            sys.exit(f"recipe `{recipe['id']}`: no source at {src}. Is the library share mounted?")
        out = REPO / recipe["out_dir"]
        out.mkdir(parents=True, exist_ok=True)
        cmd = [blender, "--background", "--factory-startup", "--python",
               str(REPO / recipe["tool"]), "--",
               "--src", str(src), "--out", str(out), *recipe["args"]]
        print(f"recipe `{recipe['id']}`: {' '.join(cmd[:4])} ...")
        if subprocess.run(cmd, check=False).returncode != 0:
            sys.exit(f"recipe `{recipe['id']}` failed; nothing was staged")
    rc = cmd_verify(args)
    if rc == 0:
        rc = cmd_stage(args)
    return rc


def main() -> int:
    p = argparse.ArgumentParser(prog="derived_assets")
    sub = p.add_subparsers(dest="cmd", required=True)
    for name, fn, help_ in [
        ("verify", cmd_verify, "check the files on disk against the manifest's hashes"),
        ("sync", cmd_sync, "copy them from the cache on the library share"),
        ("stage", cmd_stage, "put what is on disk into that cache"),
        ("build", cmd_build, "regenerate from source (needs Blender), then verify and stage"),
    ]:
        s = sub.add_parser(name, help=help_)
        s.set_defaults(fn=fn)
    args = p.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
