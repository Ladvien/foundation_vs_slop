# Handoff — the blank slate, the two mesh states, and the VLM batch

**Written 2026-08-15, branch `tiles-arrows-and-kit-tab`, HEAD `c17fa7d` + 20 uncommitted files.**
`cargo test --workspace` — **2,522 passed, 0 failed, 0 ignored**; `cargo build` — no warnings. The
editor runs and has been driven at the keyboard throughout.

**Run `--workspace`, not `-p emerge-mapper`.** The mapper suite was green for a day while 32 game
tests were red; §1 is that story.

Read `docs/2026-08-15-usability-handoff.md` first — it covers the guided-feedback loop, the research
behind the discoverability work, and the earlier half of this session. This one carries what came
after: the kit was cleared, meshes gained two states, and the VLM labeler became a batch tool.

---

## 1. The blank slate is `assets/emerge/site_v2/`, and it is a kit of its own

Asked for and executed 2026-08-15: **clear everything, author from scratch against the ozea meshes.**
It was first done by emptying `assets/emerge/` and `assets/emerge/site/` in place, and **that was
corrected the same day** — see the box below, which is the part worth reading.

| File | State |
|---|---|
| `assets/emerge/site_v2/library.ron` | `descriptors: []` |
| `assets/emerge/site_v2/compositions.ron` | `compositions: []` |
| `assets/emerge/site_v2/untitled_map.map.ron` | empty, bounds **10 × 4 × 10** |
| `assets/emerge/site_v2/project.ron` | no patches, `face_bands: 1` |

Open it with **`cargo run -p emerge-mapper --kit site_v2`**. Each file's `note:` says it is empty on
purpose. **`git checkout` on anything under `assets/emerge/site_v2/` undoes a decision** — and that
sentence applies **only** to that directory now.

> **The correction, because it will otherwise be repeated.** `assets/emerge/site/` is not just the
> mapper's kit: it is `src/site/kit.rs::SITE_PROJECT_DIR`, the game's **shipped** 45-piece Ozea kit
> that dresses the Site hub, and `assets/emerge/library.ron` is what `src/emerge_map.rs` loads.
> Emptying them took **32 game tests** down — all of `site::{kit,layout,pieces,people,smart}` and
> `emerge_map` — with `"site kit: Floor names descriptor `site/floor`, which this project does not
> define."` Nothing surfaced it for a day because only `cargo test -p emerge-mapper` was being run;
> `cargo test --workspace` is the CI hard gate and it was red the whole time. Both were restored
> from `HEAD` and the blank slate moved to its own directory. **One directory, one job** — which is
> the shape this repo already had in `site` vs `site_greybox`, *"two kits, two projects"*.
>
> A kit directory is **self-contained**, not layered over the root: `policy::layered_library` reads
> `library.ron`, `project.ron` and an optional `compositions.ron` from that one directory. So a new
> kit is four files and costs the existing ones nothing.

**The policy file has to start empty, and that is not laziness.** `Project::open` **refuses** a patch
that matches no descriptor — *"a rule that silently applies to nothing is how a policy rots"* — so a
kit with no library can carry no patches, and adding one before the pieces exist makes the editor
unstartable. **The architecture belongs back once the ozea pieces have ids**: walls 2.40 m, doorways
2.00 m, recorded as prose in `site_v2/project.ron`'s note. `assets/emerge/site/project.ron` carries the
shipped version of exactly those 8 patches and is the thing to read across from.

**The default map is 10 × 4 × 10** (`emerge-core/src/map.rs::default_bounds`). Note the trap that
cost time: with `--kit <name>` the editor opens `assets/emerge/**<name>**/untitled_map.map.ron`, not
the one beside it. An existing map carries its own bounds, so changing the default does nothing to it.

**Two asset-contract tests were retired while the kit was empty**, each with its reasoning in place
of its body: `the_authored_edge_tokens_reach_the_editor` (asserted `site/wall`'s authored lattice;
`Fixture` cannot author a subgrid, so it could not be repointed) and
`the_feedback_script_still_matches_the_shipped_kit`. **Both could now be restored** — the corpus they
assert is back — and that is a small, worthwhile job for whoever picks this up.
`the_editor_boots_on_the_site_kit` survives and asserts the shipped kit is **populated**, which is the
cheap alarm against §1 happening again.

---

## 2. Two kinds of mesh, one predicate

The model the author asked for: an **unlabeled mesh** is raw material; a **labeled mesh** can compose
a tile. Both already existed under other names.

- **`labels::needs_labels(d)`** is the predicate — unlabeled iff `kind`, `effects`, `look` or `note`
  is empty. It is also what the VLM batch picks targets by, so "what the labeler owes you" and "what
  you cannot build with yet" cannot drift apart.
- **`Shift+Delete` on the Meshes tab** is the way back — `DemoteTile`, *"back to candidates,
  stripped"*. It already existed.
- **`tiles::composable(d, pending)`** is the gate: judged **and** no proposal waiting. A machine can
  satisfy `needs_labels` on its own; a suggestion nobody has answered is a question, not an answer.

