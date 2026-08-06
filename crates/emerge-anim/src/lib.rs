//! Cosmetic pose blending — the one place a skinned model's clip weights and clip times are written.
//!
//! Every creature in the game (squad figurine, crab, manca) used to drive its skeleton the same way:
//! a discrete state, and `AnimationTransitions::play` on every state change. That call ends in
//! `AnimationPlayer::start` → `ActiveAnimation::replay`, which **rewinds the incoming clip to time
//! zero**. Cross-fading a mid-stride walk against a from-scratch run is exactly the failure Shroff
//! names in "Realizing NPCs: Animation and Behavior Control for Believable Characters", Game AI Pro 2
//! ch. 36 §36.3.1: *"if the time between footfalls … is not precisely in sync, the character's
//! appendages may scissor, freeze, windmill, or otherwise move implausibly."* His fix is **pose
//! matching** — blend on phase, not on elapsed time.
//!
//! This module makes that structural rather than per-transition. **There is no transition.** Every
//! clip of a model's blend set stays resident in the `AnimationPlayer` forever and is never restarted;
//! only two things move:
//!
//! * **weights**, eased toward the driver's targets with a frame-rate-independent exponential
//!   ([`FADE_TAU`]), and
//! * **one shared gait phase** φ ∈ `[0,1)`, from which every [`Playback::Gait`] slot's seek time is
//!   derived as `frac(φ + offset) · duration`.
//!
//! Because every gait clip is reparameterised onto the same normalised φ, left-foot-down happens at
//! the same instant in the walk and the run no matter how the weights are mixed. That is the runtime
//! reduction of the *registration curve* of Kovar & Gleicher, "Flexible Automatic Motion Blending with
//! Registration Curves", SCA 2003 (DOI 10.2312/sca.sca03.214-224) — a uniform timewarp with the
//! per-clip offsets baked offline instead of a full per-frame correspondence, which is what §36.3.1
//! recommends ("Phase information can be generated offline and stored as metadata, keeping runtime
//! calculations to a minimum").
//!
//! # Determinism
//!
//! Everything here is cosmetic. It runs on `Update`, reads `Transform`/`Velocity`/state components,
//! and writes only [`AnimationPlayer`] plus its own component — never a hashed sim component. The
//! [`PoseBlender`] lives on the model **child**, never on the sim entity, for the same reason the
//! figurine scene does (see `squad::FigurineModel`): the child's archetype churns at an async,
//! wall-clock-dependent tick, and the sim entity's must not. Keeping the animation state machine
//! separate from the AI/gameplay state is the separation of concerns of Bonny, "Separation of Concerns
//! Architecture for AI and Animation", Game AI Pro 2 ch. 12.
//!
//! For the same reason **none of these constants is a genome gene**, and none should become one.
//! `squad_ai::world_genome` and its siblings evolve knobs whose effect is visible in `snapshot_hash`;
//! everything here is invisible to it by construction, so a gene pointed at `FADE_TAU` would be a knob
//! the search could turn forever with the fitness never moving. Cosmetic tuning belongs in the
//! constants below and in `docs/artist_guide.md`, not in the RL/QD loop.

pub mod blend;
pub mod rigs;

use std::sync::Arc;

use bevy::prelude::*;

/// Frame-dt clamp, so a hitch can't slam every weight to its target in one step (mirrors
/// `squad::MAX_FRAME_DT`).
const MAX_FRAME_DT: f32 = 1.0 / 30.0;

/// Time constant of the weight cross-fade, seconds. A weight covers ~95% of its distance to the
/// target in `3 · FADE_TAU` ≈ 0.24 s — comparable reach to the 150 ms `AnimationTransitions` fade it
/// replaces, but C¹ and, crucially, without ever rewinding a clip.
const FADE_TAU: f32 = 0.08;

/// Weights below this snap to exactly `0.0`, because `animate_targets` skips a clip whose weight is
/// *bit-exactly* zero. Snapping keeps a faded-out clip genuinely free instead of leaving it to
/// evaluate 186 curves at a weight nothing can see.
const WEIGHT_EPS: f32 = 1.0e-3;

/// Clamp on the gait playback rate, as a multiple of the mixture's authored cadence, so a stalled or
/// sprinting outlier can neither freeze nor gabble the legs.
const PHASE_RATE_CLAMP: (f32, f32) = (0.5, 2.0);

