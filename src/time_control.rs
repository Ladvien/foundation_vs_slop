//! Windowed **game-speed + pause** control for inspection and RL observation.
//!
//! The pinned simulation runs on a fixed 60 Hz `FixedUpdate` (see `lib::run`), so "speed" is simply
//! how many fixed steps Bevy runs per rendered frame. Bevy accumulates `Time<Virtual>` and drains it
//! into the fixed-step accumulator, so raising the virtual clock's *relative speed* to N makes ~N
//! fixed steps run per frame — while each fixed step still sees a constant `1/60` dt. Determinism and
//! frame-rate independence are therefore preserved at any multiplier: the sim just steps more (or
//! fewer) times per frame, never with a different dt.
//!
//! **One writer rule.** The virtual clock's relative speed has exactly one writer:
//! `juice::tick_hitstop`. It composes *base game-speed × hitstop freeze × pause* (see `juice`). This
//! module only owns the `GameSpeed` resource + the keyboard input that sets it. It never calls
//! `set_relative_speed`.
//!
//! **On `Time<Virtual>::max_delta`.** We leave Bevy's 250 ms default alone. Its clamp applies to the
//! *raw real* frame delta *before* the speed multiply (bevy_time 0.19 `virt.rs::advance_with_raw_delta`:
//! `clamped = min(raw_delta, max_delta)`, then `× effective_speed`), so at 60 fps (raw ≈ 16.7 ms) it
//! never clamps and the high multipliers already reach their full step count (×64 ⇒ ≈64 steps/frame) —
//! there is no per-frame step cap to "unlock". `max_delta`'s only job is to bound the fixed-step
//! catch-up after a real stall (alt-tab, window drag, breakpoint); *raising* it would enlarge that
//! post-stall burst (at ×64 a 1.2 s stall would replay ≈4600 fixed steps in one frame), so don't.
//!
//! **Human input is speed-independent.** The camera controls (WASD pan, wheel zoom, middle-drag) must
//! feel identical at any multiplier, so they never read the sim clock: pan runs on `Time<Real>` and
//! zoom/drag use raw per-frame input deltas (see `camera::drive_camera`). Order/selection input
//! (`selection`) has no time coupling at all. Changing speed — or pausing — never alters how the mouse
//! or keyboard respond.
//!
//! **Accepted side effects at high speed** (the rungs past ×2 are a dev/inspection tool, not a
//! shipping UX — which is why they are `debug_assertions`-only and behind `Alt`, see
//! [`SHIPPING_LADDER`]):
//! - Cosmetic *gameplay-feel* systems that read the generic `Time` do scale with the multiplier —
//!   trauma decay + screen-shake phase (`juice`/`camera` shake), audio timers. At ×64 the shake buzzes
//!   and SFX race; this is intentional (they track sim time, not wall time) and is not input.
//! - Render frame-rate drops at extreme multipliers because each rendered frame runs up to ~64 fixed
//!   steps. The simulation stays correct and deterministic per step; it just does more work per frame.
//!
//! The headless RL harness (`sim_harness`, feature `test-harness`) has its own, separate speed knob
//! (`SimConfig::speed`) that advances *real* time manually and never touches `Time<Virtual>`, so it
//! never collides with this path.

use bevy::prelude::*;

/// Player-selected simulation speed. `base` is the wall-speed multiplier the swarm/lasers run at;
/// `paused` overrides it to a full freeze. Read by `juice::tick_hitstop`, the sole writer of the
/// virtual clock's relative speed.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GameSpeed {
    /// Wall-speed multiplier (`1.0` = real time). One of [`SPEED_LADDER`] when set from the keyboard,
    /// but an RL/inspection tool may write any value directly — `juice::tick_hitstop` clamps it to a
    /// finite, non-negative speed before it reaches the virtual clock, so a stray NaN/inf/negative
    /// can't panic Bevy's `set_relative_speed`.
    pub base: f32,
    /// When `true`, gameplay is frozen regardless of `base` (the virtual clock is driven to ~0).
    pub paused: bool,
}

impl Default for GameSpeed {
    fn default() -> Self {
        Self { base: 1.0, paused: false }
    }
}

/// Player-toggled pause (`Space`). Kept separate from [`SimBlocked`] so the two independent
/// pause sources compose through a *single* writer of [`GameSpeed::paused`] ([`compose_pause`])
/// instead of racing to set it. Defaults `false`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct UserPaused(pub bool);

