# Animation guide — `foundation_vs_slop`

The engineering guide to the skeletal-animation system: how it works, how to wire a new creature, how
to add or re-author a clip, and the rules that keep it out of the deterministic core. For the
**artist-facing** side — what to deliver in a GLB, per-asset clip tables, the "animations we still
need" list — see `docs/artist_guide.md` (§4, "Character assets"). This document is for someone editing
`src/anim/`, `src/squad.rs`, `src/crab/`, or `src/parasite.rs`.

---

## The one idea: no transitions

Every skinned creature (squad figurine, crab, manca) is driven the same way, through one module
(`src/anim/`). **There is no transition and no clip is ever rewound.** Every clip of a creature's blend
set stays resident in its `AnimationPlayer` forever; each frame only two things move:

- **weights** — one per clip, eased toward the driver's target with a frame-rate-independent
  exponential (`FADE_TAU`, ~0.08 s), and
- **one shared gait phase** φ ∈ `[0,1)` — from which every gait clip's seek time is derived as
  `frac(φ + offset) · duration`.

Because every gait clip is reparameterised onto the same normalised φ, left-foot-down happens at the
same instant in the walk and the run no matter how the weights are mixed. That is what stops the feet
scissoring. It replaced a per-state `AnimationTransitions::play(…)`, which ends in
`AnimationPlayer::start` → `ActiveAnimation::replay` and **rewinds the incoming clip to frame 0** — so
cross-fading a mid-stride walk against a from-scratch run scissored the legs (the failure Shroff names
in Game AI Pro 2 ch. 36 §36.3.1).

> **Do not add `AnimationTransitions` to anything the blender drives.** Its `advance_transitions`
> system runs in `PostUpdate` and would stomp the weights this module sets. One creature, one path.

---

## Architecture

Everything lives in `src/anim/`:

| Item | What it is |
|---|---|
| `PoseBlender` (component) | The live blend state of one model: the link to its `AnimationPlayer`, the slot table, live + target weights, the shared phase, and one-shot bookkeeping. Lives where the creature's animation link lives — the `FigurineModel` child for the squad, the root for crab/manca (see Determinism rule 2). |
| `Slot` + `Playback` | One wired clip: its `AnimationNodeIndex` and how its time is driven (`Free` / `Gait` / `OneShot`). The slot's **index** in the table is the handle drivers use. |
| `BlendSource` (component) | The shared graph + slot table for one creature kind, pinned by **spawn code** on the entity that should own the blender (the `FigurineModel` child for the squad, the creature root for crab/manca). Handle + `Arc`, so per-instance clones are refcount bumps. |
| `attach_pose_blenders` (system) | The one attach pass, in `PoseAttachSet`: walks up from every freshly streamed-in `AnimationPlayer` to the nearest ancestor `BlendSource`, points the player at the graph, makes every clip resident at zero weight, and inserts `PoseBlender` on that ancestor. Every clip is started **once, here**. A player with no sourced ancestor (the flashlight scene, the wound decoration) stays deliberately unwired. |
| `apply_pose_blenders` (system) | The one apply pass. Eases weights, advances the shared phase, writes both to the `AnimationPlayer`. Registered in the `PoseBlendSet` system set. |
| `PoseBlendPlugin` | Registers the attach + apply passes. Added once in `lib.rs` and `sim_harness.rs`, grouped with the squad — **not** by each creature plugin. |
| `blend.rs` | Pure, unit-testable math for the humanoid locomotion blend space: `locomotion_weights`, `dir_weights`, `tier_weights`, `travel_angle`. No ECS, no assets — runs in the `cargo test` core layer. |

### Scope: skeletal clips only

`src/anim/` owns exactly one concern: **clip-weight blending on a skinned rig's `AnimationPlayer`**.
Cosmetic motion that is not a skeletal clip deliberately lives outside it, each as its own focused
`Update` system under the same determinism rules (cosmetic, never hashed): SCP-999's mass-spring
jiggle and shader eyes (`src/scp999/`), the physics-reactive accent hair (`src/hair.rs`), the
Smiley's procedural shader face. Do not funnel those through `PoseBlender` — a jiggle solver has no
clips to weight, and one path per *feature* means one path per problem, not one module for every
kind of motion.