/// How a slot's clip time is driven. One variant per slot — this is data describing the clip, not a
/// choice of code path.
#[derive(Clone, Copy, Debug)]
pub enum Playback {
    /// A loop with no gait relationship to the rest of the set (an idle, an aim pose, a crab's chomp).
    /// Ticked by Bevy at `speed`; only its weight moves, so it is never rewound.
    Free { speed: f32 },
    /// A member of the gait sync group. **Paused**, so `advance_animations` leaves it alone
    /// (`bevy_animation::advance_animations` skips paused clips) and this module owns its seek time
    /// outright, derived from the shared φ.
    Gait {
        /// Clip length in seconds. Baked, and pinned by the asset-contract test.
        duration: f32,
        /// Where this clip's φ = 0 sits relative to the set's reference gait clip, measured offline by
        /// cross-correlating foot height (§36.3.1).
        phase_offset: f32,
        /// Ground distance in world units the clip covers in one cycle. Together with the weights this
        /// gives the mixture's stride length, and hence the cadence that keeps the feet planted —
        /// speed correction per §36.2.5, generalised from a single clip to a blend.
        cycle_distance: f32,
    },
    /// Plays through once on demand (a recoil, an eruption). A one-shot **is** meant to restart on
    /// each trigger, so this is the only slot kind that ever calls `AnimationPlayer::start`.
    OneShot { speed: f32 },
}

/// One wired clip in a model's blend set. The slot's index in [`PoseBlender::slots`] is the handle
/// drivers use, so the tables in `squad`/`crab`/`parasite` are order-sensitive by design.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub node: AnimationNodeIndex,
    pub playback: Playback,
}

impl Slot {
    pub fn free(node: AnimationNodeIndex, speed: f32) -> Self {
        Slot { node, playback: Playback::Free { speed } }
    }

    pub fn gait(node: AnimationNodeIndex, duration: f32, phase_offset: f32, cycle_distance: f32) -> Self {
        Slot { node, playback: Playback::Gait { duration, phase_offset, cycle_distance } }
    }

    pub fn one_shot(node: AnimationNodeIndex, speed: f32) -> Self {
        Slot { node, playback: Playback::OneShot { speed } }
    }
}

/// The live blend state of one model. Lives on the **model child** that owns the scene, never on the
/// sim entity (see the module docs).
#[derive(Component)]
pub struct PoseBlender {
    /// The asynchronously-spawned `AnimationPlayer` entity this drives.
    pub player: Entity,
    /// The shared slot table, cloned by refcount from the creature's animation resource.
    slots: Arc<[Slot]>,
    /// Live weights, eased toward `target`.
    weight: Vec<f32>,
    /// Weights the driver asked for this frame.
    target: Vec<f32>,
    /// The shared gait phase, `[0, 1)`. Continuous for the model's whole life — never reset, so a
    /// gait that fades out and back in resumes exactly where it left off.
    phase: f32,
    /// Ground speed in world units/second, used to advance `phase`. Cosmetically smoothed by the
    /// driver; see `squad::drive_valkyrie_animation`.
    ground_speed: f32,
    /// A one-shot the driver asked to (re)start, consumed by [`apply_pose_blenders`].
    pending_shot: Option<usize>,
    /// The one-shot currently playing, if any. Cleared when the clip reports finished.
    active_shot: Option<usize>,
    /// False until the first [`apply_pose_blenders`] pass, which snaps `weight` to `target` instead of
    /// easing from zero — otherwise a freshly streamed-in model shows one frame of bind pose.
    primed: bool,
    /// True while a scrub is pinning the phase — see [`PoseBlender::hold_phase`]. The runtime never
    /// sets this; it exists for the editor's bench.
    held: bool,
}

impl PoseBlender {
    /// Public so tests can drive the apply pass without a loaded scene; production code goes through
    /// [`attach_pose_blenders`], whose wiring also makes the clips resident on the player.
    pub fn new(player: Entity, slots: Arc<[Slot]>) -> Self {
        let n = slots.len();
        PoseBlender {
            player,
            slots,
            weight: vec![0.0; n],
            target: vec![0.0; n],
            phase: 0.0,
            ground_speed: 0.0,
            pending_shot: None,
            active_shot: None,
            primed: false,
            held: false,
        }
    }

    /// Number of wired slots.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Overwrite every target weight. `weights` must be exactly [`Self::len`] long; a shorter or
    /// longer slice is a wiring bug, so it is refused loudly rather than padded.
    pub fn set_targets(&mut self, weights: &[f32]) -> Result<(), PoseBlendError> {
        if weights.len() != self.target.len() {
            return Err(PoseBlendError::SlotCount { expected: self.target.len(), got: weights.len() });
        }
        self.target.copy_from_slice(weights);
        Ok(())
    }

    /// Put all the weight on one slot — the whole API a discrete state machine (crab, manca) needs.
    ///
    /// An out-of-range `slot` is a wiring bug (a driver `match` and the slot table disagree). It is
    /// refused loudly and the previous targets are kept: zeroing every target instead would fade the
    /// whole model to bind pose, a *silent* T-pose that is much harder to trace than a log line.
    pub fn set_only(&mut self, slot: usize) {
        if slot >= self.target.len() {
            error!("pose blender has {} slots, set_only({slot}) is out of range", self.target.len());
            return;
        }
        for (i, t) in self.target.iter_mut().enumerate() {
            *t = if i == slot { 1.0 } else { 0.0 };
        }
    }