/// Set by the windowed UI while a blocking screen (boot, title, pause, settings, roster) is open,
/// to freeze the sim underneath it. The headless replay harness never registers the UI plugin, so
/// nothing ever writes this — it stays `false` and the deterministic core is unperturbed.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct SimBlocked(pub bool);

/// Set by the windowed UI while the player's pointer is aimed at something **other than the
/// expedition**, to stop `selection`'s order-issuing input reaching a squad the player cannot see.
///
/// **Why this is a resource and not an `AppState` run condition.** `ui::state` and `ui/mod.rs` both
/// state the rule: gameplay plugins must never be gated on `in_state(AppState::InGame)`, because the
/// harness does not register `AppState` at all and the world has to keep ticking under the boot and
/// title screens. So this is the same shape as [`SimBlocked`] — one writer
/// (`ui::state::sync_order_block`), inert `false` by default, and the deterministic core cannot tell
/// it exists. `replay::ui_never_leaks_into_deterministic_core` pins that.
///
/// **Why it is separate from [`SimBlocked`].** They meant the same thing right up until
/// `input::Action::VisitSite`: standing at Site-67 now leaves an expedition **running**
/// (`docs/2026-08-01-two-live-layers.md`), so the Site must NOT freeze the sim — that exposure is the
/// whole feature — while it must still stop the mouse commanding a squad that is off-screen and 512+
/// world units away. `ui::state::should_freeze` and [`should_block_orders`] answer two now-different
/// questions, and collapsing them back into one would either freeze the unattended squad or let a
/// right-click at the Site march it to a coordinate outside the map.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrdersBlocked(pub bool);

/// Run condition: may the player's pointer command the expedition right now?
///
/// A named `fn` rather than `resource_equals(OrdersBlocked(false))` at each call site, for two
/// reasons. The mechanical one: `resource_equals` returns a closure that is not `Clone`, so it cannot
/// be used with `distributive_run_if` — and `selection` documents (measured) that a tuple-level
/// `run_if` adds a set node which moves the deterministic golden by itself. The real one: this is the
/// single spelling of "orders reach the squad", so a second consumer cannot invent a subtly different
/// one. Reads inert `false` in the harness, where nothing writes [`OrdersBlocked`].
pub fn orders_allowed(blocked: Res<OrdersBlocked>) -> bool {
    !blocked.0
}

/// Pure pause-composition rule, factored out for a unit test: the sim is frozen if the player
/// paused **or** a blocking UI screen is open.
#[inline]
pub fn paused_from(user_paused: bool, sim_blocked: bool) -> bool {
    user_paused || sim_blocked
}

/// The full inspection ladder. Index 2 (`×1.0`) is real time; left of it slows down, right of it
/// speeds up.
///
/// **Only [`SHIPPING_LADDER`] is reachable in a release build.** The rungs beyond ×2 are an
/// inspection tool — this module's own header says so — and reaching them used to cost the entire
/// `1`–`9` row, which is the prime real estate of an RTS keyboard. `BACKLOG.md` records the
/// consequence in as many words: *"Keybindings are constrained, not chosen"*, and the containment
/// verbs landed on `C`/`Z`/`X`/`F` because the digits were already gone. They are free now.
pub const SPEED_LADDER: [f32; 9] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];

/// The rungs a player can reach, as indices into [`SPEED_LADDER`]: ×0.5, ×1, ×2.
///
/// Three rungs rather than nine because more options cost *commitment time*, not perception —
/// Itthipuripat et al. 2018 (DOI 10.1523/jneurosci.0440-18.2018) isolate the mechanism behind
/// Hick's-law slowing and find added choices raise the decision threshold rather than the
/// perceptual load. Under horror time pressure that reads as freezing.
pub const SHIPPING_LADDER: [usize; 3] = [1, 2, 3];

/// Position within [`SHIPPING_LADDER`] of real time (×1.0) — where a fresh run sits.
const DEFAULT_SHIPPING_POS: usize = 1;

/// The multiplier at a position along [`SHIPPING_LADDER`].
///
/// Two chained `get`s rather than indexing: the repo's no-panic rule means a mis-authored
/// `SHIPPING_LADDER` entry must degrade to `None` here, not take the process down.
fn shipping_mult(pos: usize) -> Option<f32> {
    SHIPPING_LADDER.get(pos).and_then(|&i| SPEED_LADDER.get(i)).copied()
}

