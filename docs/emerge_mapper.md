# emerge-mapper — current design and features

A review of the editor as it stands, written from the source at `crates/emerge-mapper/` (21.6k lines) and its engine-free core `crates/emerge-core/` (20.2k lines). Counts and behaviour here were read out of the code, not recalled: 62 key bindings, 97 unit tests plus 21 headless integration tests, all green.

Design history lives in `docs/2026-08-0*-emerge-mapper-*.md`. This document is the *current state*, not the plan.

---

## 1. What it is

A standalone world-building editor. It opens a **project directory**, not a game:

```
emerge-mapper [project-dir] [map-name] [--kit <name>]
```

- `assets/emerge/vocab.ron` — what tokens exist
- `assets/emerge/library.ron` — what can be placed (**75** descriptors shipped; the `site` kit adds **45** more)
- `assets/emerge/<name>.map.ron` — the map being authored
- `assets/emerge/rigs.ron` — what the game plays, for the animation bench

The second argument is a **name, not a filename**: `emerge-mapper . site_67` opens `assets/emerge/site_67.map.ron`. Names are *forced* into snake_case rather than validated, so there is no path through the program where the filesystem and the schema disagree about what a map is called.

`--kit` selects a library + policy layer under `assets/emerge/`; the default is the furniture set, `--kit site` the architectural one whose walls, corners, doorways and pipes are what edge tokens exist for.

It writes `emerge_core::map::Map` — a schema with **no engine in it**. `crates/emerge-core/tests/engine_free.rs` fails the build if that stops being true, which is what lets the same schema, solvers and validation run in the game, in the headless search, and in the editor without the three agreeing on a renderer.

### Why a separate binary

`sim_harness.rs` is the cited precedent: same plugin graph, different entry point, *"not a second code path."* An editor welded into the game inherits its title screen, save system, camera rules and notion of a level — which is exactly how the older F7 Site editor ended up able to edit only one hub.

---

## 2. Architecture

```
emerge-mapper (bin + lib)          the application: panels, tools, tabs
   ├── emerge-core                 engine-free: schema, solvers, WFC, import, clips
   ├── emerge-bevy                 spawn_descriptor — the ONE spawner
   ├── emerge-anim                 the game's real pose blender
   └── bevy_devshot                the shared screenshot rig
```

**Borrowed, never copied.** The editor spawns pieces through `emerge_bevy::spawn_descriptor`, previews rigs through the real `emerge-anim` blender, and captures through `bevy_devshot`. The failure this prevents is concrete and already happened once: `bake.rs` and `site_editor::source_map` independently grew the same RON writer and drifted, so a map looked one way in the editor and another in the game.

**Lib + bin split.** `main.rs` holds only argument parsing and the app; everything else is a library so `tests/` can link against it. This exists because a bin-only crate gave integration tests nothing to drive — the only way to learn whether a system was registered was to run the editor and look at it, which meant taking over the machine's keyboard and display.

`emerge-core` supplies: `adjacency`, `clips`, `descriptor`, `gait`, `geom`, `glb`, `grammar`, `grid`, `import`, `library`, `map`, `naming`, `placement`, `plot`, `policy`, `rig_check`, `rigs`, `rng`, `ron_surgery`, `smart`, `stack`, `vocab`, `wfc`.

---

## 3. The four tabs

### 3.1 Map — the editing loop

**The ghost is the contract.** Everything goes through a preview standing exactly where the piece will land — snapped, aimed, and lifted onto its host the same way the real placement will be. The rule is stated as: a preview drawn somewhere the piece will *not* end up is *"worse than no preview, because it is a promise the game then breaks."*

**Aiming happens before placing.** `Z`/`C` turn the **brush**; `V` returns it to straight (the only absolute among the aim keys, because turning is relative and an author tapping `Z` is not counting). `R`/`T`/`Y`/`U` turn and tip *the selected piece*. Binding rotation to the selection made it feel broken — placing selects, so the next keypress turned the piece you had just put down while the ghost, the only thing showing a facing, sat still.

Four tools, one at a time (`EditorState::tool`):

| Tool | Behaviour |
|---|---|
| **Place** | Click puts the armed piece down. The default every other tool returns to. |
| **Remove** | Click deletes a piece; drag a box deletes everything inside it. |
| **Move** | Click picks up, click again puts down. |
| **Clone** | Drag a box to copy a set; click to stamp it, as many times as held. |