    /// Ground speed driving the shared gait phase, world units/second.
    pub fn set_ground_speed(&mut self, speed: f32) {
        self.ground_speed = speed;
    }

    /// (Re)start a [`Playback::OneShot`] slot from its first frame. Re-triggering while it is already
    /// playing restarts it, which is what sustained fire wants.
    ///
    /// Triggering a slot that is not a `OneShot` (or does not exist) is a wiring bug; it is refused
    /// loudly here rather than silently dropped by the apply pass.
    pub fn trigger(&mut self, slot: usize) {
        match self.slots.get(slot).map(|s| s.playback) {
            Some(Playback::OneShot { .. }) => self.pending_shot = Some(slot),
            Some(other) => {
                error!("trigger({slot}) refused: slot is {other:?}, not a OneShot");
            }
            None => {
                error!("pose blender has {} slots, trigger({slot}) is out of range", self.slots.len());
            }
        }
    }

    /// The one-shot slot currently playing, if any. `None` once the clip has run through, which is
    /// how a driver knows it may hand the layer back to the looping clips.
    pub fn active_shot(&self) -> Option<usize> {
        self.active_shot
    }

    /// The eased weight of a slot — the value actually handed to the `AnimationPlayer` last frame.
    /// Exposed for the harness oracle in `tests/liveness.rs`.
    pub fn live_weight(&self, slot: usize) -> f32 {
        self.weight.get(slot).copied().unwrap_or(0.0)
    }

    /// The weight a driver last *asked* for. Read before overwriting the targets, this is a one-frame
    /// memory of the driver's own request — which is how a state machine detects the edge into a
    /// one-shot state without keeping a second copy of its state (see `parasite::drive_manca_animation`).
    pub fn target_weight(&self, slot: usize) -> f32 {
        self.target.get(slot).copied().unwrap_or(0.0)
    }

    /// The shared gait phase, `[0, 1)`.
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// **Pin the shared phase for scrubbing.** While held, [`apply_pose_blenders`] eases weights
    /// and writes seek times exactly as ever but does not advance φ.
    ///
    /// This exists because pausing cannot be faked through `set_ground_speed(0.0)`:
    /// [`gait_cycles_per_sec`] clamps to half the nominal cadence at the low end — deliberately,
    /// with a test — so an unheld blender at zero speed still walks. A bench that wrote seek times
    /// itself instead would be a second author of the one formula, which is the drift this crate's
    /// header exists to prevent. The runtime never calls this.
    pub fn hold_phase(&mut self, phase: f32) {
        self.phase = wrap01(phase);
        self.held = true;
    }

    /// Resume advancing φ from wherever it sits.
    pub fn release_phase(&mut self) {
        self.held = false;
    }
}

/// The one way [`PoseBlender::set_targets`] can be misused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoseBlendError {
    SlotCount { expected: usize, got: usize },
}

impl std::fmt::Display for PoseBlendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoseBlendError::SlotCount { expected, got } => {
                write!(f, "pose blender wants {expected} weights, got {got}")
            }
        }
    }
}

/// The shared graph + slot table for one creature kind, pinned by **spawn code** on the entity that
/// should own the [`PoseBlender`] — the `FigurineModel` child for the squad figurine, the creature
/// root for crab/manca (exactly where the prior animation link lived; see the module docs on
/// determinism). [`attach_pose_blenders`] walks up from every freshly streamed-in `AnimationPlayer`
/// to the nearest ancestor carrying one and wires it, so a new creature needs **no attach system** —
/// insert this at spawn and write a driver.
///
/// Both fields are shared by refcount (`Handle` + `Arc`), so cloning one resource-held source onto
/// every spawned instance is two refcount bumps, never a copy of the table.
#[derive(Component, Clone)]
pub struct BlendSource {
    pub graph: Handle<AnimationGraph>,
    pub slots: Arc<[Slot]>,
}

/// The attach pass ([`attach_pose_blenders`]). Creature drivers order themselves
/// `.after(PoseAttachSet).before(PoseBlendSet)` so a model whose skeleton streamed in this frame
/// gets its first targets the same frame (Bevy inserts the command flush on the ordered edge).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoseAttachSet;

