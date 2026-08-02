# Site-67 — what to do next

**Handoff, 2026-08-02, revised the same day.** The first draft of this file ranked the work; the
revision records what shipped, **corrects five of its own claims that turned out to be wrong**, and
narrows what is left.

⚠️ **This document was untrusted input and it was right to treat it that way.** Five of its factual
claims did not survive being checked against `HEAD`. They are listed below rather than quietly edited,
because the pattern is the useful part: every one of them was a plausible sentence written from a grep
rather than from the code.

---

## What shipped

| | |
|---|---|
| **The hub knows which room you are in** | `SiteLayout::area_at`, `CurrentArea`, `AreaEntered`. Nothing could ask this before. |
| **Panels open themselves** | Records, Requisition, and the two research panels belong to rooms. **Keys still work anywhere** — presence offers, the key acts. |
| **The rooms say their names** | `Area::label` had twelve authors and zero readers outside two `warn!` strings. It holds, then fades (`ui::hint`'s retire rule). |
| **Knee-wall cutaway** | The hub's walls squash like the dungeon's. This was the single biggest visual defect: at two of four detents whole wings were behind a full-height wall. |
| **The Paratherapist has a job** | `Knowledge::strain` — the design doc §6.2 counter-pressure to veteran lock-in — plus a **deep debrief** that trades a `Lethal` belief for calm. |
| **The hub makes its own noise** | `area_tone` per wing, and footfalls, which the Site had none of. |
| **Dressing, committed** | Plus four real defects the new tests found in it (below). |

---

## The five claims that were wrong

| Claim | What is actually true |
|---|---|
| *"The Site is completely silent."* | **Wrong.** `load_audio` spawns the wind bed (0.22) and the calm music loop (0.32) at `Startup`, ungated, and `audio.rs:1023` records deliberately *fixing* Site silence. What was missing was anything the hub itself made: footfalls, one-shots, per-room identity. |
| *"Six player verbs."* | **Nine.** Missing from the list: cycle the study specimen, file findings, curate the archive (`antagonist.rs:206`), toggle the roster. |
| *"Put a specimen on the slab (`research/lab.rs`)."* | **Wrong file, wrong verb.** `keep_a_study_subject` auto-selects and `lay_out_the_study_subject` is explicitly cosmetic. The verb is `cycle_study_subject`, in `ui/site_hud.rs`. |
| *"Thirteen areas."* | **Twelve** authored, eleven required. Now fifteen — three runs of connective floor were unclaimed (below). |
| *"`Near`/`Facing`/`Clearance` implemented; missing only tag-scoped authoring."* | **Misleading.** True of `solvers/metropolis.rs` — but **`src/site/` never touches the solver.** The hub has five hand-coded rules in `layout.rs` and its kits author no rules at all. Reusing the grammar in the Site is first-time integration, not authoring. |

And two **in-repo** falsehoods, both corrected by making them true rather than by deletion:

- `staff.ron` and `docs/lore/2026-08-02-site-67-recommissioned.md` both said operatives carry FEAR
  between expeditions. They did not — `Drives` is run-scoped and absent from `SaveGame`. `strain` is
  what makes the sentence true.
- `coupling.rs` said the belief↔fear coupling ships at gain **zero**. It has been `0.4` since it was
  turned on, and the module's own test asserts `> 0.0`.

---

## What the new tests found

Four defects, none of which any existing test could see. They are listed because the *shape* recurs:
**a rule silently stops applying when the thing it judges changes size.**

1. **`rests_on` matched a class against a `bool`.** A mug asking for a `worktop` seated on the
   specimen slab as happily as on a table — the class existed only to be interpolated into an error
   string. Both sides now carry classes and match by bit.
2. **A host across a wall was a host.** The reach test is a 2.5 m radius and the Site's rooms are one
   cell apart, so a prop near a party wall could take its height from the next room.
3. **`is_floor_marking` was a bare height threshold**, so the 0.109 m mug, the 0.04 m folder and the
   books were all reclassified as floor decals — exempting them from the overlap rule *and* from the
   staff-exclusion set. Height was never the definition.
4. **The overlap rule's slack (0.02 m²) is larger than a mug's whole footprint (0.014 m²)**, so two
   mugs could occupy one point and never trip it. An absolute slack stops being a rule once the props
   are smaller than the slack.

Plus one content gap: **three runs of floor belonged to no area** — the south spine and two connectors.
Invisible while an area was only something you looked a rect up from; standing on them now means being
*nowhere*, with no room tone and no name, and they were unlit for the same reason. `Corridor` may now
be declared more than once, which is that variant's own definition.

---

## Still open

| Item | Size | Note |
|---|---|---|
| **The War Room readout** | S | `ui/briefing.rs` already renders `BRANCH UNIVERSE 0x… CLUTTER ▓▓▓░░ INFESTATION ▓▓░░░`. The room is called the war room and does not show it. ⚠️ `RunState::Idle` **only** — `two-live-layers` §5 forbids supervising an unattended squad. Same for Monitoring, whose verb is reviewing the **last** expedition. |
| **The staff should talk** | M | ⚠️ **`Bark::speaker` is a `usize` squad-member index that resolves to a `Unit`, and the Site deliberately has none.** So the existing bark path cannot carry a staff line without work — wiring it naively produces a system that silently never fires, which is this repo's most common bug. It also wants the **dialogue speaker re-audit** first (~15 conversations; `config.ron`'s speaker note disagrees with `RoleId`). |
| **Briefing has no panel** | S | A consequence of room-gating, and it is the room the five operatives spawn in — so the first thing a player ever sees in the hub is now a room with a name and no readout. The roster is the natural fit ("plan the next expedition" = look at your people), but it is a full-screen overlay today, and auto-opening one on entry would be intrusive. **Decide deliberately.** |
| **Dressing grammar stages 3–5** | L | Route Site clutter through the orchestrator; `Guard` against a `SiteEra` from `O5Standing::expeditions` (the one strictly-monotone persisted counter — **not** `RunSeed`, resampled per expedition). ⚠️ Reconcile, never respawn: `spawn_site_geometry` is `Startup`-only *deliberately*. |
| **Room tone as looping beds** | M | `area_tone` gives each wing its own rhythm and register from the nine shipped one-shots. Twelve rooms cannot have twelve *timbres* from nine clips; the library has 3,614 `.ogg` (**not** the 5,076 this file first claimed — that figure counts one format per track). ⚠️ ogg only. |
| **Stage C — routines, pathfinding** | L | Staff stand still. `drive_avatars` drops the order when wedged. Unblocks the D-Class escort. |
| **D-Block, D-Class bodies, the galley cook** | M | `fieldop` is the orange jumpsuit and is now the D-Class body; the cook needs a rig nobody else wears. |
| **The `replay + liveness` CI red** | ? | Failing on `main` for 3+ merges (`tests/containment.rs:842`, `:1163`). ⚠️ `BACKLOG.md` names the **wrong cause**, and the overall GitHub run still reports success. |

