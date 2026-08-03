//! Mouse control of the squad — a real RTS scheme: **left button selects, right button commands.**
//!
//! # What changed, and why the old scheme had to go
//!
//! The squad used to be *permanently and entirely* selected: `keep_squad_selected` re-inserted
//! [`Selected`] on every unit every frame, so the player could neither select nor deselect. That
//! made three pieces of machinery into decoration — the green ring drawn on all five units always,
//! the [`Selected`] component itself, and the `Move` cursor, which was live 100% of the time and so
//! carried no information. A cursor that never changes is not a cursor state.
//!
//! The scheme now:
//!
//! | Input | Meaning |
//! |---|---|
//! | Left click | Select the unit under the cursor, or clear the selection |
//! | Left drag | Marquee: select every unit inside the box |
//! | Shift + left | Add to the selection instead of replacing it |
//! | Double left click | Select every operative of that unit's role |
//! | `Ctrl+A` | Select the whole squad |
//! | Right click | Move order for the selection — **or** disarm, if a verb is armed |
//! | Shift + right | Queue the order behind the current one |
//! | `1`–`9` | Recall a control group · `Ctrl` + digit binds one |
//! | `G` | Latch the selection to **advance to contact** instead of holding ([`crate::squad::PushOrder`]) |
//!
//! Right-click keeps its disarm meaning whenever a containment verb is armed, so the one escape
//! from a modal verb that existed before still exists. With nothing armed it is the command button,
//! which is where two decades of RTS muscle memory expects it.
//!
//! # Evidence, and its limits
//!
//! **State this honestly: RTS input design is essentially absent from peer review.** Targeted
//! searches across OpenAlex/CrossRef turn up a large RTS *AI* literature and a decent *expertise*
//! literature, but nothing on selection models, control groups, tactical pause, or hybrid schemes.
//! The scheme above is convention plus first principles, not a cited result.
//!
//! What *is* evidenced, and what shaped it:
//!
//! - **Offering a fast path beside a slow one is not enough.** Cockburn, Gutwin, Scarr & Malacria
//!   2014 (*Supporting Novice to Expert Transitions in User Interfaces*, ACM Comput. Surv. 47(2),
//!   DOI 10.1145/2659796) document the intermodal-transition failure: users plateau on the slow
//!   method and never switch, because no single moment is painful enough to justify the switching
//!   cost. So control groups cannot merely *exist* — the selection readout has to show that a group
//!   was bound, or nobody will ever bind one.
//! - **The skill bottleneck moves.** Thompson, Blair, Chen & Henrey 2013 (PLoS ONE 8(9),
//!   DOI 10.1371/journal.pone.0075129), 3,360 StarCraft II players: which cognitive-motor variable
//!   predicts league *changes with league*. Early it is knowing which order to give; later it is
//!   retargeting speed. A scheme needs a low floor and a separately-designed ceiling, and they are
//!   not the same design problem.
//! - **~23% of a genre-experienced sample cannot clear a controls barrier in 20 minutes.**
//!   Iacovides et al. 2015 dropped 7 of 31 screened participants — all self-reported FPS players —
//!   for "obviously struggling with the controls". That is the budget onboarding has to beat.
//!
//! Commands use a single cursor ray → ground-plane hit (no mesh picking needed): the hit world point
//! is the move target. Green rings are drawn with `gizmos.circle` (no per-unit ring entities to
//! manage).
//!
//! # Determinism
//!
//! Selection is player-only — the harness presses nothing, so every unit stays unselected there and
//! `command_input` finds an empty query. But this module *writes* [`MoveOrder`], which is pinned, so
//! two rules apply. Sets that outlive a frame ([`ControlGroups`]) store `SquadMember` **indices**,
//! never `Entity` (ids are not stable across `App` instances), in `sort_total!` order. And the order
//! queue is strictly per-entity — no shared counter, no budget, no last-writer-wins.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use std::sync::Arc;

use crate::audio::Sfx;
use crate::dialogue::ConversationLock;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::containment::{ArmedTool, DeviceSupply, QuarantineSupply, TargetId};
use crate::squad::{MoveOrder, Selected, SquadMember, Unit};
use crate::squad_ai::role::RoleId;

/// Radius of the green selection ring.
const RING_RADIUS: f32 = 0.6;

/// How near the cursor a unit must be, in world units, for a bare click to select it. Generous
/// relative to [`RING_RADIUS`] for the same reason `aim_tolerance` is generous relative to reach: a
/// click that misses by a pixel should not teach the player that clicking units does nothing.
const PICK_RADIUS: f32 = 1.0;

/// Screen-space drag, in logical pixels, below which a left-button press+release is a *click* rather
/// than a marquee. Without a deadzone every click is a 1×2-pixel box that selects nothing, and the
/// player learns that clicking a unit deselects it.
const MARQUEE_DEADZONE: f32 = 6.0;

/// Seconds within which a second click on the same unit reads as a double-click (select by role).
const DOUBLE_CLICK_SECS: f32 = 0.35;

/// How many queued orders a unit will hold. Bounded so a player leaning on shift-click cannot grow
/// an unbounded queue — the cap is loud (the readout shows the count), not silent.
const MAX_QUEUED_ORDERS: usize = 8;

/// Outward ring search bound (in cells) for snapping a click on void/wall to the nearest floor so
/// the group still has a reachable goal. Beyond this a click is treated as "nowhere to go".
const SNAP_MAX_RING: i32 = 8;

/// How far from the cursor a verb will still claim a target (world units).
///
/// Deliberately independent of — and more forgiving than — the verbs' *reach*. This is a UI affordance
/// ("did you mean that creature?"); reach is the mechanic ("are you close enough?"). Conflating them
/// would mean a click one pixel off reads as out-of-range, which teaches the player the wrong lesson
/// about why a throw failed.
const AIM_TOLERANCE: f32 = 1.2;