/// Wire every freshly streamed-in `AnimationPlayer` to the nearest ancestor [`BlendSource`]. The one
/// attach path for every creature (squad figurine, crab, manca, and whatever comes next); a player
/// with no sourced ancestor — the Researcher's static flashlight scene, the chest-wound decoration —
/// is deliberately left unwired.
pub fn attach_pose_blenders(
    mut commands: Commands,
    // One query, mutably: `Added<AnimationPlayer>` reads the component, so a second
    // `Query<&mut AnimationPlayer>` alongside it would conflict.
    mut added: Query<(Entity, &mut AnimationPlayer), Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    sources: Query<&BlendSource>,
) {
    for (player_entity, mut player) in &mut added {
        let mut cur = player_entity;
        let found = loop {
            if let Ok(source) = sources.get(cur) {
                break Some((cur, source));
            }
            match parents.get(cur) {
                Ok(child_of) => cur = child_of.parent(),
                Err(_) => break None,
            }
        };
        let Some((owner, source)) = found else { continue };
        wire_pose_blender(
            &mut commands,
            player_entity,
            &mut player,
            source.graph.clone(),
            owner,
            source.slots.clone(),
        );
    }
}

/// Wire a freshly spawned `AnimationPlayer` to a blend set: point it at the shared graph, make every
/// slot resident at zero weight, and hand `owner` its [`PoseBlender`].
///
/// Called only from [`attach_pose_blenders`], which found `owner` by walking up to the nearest
/// [`BlendSource`]. Every clip is started **once, here** — after this the player's active-animation
/// set never changes except for one-shot restarts.
fn wire_pose_blender(
    commands: &mut Commands,
    player_entity: Entity,
    player: &mut AnimationPlayer,
    graph: Handle<AnimationGraph>,
    owner: Entity,
    slots: Arc<[Slot]>,
) {
    for slot in slots.iter() {
        let active = player.play(slot.node);
        active.set_weight(0.0);
        match slot.playback {
            Playback::Free { speed } => {
                active.repeat().set_speed(speed);
            }
            Playback::Gait { .. } => {
                // Paused: `advance_animations` skips it, so `apply_pose_blenders` owns its seek time.
                // `animate_targets` still evaluates paused clips — only event triggering is gated on
                // `!paused` — so the pose is unaffected.
                active.repeat().pause();
            }
            Playback::OneShot { speed } => {
                // `RepeatAnimation::Never` is the default, so it contributes nothing until the first
                // `trigger` restarts it.
                active.set_speed(speed);
            }
        }
    }
    commands.entity(player_entity).insert(AnimationGraphHandle(graph));
    commands.entity(owner).insert(PoseBlender::new(player_entity, slots));
}

/// Cycles per second for a gait mixture.
///
/// `speed / mean_cycle_distance` is the cadence that keeps the feet planted at this ground speed — the
/// blend-space generalisation of the single-clip playback-rate correction of Game AI Pro 2 §36.2.5.
/// It is clamped to [`PHASE_RATE_CLAMP`] × the mixture's own authored cadence, so a unit moving far
/// outside the range its clips were authored for degrades to a fast-but-readable stride instead of a
/// blur. Returns `0.0` when no gait clip carries weight — the phase then simply holds, and because it
/// is never reset, resuming is seamless.
///
/// `weight_sum` = Σ wᵢ, `weighted_distance` = Σ wᵢ·cycle_distanceᵢ, `weighted_cadence` = Σ wᵢ/durationᵢ,
/// all over the [`Playback::Gait`] slots.
pub fn gait_cycles_per_sec(
    speed: f32,
    weight_sum: f32,
    weighted_distance: f32,
    weighted_cadence: f32,
) -> f32 {
    if weight_sum <= WEIGHT_EPS || weighted_distance <= 1.0e-6 || weighted_cadence <= 1.0e-6 {
        return 0.0;
    }
    let mean_distance = weighted_distance / weight_sum;
    let nominal = weighted_cadence / weight_sum;
    let (lo, hi) = PHASE_RATE_CLAMP;
    (speed / mean_distance).clamp(lo * nominal, hi * nominal)
}

/// Wrap a phase into `[0, 1)`. `rem_euclid` already does this for finite inputs; a non-finite phase
/// would poison every seek time downstream, so it is caught here and reset rather than propagated.
pub fn wrap01(phase: f32) -> f32 {
    if !phase.is_finite() {
        return 0.0;
    }
    phase.rem_euclid(1.0)
}