Other map verbs:

- **`F` flood fill** — spreads from the cell under the cursor, stopping at anything already placed and at the map's edge. It *refuses* outside `Map::bounds` rather than clamping into them, because clamping would place a piece where the author did not point.
- **`G` generate** — learns the grammar from what is already placed and fills free cells with more of *that* arrangement (`emerge_core::grammar`, WFC). Mixed-initiative: rules come from the author's own corner of the room, not from an adjacency schema they had to write first. The id counter advances every solve, so pressing `G` twice offers a *different* arrangement — a generator you cannot ask again is one you have to undo to disagree with.
- **`O` pin** — marks a placement as deliberately kept. `Placed::owned_because` is **a reason, never a bool**: a bool lets "I could not be bothered" and "this is the cell block's only entrance" look identical in a diff, so pinning asks for the reason.
- **`H`** targets the stack under the cursor; **`[`/`]`** lift and lower by one rung of the project's
  ladder — the same rung a nudge across moves by, so "up one" and "over one" are the same distance.
- **Undo/redo**, with redo cleared by any new edit (undo addresses rows by index, so replaying across an intervening edit would put pieces back where positions now mean something else). Not cleared by changing tabs — the Tiles tab keeps its own pair.

`EditorState::brush` is `Option<usize>`, so **nothing armed** is a real state. It used to be a bare `usize`, meaning index 0 was always armed, there was nothing for `Esc` to clear, and you could not put the cursor over the map without a piece following it.

### 3.2 Tiles — bringing meshes in and saying what they are

`emerge_core::import` measures; this tab is where an author reads the measurement, assigns an id, picks a layer, tags it, and accepts it into the library.

- **The scan is lazy and reports its size.** Scanning at launch would make every session pay for a mode most never open, so it happens on first Tab and the panel says what it found — *"a list of 319 with no count is a list nobody trusts they have seen the end of."*

  ⚠️ **The in-source counts here have drifted and are worth re-checking.** `tiles.rs` says *"360 meshes and 41 are in the library, so the candidate list is around 319"*; `vlm.rs` says *"131 library entries and ~319 unlabeled candidates"*. The file actually holds **75** descriptors. Three different numbers for one quantity is exactly the drift the key census was built to stop, in a place the census does not reach.
- **Findings ship with their fix.** Every `import::Finding` with an obvious remedy carries it, and the panel shows both.
- **A subgrid cell editor** — `T`/`F`/`G`/`H` walk the cell cursor, `[`/`]` change layer, `Z`/`X`/`C`/`V` set solid / edge / anchor / clear. `B` rescans solidity from the mesh; `N`/`O`/`P` turn the mesh on each axis.
- **Edge tokens** are what `emerge_core::adjacency` reads: it walks a finished map and reports every abutting pair whose facing tokens disagree. It *reports* rather than generates.

**VLM labelling** (`vlm.rs`, `labels.rs`, `label_booth.rs`) is the notable recent addition. The judgement fields — `kind`/`effects`/`look` tags, `offers.surfaces`, `mount`, the `note` prose, `placement.rooms`/`group` — are hand-authored across hundreds of entries. A vision model proposes them from two 640 px booth renders (three-quarter front and rear), under `docs/llm_rule_authoring.md`'s guardrails:

- **Dev-time only.** The editor is never shipped, so "stripped from release" holds by construction.
- **Closed vocabulary.** A suggestion carrying a token the vocabulary does not implement is rejected **whole**, naming the axis. `library.resolve` inside the commit door stays the final gate on the exact bytes written.
- Configured via `EMERGE_VLM_KEY`/`URL`/`MODEL` from the environment or a gitignored `.env`; defaults to the SSH-tunnel form for the local `bmb` model, with Ollama Cloud a pure config flip.
- One `AsyncComputeTaskPool` task per item, with stale guards — selection moves, rescans drop candidates, re-imports reuse ids while a request is in flight, and a result whose target no longer matches is dropped.

### 3.3 Animation bench