/// Aim tolerance for a verb whose mechanic reaches `reach` world units.
///
/// **The affordance must never be tighter than the mechanic**, which is what the doc above promises and
/// what the shipped constants violated. Measured 2026-07-28 from a play log: `AIM_TOLERANCE` is `1.2`
/// while `cap_reach` is `1.5`, `device_reach` is `2.5` and `quarantine_radius` is `3.0` — so the *aim*
/// was the binding constraint on all three verbs. A player standing well inside cap range, clicking
/// 1.3 m from a nest's centre, got `no uncapped nest under the cursor` and no way to tell whether they
/// had missed or were out of range. The log that surfaced this shows **nine** consecutive failed cap
/// attempts.
///
/// Deriving it from the reach rather than raising the constant keeps one rule instead of a number that
/// has to be re-checked every time a reach is tuned — and `max` preserves the 1.2 floor for any verb
/// whose reach is smaller, so this only ever loosens, never tightens.
fn aim_tolerance(reach: f32) -> f32 {
    reach.max(AIM_TOLERANCE)
}

/// A request to arm a verb from a source that is **not** the keyboard — today, the clickable verb
/// bar (`crate::ui::verb_bar`).
///
/// Routed as a message on purpose. [`arm_tool_input`] is the single writer of [`ArmedTool`], and a
/// UI panel reaching in to write it directly would be a second writer with its own copy of the
/// toggle-to-disarm rule and its own chance to forget the `DebugCaptureActive` stand-down. This is
/// the same discipline `ui::debrief`'s F10 dev-victory follows: send the intent, let the one owner
/// apply it.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmRequest {
    /// Arm this verb, or disarm it if it is already armed.
    Toggle(ArmedTool),
    /// Flip the latched weapons-tight stance.
    ToggleWeaponsTight,
    /// Deploy the Engineer's sensor drone (`crate::sensor`). Like `ToggleWeaponsTight`, this is not
    /// an [`ArmedTool`] — there is nothing to aim, so there is no modal state to enter.
    DeploySensor,
    /// Flip the selection between holding position and advancing to contact
    /// ([`crate::squad::PushOrder`]).
    TogglePush,
}

/// Orders waiting behind the unit's active [`MoveOrder`], appended by shift + right-click.
///
/// A separate component rather than a field on `MoveOrder` so `squad::unit_movement` — which owns
/// arrival and is deep inside the pinned `FixedUpdate` chain — needs no change at all: it still
/// removes `MoveOrder` on arrival, and [`advance_order_queue`] pops the next one in before the next
/// tick reads it. Strictly per-entity, so it introduces no shared state to order.
#[derive(Component, Default)]
pub struct OrderQueue(VecDeque<Arc<FlowField>>);

impl OrderQueue {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Append, up to [`MAX_QUEUED_ORDERS`]. Returns `false` when the queue is full — the caller
    /// reports that rather than silently dropping the order.
    fn push(&mut self, field: Arc<FlowField>) -> bool {
        if self.0.len() >= MAX_QUEUED_ORDERS {
            return false;
        }
        self.0.push_back(field);
        true
    }
}

/// The nine control groups, as `SquadMember` **indices**.
///
/// Indices and not `Entity`: entity ids are not stable across `App` instances, and a group that
/// survives a save or a replay must name operatives by something the world can re-resolve. Each
/// slot is kept sorted (`sort_total!`) so the stored form is canonical.
#[derive(Resource, Default)]
pub struct ControlGroups([Vec<usize>; 9]);