/// Ease every model's weights toward its driver's targets, advance the shared gait phase, and write
/// both to the `AnimationPlayer`. Runs on `Update`, after every creature's driver (see
/// [`PoseBlendSet`]).
pub fn apply_pose_blenders(
    time: Res<Time>,
    mut blenders: Query<&mut PoseBlender>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    // Frame-rate-independent exponential ease: the fraction of the remaining distance covered this
    // frame. Identical settling time at 30 fps and 240 fps.
    let ease = 1.0 - (-dt / FADE_TAU).exp();

    for mut blender in &mut blenders {
        let Ok(mut player) = players.get_mut(blender.player) else {
            continue; // the scene's player has not streamed in (or was despawned)
        };
        // Refcount bump, so the slot table can be read while the weights are written.
        let slots = blender.slots.clone();

        // --- one-shot bookkeeping ---------------------------------------------------------------
        if let Some(slot) = blender.pending_shot.take()
            && let Some(s) = slots.get(slot)
            && matches!(s.playback, Playback::OneShot { .. })
        {
            player.start(s.node); // a one-shot restarting IS the intent (sustained fire keeps recoiling)
            blender.active_shot = Some(slot);
        }
        if let Some(slot) = blender.active_shot
            && slots
                .get(slot)
                .is_none_or(|s| player.animation(s.node).is_none_or(|a| a.is_finished()))
        {
            blender.active_shot = None;
        }

        // --- weights ----------------------------------------------------------------------------
        // The first pass snaps instead of easing: easing up from zero would show a frame of bind pose
        // on a model that has only just streamed in.
        let k = if blender.primed { ease } else { 1.0 };
        blender.primed = true;
        for i in 0..slots.len() {
            let t = blender.target[i];
            let w = blender.weight[i];
            let mut next = w + (t - w) * k;
            // A clip that has faded out snaps to *bit-exact* zero, which is what makes
            // `animate_targets` skip it outright instead of evaluating 186 curves nobody can see.
            // Only on the way down: clamping a rising weight would stall a fade-in that starts below
            // the epsilon, and would quietly cost the mixture its partition of unity.
            if t <= 0.0 && next < WEIGHT_EPS {
                next = 0.0;
            }
            blender.weight[i] = next;
        }

        // --- shared gait phase ------------------------------------------------------------------
        let mut weight_sum = 0.0;
        let mut weighted_distance = 0.0;
        let mut weighted_cadence = 0.0;
        for (i, slot) in slots.iter().enumerate() {
            if let Playback::Gait { duration, cycle_distance, .. } = slot.playback {
                let w = blender.weight[i];
                weight_sum += w;
                weighted_distance += w * cycle_distance;
                if duration > 1.0e-6 {
                    weighted_cadence += w / duration;
                }
            }
        }
        let cps = gait_cycles_per_sec(
            blender.ground_speed,
            weight_sum,
            weighted_distance,
            weighted_cadence,
        );
        if !blender.held {
            blender.phase = wrap01(blender.phase + cps * dt);
        }
        let phase = blender.phase;

        // --- write through ----------------------------------------------------------------------
        for (i, slot) in slots.iter().enumerate() {
            let Some(active) = player.animation_mut(slot.node) else {
                continue; // only reachable if something else stopped the clip
            };
            active.set_weight(blender.weight[i]);
            if let Playback::Gait { duration, phase_offset, .. } = slot.playback {
                // `set_seek_time` (not `seek_to`) so the jump never replays events between the two times.
                active.set_seek_time(wrap01(phase + phase_offset) * duration);
            }
        }
    }
}

/// The apply pass. Every creature's driver is registered `.before(PoseBlendSet)` so it writes this
/// frame's targets first.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoseBlendSet;

pub struct PoseBlendPlugin;

