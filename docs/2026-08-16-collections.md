# Separating the project from the kit

A design for making kits, tiles and maps reusable across each other. Written after measuring the
current behaviour rather than from memory; every number below was read out of the code or off disk
on 2026-08-16.

**Status: built.** All six stages landed on 2026-08-16, the day this was written. The blast radius is
the schema and the game's loader, so it wanted agreement before code and got it. Measured after:
**2,550 passed, 57 failed, and every one of the 57 names the missing site kit** (FVS-R-39) — the
split introduced none of them.

---

## 1. What is there now, and why "kit" means three things

`policy::layered_library` reads **exactly one directory** (`emerge-core/src/policy.rs:62`). From it:
`library.ron` and `project.ron`, both required — *"A missing `project.ron` could reasonably mean 'no
rules', and that is exactly the reasoning that grows a second code path"* (`policy.rs:58`) — and
`compositions.ron`, optional. Maps sit beside them, and `Project::open` resolves one by name in the
same directory (`emerge-mapper/src/project.rs:168`).

So a kit directory holds a library, a policy, a tile set and every map. The editor opens one; `--kit`
selects one; `Project` carries **one of each** (`project.rs:31-96`). The word "kit" is doing the work
of **project**.

Two things already sit outside that, and both were deliberate:

- **The vocabulary.** `assets/emerge/vocab.ron`, at the root. *"The vocabulary is not per-kit and
  stays at the root: tokens are what this project means, and a kit that could redefine them would be
  a second vocabulary to keep in step"* (`project.rs:41`).
- **The mesh pool.** `assets/derived.json` content-addresses 418 Ozea meshes; a kit labels a subset
  of them and owns none.

And one thing sits outside in a way nothing can see: **the game.** `src/site/kit.rs:38` holds
`assets/emerge/site` in a `&'static str`, `:43` holds `assets/emerge/site_greybox`, and
`assets/site/kit_ozea.ron` names all 45 ids. `SitePlugin::build` panics without them
(`src/site/mod.rs:153`; a budgeted panic, `tests/panic_budget.rs:64`).

### The measurement that decides the model

`site/` and `site_greybox/` define the **identical 45 ids** — `site/floor`, `site/wall`,
`site/wall_corner` … `site/wall_window`. Not similar sets: the same set, and
`assets/site/kit_ozea.ron` and `kit_greybox.ron` name the same ids too, which is what
`the_site_kit_is_swappable_by_authoring_one_project` (`src/site/kit.rs:604`) exists to prove.

They are not two kits with two namespaces. They are **two implementations of one interface.**

That single fact answers most of the questions the flip raises, and it answers them differently from
the obvious guesses. It is the spine of §3.

---

## 2. What the schema already answers

Six problems worth raising against a reusable-collections design are already built and tested.
Naming them so nothing here re-decides them:

| Concern | Already answered | Where |
|---|---|---|
| Nesting cycles, runaway expansion | `MAX_COMPOSITION_DEPTH = 8`, `MAX_RESOLVED_MEMBERS = 256`, cycle refusal | `composition.rs:75`, `:82`, `:539` |
| One stamped instance needs to differ | `Override { member, patch, because }` — and overriding a **nested** composition's internals is refused | `composition.rs:305`, `:1340` |
| Break-link | `Stamped::owned` + `owned_because` | `composition.rs:270` |
| Staleness down a dependency chain | `of_fingerprint` on `Member` *and* `Stamped`, plus `Freshness` / `Stale` / `stale_members` | `composition.rs:146`, `:1185`, `:1212` |
| The vocabulary must be one table | Already a project-root singleton, with the argument written at the field | `project.rs:41`, `vocab.rs:37` |
| The alphabet cannot absorb a cross product | `Body::Slot` — a declared hole, deliberately dropped from `interface`, so it costs the solver nothing | `composition.rs:198`, `:1490` |

Two of those are literature verbatim. The `Override` refusal is USD's encapsulation rule — a child's
internal structure is immutable from the parent, which is *"what makes the LIVRPS composition
algorithm explainable … because it is properly recursive"*. The `of_fingerprint` pair is a verifying
trace with early cutoff. Both were chosen on the record in
`docs/research/2026-08-08-editor-model-design-guide.md` §I4 and §I5, and both are now shipped.

**A stamp is already a reference and already one-way.** *"Expanded by `composition::expand` at
render, at validation and at load, and **never** written back into `placements`. That is what makes
editing a composition change every map that stamped it"* (`map.rs:109`). The reuse this design is
for is half-built; what is missing is that it cannot cross a directory.