impl ControlGroups {
    pub fn get(&self, slot: usize) -> &[usize] {
        self.0.get(slot).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Which groups a given operative belongs to, as 1-based labels — what the roster chip shows so
    /// that a bound group is *visible*. Cockburn et al. 2014: an expert mechanism nobody can see
    /// having worked is one nobody adopts.
    pub fn labels_for(&self, member: usize) -> Vec<usize> {
        (0..self.0.len())
            .filter(|&slot| self.get(slot).contains(&member))
            .map(|slot| slot + 1)
            .collect()
    }

    fn bind(&mut self, slot: usize, mut members: Vec<usize>) {
        let Some(entry) = self.0.get_mut(slot) else { return };
        // SORT-OK: canonicalising a SET of distinct squad indices for storage. `dedup` after the
        // sort makes the total-order precondition hold by construction, so `sort_total!`'s tie panic
        // would be unreachable rather than protective.
        members.sort_unstable();
        members.dedup();
        *entry = members;
    }
}

/// Transient left-drag state for the marquee.
#[derive(Resource, Default)]
pub struct Marquee {
    /// Where the left button went down, in logical screen px. `None` while not dragging.
    pub anchor: Option<Vec2>,
    /// Live cursor position while dragging, so the UI can draw the box.
    pub current: Vec2,
}

impl Marquee {
    /// The screen-space rect being dragged, once it clears [`MARQUEE_DEADZONE`].
    pub fn rect(&self) -> Option<Rect> {
        let anchor = self.anchor?;
        let d = (self.current - anchor).abs();
        if d.x < MARQUEE_DEADZONE && d.y < MARQUEE_DEADZONE {
            return None;
        }
        Some(Rect::from_corners(anchor, self.current))
    }
}

/// Remembers the last unit clicked and when, so a second click on it reads as "select this role".
#[derive(Resource, Default)]
pub struct LastClick {
    entity: Option<Entity>,
    at: f32,
}

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        // Same rule as `UiPlugin`: `command_input` reads `DebugCaptureActive` non-optionally, so this
        // plugin guarantees it. Today the system is skipped headless anyway (its `Single<&Window>` finds
        // no match), which is the only reason the harness never hit the missing-resource panic `UiPlugin`
        // did — that is luck, not a contract, so claim the resource explicitly. `init_resource` is idempotent.
        app.init_resource::<crate::DebugCaptureActive>();
        // Same argument for the binding table `arm_tool_input` reads.
        crate::input::claim_bindings(app);
        // Registered here rather than in `UiPlugin` because `arm_tool_input` (the READER) lives here
        // and runs in the harness, where `UiPlugin` never exists. A message whose reader can run
        // without its registration is a panic waiting for the first headless frame.
        app.add_message::<ArmRequest>();
        // Order-issuing input runs in `RunFixedMainLoop` *before* the fixed step, not in `Update`. `Update`
        // runs after the fixed loop, so a `MoveOrder` inserted there wasn't seen by `unit_movement` (on
        // `FixedUpdate`) until the next frame — a one-frame lag. `BeforeFixedMainLoop` flushes the command
        // ahead of the loop's exclusive runner, so the mover consumes it the same frame. It still runs once
        // per frame on fresh `PreUpdate` input, so left-click edge-detection is unaffected.
        app.add_systems(
            RunFixedMainLoop,
            (
                // Left button resolves the selection before the right button commands it, so a
                // select-then-order in one frame does what it looks like.
                //
                // The `ConversationLock` guard is on these two as well, and it has to be: they are
                // LEFT-click consumers, and a conversation owns the left click (`dialogue::runtime`
                // reads it for line-advance and choice picks). Without it, clicking a dialogue bubble
                // also resolved as a click on empty floor and silently cleared the whole selection.
                selection_input.run_if(not(resource_exists::<ConversationLock>)),
                control_group_input.run_if(not(resource_exists::<ConversationLock>)),
                // **`command_input` runs BEFORE `arm_tool_input`, and the order is load-bearing.**
                // Both read the right button — one to command, one to disarm — and Bevy evaluates a
                // system's run conditions inline just before running it, *after* its `.chain()`
                // dependency has finished. So with `arm_tool_input` first, its immediate
                // `*armed = ArmedTool::None` made `resource_equals(ArmedTool::None)` pass on the very
                // same frame, and one right-click both put the verb away and marched the selection to
                // wherever the player happened to be aiming.
                //
                // Evaluating `command_input` while `ArmedTool` still holds the PRE-click value is what
                // makes the two mutually exclusive: armed ⇒ skipped, then disarmed; unarmed ⇒ a move
                // order, and `arm_tool_input`'s right-click branch is itself guarded on being armed.
                //
                // While a dialogue exchange owns the left-click, none of the verb consumers run either.
                // Exactly ONE of them may claim a given left-click, decided by `ArmedTool`.
                command_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::None)),
                throw_device_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Device)),
                place_quarantine_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Quarantine)),
                cap_nest_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Cap)),
                throw_lure_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Lure)),
                // LAST of the right-button readers — see the ordering note on `command_input`.
                arm_tool_input.run_if(not(resource_exists::<ConversationLock>)),
                // A latched stance, so it is read wherever the other verb inputs are read.
                toggle_push_order.run_if(not(resource_exists::<ConversationLock>)),
                // Refill an emptied `MoveOrder` from the queue in the SAME schedule the orders are
                // issued in — see the system's doc for why this is not on `FixedUpdate`.
                advance_order_queue,
            )
                .chain()
                // FVS-G-6: none of these mean anything without an expedition, and `command_input` /
                // `place_quarantine_input` take `Res<Dungeon>` non-optionally — the exact pair whose
                // panic blocked a world-less frame. `distributive_run_if`, not `run_if`: the tuple form
                // wraps an anonymous set whose extra graph node permutes the schedule's linearisation
                // and moves the deterministic golden by itself (measured).
                .distributive_run_if(in_state(crate::session::RunState::Active))
                // …and the player must be LOOKING at the expedition. `RunState::Active` alone was
                // right while the only route to Site-67 was to end the run first; since
                // `input::Action::VisitSite` an expedition stays `Active` while the player stands in
                // the hub, and every system above was still live there — right-click marched the squad
                // at a Site-space ray's `y = 0` hit 512+ units outside the map, one left-click both
                // walked the avatar and re-selected the squad, and the armed verbs still threw.
                //
                // A resource, not `in_state(AppState::InGame)`: `ui::state` and `ui/mod.rs` both
                // forbid gating gameplay on `AppState`, because the harness never registers it. This
                // reads `false` there and headless behaviour is unchanged — the same argument
                // `SimBlocked` makes, pinned by `replay::ui_never_leaks_into_deterministic_core`.
                // `distributive_run_if` for the reason stated above: a tuple-level `run_if` would add
                // a set node and move the golden by itself.
                .distributive_run_if(crate::time_control::orders_allowed)
                .after(crate::ui::state::sync_order_block)
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        );
        app.init_resource::<ControlGroups>()
            .init_resource::<Marquee>()
            .init_resource::<LastClick>();
        // The squad starts an expedition selected — once, on entry, not re-asserted every frame.
        // `.after(Populate)`, NOT `.in_set(Populate)`. `squad::spawn_squad` lives in that set, and two
        // systems in one set have no ordering between them — so as a member this would sometimes run
        // before the squad exists and select nothing, and sometimes after and select everyone. That
        // difference is an archetype difference (`Selected` present or absent on five entities), and
        // archetypes drive ECS iteration order, which this codebase pins goldens against. Ambiguity
        // there is the exact shape of bug `docs/ui.md` §4.3 and `tests/determinism_lint.rs` exist for.
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            select_whole_squad_on_run_start.after(crate::session::RunBuild::Populate),
        );
        // Cosmetic only — ring gizmos + cursor icon read state but feed nothing pinned, so they stay on `Update`.
        // Same two conditions as the order input, for the same reason: at the Site these would draw
        // selection rings around an off-screen squad and hand the player an armed-tool cursor for a
        // verb they cannot use. Cosmetic and on `Update`, so neither condition can touch the golden.
        app.add_systems(
            Update,
            (draw_selection_rings, update_cursor)
                .distributive_run_if(in_state(crate::session::RunState::Active))
                .distributive_run_if(crate::time_control::orders_allowed),
        );
    }
}