**Where the split is enforced, and the deviation from what was asked.** The **Tiles palette** lists
only composable meshes (`library_ids(.., labeled_only: true, pending)`), and its rows render green
(`chrome::LABELED`). The **Meshes tab shows everything** — the author asked for a strict two-way
split, and that was not built, because a labeled mesh must be selectable somewhere to un-label it and
`Shift+Delete` lives on that tab. The author knows; if strictness is wanted, the un-label verb needs
a home on Tiles and a key that is **not** `Shift+Delete` (that is `ClearTile` there).

**`Fixture`'s default descriptor is now fully judged.** Otherwise 22 existing tests place meshes the
palette no longer shows. `Fixture::unjudged_descriptor()` is the other entity, and
`the_tiles_palette_lists_only_judged_meshes` contrasts them so the fixture default cannot hide a
regression.

---

## 3. The VLM batch

`Shift+L` on the Meshes tab. What changed today, all of it from live failures:

- **Preflight** (`vlm::probe` → `Reach::{Ready, Warming, Unreachable}`). Configured and reachable are
  different questions and only the first was asked: with the SSH forward down, a walk queued 778
  meshes and reported 778 identical failures. **Refused** → refuse the walk and print
  `ssh -fN -L 9292:127.0.0.1:9292 bmb`; **slow** → start anyway, because `llama-swap` cold-loading a
  30B model is ordinary. A TCP connect, not a chat request. Remote endpoints are not probed.
- **A dead transport stops the walk** rather than burning the queue one failure at a time. Gate
  rejections still skip only their own mesh.
- **`Shift+L` mid-walk HOLDS** (`LabelQueue::paused`) and keeps its place; `Shift+Y` abandons
  everything. Cancelling meant re-photographing hundreds of meshes to get back.
- **Scope is the filter box.** `F`, type `ozea`, `Shift+L` walks only matching meshes. Library rows
  match on `d.id`, candidates on `c.mesh` — **matching a candidate by id would silently take the
  whole set**, since a candidate has no id.
- **A batch confirms its own proposals** (`LabelQueue::auto_apply`, `tiles::auto_apply_batch`, one per
  frame). The single `L` still stages for `U`. Both go through **`tiles::apply_suggestion`**, extracted
  today so the guards and the righting branch cannot diverge.
- **Progress is visible** — `LABELING 16/778 Shift+L holds`, a bar, and the live subject, above the
  list (`paint_label_progress`). Before, progress was a status-line string the next note overwrote,
  which is why a running walk looked identical to a hung one.

### The prompt, twice corrected from real output

Both fixes are the same shape: the model was answering about the *photograph* or filling a field
because a field was offered.

1. **Mounts.** Every asset is photographed standing on a plain floor, so `on floor` described the
   picture of nearly everything. `vlm::mount_meaning` now gives each token its meaning — *"stands on
   the ground by itself… Do NOT choose this merely because the photo shows it on a floor"* — and the
   section opens by telling the model the photo cannot answer the question. Tokens stayed short; a
   small model copies a short word more reliably.
2. **Effects.** A barrel came back tagged `uses-electricity`, whose own note already said "stops
   working when the power does". `axis_lines` now says **"MOST OBJECTS HAVE NONE"** for `effects` and
   *"do not infer it from what the object is made of, what it might contain, or where it might be
   plugged in"*. The generic "prefer an empty list" line was already present and insufficient.

**Verify prompt changes by printing the assembled prompt**, not by reading the builder. A throwaway
test that calls `build_prompt` and prints the section is how both of these were checked; note the
mount lines are in the **system** prompt, not the user one.

---

## 4. Operational: the model lives on `bmb`