**Per-frame flow** (all on `Update`):

```
attach_pose_blenders   (wire new AnimationPlayers to the nearest BlendSource)     [PoseAttachSet]
        ▼
creature driver  (reads Velocity/state, writes PoseBlender targets + ground speed + one-shots)
        │  .after(PoseAttachSet), .before(PoseBlendSet)
        ▼
apply_pose_blenders   (ease weights → advance shared φ → write AnimationPlayer)   [PoseBlendSet]
        ▼
bevy_animation::advance_animations / animate_targets   (PostUpdate)
```

Each creature registers one **driver**: `drive_*.after(anim::PoseAttachSet).before(anim::PoseBlendSet)`
(the ordered edges make Bevy flush the wire commands in between, so a model that streamed in this frame
gets its first targets this frame). The driver writes *this* frame's targets; the shared apply pass
consumes them. There is no per-creature attach system.

### The three playback kinds

Pick per slot — this is data describing the clip, not a code path:

- **`Slot::free(node, speed)`** — a loop with no gait relationship to the rest of the set (an idle, an
  aim pose, a crab's chomp). Ticked by Bevy at `speed`; only its weight moves, never rewound.
- **`Slot::gait(node, duration, phase_offset, cycle_distance)`** — a member of the shared gait group.
  **Paused**, so `advance_animations` leaves it alone and the apply pass owns its seek time outright,
  derived from φ. `cycle_distance` (world units travelled per cycle) sets the cadence; `phase_offset`
  aligns it to the reference clip. See "The clip contract" for how these are measured.
- **`Slot::one_shot(node, speed)`** — plays through once on `trigger(slot)` (a recoil, an eruption).
  The **only** slot kind that ever restarts a clip — restarting on trigger is the intent. Poll
  `active_shot()` to know when it has finished.

### The driver API (`PoseBlender`)

```rust
blender.set_targets(&weights)?;   // full vector; must be exactly `len()` long (else PoseBlendError)
blender.set_only(slot);           // one-hot — the whole API a discrete state machine needs
blender.set_ground_speed(speed);  // world u/s; drives the shared gait phase
blender.trigger(slot);            // (re)start a OneShot slot
blender.active_shot();            // Some(slot) while a one-shot plays, None once it finishes
blender.target_weight(slot);      // what the driver asked for last frame — edge-detect a state entry
blender.live_weight(slot);        // the eased weight actually sent to the player (for test oracles)
blender.phase();                  // the shared gait phase [0,1)
```

---

## Determinism rules (load-bearing)

The animation layer is **cosmetic and must stay invisible to `snapshot_hash`.** The rules that keep it
so — break any one and the replay/exact-hash gate (`tests/replay.rs`) will red:

1. **`Update` only, never `FixedUpdate`.** Nothing here may appear in `snapshot_hash`. It reads
   `Transform`/`Velocity`/state components and writes only `AnimationPlayer` + its own `PoseBlender`
   (and, for the squad, `LocoSmooth`).
2. **Never introduce a *new* component on the hashed sim entity at an async tick.** The
   `AnimationPlayer` streams in at a wall-clock-dependent tick; churning the sim entity's archetype then
   shifts ECS iteration order between same-seed runs (issue #18). The `BlendSource` decides where the
   blender lands, so put it exactly where the creature's prior animation link lived: the squad's rides
   the `FigurineModel` **child** (the figurine scene, with all its instanced children, attaches there
   and never to the `Unit`); the crab/manca sources sit **on the same root** the old
   `CrabAnimPlayer`/`MancaAnimPlayer` links sat on, so the async `PoseBlender` insert churns nothing it
   didn't already churn. `BlendSource` itself rides the spawn batch (never an async insert). The replay
   gate is the proof this holds.
3. **No genome genes.** `squad_ai::world_genome` and its siblings evolve knobs whose effect shows up in
   `snapshot_hash`. Everything here is invisible to it by construction, so a gene pointed at `FADE_TAU`
   or `ACTION_ALPHA` would be a knob the RL/QD search turns forever with the fitness never moving.
   Cosmetic tuning belongs in the constants and in `docs/artist_guide.md`, **not** the evolving systems.
4. **No shared RNG, counter, or `take(n)` budget, so no sort to make total.** The membership sets here
   (e.g. "who fired a bolt this frame") are order-independent ORs. Keep it that way; a raw sort would
   trip `tests/determinism_lint.rs` for nothing.

This is the separation-of-concerns of Bonny, Game AI Pro 2 ch. 12: the animation state machine is kept
entirely apart from the AI/gameplay decision state.

---

## The squad blend space

`src/squad.rs` is the one creature with a *continuous* blend space rather than a discrete state machine.
Ten resident clips: eight locomotion + two masked action clips.

**Locomotion (8 slots, `blend.rs`).** A moving unit is described by two continuous parameters — smoothed
**speed** and **travel direction in its own frame** (θ, `0` ahead, `+π/2` right) — because units yaw to
face what they shoot (`unit_facing`), so travel and facing routinely disagree. `locomotion_weights`
turns that pair into a **partition of unity** over `{idle, idle_alert, walk, run, walk_back, run_back,
strafe_l, strafe_r}`:

- `tier_weights(speed)` → `(moving, fast)` via two overlapping `smoothstep` bands (`MOVE_BAND`,
  `RUN_BAND`). The overlap is Shroff §36.2.5's blended speed ranges; it replaces the old hard
  `RUN_SPEED_FRAC` step that a unit hovering at the threshold flapped across (and every flap was a clip
  restart).
- `dir_weights(θ)` → a 4-way angular blend, ≤ 2 lobes non-zero, C¹ across the cardinal seams.

**The upper-body layer (2 slots).** `aim` (looping) and `fire` (one-shot) are added to the graph
**masked** so they drive only `spine_01` and above. The layer weight α (`ACTION_ALPHA = 0.9`) scales the
locomotion weights by `1 − α` and gives the action clips α. The root blend node normalises per bone, so
on the **lower body** the action clips are masked out and never pushed → the locomotion mixture is the
sole contributor and the legs keep walking; on the **upper body** the action clip takes exactly share α.
α is held < 1 on purpose: at 1.0 every locomotion weight is bit-zero, `animate_targets` skips them all,
and the legs fall to the bind pose. This is Shroff §36.4.1/§36.4.3 (masking + layering).

**Cosmetic smoothing (`LocoSmooth`).** `unit_movement` slams `Velocity` to zero the tick a unit arrives,
so the raw signal is a cliff. `LOCO_SMOOTH_TAU` (0.10 s) filters speed and direction on the cosmetic
side — it can't live in `unit_movement` because `Velocity` is hashed sim state. Kept short: the same
smoothed speed drives the gait phase, so a long tail would keep the legs striding after the unit stops.

The per-slot metadata (durations, phase offsets, cycle distances, and the strafe-clips-are-named-
backwards caveat) is in `docs/artist_guide.md`. Weight assembly is `valkyrie_weights` and is unit-tested
in `src/squad.rs`'s test module.

---

## How-to: wire a new animated creature

Worked examples in the tree: `crab` (three `Free` clips) and `parasite`/manca (five `Free` + one
`OneShot`). Both are discrete state machines that use `set_only`. The steps:

1. **Slot constants.** Give each clip a slot index (`const SLOT_IDLE: usize = 0; …`) in the order you'll
   build the table. The index is the driver's handle — keep it stable.

2. **Build the graph + slot table** in a `Startup` system, stored on a resource:
   ```rust
   let (graph, nodes) = AnimationGraph::from_clips([ /* clips, glb order */ ]);
   let slots: Arc<[anim::Slot]> = Arc::from([
       anim::Slot::free(nodes[/*idle*/], 1.0),
       anim::Slot::free(nodes[/*walk*/], WALK_ANIM_SPEED),
       anim::Slot::one_shot(nodes[/*special*/], SPEED),
   ]);
   commands.insert_resource(MyAnim { graph: graphs.add(graph), slots });
   ```
   Slot-table order is *your* order (the `SLOT_*` constants), not necessarily the glb's clip order.
   **Verify the `Animation(i)` indices against the actual glb bytes before trusting any clip list** —
   exporters commonly reorder clips (the SCP-150 export sorts them alphabetically, which is how the
   manca shipped huddling to `Attack1`) — and pin them in `tests/creature_clip_contract.rs` so a
   re-export fails the gate instead of silently playing the wrong clips.

3. **Pin a `BlendSource` at spawn** — insert
   `anim::BlendSource { graph: my_anim.graph.clone(), slots: my_anim.slots.clone() }` on the entity
   that should own the `PoseBlender` (the model child if the creature's sim entity is hashed, the root
   otherwise — see Determinism rule 2). There is **no per-creature attach system**: the shared
   `anim::attach_pose_blenders` pass wires the streamed-in `AnimationPlayer` to its nearest sourced
   ancestor.

4. **Drive system** — read the creature's state, call `blender.set_only(slot)` (state machine) and,
   for a one-shot, `blender.trigger(slot)` on the *edge* into that state (detect with
   `target_weight(slot) <= 0.0`, not `active_shot()`, which the apply pass updates a frame later).

5. **Register** `drive_*.after(anim::PoseAttachSet).before(anim::PoseBlendSet)` on `Update`. Do **not**
   add `PoseBlendPlugin` here — it's registered once with the squad.

For a creature that should blend continuously (like the squad), write targets with `set_targets(&w)`
where `w` sums to 1, and call `set_ground_speed` so the gait phase advances. Only use `Gait` slots when
several clips genuinely share a locomotion cycle that must stay phase-locked; a scuttle and a chomp do
not, so the crab/manca use `Free` throughout.

---

## How-to: add or change a clip on an existing creature

- **Adding a slot:** extend the `SLOT_*` constants and the slot table together; update any driver
  `match` and the weight-vector length. For the squad, `N_SLOTS` and the tests that assert the partition
  of unity will catch a mismatch.
- **Re-authoring a gait clip (squad):** the `(duration, phase_offset, cycle_distance)` numbers are
  **measured off the GLB**, not guessed. If a clip is re-exported you must re-measure, or the feet drift.
  `tests/valkyrie_asset.rs` pins the durations to the asset (±1 frame) and fails loudly if a re-export
  drifts them — but it cannot check the phase offset or cycle distance, so those are on you.

See "The clip contract" for the measurement method.

---

## The clip contract (what a gait clip must satisfy)

For the shared-phase design to hold, gait clips must be:

1. **In-place.** Root translation must be bit-zero — `unit_movement` drives the character's transform,
   so baked root motion would move it twice. `tests/valkyrie_asset.rs` enforces this.
2. **One gait cycle, phase-aligned.** All gait clips are reparameterised onto one φ, so left-foot-down
   must land at the same fraction of every clip; the per-clip `phase_offset` absorbs the residual.
3. **Cycle-distance honest.** The cadence is `speed / mean_cycle_distance` (Shroff §36.2.5, generalised
   to a blend by `gait_cycles_per_sec`), so the baked `cycle_distance` must be the real ground distance
   the clip covers per cycle.

**Measurement method** (how the committed numbers were produced — currently a manual offline step, not a
repo tool): sample the GLB's animation channels, run forward kinematics down the leg chains to get each
foot's world position over the cycle, then:

- **cycle_distance** = the planted foot's speed relative to the static `Root`, ×`FIGURINE_SCALE`;
- **phase_offset** = the negative of the lag that best cross-correlates the two clips' foot-height
  curves (φ is expressed in the reference clip's frame);
- **duration** = the clip's longest keyframe time.

If gait clips are re-authored often, committing that script as a `train`/dev subcommand would remove the
"how do I re-measure" gap — flagged as a possible follow-up, not done here.

Mask groups (which bones the upper-body layer excludes) are matched **by name** against the live
skeleton, never a precomputed path, so a re-export that renames a bone surfaces as a missing name rather
than a silently wrong mask. The name list (`LOWER_BODY_BONES`) is asserted against the GLB by
`tests/valkyrie_asset.rs`.

---

## Testing

- **`src/anim/blend.rs`** (core, `cargo test`) — the blend space is a partition of unity everywhere,
  continuous in speed and angle, `travel_angle` matches Bevy's −Z-forward convention, tiers are
  monotone, degenerate inputs don't NaN.
- **`src/anim/mod.rs`** (core) — the apply pass through a real `App` on a bare `AnimationPlayer` (no
  assets): weights ease without jumps and reach the player, gait clips share one phase and stay paused,
  the phase holds while idle, a one-shot restarts on trigger. Plus the cadence math.
- **`src/squad.rs`** (core) — `valkyrie_weights`: aiming-while-walking layers instead of replacing, the
  full vector is a partition of unity, `ACTION_ALPHA` never starves the legs.
- **`tests/valkyrie_asset.rs`** (core) — the clip-index / duration / bone-name / in-place contract,
  read straight from the GLB bytes.
- **`tests/creature_clip_contract.rs`** (core) — the same clip-index → name contract for the crab and
  SCP-150 rigs (shared GLB reader in `tests/common/mod.rs`). Every creature that wires clips by
  `Animation(i)` index gets a pin here.
- **`tests/liveness.rs`** (`--features test-harness`) — figurines actually wire and keep a well-formed
  blend through a live run; a unit shooting on the move keeps its legs running.

See `TESTING.md` for how to run each layer. A change here must leave `tests/replay.rs`'s exact-hash
gate unmoved (it will, if the four determinism rules above hold).

---

## Research grounding

The design is drawn from work in the `home-still` corpus, cited inline where it's applied:

- **Shroff, "Realizing NPCs: Animation and Behavior Control for Believable Characters", Game AI Pro 2
  ch. 36** — §36.2.5 speed correction via playback-rate + overlapping speed ranges; §36.3.1 pose
  matching (blend on phase, not elapsed time); §36.4.1/§36.4.3 animation masking + layering.
- **Kovar & Gleicher, "Flexible Automatic Motion Blending with Registration Curves", SCA 2003**
  (DOI 10.2312/sca.sca03.214-224) — the shared-phase reparameterisation is the runtime reduction of a
  registration curve (a uniform timewarp with per-clip offsets baked offline).
- **Bonny, "Separation of Concerns Architecture for AI and Animation", Game AI Pro 2 ch. 12** — the
  animation state machine kept apart from AI/gameplay state, which is what makes rule (1)–(3) natural.

---

## Key file references

| Path | Role |
|---|---|
| `src/anim/mod.rs` | `PoseBlender`, `Slot`/`Playback`, `BlendSource`, `attach_pose_blenders`, `apply_pose_blenders`, `PoseBlendPlugin` |
| `src/anim/blend.rs` | The humanoid locomotion blend space (pure math) |
| `src/squad.rs` | Squad 2D blend space + masked action layer; `build_valkyrie_anim`, `drive_valkyrie_animation`, `valkyrie_weights` |
| `src/crab/movement.rs`, `src/crab/setup.rs` | Crab: three `Free` clips, one-hot driver |
| `src/parasite.rs` | Manca: five `Free` + one `OneShot` (BurrowOut) |
| `docs/artist_guide.md` §4 | Per-asset clip tables, authoring contract, "animations we still need" |
| `tests/valkyrie_asset.rs` | The GLB asset contract |