/// Ground point under the cursor (y = 0 plane), or `None` if off-window / no camera ray.
pub(crate) fn cursor_ground_point(
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<Vec3> {
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let dist = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    Some(ray.get_point(dist))
}

/// Select the whole squad once, when an expedition begins.
///
/// This is what remains of `keep_squad_selected`, and the difference is the whole point: it runs
/// **once on entering a run** rather than every frame, so "the squad starts selected" stays true
/// while "the player cannot deselect" stops being true. A unit spawned mid-run is deliberately
/// *not* auto-selected — joining the world should not silently join whatever the player currently
/// has picked out.
fn select_whole_squad_on_run_start(mut commands: Commands, units: Query<Entity, With<Unit>>) {
    for e in &units {
        commands.entity(e).insert(Selected);
    }
}

/// Screen position of a unit, or `None` when it is behind the camera / off-viewport.
fn unit_screen_pos(camera: &Camera, cam_tf: &GlobalTransform, world: Vec3) -> Option<Vec2> {
    camera.world_to_viewport(cam_tf, world).ok()
}

/// Left button: click-select, shift-add, marquee, double-click-by-role.
///
/// Runs before `command_input` so the right-button order in the same frame sees this frame's
/// selection.
#[allow(clippy::too_many_arguments)]
fn selection_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
    armed: Res<ArmedTool>,
    capture: Res<crate::DebugCaptureActive>,
    // Every interactive UI widget carries `Hovered` (the verb chips, the Site buttons, menu buttons);
    // every readout is `Pickable::IGNORE` and so has none. That makes "is the cursor over a control?"
    // answerable without a second hit-test of our own.
    hovered_ui: Query<&bevy::picking::hover::Hovered>,
    units: Query<(Entity, &Transform, &SquadMember, Option<&RoleId>), With<Unit>>,
    selected: Query<Entity, With<Selected>>,
    mut marquee: ResMut<Marquee>,
    mut last_click: ResMut<LastClick>,
    mut sfx: MessageWriter<Sfx>,
) {
    // The dev region-capture tool owns the mouse while armed, and a left-click belongs to the armed
    // verb (that is `throw_device_input` and friends), not to selection.
    if capture.0 || *armed != ArmedTool::None {
        marquee.anchor = None;
        return;
    }
    // **A click on a control is not a click on the world.** This system reads the raw mouse, so
    // without the check a click on the SENSOR chip resolved as a click on empty floor and cleared the
    // selection — and then the chip's own `ArmRequest` arrived to find nothing selected, so the verb
    // it had just pressed could never succeed. Every pickable widget the player can hit is a `Button`
    // with `Hovered`, so one query answers it for all of them.
    if hovered_ui.iter().any(|h| h.0) {
        marquee.anchor = None;
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, cam_tf) = *camera;
    // Shift ADDS. Every other combination replaces, which is the convention and also the safer
    // default: an accidental click costs you a selection you can rebuild, not one you must undo.
    let additive = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    if mouse.just_pressed(MouseButton::Left) {
        marquee.anchor = Some(cursor);
        marquee.current = cursor;
        return;
    }
    if mouse.pressed(MouseButton::Left) {
        marquee.current = cursor;
        return;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    marquee.current = cursor;
    let Some(anchor) = marquee.anchor.take() else {
        return; // Release without a press we saw — e.g. the button went down over a UI panel.
    };

    // --- Marquee: everything inside the box. ---
    if let Some(rect) = (Marquee { anchor: Some(anchor), current: cursor }).rect() {
        let hits: Vec<Entity> = units
            .iter()
            .filter(|(_, tf, _, _)| {
                unit_screen_pos(camera, cam_tf, tf.translation)
                    .is_some_and(|p| rect.contains(p))
            })
            .map(|(e, _, _, _)| e)
            .collect();
        apply_selection(&mut commands, &selected, &hits, additive, &mut sfx);
        return;
    }

    // --- Click: the nearest unit within `PICK_RADIUS` of the ground point under the cursor. ---
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    let picked = units
        .iter()
        .map(|(e, tf, member, role)| {
            ((tf.translation - point).length(), e, member.0, role.copied())
        })
        .filter(|(d, _, _, _)| *d <= PICK_RADIUS)
        // SORT-OK: total key — distance, then SquadMember index (unique per operative). A distance
        // tie IS reachable: wall-clamped operatives hold bit-identical coordinates (the measured
        // case in tests/determinism_lint.rs's header) — this pick's previous justification claimed
        // ties were "impossible under ORCA", the exact argument shape that lint exists to refuse.
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));

    let Some((_, entity, _, role)) = picked else {
        // Empty ground: clear, unless the player is adding.
        if !additive {
            for e in &selected {
                commands.entity(e).remove::<Selected>();
            }
        }
        return;
    };

    // Double-click on a unit selects everyone sharing its role — the cheap "select all medics" that
    // costs no extra binding and no extra thing to learn.
    let now = time.elapsed_secs();
    let is_double =
        last_click.entity == Some(entity) && (now - last_click.at) <= DOUBLE_CLICK_SECS;
    last_click.entity = Some(entity);
    last_click.at = now;

    let hits: Vec<Entity> = if is_double {
        match role {
            Some(want) => units
                .iter()
                .filter(|(_, _, _, r)| r.copied() == Some(want))
                .map(|(e, _, _, _)| e)
                .collect(),
            None => vec![entity],
        }
    } else {
        vec![entity]
    };
    apply_selection(&mut commands, &selected, &hits, additive, &mut sfx);
}

/// Replace or extend the selection. One place, so "shift adds" cannot drift between the marquee and
/// the click paths.
fn apply_selection(
    commands: &mut Commands,
    selected: &Query<Entity, With<Selected>>,
    hits: &[Entity],
    additive: bool,
    sfx: &mut MessageWriter<Sfx>,
) {
    if !additive {
        for e in selected {
            if !hits.contains(&e) {
                commands.entity(e).remove::<Selected>();
            }
        }
    }
    for &e in hits {
        commands.entity(e).insert(Selected);
    }
    if !hits.is_empty() {
        sfx.write(Sfx::MoveOrder);
    }
}

/// `Ctrl+A` selects the squad; `1`–`9` recall a control group and `Ctrl` + digit binds one.
///
/// **The digits are read raw, not through `input::Action`.** Nine numbered slots are one *mechanism*
/// with nine positions, not nine independently rebindable actions — enumerating them would add
/// eighteen enum variants and eighteen controls-screen rows for something no player thinks of as
/// separate bindings. `input::the_bare_digits_are_left_for_control_groups` is what keeps any
/// registered action from shadowing them.
fn control_group_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<crate::DebugCaptureActive>,
    owned: Res<crate::input::KeyboardOwned>,
    mut groups: ResMut<ControlGroups>,
    units: Query<(Entity, &SquadMember), With<Unit>>,
    selected: Query<Entity, With<Selected>>,
) {
    if capture.0 || owned.any() {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    // Alt is the dev inspection ladder (`time_control`); never treat it as a group command.
    if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        return;
    }

    if ctrl && keys.just_pressed(KeyCode::KeyA) {
        for (e, _) in &units {
            commands.entity(e).insert(Selected);
        }
        return;
    }

    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let Some(slot) = DIGITS.iter().position(|&k| keys.just_pressed(k)) else {
        return;
    };

    if ctrl {
        let members: Vec<usize> = units
            .iter()
            .filter(|(e, _)| selected.contains(*e))
            .map(|(_, m)| m.0)
            .collect();
        groups.bind(slot, members);
        return;
    }

    let want = groups.get(slot);
    // An unbound group is a no-op, not a "deselect everything" surprise — and so is a group whose
    // operatives have all DIED. `ControlGroups` stores `SquadMember` indices, which outlive the
    // entities, so `want` stays non-empty after `squad::despawn_dead_units` has removed everyone in
    // it; the loop below would then match nobody and strip `Selected` from every survivor. Checking
    // that the group still resolves to a live operative is what makes the guard mean what it says.
    if want.is_empty() || !units.iter().any(|(_, m)| want.contains(&m.0)) {
        return;
    }
    for (e, member) in &units {
        if want.contains(&member.0) {
            commands.entity(e).insert(Selected);
        } else {
            commands.entity(e).remove::<Selected>();
        }
    }
}