---

## Traps this hub has already sprung

Every one of these cost real time and none would show up in a test.

- **Three of the last four real defects were invisible to `cargo test`** — staff facing walls, a staff
  member inside a containment booth, a saturated-orange mug. All found by rendering and looking.
  **Budget a capture pass per change, from more than one yaw detent.**
- ⚠️ **A capture taken on the same tick as the state change shows the frame BEFORE it.** The archive
  panel read as blank-and-broken for twenty minutes on 2026-08-02 because the tour teleported the
  player and requested the screenshot in one tick. Instrument before believing a screenshot.
- **`SitePiece::ALL` is not compile-enforced.** Adding a piece touches five places and that is the
  silent one.
- **`cells:` is a separate list from `props:`** — a rule written against props skips a sixth of the
  containment wing.
- **The Site's rooms have no doors.** An opening is the *absence* of wall, so a room can be open along
  a whole side. Furniture authored against a north wall that did not exist was 31 faults, and it is
  why the room names went in the HUD rather than on a placard.
- **Props are a hard exclusion for staff positions** — dressing a room shrinks where its staff can
  stand. Re-run the staff tests after any dressing change.
- **Colour is load-bearing.** Grayscale is contained, colour is anomalous, **orange means D-Class**.
- **`tests/determinism_lint.rs` is textual** and catches `min_by`/`max_by` too — but it *cannot* see a
  hand-rolled loop, so a totality claim in a comment is unenforced unless a test asserts it.
- **The GPU is shared.** A TRELLIS job took 18.9 of 24 GB mid-session and the game began exiting at
  startup with `Quitting the application due to OutOfMemory RenderError`. Check `nvidia-smi` before
  blaming your change, and never quote a frame time without checking the GPU is idle.
- **Harness guards peak ~9 GB RSS** — check `free` first or the OOM killer takes them (signal 9).

---

## Verification

- `cargo test` — the hard gate, GPU-free. **1,025 lib tests, 30 binaries green** at the time of
  writing. Includes the placement rules, both kits, the staff asset contract, the determinism lint,
  the genome-coverage ledger and the panic budget.
- `cargo test --features test-harness -- --test-threads=1 --no-fail-fast` — needs a GPU, ~62 min.
  ⚠️ Check `free` first.
- ⚠️ **The `strain` coupling is bit-exact against the goldens by CONSTRUCTION, not by measurement**:
  strain accrues on `OnEnter(AppState::Debrief)` and the harness never enters an `AppState`, so it is
  `0.0` in every rollout and the floor's inner guard never fires. The day the harness runs campaigns
  that stops being true, and the re-pin will have a reason.
- **Look at it.** `src/devshot.rs` + the `screenshots` skill. Keystroke injection is blocked here, so
  drive the camera with a **temporary tour system** and revert it — that is how every visual defect
  above was found, and how the cutaway was judged at the detents where a wing *used to* be hidden.
