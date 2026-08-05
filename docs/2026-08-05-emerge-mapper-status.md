# emerge-mapper — where it stands (2026-08-05)

Supersedes the "do these first" list in `docs/2026-08-04-emerge-mapper-next.md` §2, all of which is
now done. That document's **§4 "Traps this work paid for"** is still worth reading and still accurate;
so is `docs/2026-08-04-emerge-mapper-handoff.md`, which remains the no-prior-context orientation.

Branch **`feat/emerge-lattice`**, 23 commits off `bc7a92a`, **not pushed**.

---

## 1. Where it stands

| suite | result |
|---|---|
| `cargo test --lib` | 975 passed, 1 ignored |
| `cargo test -p emerge-core` | 257 + 2 + 4 |
| `cargo test -p emerge-mapper` | 40 |
| `cargo check -p emerge-core -p emerge-mapper -p fvs --all-targets` | **zero warnings** |

Known-failing, none of it caused here: `tests/lore_canon.rs` (fails on the user's untracked
`docs/lore/2026-08-01-scp-gear.md`) and two of `tests/replay.rs`
(`authored_world_config_override_is_a_noop`, `deterministic_core_is_bit_identical_across_many_builds`).
The gates that matter — `deterministic_core_is_bit_identical` and the
`a0_fvs_j6_mutant3_on_world_0x5c09191` golden — are green, so `snapshot_hash` has not moved.

**There are still zero authored `SubCell`s in the repo**, but the reason has changed. It was that
authoring was meaningless (incommensurable lattices), impractical (hand-marking), and destructive
(`write_library` corrupting the kit). All three are fixed. What is left is that nobody has sat down
and done it — and `#9` below is the last thing that would make doing it pleasant.

---

## 2. What changed, and the three things that surprised

### The lattice is now real

`Subgrid` lost `div`. Divisions are derived from a piece's own size times one project number,
`policy::divisions` (shipped at 1, a 0.5 m subunit). Merrell & Manocha 2009 §4.4–4.5 is the source and
names this exact remedy; `descriptor::Subgrid`'s doc carries the argument. An edge token on a 3 m wall
now means the same thing as one on a 0.5 m chair, which it could not before.

`align.front` names a `Face` (N/E/S/W) rather than an arbitrary yaw. `align.rotate` is a new authored
quarter-turn for a mesh exported the wrong way up, **baked into `extent` at import** so no reader of
`extent` needs to know it exists.

### Three things the code said, and the assets disagreed

1. **A wall and a doorway cannot meet.** The kit reaches 2.40 m by stacking a 2.00 m doorway under a
   0.40 m header, so their faces are 5 rows and 4 rows and `may_abut` refuses them. That refusal is
   *correct and new* — under the old 3×3×3 they "matched", comparing a doorway's 0.67 m band against a
   wall's 0.80 m band. Pinned in `the_site_kit_derives_the_lattices_its_architecture_implies`.
2. **The same problem exists horizontally.** A 3 m wall meeting a 1 m doorway presents faces of
   different lengths, so they refuse rather than comparing the part that overlaps. **These two are one
   question and want deciding together** — see §4.
3. **Vertex occupancy was wrong for architecture.** A wall slab has vertices only at its corners, so it
   scanned 4 cells of 10. Now rasterised (Akenine-Möller 2001 triangle-box SAT) and a wall is solid all
   the way up. Kept honest by a test that fails if a doorway ever comes back full.

### Editor

Screen-aligned WASD panning (it had claimed screen axes since it was written and moved along world
axes); the map vocabulary under one hand (`X` removes, `V` aims straight); `Cmd`/`Ctrl` places freely
off the grid (XZ only — Y comes from `mount`); the aim keys repeat while held at 150 ms; candidates
scan themselves on selection and the chip is `rescan mesh`.

---

## 3. Review findings

Six of the eight from the max-effort review are now fixed: **#1** (`write_library` baking the policy
layer into the measurements file), **#3** (arrows walking filtered-out tiles), **#4** (`faults` pairing
by centre cell), **#6** (`PreviewOf` on a non-unique id), **#7** (the O(n²) recheck), **#8**
(`Subgrid::default` as an unset sentinel). Two remain:

| # | What | Where | Why it is still open |
|---|---|---|---|
| 2 | A rig missing from `rigs.ron` panics an unrelated system at Startup | `src/crab/setup.rs:32` + 5 more | Needs a decision about where "the rigs the game requires" is written down |
| 5 | Focus can move between opening a text field and committing it, redirecting the edit | `tiles.rs` | Fix is to capture the target at field-open; touches all four fields and wants GUI verification |

---

## 4. Decisions owed

- **The seam question, both axes.** Should `may_abut` compare the *overlapping part* of two faces
  rather than requiring equal lengths — and should a vertical seam consider the doorway-plus-header
  stack? Deliberately deferred until real tokens exist, which is still the right call, but the numbers
  are real now and the question is answerable.
- **`FVS-Q-9` and `FVS-Q-10`.** `BACKLOG.md` defers both on the premise that *"all 42 descriptors have
  empty lattices"*, so `solid` cannot be a single unconditional rule without a bounding-box fallback.
  **That premise is now false** — divisions are derived and one button authors a lattice. The backlog
  entry's reasoning is stale; re-read it before answering.
- **`id: "is"`** in `assets/emerge/library.ron`, pointing at `characters/cipher_field.glb`. Delete, or
  rename and move out of the furniture library.
- **`EMERGE_WRITE_SITE=1`** appears in three files' notes and exists nowhere in the tree. Restore the
  generator or fix the prose.
- **`docs/lore/2026-08-01-scp-gear.md`** is untracked and fails `lore_canon`. Track it, or teach the
  test about it, and the suite goes fully green.

---

## 5. What is verified on screen, and what is not

Driven and screenshotted, so trust it: the divisions readout and its arithmetic; the rotate chips
(`N`/`O`/`P`) turning a piece and reshaping its lattice; `rescan mesh` marking cells; the shortcuts
overlay showing `X` = removal mode and `V` = aim straight; the aim keys repeating (240° from a
known-zero in two seconds, ~137 ms a step); `floor_grate` placing at `(5.50, 3.50)` snapped and
`(7.37, 3.57) free` with the modifier held; and the filter blur — the sequence that used to do
nothing now places.

**Not seen, only tested:** screen-aligned panning (covered at all four detents by projecting motion
into the camera basis), the auto-scan-on-selection rule, and the two fixes above (#3, #6).

---

## 6. Traps this session paid for, beyond the earlier doc's §4

- **Verify frontmost before believing *any* driven test, and abort if it is not.** VS Code took focus
  mid-sequence and a click landed there as a stray `vanity` placement I briefly read as signal. The
  check now aborts rather than reporting.
- **Establish a baseline; do not assume one.** I measured the key repeat as "3 steps" and flagged it as
  suspect. It was 6 steps from an unverified starting yaw of 315°. Pressing `X` to zero the aim first
  made the measurement trivial and correct. This is the earlier doc's "measure, don't squint" trap
  wearing a different hat.
- **A shared epsilon cancels itself.** Nudging both edges of a span the same way made every piece a
  cell too wide, so everything abutted everything. Near edge rounds up, far edge rounds down.
- **`fvs` is the launcher.** `cargo fvs edit <map> --kit site --fullscreen`. The alias is opt-in per
  machine (`.cargo/config.toml` is deliberately uncommitted — a machine-specific `target-dir` in it
  once broke CI); `cargo install --path crates/fvs` gives a global `fvs` that still rebuilds the editor
  from live source, because it only shells out to `cargo run`.