/// Toggle the selection between holding position and advancing to contact.
///
/// A **latched stance**, like `WeaponsTight` and for the same reason: a containment hold runs for
/// seconds, and asking the player to keep a key down through one would compete with the mouse.
///
/// The toggle is decided by what the *selection as a whole* is currently doing — if any selected
/// operative is pushing, the press calls everyone off; otherwise everyone advances. A per-unit toggle
/// would split a mixed selection into two halves doing opposite things from one keypress, which is
/// indistinguishable from a bug.
fn toggle_push_order(
    mut commands: Commands,
    actions: crate::input::Actions,
    mut requests: MessageReader<ArmRequest>,
    capture: Res<crate::DebugCaptureActive>,
    selected: Query<(Entity, Option<&crate::squad::PushOrder>), (With<Unit>, With<Selected>)>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 {
        return;
    }
    // Drained, so an unread request cannot be redelivered next frame and toggle twice.
    let clicked = requests.read().any(|r| *r == ArmRequest::TogglePush);
    if !clicked && !actions.just_pressed(crate::input::Action::TogglePush) {
        return;
    }
    if selected.is_empty() {
        // A real state with a real cause. Silence here would read as a dead key.
        sfx.write(Sfx::Invalid);
        return;
    }
    let anyone_pushing = selected.iter().any(|(_, p)| p.is_some());
    for (entity, _) in &selected {
        if anyone_pushing {
            commands.entity(entity).remove::<crate::squad::PushOrder>();
        } else {
            commands.entity(entity).insert(crate::squad::PushOrder);
        }
    }
    sfx.write(Sfx::MoveOrder);
}

/// Pop the next queued order into [`MoveOrder`] once the active one completes.
///
/// `squad::unit_movement` removes `MoveOrder` on arrival and is the single owner of that decision;
/// this only fills the gap it leaves.
///
/// # Why this is NOT on `FixedUpdate`
///
/// It writes `MoveOrder`, which feeds `Transform` and therefore `snapshot_hash`, and the repo rule
/// says such a system belongs on `FixedUpdate`. The rule's *purpose* is that pinned writes happen at
/// a fixed cadence — and this sits in `BeforeFixedMainLoop` beside `command_input`, which has always
/// written `MoveOrder` from exactly there for exactly that reason (see the schedule note in
/// [`SelectionPlugin`]). Both are player commands; neither is simulation.
///
/// Putting it on `FixedUpdate` would add a floating node to that graph, and `BACKLOG.md` (FVS-B-3)
/// records the cost: a new `FixedUpdate` node permutes the schedule's linearisation and can move the
/// deterministic goldens *by itself*, independent of what it does. Sharing the existing schedule
/// costs one thing instead — at a speed multiplier above ×1 several fixed ticks run per frame, so a
/// queued order is picked up at most once per frame rather than once per tick. At the ×2 ceiling of
/// [`crate::time_control::SHIPPING_LADDER`] that is a sub-frame of latency on an order the player
/// queued seconds ago.
///
/// In the harness nothing ever fills a queue, so `pop_front` yields `None` for every unit and this
/// writes nothing at all.
fn advance_order_queue(
    mut commands: Commands,
    mut waiting: Query<(Entity, &mut OrderQueue), Without<MoveOrder>>,
) {
    for (entity, mut queue) in &mut waiting {
        let Some(next) = queue.0.pop_front() else { continue };
        commands.entity(entity).insert(MoveOrder::new(next));
    }
}

/// **Right-click issues the move order** for whatever is selected; shift queues it behind the
/// current one.
///
/// Right rather than left because left now selects (see the module header). When a containment verb
/// is armed this system does not run at all — its `run_if(resource_equals(ArmedTool::None))` sees to
/// that — so the right-button disarm in `arm_tool_input` keeps its meaning and the two never race
/// for the same click.
#[allow(clippy::too_many_arguments)]
pub fn command_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    dungeon: Res<Dungeon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
    selected: Query<(Entity, &Transform, Option<&mut OrderQueue>), With<Selected>>,
    capture: Res<crate::DebugCaptureActive>,
    mut sfx: MessageWriter<Sfx>,
) {
    // Stand down while the dev-only region-capture tool (Ctrl+P) owns the mouse, so a capture drag
    // doesn't also issue a squad move order. Always `false` in release (the plugin isn't registered).
    if capture.0 {
        return;
    }
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };

    if selected.is_empty() {
        // Nothing selected is a real state with a real cause, so say so rather than swallowing the
        // click — the alternative teaches the player that right-click is broken.
        sfx.write(Sfx::Invalid);
        return;
    }
    let queued = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    // Snap the click to a floor cell, then build ONE flow field the whole selection shares. Units
    // flow to the same goal and ORCA packs them into a blob — no per-unit goal cells to fight over.
    // (One field per *command*, so two sub-selections sent to two places simply build two fields;
    // `FlowField` is `Arc`-shared and O(cells) once, per its module note.)
    let raw = dungeon.world_to_cell(point);
    let Some(goal) = nearest_floor(&dungeon, raw) else {
        warn!("move order ignored: no floor within {SNAP_MAX_RING} cells of the click");
        sfx.write(Sfx::Invalid);
        return;
    };
    let Some(field) = FlowField::build(&dungeon, goal) else {
        warn!("move order ignored: could not build a flow field to {goal:?}");
        sfx.write(Sfx::Invalid);
        return;
    };
    let field = Arc::new(field);
    let mut ordered_any = false;
    let mut queue_full = false;
    let mut selected = selected;
    for (entity, tf, queue) in &mut selected {
        // Skip a unit that can't reach the goal at all (different connected component) — loud, not
        // a silent stall.
        let start = dungeon.world_to_cell(tf.translation);
        if !field.reachable(start) {
            warn!("unit at {start:?} cannot reach goal {goal:?}; order skipped for it");
            continue;
        }
        if queued {
            match queue {
                Some(mut q) => {
                    if q.push(field.clone()) {
                        ordered_any = true;
                    } else {
                        queue_full = true;
                    }
                }
                None => {
                    let mut q = OrderQueue::default();
                    q.push(field.clone());
                    commands.entity(entity).insert(q);
                    ordered_any = true;
                }
            }
        } else {
            // A fresh order REPLACES the queue as well as the active order. Anything else means a
            // player who re-clicks to correct a mistake watches the squad walk the old route first.
            if let Some(mut q) = queue {
                q.0.clear();
            }
            commands.entity(entity).insert(MoveOrder::new(field.clone()));
            ordered_any = true;
        }
    }
    if queue_full {
        warn!("order queue full ({MAX_QUEUED_ORDERS}); the newest order was not appended");
        sfx.write(Sfx::Invalid);
    }
    // One acknowledgement for the whole order (not one per unit).
    if ordered_any {
        sfx.write(Sfx::MoveOrder);
    }
}

