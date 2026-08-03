# Review of the emerge-mapper plan — what the first draft got wrong

A review of `2026-08-03-emerge-mapper-plan.md` before any code was written. Recorded because three of its
findings changed the schema, and two of them are the kind that are cheap now and a migration later.

Everything below was re-verified against the vendored `bevy-0.19.0` source and the local paper corpus;
where the review and the source disagree, that is noted.

---

## 1. The blind spot: BSN

**The draft did not mention BSN at all.** That was the most serious omission. Bevy 0.19's headline
feature is the next-generation scene system, and the overlap with a bespoke descriptor format is
substantial:

- A BSN expression is a **patch** — it does not write a full instance of every type, so scenes layer on
  top of each other with each layer writing its fields onto defaults. That is precisely the
  descriptor → map-instance-override model. The draft was reinventing patch composition without saying
  so.
- **Instance caching** (`:`) resolves a scene once and layers per-instance data on top — built for the
  "200 copies of one prop" case.
- `bevy_scene` was renamed/split with `bevy_world_serialization` explicitly to make room for it.

**Verified in the tree:** `bsn!` and `bsn_list!` exist in `bevy_scene_macros-0.19.0/src/lib.rs:48,67`;
`bevy_world_serialization-0.19.0` is present.

**Where the review overstated slightly.** It offered `SceneComponent` as recovering the identity
guarantee lost when `SiteKit`'s closed enum is dropped. The source says otherwise:
`bevy_scene-0.19.0/src/scene_component.rs` defines `SceneComponentInfo { spawned_from_scene: bool, … }`
gated `#[cfg_attr(debug_assertions, component(on_add))]`. That is a **debug-build runtime check**, not a
parse-time proof — a peer of the plan's `required: [id, …]` list, not an upgrade on it. Both catch at
load what `deny_unknown_fields` caught at parse.

**Resolution:** the plan now takes an explicit position — descriptor stays RON *data* shaped as a patch
over defaults, so a `.bsn` port is mechanical; revisit when a first-party `.bsn` asset loader lands.

---

## 2. The schema change: interactions belong to a *location*, not a descriptor

The draft hung `interactions: [(verb, slot, requires, guard, effects)]` off each descriptor. That is a
single-actor, single-object model, and **a table plus four chairs is one affordance, not five.**

Two corpus sources the draft had not cited say so directly, both verified verbatim:

- **Game AI Pro 3 ch.35 (FINAL FANTASY XV):** *"smart locations abstract away from concrete objects…
  They are invisible objects that refer to multiple concrete objects. For example, a single smart
  location may refer to two chairs and a table. This allows it not only to inform agents about the
  existence and usability of individual objects, but also to capture relationships between them, such
  as furniture grouping… But smart locations do not just contain information; they essentially govern
  the usage of the objects they refer to."* Role allocation is a randomized greedy Monte-Carlo
  algorithm explicitly allowed to fail occasionally; typical instances are under four roles.
- **Game AI Pro 2 ch.11 (Smart Zones):** roles are stratified — *"Main roles are essential to execute
  the Living Scene… The scene won't start unless all the main roles are fulfilled"*; supporting roles
  are favourable, extras optional; and *"If no NPC is able to take the role, the module starts a
  dynamic search operation… using an expanded zone, which is automatically extended until the NPC is
  found."*

The repo's own Geishauser/Cheong/Nelson citation is on the same axis — Territories compute slots in 2D
space (a store table has one merchant slot and three customer slots) as *"a step away from playing
animations in rigidly fixed slots as seen for example in The Sims."*

**Two changes, free now and a migration after Stage 6:**

1. Sockets carry a **role** name, not just an id — `(id: "seat_n", role: "diner", …)`.
2. Interactions move to a map-level `locations:` list referencing props by id.

A single-prop interaction is then the degenerate case (one prop, one Main role); nothing is lost.

---

## 3. The missing field: clearance

Tutenel's semantic library encodes the surfaces split the repo already has, but with more: classes
contain *features*, and feature types carry embedded layout semantics — **off-limits** features cannot
overlap anything, **clearance** features may only overlap other clearance features, guaranteeing free
space in front of a cupboard or vending machine.

The draft's `extent: (footprint, height)` has no clearance concept, so nothing in the schema forbids a
chair flush to a wall with its seat socket inside the wall — the same class of bug as the
`is_floor_marking` mug misclassification, one level up. Merrell et al. 2011 (already implemented here
as `solvers/metropolis.rs`) supplies the numbers: 36″ bedside, 30″ in front of a seat, 24″ in front of
shelving, 36″ around a dining table, 16–18″ coffee table to seat.