---

## 3. The entity model

```
Mesh pool     files, content-addressed          assets/derived.json     shared by everything
Vocabulary    the token tables; bit = position  assets/emerge/vocab.ron project singleton
Namespace     a set of piece ids — INTERFACE    site/*                  what a map depends on
Kit           one implementation of it — SKIN   a directory             bound, swappable
Tileset       compositions over those ids       compositions            reusable across maps
Map           placements + stamps + bounds      a file                  the leaf
```

### References point one way down that ladder, never up and never sideways

Smits, Konat & Visser 2020 (`10.48550/arXiv.2002.06183`) is the argument, and it is about exactly
this shape of problem. Their history: independent compilation was fast and unsafe; **Mesa** made it
safe by emitting a *symbol file* — *"the result includes a symbol file that can be used during the
compilation of other modules that depend on that module."* And their motivating failure:

> *"Compilation of Stratego's open extensibility requires the integration of definitions from
> multiple modules, precluding a simple separate compilation model."*

The rule that falls out: **a summary crosses the boundary, not the module.** The derived `Interface`
(`composition.rs:1475`) is that symbol file — *"Read off the members, never authored. There is no
field anywhere for a hand-written interface"* (`:440`). A map depends on a composition's interface,
not on its members.

That works only while the graph stays acyclic and one-directional. Stratego lost separate
compilation because a definition could span modules (857 strategies with definitions in more than
one). The analogue here is a **kit** ever depending on a **composition** — a descriptor whose body
was a group. Nothing does today, and nothing may. `Body::Descriptor { patch }` and
`Override { patch }` both patch *downward*, which is the direction that keeps working.

### The dependency edge is the namespace in the id

`naming::is_id` (`naming.rs:157`) already splits on `/` per segment, specifically so
`site/wall_corner` parses:

> *"The separator is a namespace, not decoration: the site kit ships `site/wall_corner`, and the two
> halves mean 'which kit' and 'which piece'."*

with `a_kit_namespaced_id_is_a_valid_id` (`naming.rs:181`) as its test. Nothing else in the codebase
uses it. Making ids consistently qualified buys four things at once:

- **A map's dependency set is computed, never stored** — the namespaces in `placements`, unioned with
  the transitive closure through `stamps` → `Body::Composition` → `Body::Descriptor`. There is no
  declared list to drift from the content.
- **The kit tick-boxes cannot lie**, because they are not data. They filter a palette; a tick can
  never break a map and an untick can never strand one. This is the same discipline
  `tests/census_is_the_one_counter.rs` already enforces for counts, applied to dependencies.
- **A cross-kit id "collision" is a variant, not an error** — which is the only reading consistent
  with §1's measurement.
- **The game's dependency becomes machine-visible.** `assets/site/kit_ozea.ron` names `site/floor`,
  so a scan finds it. Today that dependency is a string constant in Rust and no scan of the editor's
  own content could ever see it.

That last point is the sharpest problem this design found, and it is worth stating on its own:
**the editor cannot see all of its dependents.** A content scan finds maps and compositions. It
cannot read `src/`.

### Binding is the skin swap, generalized

A namespace is provided by one or more directories. Which one a project uses is a **binding** — and
the existing `--kit` flag is exactly that binding, restricted to one at a time. Generalizing it from
*"open this directory"* to *"resolve `site/*` here and `lab/*` there"* is the whole of stage 4.

The `Policy::apply` rule survives unchanged and constrains this: **a patch that matches nothing is an
error** (`policy.rs:398`), deliberately, because *"a rule that silently applies to nothing is how a
policy rots."* So a kit's patches may only name ids in its own namespace. Otherwise load order starts
deciding whether a project opens.

---

## 4. The four decisions

**One.** *Compositions get their own collection.* They are the thing meant to be reused across maps,
and a map-scoped tile is not reusable. `Compositions` is already one file with a version
(`composition.rs:1744`) and one writer (`project.rs:326`, held by `tests/compose_is_read_only.rs`),
so the collection exists — it is only mis-located.

**Two.** *A namespace is an interface; a directory is a skin.* §1's measurement. The alternative —
one unique namespace per kit, `site_greybox/*` renamed — costs the re-skin fixture and the eight
tests that assert both kits name the same pieces, and buys nothing the binding does not already give.