/// Arm / disarm a containment verb, and toggle weapons tight.
///
/// **The keys live in `crate::input`, not here.** This doc comment used to carry a hand-written
/// census of which keys were taken — as did four other modules — and every copy of it had drifted
/// (all five named `T` as taken long after the `T` hotkey was deleted). `input::Action` is that
/// census as data, and `input::the_key_space_has_no_collisions` is what keeps it true.
///
/// Re-pressing an armed verb disarms it, and so does a right-click — there is no modal state the player
/// can get stuck in, and `Escape` is left alone so it always means "pause".
pub fn arm_tool_input(
    actions: crate::input::Actions,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<crate::DebugCaptureActive>,
    mut requests: MessageReader<ArmRequest>,
    mut armed: ResMut<ArmedTool>,
    mut tight: ResMut<crate::laser::WeaponsTight>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 {
        return;
    }
    // Weapons tight is a latched stance, not a held key: containment holds run for seconds and asking
    // the player to hold a key through one would compete with the mouse.
    let mut toggle_tight = actions.just_pressed(crate::input::Action::ToggleHoldFire);

    // The sensor is deliberately NOT handled here. `sensor::deploy_sensor` owns it and reads both of
    // its input sources itself — the chip's `ArmRequest::DeploySensor` message and the `V` key. This
    // system cannot forward the key, because taking a `MessageWriter<ArmRequest>` beside its own
    // `MessageReader<ArmRequest>` is a Bevy B0002 access conflict (one resource, read and write, one
    // system). The harness's schedule build is what caught that; the unit tests could not, because
    // they never construct a schedule.

    let mut requested = if actions.just_pressed(crate::input::Action::ArmDevice) {
        Some(ArmedTool::Device)
    } else if actions.just_pressed(crate::input::Action::ArmQuarantine) {
        Some(ArmedTool::Quarantine)
    } else if actions.just_pressed(crate::input::Action::ArmCap) {
        Some(ArmedTool::Cap)
    } else {
        None
    };

    // The clickable verb bar arrives here rather than writing `ArmedTool` itself, so this stays the
    // single writer and the click and the key cannot diverge — clicking a chip *is* pressing its
    // key, including the toggle-to-disarm behaviour and the `DebugCaptureActive` stand-down above.
    for req in requests.read() {
        match *req {
            ArmRequest::Toggle(tool) => requested = Some(tool),
            ArmRequest::ToggleWeaponsTight => toggle_tight = true,
            // Handled by `sensor::deploy_sensor` and `toggle_push_order`, which read the same
            // channel. Named rather than caught by a `_` arm so that adding a request variant is a
            // COMPILE ERROR here instead of a silently ignored player input.
            ArmRequest::DeploySensor | ArmRequest::TogglePush => {}
        }
    }

    if toggle_tight {
        tight.0 = !tight.0;
        sfx.write(Sfx::MoveOrder);
    }
    if let Some(want) = requested {
        // Toggle: pressing the armed verb's own key puts it away.
        *armed = if *armed == want { ArmedTool::None } else { want };
        return;
    }
    if mouse.just_pressed(MouseButton::Right) && *armed != ArmedTool::None {
        *armed = ArmedTool::None;
    }
}

/// Throw a capture device at the anomaly under the cursor (archetype 1).
///
/// The device **names** its target rather than searching for one at deploy time — that is FVS-B-5's
/// design and it is what keeps `deploy_devices` free of a pick. The pick happens here instead, once, in
/// windowed input, and it is resolved through `containment::verbs::pick_target` so a tie between two
/// co-located anomalies is broken by `TargetId` rather than by ECS iteration order.
pub fn throw_device_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut supply: ResMut<DeviceSupply>,
    mut armed: ResMut<ArmedTool>,
    targets: Query<(&TargetId, Entity, &Transform), With<crate::containment::Containment>>,
    units: Query<(&Transform, &crate::health::Health), With<Unit>>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    if supply.0 == 0 {
        warn!("no capture devices left this expedition");
        sfx.write(Sfx::Invalid);
        return;
    }
    // Aim tolerance is generous relative to the reach: the player should be able to click a creature,
    // not a pixel. Whether the throw CONNECTS is `deploy_devices`' call, using `reach` below.
    let Some(target) = crate::containment::verbs::pick_target(
        point,
        aim_tolerance(tuning.containment.device_reach),
        targets.iter().map(|(id, e, tf)| (*id, e, tf.translation)),
    )
    else {
        warn!("no containable anomaly under the cursor");
        sfx.write(Sfx::Invalid);
        return;
    };
    // Thrown from the nearest living operative, so "get close enough to throw" is the mechanic. A dead
    // squad cannot throw.
    let Some(from) = units
        .iter()
        .filter(|(_, hp)| hp.current > 0.0)
        .map(|(tf, _)| tf.translation)
        // SORT-OK: total by the whole value — distance, then the position bits themselves.
        // Equidistant flanks resolve on coordinates rather than query order; bit-identical
        // coordinates mean the same origin, where either winner is the same throw. (Previously
        // unannotated with no tiebreak: a reachable tie handed the throw origin to ECS order.)
        .min_by(|a, b| {
            let (da, db) = ((a.xz() - point.xz()).length(), (b.xz() - point.xz()).length());
            da.total_cmp(&db)
                .then(a.x.total_cmp(&b.x))
                .then(a.y.total_cmp(&b.y))
                .then(a.z.total_cmp(&b.z))
        })
    else {
        return;
    };

    commands.spawn((
        crate::session::run_scoped(),
        crate::containment::ContainmentDevice { target, reach: tuning.containment.device_reach },
        Transform::from_translation(from),
    ));
    // Spent on the throw, not on the connection — a miss costs you the canister. `deploy_devices`
    // already treats a dead/out-of-reach/already-contained target as a spent miss.
    supply.0 -= 1;
    *armed = ArmedTool::None;
    sfx.write(Sfx::MoveOrder);
}

