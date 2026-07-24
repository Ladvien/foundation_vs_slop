//! Dev-only **Research Room** — a hand-built SCP observation / testing chamber.
//!
//! Launch with `FVS_RESEARCH_ROOM=1 cargo run` to boot straight into the **real WFC dungeon** — the
//! actual game level, with mycelia / mushrooms / the furniture-placement grammar / every auto-spawn
//! running natively — plus an **F6 debug panel** on top to inject extra creatures, props, and furniture,
//! batch-spawn (Quantity), pause / resume the sim, and clear. It is the game itself with a
//! spawn-and-control overlay, for debug testing against faithful systems; inspect with the `devshot`
//! screenshot tool (`touch screenshot.request`). Secondarily the human-in-the-loop **observation
//! instrument** for the offline Quality-Diversity search — witness evolved elites in the real level.
//!
//! Design grounding (home-still corpus):
//! - Co-creative MAP-Elites — a human edits the room's contents and witnesses / steers the evolved
//!   output — Gravina, Khalifa, Liapis, Togelius & Yannakakis, "Procedural Content Generation through
//!   Quality Diversity", IEEE CoG 2019 (arXiv:1907.04053); Mouret & Clune, "Illuminating search spaces
//!   by mapping elites", 2015 (arXiv:1504.04909).
//! - The room as a controlled sandbox test environment — Bergdahl, Gordillo, Tollmar & Gisslén,
//!   "Augmenting Automated Game Testing with Deep Reinforcement Learning", IEEE CoG 2021
//!   (arXiv:2103.15819).
//! - A benchmark environment must reset with no legacy state (the empty chamber + the scoped
//!   "clear room" despawn) — Zhu et al., "Establishing Best Practices for Building Rigorous Agentic
//!   Benchmarks", 2025 (arXiv:2507.02825). This dovetails with the project's one-path / no-fallback rule.
//!
//! **Determinism / one path.** Everything here is `#[cfg(debug_assertions)]`, and every system is gated
//! on the presence of [`crate::ResearchRoomActive`], which is inserted only by [`install_if_requested`]
//! when the env var is set. The module is never registered in the headless `sim_harness`, so the
//! deterministic replay core cannot see it. All systems run on `Update` — nothing here touches pinned
//! (`FixedUpdate`) state or `snapshot_hash`.
//!
//! `// EXEMPT:` from being an evolvable population — dev-only observation instrument, never in
//! `sim_harness`, mutates no pinned state. It *integrates* with RL/QD as the archive inspector (a later
//! phase) rather than being evolved itself.

use bevy::prelude::*;

use crate::ui::state::AppState;

mod editor;

/// Marker on every entity the Research Room spawns, so "clear room" is a scoped despawn
/// (`despawn_scoped::<RoomSpawned>`). Chamber tiles are deliberately NOT tagged — they are the room.
#[derive(Component)]
pub struct RoomSpawned;

/// Read `FVS_RESEARCH_ROOM`; if it is exactly `1`, insert the [`crate::ResearchRoomActive`] marker that
/// arms the F6 spawn palette. The room is **game-faithful**: `DungeonPlugin` still generates the real WFC
/// level and every auto-spawner runs natively — this only adds a debug overlay on top, it swaps and
/// suppresses nothing. Called once from `lib::run` under `#[cfg(debug_assertions)]`, alongside the
/// `FVS_POLICY_ELITE` pre-install.
pub fn install_if_requested(app: &mut App) {
    if std::env::var("FVS_RESEARCH_ROOM").as_deref() == Ok("1") {
        // Game-faithful mode: DON'T fabricate a dungeon — let `DungeonPlugin` generate the real WFC
        // level, so mycelia / mushrooms / the furniture grammar / every auto-spawn run natively. This
        // marker only enables the F6 debug panel (spawn / pause / quantity) on top.
        app.insert_resource(crate::ResearchRoomActive);
        info!("research_room: FVS_RESEARCH_ROOM=1 — real WFC dungeon + F6 debug panel active");
    }
}

/// Windowed dev-only plugin. Inert unless [`crate::ResearchRoomActive`] is present (see
/// [`install_if_requested`]); never registered in the headless harness.
pub struct ResearchRoomPlugin;

impl Plugin for ResearchRoomPlugin {
    fn build(&self, app: &mut App) {
        let active = resource_exists::<crate::ResearchRoomActive>;
        app.init_resource::<editor::EditorState>()
            .add_systems(
                Update,
                (
                    // Auto-trigger the title "NEW RUN" (Title → Warmup → play) so `FVS_RESEARCH_ROOM=1`
                    // is a scriptable one-shot. Self-limiting — it stops firing once the state leaves
                    // `Title`.
                    enter_room_state.run_if(in_state(AppState::Title)),
                    // F6 toggles the spawn palette (drop props/creatures / clear room).
                    editor::toggle_editor,
                    // Space pauses/resumes the sim (stage a scene frozen, then run it); the palette's
                    // status label tracks the pause state.
                    editor::toggle_pause_hotkey,
                    editor::refresh_pause_label,
                    editor::refresh_quantity_label,
                )
                    .run_if(active),
            );
    }
}

/// Auto-advance `Title` → `Warmup`, exactly as the title screen's "NEW RUN" button does (`ui::title`), so
/// `FVS_RESEARCH_ROOM=1` is a scriptable one-shot that still runs the real warmup: `Warmup` holds the sim
/// frozen while the mold colonizes the WFC level on `Time<Real>`, then `ui::warmup::advance_when_warm`
/// hands off to `InGame` once `MoldWarm` is set (usually the first frame).
fn enter_room_state(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::Warmup);
}