**Nothing is wrong with bmb** — this was investigated. Up 61 days, `llama-swap` under
`com.local.llama-swap.plist`, `qwen3-vl-30b` served (the editor's default model).

**The fragility is this machine's SSH forward**, a bare `ssh -fN` with nothing supervising it:

```sh
ssh -fN -L 9292:127.0.0.1:9292 bmb          # bring it up
curl -s http://127.0.0.1:9292/health         # expect OK
```

Direct LAN (`192.168.1.113:9292`) is **blocked by bmb's firewall**, so the forward is the only path.
Making it durable (`autossh`, or a LaunchAgent) was offered and **not done** — it changes the user's
login items and is their call.

**Contention is real and looks like a hang.** home-still's scribe drives `glm-ocr` on the same
llama-swap; with a conversion in flight the log showed **818 requests from it against 102 from the
editor** and 12 model swaps, and each swap reloads a multi-GB model — ~20 s per mesh instead of a few.
llama-swap's own log is the diagnostic:

```sh
KEY=$(grep '^EMERGE_VLM_KEY=' .env | cut -d= -f2-)
curl -s -H "Authorization: Bearer $KEY" "http://127.0.0.1:9292/logs?count=200" | tail -30
curl -s -H "Authorization: Bearer $KEY" http://127.0.0.1:9292/running
```

`ureq/3.3.0` is the editor; an empty user-agent is the other workload.

---

## 5. Also shipped today (uncommitted)

Explicit tile naming (`N` opens `chrome::NameBox`; `Cmd+S` on an editor-named tile asks first;
`Build::naming: Option<NamePrompt>` carries **why** it was raised — inferring that from state
silently renamed the tile in hand). Ray-picking by volume (`editor::ray_pick`) orchestrated with the
ground pick by what is armed. Pointer capture (`ui_blocks_gesture`). `F` focuses the filter. `left`
leaves the KIT list. The nav gizmo (`compass.rs`). Grid-locked removal box. `try_insert` on both
material painters — a plain `insert` on a despawned entity is a **hard panic** in Bevy 0.19, and it
killed the editor mid-session.

---

## 5b. Two new guide cards, for the next feedback session

`guides/` gained two, both on **`--kit site`** (the restored kit) and both accepted by the two
ratchets — every checkpoint they name is registered and runs, every piece they name ships.

- **`branch_verbs.json`** — walks what this branch added and no person has yet used: all four arrows
  on a held member, the `J` rung ladder, explicit naming through `N`, reopening from the KIT list,
  the nav gizmo, the grid-locked removal box, and a **tipped box fill**. Half its steps are
  `checkpoint: null` on purpose — "did every arrow answer", "is the gizmo readable" are the questions
  a machine cannot answer and the reason a person is being asked.
- **`build_a_room.json`** — authors a corner tile (two walls, the second turned), a wall tile and a
  door tile (`site/wall_doorway`), then stamps a room out of them: four runs, four corners, one
  door. Held by `the_room_script_can_actually_be_followed`.

**Two things this needed that did not exist.** `the map has tiles on it` counts `Map::stamps`, which
`the map has placements` deliberately cannot see — a stamped tile is a *reference*, so a room built
from tiles leaves the placement count at zero and every step of the second half would have had to be
a judgement call. And `the Compose tab is open`, since arming a tile (tab `4`, walk, `Enter`) is the
step an author is least likely to guess and there was no way to confirm they had arrived.

**Read the key census before writing a card, do not recall it.** Writing these from memory would have
put four wrong keys in: on the **Map** tab `N` is `RenameMap` and `F` is `Fill`, while `F` is
`FocusFilter` only on **Tiles**; on **Meshes** `F` is `CellLeft`. Printing `keys::in_context` is two
minutes and is how all four were caught — the same "print the assembled thing" lesson §3 records for
the VLM prompt.

**And check what a checkpoint's argument actually counts.** `the tile has turns` counts **distinct**
quarter-turns, so a corner is `n: 2`. It was first written `n: 1`, which passes on any tile with one
piece in it — a checkpoint that cannot fail, reading as a guarantee. The drive test now pins that
count by value, and was checked against the defect put back.

---

## 6. Next, in the order I would take it

1. **Apply-all for staged proposals.** The single `L` still stages, and there is no bulk verb. Only
   the batch auto-confirms.
2. **A queue list** — the author asked for "maybe a second scroll view" of what is queued. There is a
   bar, a count and the live subject; not the list.
3. **Commit.** ~24 files are uncommitted, including the new `assets/emerge/site_v2/` kit.
   `scripts/mirror_crates.sh` after, and note it splits from `HEAD` — this branch is 6 commits ahead
   of `main`, which is safe because this repo merges with merge commits (verified), not squashes.
   **`cargo test --workspace` is green** (2,522 tests) as of the kit correction in §1; it was red for
   a day before it, so run it rather than trusting `-p emerge-mapper`.
4. ~~**The guides still name the old corpus.**~~ **Resolved — and the guides were never the problem.**
   `every_piece_a_shipped_guide_names_exists_in_the_shipped_kit` (`tests/headless.rs`) reads what
   actually ships and would name every stranded card at once. It was written **red**, listing four
   files and `site/floor`, `site/wall`, `site/wall_low`, `site/tile_4` — and the obvious reading, that
   the cards had rotted, was wrong. The *kit* had been emptied (§1). Restoring it turned this green
   **without a word of any card changing**, so it now runs un-ignored as a live guarantee.
   `author_a_tile.json` and `place_and_generate.json` name no piece at all. The lesson is in the
   test's own doc: when it fails, the cheaper explanation is that the corpus moved, not that the
   prose is stale.
5. ~~**`fill::box_fill` takes a yaw and no tip.**~~ **Fixed 2026-08-15, and it was three defects.**
   Both `fill::flood` and `fill::box_fill` built their `Placed` with `..Placed::default()` after
   naming `yaw`, so **both** dropped the tip — and the lattice was wrong underneath them, because
   `cell_extents`/`brush_span` measured the *standing* footprint. A tipped piece lies down, so its
   height becomes a floor dimension. `descriptor::tipped_footprint` is the new one answer; the tip
   now threads through `brush_at` (so the ghost and the click stay on one lattice), the drag
   outline, `StampRow`, and `ray_pick`'s hit box. Held by
   `a_fill_lays_the_brush_tipped_the_way_the_author_tipped_it` and
   `a_tipped_brush_fills_on_the_footprint_it_actually_presents`, both checked against the reverted
   fix. `composition::Member` has **no** tip, so `build.rs` answers `MEMBERS_STAND_UP` — named, so
   the day it grows one the seven sites are findable.