/// Place a quarantine region on the floor under the cursor (archetype 2).
///
/// A ground point, not an entity — so unlike the device throw this is **not a pick** and needs no
/// canonical order.
pub fn place_quarantine_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    dungeon: Res<Dungeon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut supply: ResMut<QuarantineSupply>,
    mut armed: ResMut<ArmedTool>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    if supply.0 == 0 {
        warn!("no quarantine charges left this expedition");
        sfx.write(Sfx::Invalid);
        return;
    }
    let Some(cell) = nearest_floor(&dungeon, dungeon.world_to_cell(point)) else {
        warn!("quarantine ignored: no floor within {SNAP_MAX_RING} cells of the click");
        sfx.write(Sfx::Invalid);
        return;
    };
    commands.spawn((
        crate::session::run_scoped(),
        crate::containment::Quarantine { radius: tuning.containment.quarantine_radius },
        Transform::from_translation(dungeon.cell_center(cell)),
    ));
    supply.0 -= 1;
    *armed = ArmedTool::None;
    // Its own cue, not the move-order tick it used to borrow. The supply is ONE charge per
    // expedition (`config.ron: quarantine_supply`), so this is the least repeated and most
    // consequential click in the game, and it should not sound like ordering someone to walk.
    sfx.write(Sfx::CordonPlaced);
}

/// Throw a noisemaker onto the floor under the cursor (`crate::lure`, FVS-B-10 stage 1).
///
/// Mirrors [`place_quarantine_input`] deliberately — snap to floor, spend a charge, disarm, cue —
/// because a second placement idiom would be a second set of edge cases for the same gesture. The
/// spend and the habituation step live in `lure::throw_lure`, not here: the Research Room palette
/// places lures too, and the two callers must not disagree about what a throw costs.
pub fn throw_lure_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    dungeon: Res<Dungeon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut supply: ResMut<crate::lure::LureSupply>,
    mut hab: ResMut<crate::lure::Habituation>,
    mut seq: ResMut<crate::lure::LureSeq>,
    mut armed: ResMut<ArmedTool>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    if supply.0 == 0 {
        warn!("no lures left this expedition");
        sfx.write(Sfx::Invalid);
        return;
    }
    let Some(cell) = nearest_floor(&dungeon, dungeon.world_to_cell(point)) else {
        warn!("lure ignored: no floor within {SNAP_MAX_RING} cells of the click");
        sfx.write(Sfx::Invalid);
        return;
    };
    if crate::lure::throw_lure(
        &mut commands,
        dungeon.cell_center(cell),
        &tuning.lure,
        &mut supply,
        &mut hab,
        &mut seq,
    )
    .is_none()
    {
        sfx.write(Sfx::Invalid);
        return;
    }
    *armed = ArmedTool::None;
    sfx.write(Sfx::CordonPlaced);
}

/// Cap the nest under the cursor (archetype 3).
///
/// **Grants nothing, on purpose.** `Capped` is a terminal marker with no `on_add` hook: sealing a
/// structure is honestly "kill the source for no specimen", and giving it a reward would quietly undo
/// the win-by-containing pivot the whole backlog is built on. Same pick discipline as the device.
pub fn cap_nest_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut armed: ResMut<ArmedTool>,
    nests: Query<
        (&TargetId, Entity, &Transform),
        (With<crate::nest::Nest>, Without<crate::containment::Capped>),
    >,
    units: Query<(&Transform, &crate::health::Health), With<Unit>>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    let Some((nest, nest_pos)) = crate::containment::verbs::pick_target(
        point,
        aim_tolerance(tuning.containment.cap_reach),
        nests.iter().map(|(id, e, tf)| (*id, (e, tf.translation), tf.translation)),
    ) else {
        warn!("no uncapped nest under the cursor");
        sfx.write(Sfx::Invalid);
        return;
    };
    // Reach check mirrors `device::deploy_devices`: a unit has to be at the nest to seal it.
    let in_reach = units.iter().any(|(tf, hp)| {
        hp.current > 0.0
            && (tf.translation.xz() - nest_pos.xz()).length() <= tuning.containment.cap_reach
    });
    if !in_reach {
        warn!("no operative within {} m of that nest", tuning.containment.cap_reach);
        sfx.write(Sfx::Invalid);
        return;
    }
    commands.entity(nest).insert(crate::containment::Capped);
    *armed = ArmedTool::None;
    sfx.write(Sfx::MoveOrder);
}

/// Nearest floor cell to `c` by an outward ring search, so a click on a wall/void still yields a
/// reachable goal. Bounded by [`SNAP_MAX_RING`] so a click deep in the void fails loudly.
pub(crate) fn nearest_floor(dungeon: &Dungeon, c: IVec2) -> Option<IVec2> {
    if dungeon.is_floor(c) {
        return Some(c);
    }
    for r in 1..=SNAP_MAX_RING {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue; // ring perimeter only
                }
                let cell = IVec2::new(c.x + dx, c.y + dy);
                if dungeon.is_floor(cell) {
                    return Some(cell);
                }
            }
        }
    }
    None
}

fn draw_selection_rings(mut gizmos: Gizmos, selected: Query<&Transform, With<Selected>>) {
    for tf in &selected {
        let iso = Isometry3d::new(
            tf.translation + Vec3::Y * 0.03,
            Quat::from_rotation_x(-FRAC_PI_2),
        );
        gizmos.circle(iso, RING_RADIUS, crate::palette::SELECTION_RING);
    }
}