impl Plugin for PoseBlendPlugin {
    fn build(&self, app: &mut App) {
        // Cosmetic: `Update`, never `FixedUpdate` — nothing here appears in `snapshot_hash`.
        // Attach before apply, so a blender wired this frame is applied this frame (the ordered edge
        // makes Bevy flush the wire commands in between) even for a creature with no driver yet.
        app.add_systems(
            Update,
            (
                attach_pose_blenders.in_set(PoseAttachSet),
                apply_pose_blenders.in_set(PoseBlendSet).after(PoseAttachSet),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk covers 1.388 world units per 1.417 s cycle; run covers 2.135 per 0.750 s (measured from
    /// the planted foot's travel — see `squad`'s gait table). A pure clip at its own authored speed
    /// must come out at rate 1.0, i.e. its authored cadence.
    #[test]
    fn a_pure_clip_at_its_authored_speed_runs_at_cadence_one() {
        let walk_cps = gait_cycles_per_sec(1.388 / 1.417, 1.0, 1.388, 1.0 / 1.417);
        assert!(
            (walk_cps - 1.0 / 1.417).abs() < 1.0e-4,
            "walk at its authored speed should play at 1×: {walk_cps}"
        );
        let run_cps = gait_cycles_per_sec(2.135 / 0.750, 1.0, 2.135, 1.0 / 0.750);
        assert!(
            (run_cps - 1.0 / 0.750).abs() < 1.0e-4,
            "run at its authored speed should play at 1×: {run_cps}"
        );
    }

    /// A 50/50 walk/run mixture at the mixture's own authored speed must also land near cadence 1 —
    /// this is the property that keeps the feet planted *through* the overlap band, not just at its ends.
    #[test]
    fn a_mixture_at_its_own_authored_speed_stays_near_cadence_one() {
        let (ww, wr) = (0.5, 0.5);
        let weight_sum = ww + wr;
        let weighted_distance = ww * 1.388 + wr * 2.135;
        let weighted_cadence = ww / 1.417 + wr / 0.750;
        let nominal = weighted_cadence / weight_sum;
        let authored_speed = weighted_distance / weight_sum * nominal;
        let cps = gait_cycles_per_sec(authored_speed, weight_sum, weighted_distance, weighted_cadence);
        assert!((cps - nominal).abs() < 1.0e-4, "mixture cadence drifted: {cps} vs {nominal}");
    }

    #[test]
    fn the_gait_rate_is_clamped_at_both_ends() {
        let (lo, hi) = PHASE_RATE_CLAMP;
        let nominal = 1.0 / 1.417;
        let fast = gait_cycles_per_sec(1000.0, 1.0, 1.388, nominal);
        assert!((fast - hi * nominal).abs() < 1.0e-5, "runaway speed must clamp: {fast}");
        let slow = gait_cycles_per_sec(0.001, 1.0, 1.388, nominal);
        assert!((slow - lo * nominal).abs() < 1.0e-5, "a crawl must not freeze the legs: {slow}");
    }

    #[test]
    fn no_gait_weight_freezes_the_phase_instead_of_dividing_by_zero() {
        assert_eq!(gait_cycles_per_sec(6.0, 0.0, 0.0, 0.0), 0.0);
        assert_eq!(gait_cycles_per_sec(6.0, 1.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn phase_wraps_into_the_unit_interval() {
        for x in [-3.25_f32, -0.001, 0.0, 0.5, 1.0, 1.25, 97.75] {
            let w = wrap01(x);
            assert!((0.0..1.0).contains(&w), "wrap01({x}) = {w} escaped [0,1)");
        }
        assert_eq!(wrap01(f32::NAN), 0.0);
        assert_eq!(wrap01(f32::INFINITY), 0.0);
    }

    /// The ease is a fixed fraction of the *remaining* distance, so if the targets sum to 1 the live
    /// weights keep summing to 1 — which is what lets the driver express an upper-body layer as
    /// `(1-α)` on the locomotion slots and `α` on the action slots without the total drifting.
    #[test]
    fn easing_preserves_a_partition_of_unity() {
        let mut w = [0.25_f32, 0.25, 0.5];
        let t = [0.0_f32, 0.7, 0.3];
        for _ in 0..200 {
            let k = 1.0 - (-(1.0 / 60.0) / FADE_TAU).exp();
            for i in 0..3 {
                w[i] += (t[i] - w[i]) * k;
            }
            let sum: f32 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1.0e-4, "weights drifted off unity: {sum}");
        }
    }

    // --- the apply pass, driven through a real `App` -------------------------------------------
    //
    // No assets and no GPU: `apply_pose_blenders` only ever touches `AnimationPlayer`, never the clip
    // assets, so a graph-less player with hand-made node indices exercises the real system exactly.

    /// One simulated frame at a fixed 60 Hz. `TimePlugin` is deliberately absent: it drives `Time` from
    /// the wall clock, and a tight test loop would hand the ease a delta of a few microseconds, making
    /// every settling assertion vacuous. Driving `Time` directly also skips Bevy's zero-delta first frame.
    fn tick(app: &mut App) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f64(1.0 / 60.0));
        app.update();
    }

    /// An idle/walk/run set: two gait slots sharing a phase plus a free idle and a one-shot, wired onto
    /// a bare `AnimationPlayer`. Returns `(app, blender entity, player entity)`.
    fn harness() -> (App, Entity, Entity) {
        let slots: Arc<[Slot]> = Arc::from([
            Slot::free(AnimationNodeIndex::new(1), 1.0),
            Slot::gait(AnimationNodeIndex::new(2), 1.417, 0.000, 1.388),
            Slot::gait(AnimationNodeIndex::new(3), 0.750, -0.016, 2.135),
            Slot::one_shot(AnimationNodeIndex::new(4), 1.0),
        ]);

        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.add_systems(Update, apply_pose_blenders);

        let mut player = AnimationPlayer::default();
        for slot in slots.iter() {
            let active = player.play(slot.node);
            active.set_weight(0.0);
            match slot.playback {
                Playback::Free { speed } => {
                    active.repeat().set_speed(speed);
                }
                Playback::Gait { .. } => {
                    active.repeat().pause();
                }
                Playback::OneShot { speed } => {
                    active.set_speed(speed);
                }
            }
        }
        let player_entity = app.world_mut().spawn(player).id();
        let blender = app
            .world_mut()
            .spawn(PoseBlender::new(player_entity, slots))
            .id();
        (app, blender, player_entity)
    }

    fn set(app: &mut App, blender: Entity, targets: &[f32], speed: f32) {
        let mut b = app.world_mut().get_mut::<PoseBlender>(blender).expect("blender");
        b.set_targets(targets).expect("slot count");
        b.set_ground_speed(speed);
    }

    fn weights(app: &mut App, blender: Entity) -> Vec<f32> {
        let b = app.world().get::<PoseBlender>(blender).expect("blender");
        (0..b.len()).map(|i| b.live_weight(i)).collect()
    }

    /// The whole point of the module: sweeping the targets from a standstill to a full run and back
    /// must never step a weight by more than one ease-worth, and the weights the `AnimationPlayer`
    /// actually receives must match the ones the blender reports.
    #[test]
    fn weights_ease_smoothly_and_reach_the_player() {
        let (mut app, blender, player_entity) = harness();
        // Prime: the first pass snaps, which is deliberate (a freshly streamed-in model must not show
        // a frame of bind pose), so take the sweep's baseline after it.
        set(&mut app, blender, &[1.0, 0.0, 0.0, 0.0], 0.0);
        tick(&mut app);
        assert_eq!(
            weights(&mut app, blender),
            vec![1.0, 0.0, 0.0, 0.0],
            "the first pass must snap, not ease up from bind pose"
        );

        let mut prev = weights(&mut app, blender);
        // Idle → walk → run → idle, flipping the targets outright each time (the worst case).
        for (step, targets) in [
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ]
        .into_iter()
        .enumerate()
        {
            for _ in 0..40 {
                set(&mut app, blender, &targets, 3.0);
                tick(&mut app);
                let now = weights(&mut app, blender);
                for (i, (a, b)) in prev.iter().zip(now.iter()).enumerate() {
                    assert!(b.is_finite(), "slot {i} went non-finite at step {step}");
                    // One frame can only close `ease` of the remaining gap, and the gap is at most 1.
                    assert!(
                        (b - a).abs() <= 0.5,
                        "slot {i} jumped {} at step {step} — {prev:?} → {now:?}",
                        (b - a).abs()
                    );
                }
                prev = now;
            }
            // Having held the target for 40 frames, the mixture must have essentially arrived.
            for (i, (w, t)) in prev.iter().zip(targets.iter()).enumerate() {
                assert!((w - t).abs() < 0.05, "slot {i} settled at {w}, wanted {t}");
            }
        }

        // And the player is what actually got them.
        let player = app.world().get::<AnimationPlayer>(player_entity).expect("player");
        for (i, slot) in [1u32, 2, 3, 4].into_iter().enumerate() {
            let active = player.animation(AnimationNodeIndex::new(slot as usize)).expect("clip");
            assert!(
                (active.weight() - prev[i]).abs() < 1.0e-6,
                "slot {i}: player has {}, blender reports {}",
                active.weight(),
                prev[i]
            );
        }
    }

    /// Both gait clips must be driven from the one phase, so their seek times stay locked in a fixed
    /// ratio no matter how the weights are mixed — that is what stops the feet scissoring.
    #[test]
    fn gait_clips_share_one_phase() {
        let (mut app, blender, player_entity) = harness();
        set(&mut app, blender, &[0.0, 0.5, 0.5, 0.0], 2.0);
        for _ in 0..90 {
            tick(&mut app);
        }
        let phase = app.world().get::<PoseBlender>(blender).expect("blender").phase();
        assert!((0.0..1.0).contains(&phase), "phase escaped [0,1): {phase}");
        assert!(phase > 0.0, "the phase must actually advance while the unit moves");

        let player = app.world().get::<AnimationPlayer>(player_entity).expect("player");
        let walk = player.animation(AnimationNodeIndex::new(2)).expect("walk");
        let run = player.animation(AnimationNodeIndex::new(3)).expect("run");
        assert!(
            (walk.seek_time() - wrap01(phase + 0.000) * 1.417).abs() < 1.0e-5,
            "walk seek {} is not φ·duration for φ={phase}",
            walk.seek_time()
        );
        assert!(
            (run.seek_time() - wrap01(phase - 0.016) * 0.750).abs() < 1.0e-5,
            "run seek {} is not (φ+offset)·duration for φ={phase}",
            run.seek_time()
        );
        assert!(walk.is_paused() && run.is_paused(), "gait clips must stay paused — we own their time");
    }

    /// **A held phase freezes while everything else keeps working** — the bench's scrub contract.
    /// `set_ground_speed(0.0)` cannot pause: the cadence clamp floors at half the nominal rate,
    /// deliberately. Holding pins φ; weights still ease and the seek times follow the formula.
    #[test]
    fn a_held_phase_freezes_while_weights_still_ease() {
        let (mut app, blender, player_entity) = harness();
        set(&mut app, blender, &[0.0, 1.0, 0.0, 0.0], 2.0);
        for _ in 0..30 {
            tick(&mut app);
        }
        app.world_mut()
            .get_mut::<PoseBlender>(blender)
            .expect("blender")
            .hold_phase(0.25);
        // Retarget while held: the freeze is the phase's alone.
        for _ in 0..40 {
            set(&mut app, blender, &[0.0, 0.0, 1.0, 0.0], 2.0);
            tick(&mut app);
        }
        let b = app.world().get::<PoseBlender>(blender).expect("blender");
        assert!(
            (b.phase() - 0.25).abs() < 1.0e-6,
            "held phase drifted to {}",
            b.phase()
        );
        assert!(b.live_weight(2) > 0.9, "weights must keep easing while held");
        let player = app.world().get::<AnimationPlayer>(player_entity).expect("player");
        let run = player.animation(AnimationNodeIndex::new(3)).expect("run");
        assert!(
            (run.seek_time() - wrap01(0.25 - 0.016) * 0.750).abs() < 1.0e-5,
            "a held phase still writes seek through the one formula: {}",
            run.seek_time()
        );

        // Release resumes the advance from where it sits.
        app.world_mut()
            .get_mut::<PoseBlender>(blender)
            .expect("blender")
            .release_phase();
        for _ in 0..10 {
            tick(&mut app);
        }
        let phase = app.world().get::<PoseBlender>(blender).expect("blender").phase();
        assert!(
            (phase - 0.25).abs() > 1.0e-4,
            "a released phase must advance again"
        );
    }

    /// A standing model must not stride in place: with no gait weight the phase holds, and because it
    /// is never reset, resuming picks up exactly where it stopped.
    #[test]
    fn the_phase_holds_while_idle_and_resumes_where_it_left_off() {
        let (mut app, blender, _) = harness();
        set(&mut app, blender, &[0.0, 1.0, 0.0, 0.0], 1.0);
        for _ in 0..60 {
            tick(&mut app);
        }
        let moving = app.world().get::<PoseBlender>(blender).expect("blender").phase();

        set(&mut app, blender, &[1.0, 0.0, 0.0, 0.0], 0.0);
        for _ in 0..60 {
            tick(&mut app);
        }
        let stopped = app.world().get::<PoseBlender>(blender).expect("blender").phase();
        // The gait weight eases out rather than snapping, so a little phase is still consumed on the
        // way down — but nowhere near the ~0.7 cycles/s it was covering while moving.
        assert!(
            (stopped - moving).abs() < 0.2,
            "the phase kept running after the model stopped: {moving} → {stopped}"
        );
    }

    #[test]
    fn a_one_shot_restarts_on_trigger_and_reports_when_it_is_done() {
        let (mut app, blender, player_entity) = harness();
        set(&mut app, blender, &[1.0, 0.0, 0.0, 0.0], 0.0);
        tick(&mut app);
        assert_eq!(
            app.world().get::<PoseBlender>(blender).expect("blender").active_shot(),
            None,
            "nothing should be playing before the first trigger"
        );

        app.world_mut().get_mut::<PoseBlender>(blender).expect("blender").trigger(3);
        tick(&mut app);
        assert_eq!(
            app.world().get::<PoseBlender>(blender).expect("blender").active_shot(),
            Some(3)
        );
        let player = app.world().get::<AnimationPlayer>(player_entity).expect("player");
        let shot = player.animation(AnimationNodeIndex::new(4)).expect("one-shot");
        assert!(!shot.is_finished(), "a just-triggered one-shot must not read as finished");

        // `AnimationPlayer::start` rewinds it — which for a recoil is the whole point.
        assert_eq!(shot.seek_time(), 0.0);
    }

    /// Misuse is refused loudly and safely: an out-of-range `set_only` keeps the previous targets
    /// (never a silent fade to bind pose), and a `trigger` on a non-one-shot or missing slot is
    /// dropped at the call site instead of silently vanishing in the apply pass.
    #[test]
    fn slot_misuse_is_refused_without_wrecking_the_pose() {
        let (mut app, blender, _) = harness();
        set(&mut app, blender, &[0.0, 1.0, 0.0, 0.0], 1.0);
        tick(&mut app);

        {
            let mut b = app.world_mut().get_mut::<PoseBlender>(blender).expect("blender");
            b.set_only(99); // out of range — refused, previous targets kept
            assert_eq!(b.target_weight(1), 1.0, "set_only(99) must keep the previous targets");
            b.trigger(0); // slot 0 is Free, not OneShot — refused
            b.trigger(99); // out of range — refused
        }
        tick(&mut app);
        assert_eq!(
            app.world().get::<PoseBlender>(blender).expect("blender").active_shot(),
            None,
            "refused triggers must never start a shot"
        );
        assert_eq!(
            app.world().get::<PoseBlender>(blender).expect("blender").target_weight(1),
            1.0,
            "the walk target must survive the refused calls"
        );
    }
}