`emerge_core::clips` measures a GLB, `rigs.ron` records what the game plays, and this tab shows **both at once**. It closes a gap `docs/animation.md` names explicitly: getting a gait's `(duration, phase_offset, cycle_distance)` was *"a manual offline step, not a repo tool"*, and `staff_anim.rs` calls that measuring *"the largest hidden cost in animating a new character."*

- **Every row shows declared beside measured.** A manifest agreeing with itself proves nothing; the failure it catches is an artist re-exporting a rig whose timings moved.
- **The staged figure plays it faithfully** — every clip resident on one `AnimationPlayer`, one `AnimationGraph` from the same `emerge_anim::rigs::build` the game uses, weights and one shared phase moved by the same `apply_pose_blenders`. No transitions, nothing rewound. What a "play clip N" preview cannot show is whether the *set* reads together.
- **Diagnostic plots** answer *where* in the cycle, not just by how much. Because the runtime has no transitions, a wrong duration *"doesn't glitch, it skates"* — a low-amplitude error smeared across the cycle. Every curve is drawn against the shared phase, sampled at `wrap01(phi + declared_offset)`, so correct offsets align vertically and a wrong one is a visibly displaced trough. A top-down trace draws measured travel at the declared cycle distance.
- **It notices re-exports** rather than waiting to be pointed at one: `RigWatch` polls GLB mtimes, counting a change only when the new mtime is seen **equal on two consecutive polls**, so an exporter mid-write never triggers a measurement of half a file. Re-measures one rig per frame.
- **The cache is persisted** under `target/` so the STALE badge is truthful at startup. An entry survives only if the rig still exists, the manifest `Rig` equals the one measured under, and the GLB's bytes hash to the recorded fingerprint.
- `Enter` adopts measured values into `rigs.ron`; `Space` plays/scrubs; `←`/`→` scrub phase (Shift for fine); `G` ghosts measured over declared; `V` cycles camera presets.

---

### 3.4 Compose — reusable groups, and what they present

A **composition** is a named set of placements a map holds a *reference* to, not a copy of. It is
`emerge_core::composition`, and the whole reason the reference model was chosen over flattening at save
time is that editing a group changes every map that stamped it.

- **The map stores `stamps`, never the rows.** `Map::stamps` is a list of `Stamped { id, of, at, yaw,
  overrides }`; `composition::expand` is the one function that turns it into `Placed` rows, and it is
  called at render, at validation, and at game load — never written back. The status line says so:
  a map with one stamped group reads *"0 placed, 1 stamped"*.
- **Overrides are sparse, reasoned, and encapsulated.** The strength order is fixed and tested —
  `library.ron` < `project.ron` patch < `Member` patch < `Stamped::Override` — and the two authored
  layers arrive merged into the one `Placed::patch` the map already had, so there stays exactly one
  place a patch meets a descriptor. An override names a member by **id**, must carry a `because`, and
  may not reach into a nested group: that is USD's encapsulation rule, and Unity's evaporating nested
  overrides are the cautionary tale.
- **The interface is derived, never authored.** A `Bounded` group's edge tokens are read off its
  members' boundary cells. Where two members disagree about a face it reports a **fault** naming both
  and refuses to be a solver prototype — `adjacency::faults`' shape, because silently picking a winner
  is how a tool ends up modelling something other than what the author has in their head.
- **STALE is a verifying trace, and `UNRECORDED` is not STALE.** Each member records the fingerprint
  (`glb::fnv1a`, toolchain-stable) of the interface it was built against, and a mismatch is shown with
  both numbers. `of_fingerprint` is an `Option` on purpose: a bare `u64` defaulting to zero made every
  hand-written group read *"STALE — 3 members changed"* against `recorded 0x0000000000000000`, which is
  a sentence about drift that never happened.
- **Affordances travel.** A group carries its own `locations`; stamping repoints their `props` at the
  rows it produced, so two stamps are two independent affordances.

Caps refuse and name rather than truncate: `MAX_COMPOSITION_DEPTH` (8) and `MAX_RESOLVED_MEMBERS` (256).

**What this tab does not do yet:** it reads groups, it does not author them. `compositions.ron` is
hand-written, which is deliberate — the schema and its expander shipped before the file did, and the
file is proof the format is hand-authorable. Building a group from a map selection, break-link, and
editing interactions are the open verbs.

## 4. Cross-cutting design