**Three.** *Kit tick-boxes are a palette filter; the dependency set is derived from content.* The
declared alternative has a failure mode with no good answer: tick kits A and B, stamp a composition
drawing from C. Either the checkbox shows a tick nobody set, or the tick list is not the dependency
list. Deriving it makes the question unaskable.

**Four.** *The deleted kits stay deleted.* `assets/emerge/site/`, `site_greybox/` and `site_v2/` are
unstaged deletions in the working tree and are not being restored. §7 carries what that costs.

---

## 5. What the corpus says about the ceiling

`MAX_PROTOTYPES = 32`, *"because `collapse_grid` packs a domain into a `u32`"* (`grammar.rs:74`), and
`constraints::AMO_PAIRWISE_MAX` encodes exactly-one **pairwise**, so clause count is quadratic in it
(`constraints.rs:125`). All three grammar builders push four turns per source item
(`grammar.rs:1657`, `:1801`), deduping by face signature within one item.

Pooling compositions across kits makes that budget shared, and the naive reading — *"we will run out
of slots"* — is the wrong worry. Nie, Zheng, Zhuang & Song 2023 (`10.48550/arXiv.2308.07307`) put
tileset size `d` in `O(d^(M×N) + (M×N)²d³)` and then give the requirement that actually binds. A
**complete** tileset needs `|T| ≥ |E_NS|²·|E_WE|²`; a **sub-complete** one needs only
`|T| ≥ max{|E_NS|², |E_WE|²}` — and under a nested solve a sub-complete tileset is provably
backtrack-free, *"guaranteed to return an accepted solution."*

So the quantity to watch is **edge-vocabulary richness**, not collection size. 32 ÷ 4 turns ≈ 8
authored tiles, which supports two tokens per axis comfortably and three not at all (3² = 9 > 8), and
the 3-D case (`E_UD` as well) is stricter still. Measured today: the `edge` axis in
`assets/emerge/vocab.ron` has **exactly one token** (`wall`), and `grammar::declared`'s own doc
records *"8 tokened descriptors × 4 turns is 32 candidates, and 11 of them are distinct"*
(`grammar.rs:1745`). There is headroom, and it is bounded by a number nobody is currently shown.

`Body::Slot` is the escape hatch already built for this, and its doc already carries the arithmetic:
*"one tile with a four-way slot would cost sixteen of them once turned"* (`composition.rs:215`).

**Two other citations, unchanged in force.** Lagae & Dutré's *colored edges versus colored corners*
(`10.1145/1183287.1183296`) — *"Wang tiles do not directly constrain their diagonal neighbors …
commonly known as the corner problem"* — is still the open schema question, and it is **now in the
corpus**; `docs/2026-08-09-unified-composition.md` §3 records it as un-downloadable and that line is
stale. Lai, Leymarie & Latham (`10.1145/3402942.3402946`) name the pillar this design is most at risk
of breaking — *respect existing work processes*, which asks *"exactly where a new tool fits into the
workflow, who provides data for it, where the generated content goes next"*; that is §6's whole
point. Códices et al. (`10.1109/access.2022.3168832`) argue for connection rules a designer can read
directly, which is why the prototype budget should be visible rather than only enforced.

---

## 6. Blast radius

| Touches | What |
|---|---|
| `emerge-core/src/policy.rs` | `layered_library` resolves several directories, not one |
| `emerge-core/src/naming.rs` | qualified ids become the rule rather than the option |
| `emerge-core/src/composition.rs` | composition ids namespaced; `Body::Composition` re-pointed |
| `emerge-mapper/src/project.rs` | `Project` stops being one-of-each; `emerge_dir` stops being an identity |
| `emerge-mapper/src/chooser.rs` | one door per entity kind; `launch_args` shape; `Catalog` |
| `emerge-mapper/src/build.rs:1914` | the namespace inference retires |
| `src/site/kit.rs`, `src/emerge_map.rs` | the game's loader, in the same change — no compatibility branch |
| `crates/emerge-mapper/tests/fixtures/` | a second kit, which no fixture can currently make |
| `.gitignore:78` | `assets/emerge/*.map.ron` is ignored; kit-subdirectory maps are tracked |
| **every shipped RON**, and the goldens | |

---

## 7. Staging

Each step is a green suite, except where noted.

1. **A second-kit `Fixture`.** `crates/emerge-mapper/tests/fixtures/mod.rs:107` makes only
   `dir/assets/emerge`, so **nothing in the suite has ever had two kits** — multi-kit behaviour is
   exercised by exactly one asset-contract test, `the_chooser_sees_the_shipped_kits`
   (`tests/headless.rs:8189`), which hardcodes four kit names and "exactly one kit has
   `flag.is_none()`". No schema change; nothing after this can be verified without it.