/// Digit keys `1..=9`, positionally aligned with [`SPEED_LADDER`]'s rungs (digit 1 → rung 0, …).
///
/// **Debug builds only, and behind `Alt`.** Bare `1`–`9` now belong to the player (control groups);
/// `Alt` keeps the inspection ladder reachable without shadowing them. Kept as a parallel array
/// rather than `(KeyCode, index)` pairs so the rung index is derived from position; `zip` with
/// `SPEED_LADDER` means a length mismatch simply ignores the extra entries — never an
/// out-of-bounds panic (repo no-panic rule).
#[cfg(debug_assertions)]
const DIGIT_KEYS: [KeyCode; 9] = [
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

pub struct TimeControlPlugin;

impl Plugin for TimeControlPlugin {
    fn build(&self, app: &mut App) {
        crate::input::claim_bindings(app);
        app.init_resource::<GameSpeed>()
            .init_resource::<UserPaused>()
            .init_resource::<SimBlocked>()
            .init_resource::<OrdersBlocked>()
            // `read_speed_input` writes `UserPaused`; `compose_pause` then folds `UserPaused` +
            // `SimBlocked` into the single `GameSpeed::paused` write. `.chain()` keeps that order so
            // a key press and its resulting pause state land in the same frame.
            // **Reset the player pause on run entry.** `UserPaused` is process-lifetime, and pressing
            // `Space` at Site-67 (a natural "confirm" reflex) latched it with no readout anywhere to
            // say so — `SpeedText` only exists `OnEnter(AppState::InGame)`. The next expedition then
            // booted frozen, and orders appeared to do nothing. Run-scoped intent has to be cleared
            // where the run begins.
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                |mut paused: ResMut<UserPaused>, mut speed: ResMut<GameSpeed>| {
                    paused.0 = false;
                    // Also drop an inspection rung back onto the player's ladder, so `Alt+9` in one
                    // expedition cannot leave the next one running at ×64.
                    speed.base = shipping_mult(DEFAULT_SHIPPING_POS).unwrap_or(1.0);
                },
            )
            .add_systems(Update, (read_speed_input, compose_pause).chain());
        #[cfg(debug_assertions)]
        app.add_systems(Update, read_inspection_ladder.before(compose_pause));
    }
}

/// Step one rung along [`SHIPPING_LADDER`], clamped at both ends.
///
/// Pure so the walk is testable without an `App`. Takes the *current* multiplier rather than a
/// stored index because an inspection rung (or an RL tool) may have written a multiplier that is
/// not on the shipping ladder at all — this snaps back onto it rather than carrying a second,
/// divergent notion of "where the player is".
fn step_rung(current: f32, up: bool) -> f32 {
    // Nearest shipping rung to where we are. A linear scan over three entries, and it handles the
    // off-ladder case (an inspection rung, or an RL tool that wrote an arbitrary multiplier) by
    // snapping onto the ladder rather than carrying a second, divergent notion of "where the player
    // is" — so one keypress always produces a predictable speed.
    let mut here = DEFAULT_SHIPPING_POS;
    let mut best = f32::INFINITY;
    for pos in 0..SHIPPING_LADDER.len() {
        let Some(m) = shipping_mult(pos) else { continue };
        let d = (m - current).abs();
        if d < best {
            best = d;
            here = pos;
        }
    }
    let last = SHIPPING_LADDER.len().saturating_sub(1);
    let next = if up { (here + 1).min(last) } else { here.saturating_sub(1) };
    shipping_mult(next)
        .or_else(|| shipping_mult(DEFAULT_SHIPPING_POS))
        .unwrap_or(1.0)
}

/// The shipping time controls: pause, and one step along [`SHIPPING_LADDER`].
///
/// Pause is written to [`UserPaused`], not `GameSpeed` directly, so it composes with the UI's
/// [`SimBlocked`] through the single writer [`compose_pause`].
///
/// The focus guard that keeps `Space` from both activating a focused menu button and toggling the
/// pause lives in `input::Actions` now, applied to every non-menu action — `research_room::editor`
/// used to carry a hand-rolled copy of it for this exact key.
fn read_speed_input(
    actions: crate::input::Actions,
    mut speed: ResMut<GameSpeed>,
    mut user_paused: ResMut<UserPaused>,
) {
    if actions.just_pressed(crate::input::Action::TogglePause) {
        user_paused.0 = !user_paused.0;
    }
    let down = actions.just_pressed(crate::input::Action::SpeedDown);
    let up = actions.just_pressed(crate::input::Action::SpeedUp);
    // Both at once is a no-op rather than last-writer-wins, so a stuck key can't walk the ladder.
    if down != up {
        speed.base = step_rung(speed.base, up);
        user_paused.0 = false;
    }
}