**The key census.** All 62 bindings live in one table in `keys.rs`, with a collision test. Nothing else in the crate may name a `KeyCode` for an action. This is the direct remedy for `docs/ui.md` §3.5, where the game's key allocation lived in *five* hand-written prose censuses, *"all five of which had drifted to the same wrong answer."* Contexts (`Global`, `Map`, `Tiles`, `Anim`, `Typing`) make legal collisions explicit — `M` means mount in Tiles and nothing on the map — because a flat uniqueness rule *"would force a worse binding"*. `Typing` overlaps everything and suppresses everything.

**Shared chrome.** Every tab is the same two shapes: a controls panel down one side, a list down the other. They were written twice and were already drifting — the census key-row block appeared in `editor.rs` and `tiles.rs` byte-for-byte *including its five-line comment*, rendering in two different pairs of colours nobody chose, from a colour table declared twice with duplicate entries under different names. A third tab now costs ~30 lines instead of ~110, and cannot come out looking like a different program.

**Filtering, not re-ranking.** Typing narrows a list; surviving rows keep their exact order, so what an author learned about where things sit stays true. Nothing is re-ranked, ever. The filter text persists when focus leaves — *"a filter you have to retype every time you click a row is a filter nobody uses twice."*

**The thumbnail booth.** One baked render per descriptor, staged 4 km from the origin so the booth camera sees the subject and nothing else (no `RenderLayers`, which is where layer masking usually goes wrong on GLB scene children). One camera walks the library and then **despawns**, because a live render target costs a full pass every frame forever.

**The camera** mirrors the game's: WASD pans along screen axes, wheel zooms, Q/E rotate in quarter detents, orthographic so a piece reads as it will in the game. The ground is Bevy's own `dev_tools::infinite_grid`.

**Refuses rather than opening empty.** Every load failure is fatal and names the file and reason. An editor that comes up with an empty palette looks exactly like an editor whose project has no assets.

---

## 5. How it gets verified

Two sanctioned paths, neither touching the machine's real input devices:

1. **Headless** — `harness::build_headless` builds the same plugin graph with `WgpuSettings { backends: None }` and no window, so `tests/headless.rs` steps frames and asserts. This answers the one question a unit test cannot: *does this app survive its first frame* — which has teeth in Bevy 0.19, where a missing `Res<T>` **panics its system** and every run condition is evaluated with no short-circuit. Several tests exist purely to assert that a plugin registers the resources its systems take.
2. **Sentinel files** — `devshot.rs` reads `drive.request` (whitespace-separated verbs: `tiles`, `map`, `anim`, `compose`, `arm`, `stamp`, `down`, `up`) applied through the same resources the key handlers write, with `bevy_devshot` reading `screenshot.request` beside it. A capture script reproduces an author's exact steps in a real window with nobody at the keyboard. Three Site editor bugs were invisible to a green suite and visible only in a measured frame.

`EMERGE_FULLSCREEN=1` forces borderless fullscreen, because the virtual pointer used for automated checks is **absolute over the whole output** — under a tiling WM the editor gets an arbitrary slot and clicks aimed at the palette land on the desktop.

**Rules the crate holds itself to:** no `unwrap()` (everything is user input or a file on disk); one path per feature, no fallbacks or stubs; staged edits go through the commit door and nothing writes a degraded result behind it.

---

## 6. Observations

Things worth knowing, from reading rather than running:

- **`tiles.rs` is 5,707 lines and `editor.rs` is 4,612** — together over half the crate. The chrome extraction addressed panel duplication, but these two files still carry tool state, input handling, panel building and commit logic in one place each. They are the obvious candidates if a fourth tab is ever added.
- **The animation bench is the most self-contained subsystem** (five modules, its own cache, its own watcher) and reads as the most finished.
- **The VLM path is the newest and the most asynchronous**, and it is the only place in the crate that does real concurrency — the rest is deliberately frame-driven, no-threads ECS. Its stale-guard discipline is what makes that safe.
- **`emerge-core` is the reusable asset here**, not the editor. It is engine-free by enforced test, and it is what a second game would take.
- The crate is **not a dependency of the game**. The game reads what it writes via `src/emerge_map.rs`, and `FVS_EMERGE_MAP=<name> FVS_EMERGE_MAP_AT=x,z` puts an authored map in the running game.