2. **Namespace enforcement.** A kit's namespace is read from its library, not inferred from the first
   descriptor — `build.rs:1914` currently does the latter, which is why `site_v2/`, whose pieces were
   named `site/*`, minted `site/tile_n`. A library mixing namespaces is refused at load.
3. **The derived dependency scan.** Computed on demand, never stored, on the model
   `census_is_the_one_counter.rs` already ratchets. Wired into delete-refusal. **Shipped early — §8.**
4. **Binding.** The project maps namespace → directory; `layered_library` grows a multi-directory
   form; `face_bands` and `snap_divisor` move up. *This is where a golden may move.*
5. **Compositions out of the kit.** Their own collection, ids namespaced, `stamps` and
   `Body::Composition` re-pointed. **One-shot migration, no shim** — a compatibility reader would be
   a second, permanent path to one schema, which is the rule this repo does not bend.
6. **Maps out of the kit.** Chooser doors, `launch_args`, `.gitignore`.

### `face_bands` and `snap_divisor` are the ones to watch

Both live in per-kit policy (`policy.rs:187`, `:217`) and both describe a **lattice**. A map has one
lattice. Two bound kits disagreeing about either has no local answer, so under the flip both belong
to the map or the project. `face_bands` in particular was renamed from `divisions` to make exactly
this kind of confusion harder — *"the rename is the point"* (`policy.rs:193`) — and putting a second
claimant on the same number would undo it.

### `site/*` has no provider, and green tests gate on it

With the kits deleted, `SitePlugin::build` panics at startup and `cargo test --workspace` is red: 51
test functions across `src/site/{kit,layout,pieces,people,smart}.rs`, `tests/site_descriptors.rs`,
`tests/site_editor.rs`, `tests/mesh_measurement.rs` and `tests/importer_against_real_meshes.rs`, plus
every harness test that builds the sim app (`SitePlugin` is in `src/sim_harness.rs:448`). Three
`emerge-core` examples hardcode the same path.

Two ways out, and they are not the same size: **re-author `site/*` against the ozea meshes**, which
is what `site_v2` was created for and what the Meshes tab and the VLM labeler exist to make tractable;
or **park the Site hub**, which means `src/site/` and its 51 tests go behind a decision rather than a
missing file. Neither is a `git checkout`, and this document does not choose between them.

---

## 8. What shipped ahead of this, and why

**The delete verb had one guard and it was the wrong one.** `Chooser::confirm_delete`
(`chooser.rs`) is `remove_dir_all`, and the only thing it refused was the root kit — *"`assets/emerge`
itself … `remove_dir_all` on it would take the whole library."* Correct as far as it went. It did not
know that `src/site/kit.rs:38` names a subdirectory, and so it took `site/`, `site_greybox/` and
`site_v2/`, the game's ability to boot, and 51 tests.

That is stage 3 arriving as a bug rather than a feature, so it was built as stage 3 rather than as a
patch: `dependents()` asks **not "is anything using this kit" but "is this kit the last provider"**,
which is the distinction §1's measurement forces. Removing one of two directories that define the
same ids strands nothing; removing the only one strands every reference. Held by four tests, one of
which is `the_game_kit_file_is_a_dependent_no_content_scan_would_find`.

Two readers, because there are two formats and one belongs to another crate. Maps and
`compositions.ron` are **parsed**, so a `note:` mentioning `site/wall` is prose rather than a
dependency. `assets/site/kit_*.ron` is read as text: `SiteKit`'s schema lives in the game,
`emerge-mapper` does not depend on the game and must not start, and a quoted-id match over a file
that is a list of quoted ids is exact enough to be worth more than the coupling.

The scan runs on a keypress and reads every map in the project. That is the trade the chooser already
makes in the other direction — `read_kit` parses only `library.ron` *"for a list nobody has chosen
from yet"* — and it is the right way round: **listing is cheap because it happens constantly, and
deleting is thorough because it happens once and cannot be undone.**

---

## 8b. And the lattice settings, for the same reason

`face_bands` and `snap_divisor` moved from `project.ron` onto `Map` the same day. Both describe a
lattice; a kit does not have one. The migration is one-shot with no shim, which the schemas' own
rules made cheap in opposite ways: `POLICY_VERSION` is an **equality**, so every `project.ron` had to
be edited and was; `MAP_VERSION` is a **floor**, so a map written before the move reads as exactly
what it already meant — `#[serde(default)]` and nothing else.