Note `Predicate::Clearance(f32)` **already exists** in `placement/ir.rs` — the constraint side is
there; the *descriptor* has no way to state it. Adding `clearance: [(dir, dist)]` at Stage 1 is cheap;
adding it after `check_prop_placements` is calibrated against Stage 3's gate is not. It also makes a
socket validatable: a socket with no reachable clearance is a fault the importer can flag.

---

## 4. An overstated citation, corrected

The draft cited **Smelik, Tutenel, de Kraker & Bidarra (2010)** for a "Lock / Scope / Group" triad as
though it were shipped prior art.

The review's objection is right in substance and slightly wrong in detail. The three names *are* in the
paper — §5.3 introduces *"possible facilities, which are inspired from image processing software, but
have more advanced and complex semantics"* and then names **Locking**, **Scoping** and **Grouping** in
that order. But they are **proposals**, in a paper whose own assessment is that integrating procedural
generation with manual editing is *"so far as good as unaddressed,"* with preserving manual operations
through regeneration called out as particularly difficult and the proposed fix a sketch.

**Resolution:** cite as aspiration, not validated mechanism, in both this plan and
`2026-08-03-kitbash-editor.md`, which inherited the overstatement.

---

## 5. Trap-list corrections

| Claim | Verdict |
|---|---|
| `add_plugins` cap is 16, not 15 | **The draft was right.** `bevy_app-0.19.0/src/plugin.rs:186` reads `all_tuples!(#[doc(fake_variadic)] impl_plugins_tuples, 0, 15, P, S)`. The review's substantive point stands though: nesting sidesteps it, so it is a shape constraint, not a ceiling. |
| Missing `Res<T>` panicking is not a 0.19 thing | **Correct**, and a useful addition: it is the 0.16 fallible-param model, a `SystemParamValidationError` routed to the error handler, and it is **configurable**. Want warn-not-panic in the editor, panic in the game. |
| `Single` silently skips, with upstream pressure to invert | **Correct.** Cause of two bugs on 2026-08-03. The actionable half is the advice: encode the assumption in exactly one place, not nine. |
| One `App` per process | Holds; two binaries, not two Apps. |
| Do the 0.19 engine bump as its own gated step | **Moot** — `Cargo.toml` already pins `bevy = "0.19.0"`. The resources-as-components lookup-indirection regression remains the first thing to suspect if a perf number moves. |

## 6. Editor components: adopt three, refuse one

The review is right that 0.19 upstreamed work the draft was budgeting for — **`InfiniteGridPlugin`**
(the importer's ground plane), **Feathers number input** (nudging `align`), **`SettingsPlugin`**, and
**`save_using_saver`** (baked thumbnails; also half of the export question).

**But not `TransformGizmoPlugin`.** The review recommends adopting it. Measured in this codebase on
2026-08-03, it is unusable:

- Its overlay camera is spawned as `Camera { order: 1, ..Default::default() }`, and `Camera::default()`
  carries `ClearColorConfig::Default` — which clears — under a comment claiming it renders *"without
  clearing the color buffer."* It blanked the main camera's HDR output: the frame went from **13,343
  distinct colours to 183**, median luminance **57 → 0**. Forcing `ClearColorConfig::None` was not
  enough; it composites over an HDR/bloom camera and blanks it regardless.
- That second `Camera3d` also silently broke every `Single<.., With<Camera3d>>` in the tree — the audio
  listener, all billboards, `selection`'s click-to-command, and `camera::drive_camera`.

Removed on `main`; `crate::MainCamera` is the positive filter that prevents a recurrence.

---

## 7. Where the plan was already sound

Typed axes, the capability bitmask, `mount` as a single layering axis, and "measure, never dial by eye"
track the sources closely. The importer is the strongest part and the part the literature does *not*
cover — deriving `front` from the upper-45% XZ centroid, flagging the 100× centimetre case, and
re-deriving 45 shipped measurements as its gate.

One process note adopted: **Stage 6's gate was satisfiable in an afternoon and proved nothing.** Both
game-AI chapters report the query API is the easy half and role allocation the hard one, so it is now
split — 6a single-actor, 6b multi-actor with the gate *"four agents fill a four-seat table with no
deadlock and no double-booking."*