/// Show the "Move" cursor while anything is selected, else the default arrow. Guarded so the
/// component is only re-inserted on a state change.
///
/// This carries information again. Under the always-selected scheme `active` was true on every
/// frame of every run, so the cursor was a constant dressed as a state — it could never tell the
/// player anything, least of all "you have nothing selected; right-click will do nothing".
fn update_cursor(
    mut commands: Commands,
    window: Single<Entity, With<PrimaryWindow>>,
    selected: Query<(), With<Selected>>,
    mut last: Local<Option<bool>>,
) {
    let active = !selected.is_empty();
    if *last == Some(active) {
        return;
    }
    *last = Some(active);
    let icon = if active {
        SystemCursorIcon::Move
    } else {
        SystemCursorIcon::Default
    };
    commands.entity(*window).insert(CursorIcon::from(icon));
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn a_click_is_not_a_marquee() {
        // Without a deadzone every click is a 1×2-pixel box that catches nothing, so clicking a unit
        // would DESELECT it — the single most confusing thing a selection scheme can do.
        let click = Marquee { anchor: Some(Vec2::new(100.0, 100.0)), current: Vec2::new(102.0, 101.0) };
        assert!(click.rect().is_none(), "a 2px wobble must read as a click");

        let drag = Marquee { anchor: Some(Vec2::new(100.0, 100.0)), current: Vec2::new(200.0, 180.0) };
        let rect = drag.rect().expect("a real drag is a marquee");
        assert!(rect.contains(Vec2::new(150.0, 140.0)), "the box covers what it spans");
        assert!(!rect.contains(Vec2::new(50.0, 140.0)));
    }

    #[test]
    fn a_marquee_dragged_up_and_left_still_has_positive_area() {
        // `Rect::from_corners` normalises, so dragging toward the origin selects too. Dragging only
        // one way is a classic marquee bug and it is invisible until someone drags the other way.
        let up_left = Marquee { anchor: Some(Vec2::new(200.0, 180.0)), current: Vec2::new(100.0, 100.0) };
        let rect = up_left.rect().expect("a reversed drag is still a drag");
        assert!(rect.contains(Vec2::new(150.0, 140.0)));
    }

    #[test]
    fn no_drag_means_no_rect() {
        assert!(Marquee::default().rect().is_none());
    }

    #[test]
    fn a_control_group_is_stored_canonically() {
        // Sorted and deduped, so the same SET of operatives always produces the same stored form —
        // which is what lets a group survive a save or a replay without depending on the ECS query
        // order it was collected in.
        let mut g = ControlGroups::default();
        g.bind(0, vec![3, 1, 3, 0]);
        assert_eq!(g.get(0), &[0, 1, 3]);
    }

    #[test]
    fn binding_a_group_replaces_it_rather_than_growing_it() {
        // Ctrl+N means "this group is now THIS", not "add these". Anything else and a group only
        // ever grows, which makes it useless within a minute of play.
        let mut g = ControlGroups::default();
        g.bind(1, vec![0, 1, 2]);
        g.bind(1, vec![4]);
        assert_eq!(g.get(1), &[4]);
    }

    #[test]
    fn an_operative_reports_every_group_it_is_in() {
        // The roster chip shows these. Cockburn et al. 2014: an expert mechanism whose effect is
        // invisible is one players never adopt — so "did that bind?" has to be answerable by looking.
        let mut g = ControlGroups::default();
        g.bind(0, vec![0, 1]);
        g.bind(2, vec![1]);
        assert_eq!(g.labels_for(1), vec![1, 3], "1-based labels, in slot order");
        assert_eq!(g.labels_for(0), vec![1]);
        assert!(g.labels_for(4).is_empty());
    }

    #[test]
    fn an_out_of_range_group_slot_is_empty_not_a_panic() {
        // The repo's no-panic rule. `get` is reachable from a digit index, so it must not index.
        let g = ControlGroups::default();
        assert!(g.get(99).is_empty());
        let mut g = ControlGroups::default();
        g.bind(99, vec![0]); // no slot 99 — must be a no-op, not an out-of-bounds write
        assert!(g.get(0).is_empty());
    }

    #[test]
    fn the_order_queue_is_bounded_and_says_so() {
        // A player leaning on shift-click must not grow an unbounded queue. The cap reports itself
        // (`push` returns false, and `command_input` warns + plays `Invalid`) rather than silently
        // discarding the order, which would look like the click missed.
        let dummy = || Arc::new(FlowField::placeholder_for_test());
        let mut q = OrderQueue::default();
        assert!(q.is_empty());
        for i in 0..MAX_QUEUED_ORDERS {
            assert!(q.push(dummy()), "order {i} should fit");
        }
        assert_eq!(q.len(), MAX_QUEUED_ORDERS);
        assert!(!q.push(dummy()), "the cap must refuse, not silently drop");
        assert_eq!(q.len(), MAX_QUEUED_ORDERS, "a refused push changes nothing");
    }
}

#[cfg(test)]
mod aim_tests {
    use super::*;

    #[test]
    fn the_affordance_is_never_tighter_than_the_mechanic() {
        // THE invariant `AIM_TOLERANCE`'s doc states and the shipped constants violated: aim is a UI
        // affordance ("did you mean that one?"), reach is the mechanic ("are you close enough?"), and
        // an aim tighter than the reach means a player who IS in range is told they missed. Nine
        // consecutive failed cap attempts in a play log is what that looks like from the outside.
        for reach in [1.5_f32, 2.5, 3.0] {
            assert!(
                aim_tolerance(reach) >= reach,
                "aim {} must not be tighter than reach {reach}",
                aim_tolerance(reach)
            );
        }
    }

    #[test]
    fn a_short_reach_still_gets_the_baseline_affordance() {
        // The `max` floor: a verb with a tiny reach must not inherit a tiny click target. Clicking is a
        // mouse-precision problem, not a game-distance one, so it has its own minimum.
        assert_eq!(aim_tolerance(0.1), AIM_TOLERANCE);
        assert!(aim_tolerance(0.0) > 0.0);
    }

    #[test]
    fn the_shipped_reaches_are_all_looser_than_they_were() {
        // Pins the actual regression: every shipped verb reach exceeds the old flat constant, so all
        // three were mis-gated by the aim, not by the mechanic.
        for reach in [1.5_f32, 2.5, 3.0] {
            assert!(reach > AIM_TOLERANCE, "reach {reach} was tighter-gated by aim {AIM_TOLERANCE}");
        }
    }
}