/// `Alt` + `1..=9` pick any rung of the full [`SPEED_LADDER`] — the inspection tool this module's
/// header describes, kept out of the player's key space.
///
/// Reads raw keys rather than going through `input::Action`: nine debug rungs would be nine enum
/// variants and nine controls-screen rows for something no player can reach. The `Alt` requirement
/// is what keeps it from colliding with the bare digits, and `input::the_key_space_has_no_collisions`
/// covers the bindings that ARE in the registry.
#[cfg(debug_assertions)]
fn read_inspection_ladder(
    keys: Res<ButtonInput<KeyCode>>,
    mut speed: ResMut<GameSpeed>,
    mut user_paused: ResMut<UserPaused>,
) {
    if !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        return;
    }
    for (&mult, key) in SPEED_LADDER.iter().zip(DIGIT_KEYS) {
        if keys.just_pressed(key) {
            speed.base = mult;
            user_paused.0 = false;
        }
    }
}

/// The **sole writer** of [`GameSpeed::paused`]: the sim freezes if the player paused
/// ([`UserPaused`]) or a blocking UI screen is open ([`SimBlocked`]). Keeping a single writer
/// preserves the one-writer discipline `juice::tick_hitstop` (the virtual-clock writer) relies on.
/// In the headless harness both inputs stay `false`, so `paused` stays `false` and `FixedUpdate`
/// keeps stepping bit-identically — the deterministic core is untouched.
fn compose_pause(
    user_paused: Res<UserPaused>,
    sim_blocked: Res<SimBlocked>,
    mut speed: ResMut<GameSpeed>,
) {
    let paused = paused_from(user_paused.0, sim_blocked.0);
    // Guard the write so `GameSpeed` isn't needlessly marked changed every frame.
    if speed.paused != paused {
        speed.paused = paused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_composition_is_logical_or() {
        assert!(!paused_from(false, false));
        assert!(paused_from(true, false), "player pause freezes the sim");
        assert!(paused_from(false, true), "an open menu freezes the sim");
        assert!(paused_from(true, true));
    }

    #[test]
    fn the_shipping_ladder_is_a_real_slice_of_the_full_one() {
        // Indices, not copied numbers, so the two can never disagree about what "×2" means.
        for pos in 0..SHIPPING_LADDER.len() {
            assert!(shipping_mult(pos).is_some(), "SHIPPING_LADDER[{pos}] is out of range");
        }
        assert_eq!(shipping_mult(DEFAULT_SHIPPING_POS), Some(1.0), "the middle rung is real time");
        // Ascending, or "speed up" would sometimes slow down.
        for pos in 1..SHIPPING_LADDER.len() {
            let (prev, cur) = (shipping_mult(pos - 1), shipping_mult(pos));
            assert!(prev < cur, "the shipping ladder must ascend: {prev:?} then {cur:?}");
        }
        // The inspection rungs are strictly beyond it, which is what makes them worth hiding.
        assert!(SPEED_LADDER.iter().any(|&m| m > 2.0), "there is nothing left to gate behind Alt");
    }

    #[test]
    fn stepping_walks_the_shipping_ladder_and_stops_at_both_ends() {
        assert_eq!(step_rung(1.0, true), 2.0);
        assert_eq!(step_rung(1.0, false), 0.5);
        // Clamped, not wrapped: holding "faster" must never drop the player back to ×0.5.
        assert_eq!(step_rung(2.0, true), 2.0);
        assert_eq!(step_rung(0.5, false), 0.5);
    }

    #[test]
    fn stepping_from_an_inspection_rung_snaps_back_onto_the_shipping_ladder() {
        // `Alt+9` (×64) or an RL tool can leave `base` far off the player's ladder. One press must
        // then produce a predictable speed rather than an arbitrary jump — the reason `step_rung`
        // reads the current multiplier instead of storing an index beside it.
        assert_eq!(step_rung(64.0, false), 1.0, "×64 is nearest ×2, so 'slower' gives ×1");
        assert_eq!(step_rung(64.0, true), 2.0, "already at the top of what a player can reach");
        assert_eq!(step_rung(0.25, true), 1.0, "×0.25 is nearest ×0.5, so 'faster' gives ×1");
        // A value on no rung at all still lands somewhere sane.
        assert_eq!(step_rung(0.0, true), 1.0);
    }
}