The check that left with them is the interesting part. `layered_library` was *"the one loader, so
there is no path on which the check is skipped"*, and that property is real. It now belongs to each
loader that holds a map — `Project::open` for the editor, `EmergeWorld::with_compositions` for the
game — and **not** to `src/site/kit.rs`, which opens a kit for the hub's own layout and never reads
an edge token. That is a consumer the check does not apply to rather than a path that skips it, and
it is written down at all three sites.

Measured after: 2,542 passed, 57 failed, and every one of the 57 names the missing site kit. The
move introduced none of them.

**Corrected the same day: the move went one stop too far.** The argument above is that a kit has no
lattice and a map has exactly one. The first half holds; the second does not survive a project with
two maps in it. `composition::interface` takes the band count as an argument and every call site
passes `project.map.face_bands`, so two maps at different band counts give the same tile two
different adjacency contracts — a kit of tiles is coherent at exactly one band count, which makes the
setting a fact about the **project**, not about either level below it. Found from the other side,
while designing the door split: the Tiles door has to derive an interface with no map open at all.
`docs/2026-08-16-doors.md` §3.1 and FVS-R-42.

## 8c. Binding, and what it cost

**`kits.ron` declares the binding and loading verifies it.** Declared-only drifts the first time a kit
is re-authored; derived-only cannot express the re-skin pair at all, because both directories answer
`site`. Stating it and checking it is the discipline Mesa's symbol files put on separate compilation
(§3) — and `Library::namespace` is the check.

**Nothing new guards the merge.** Two directories bound to one namespace is refused by
`Kits::validate`, naming both skins; a duplicate id that slips past is refused by `Library::validate`,
which already said *"a map references descriptors by id, so a duplicate makes every reference to it
ambiguous"*. The rule that was there catches the exact mistake binding exists to prevent.

**`--kit` changed meaning rather than disappearing**, which is the thing this document previously got
wrong. It used to select the *only* kit loaded — which is precisely what made a tile authored in one
kit invisible to every map in another. It now names the **authoring** kit: where an imported mesh
lands and what a new tile is called. Every bound kit is loaded either way, so it is not a filter on
what can be placed. That is a smaller and more useful question, and it is why stage 6 was reachable
at all: with the map no longer tied to a kit, the chooser's two columns became independent.

**Three things fell out of doing it.** `commit_measured` had to rebuild the *merge* rather than
replace the library, or the first import in a session would look like it had deleted every other
kit's pieces. `confirm_delete` had to unbind **before** removing the directory, so a refusal costs
nothing and a project is never left naming a kit that is gone. And the chooser stopped resetting the
map selection when the kit changed — there is one list now, so there is no index to invalidate, and
resetting would move the row an author is reading out from under them.

## 9. Open

- **Edge tokens or corner tokens.** Still open, still a schema decision, and the paper that settles it
  is now readable (§5). Whichever way it goes, it should be taken before interfaces move.
- **One door or two for compositions.** A `Bounded` composition is a solver prototype and spends the
  32; an `Anchored` one is a stamp and spends nothing. They are one type by a decision the literature
  supported, so splitting the *storage* would undo it — but a single list whose budget applies to
  some rows and not others is a list that cannot be read. Likeliest answer: one collection, the
  envelope visible on the row, and the budget shown as a count.

  **Taken 2026-08-16 as two doors, storage unsplit** — `docs/2026-08-16-doors.md` §4.2. Tiles lists
  the `Bounded` rows and Compose the `Anchored` ones, out of one `compositions.ron`, so the decision
  the literature supported is untouched and the unreadable mixed list is what goes. Recorded as a
  decision rather than a finding: if a `Bounded` group larger than one cell turns out to be ordinary,
  the doors are wrong and this reopens.
- ~~**Whether the game's loader resolves a multi-kit map.**~~ **Answered: it does.** `src/emerge_map.rs`
  reads `kits.ron` and calls `kits::bound_library`, the same call `Project::open` makes — one path, so
  a tile that previews in the editor loads in the game. `src/site/kit.rs` still calls
  `policy::layered_library` on one directory and is right to: it opens a kit for the hub's own layout
  and never reads a map.
- **The furniture kit's 75 ids are still flat.** They merge fine and they are unique, but the
  namespace-is-the-dependency-edge property does not hold for them: a map naming `lamp_tall` cannot
  say which kit provides it. Qualifying them is a data migration nothing currently forces.
