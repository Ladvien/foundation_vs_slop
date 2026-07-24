//! Research Room editor — an F6-toggled spawn palette for populating the live dungeon and clearing it
//! again. This is the mixed-initiative co-creation surface (Liapis, Smith & Shaker, *Mixed-initiative
//! content creation*, PCG Book ch.11; Yannakakis, Alexopoulos & Liapis, *Mixed-Initiative Co-Creativity*,
//! FDG 2014): the developer authors the room's contents by hand, one drop at a time.
//!
//! Buttons drop three kinds of thing at the spawn cell, fanned out so drops don't stack: **static** GLB
//! props (bind pose, for art/scale inspection), **live** creatures spawned through the game's real
//! runtime paths (`spawn_crab_on_patch` / `spawn_unit` / `spawn_manca_on_patch`, so they behave and
//! animate on `FixedUpdate`), and **furniture** cycled from the placement manifest. Quantity batches each
//! drop; Space (or the button) pauses; "Clear Room" is the no-legacy-state reset (Zhu et al. 2025) that
//! despawns every `RoomSpawned` entity.
//!
//! Determinism: everything here is windowed dev-only (`#[cfg(debug_assertions)]`, gated on
//! [`crate::ResearchRoomActive`]) and runs on `Update`; the live spawns take the real paths but on a
//! decorrelated per-spawn seed (the room is exempt from the deterministic core and never registered in
//! `sim_harness`), so it never perturbs pinned state or `snapshot_hash`.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::RoomSpawned;
use crate::dungeon::Dungeon;
use crate::ui::theme::{FontAssets, UiTheme, Z_MENU};
use crate::ui::widgets::{button_visual, panel, text, text_colored};

/// Editor UI state: whether the palette is open, and a running counter used to fan spawned props out
/// across the floor so successive drops do not stack.
/// Quantity presets the "Quantity" button cycles through — how many entities each spawn button drops
/// in one click.
const QUANTITIES: [u32; 6] = [1, 5, 10, 25, 50, 100];

#[derive(Resource, Default)]
pub(super) struct EditorState {
    open: bool,
    spawn_count: u32,
    furniture_idx: usize,
    quantity_idx: usize,
}

impl EditorState {
    /// How many entities each spawn button drops per click (the cycled quantity preset).
    fn quantity(&self) -> u32 {
        QUANTITIES[self.quantity_idx % QUANTITIES.len()]
    }
}

/// Root marker for the palette panel (despawned when the palette closes).
#[derive(Component)]
pub(super) struct EditorRoot;

/// A spawnable prop: a GLB scene shown at a fixed scale / yaw for inspection. `scale`/`yaw` mirror the
/// gameplay spawn sites so the model reads at its in-game size (e.g. the Valkyrie's `FIGURINE_SCALE` and
/// 180° yaw, the crab's `CRAB_RENDER_SCALE`).
#[derive(Clone, Copy)]
struct PropSpec {
    label: &'static str,
    glb: &'static str,
    scale: f32,
    yaw: f32,
}

/// The v1 palette — confirmed-present assets (see `BEVY_GAME_INFO.md` / the artist catalog).
const PROPS: &[PropSpec] = &[
    // Scales/yaws mirror the gameplay spawn sites (BEVY_GAME_INFO.md §4) so each reads at its in-game size.
    PropSpec { label: "Valkyrie (squad)", glb: "characters/valkyrie.glb", scale: 1.13, yaw: std::f32::consts::PI },
    PropSpec { label: "Dimensional Crab", glb: "dimensional_crab/dimensional_crab.glb", scale: 0.15, yaw: 0.0 },
    // SCP-150: render scale 0.07 (~0.25 m juvenile), authored facing −X → −90° about +Y.
    PropSpec { label: "SCP-150 Parasite", glb: "scp150/scp-150.glb", scale: 0.07, yaw: -std::f32::consts::FRAC_PI_2 },
    PropSpec { label: "Flashlight", glb: "low_poly_flashlight/low_poly_flashlight.glb", scale: 1.0, yaw: 0.0 },
    PropSpec { label: "Meat Chunks", glb: "meat_chunks/meatpack.glb", scale: 1.0, yaw: 0.0 },
];

/// The prop buttons are spawned by an unrolled `prop_button!` per index (see `spawn_palette`) because a
/// Bevy observer closure must be non-capturing — it can name the `PROPS` const directly but not a loop
/// variable — so the count is duplicated at the call site. This fails the build loudly if `PROPS` changes
/// length without the unrolled calls following.
const _: () = assert!(PROPS.len() == 5, "update the unrolled prop_button! calls in spawn_palette to match PROPS");

/// F6 toggles the palette open/closed, spawning or despawning the panel.
pub(super) fn toggle_editor(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<EditorState>,
    mut commands: Commands,
    roots: Query<Entity, With<EditorRoot>>,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
) {
    if !keys.just_pressed(KeyCode::F6) {
        return;
    }
    state.open = !state.open;
    if state.open {
        spawn_palette(&mut commands, &theme, &fonts);
    } else {
        for e in &roots {
            commands.entity(e).despawn();
        }
    }
}

/// Spawn one prop button per [`PropSpec`]; the closures are non-capturing (they reference the `PROPS`
/// const directly), matching the codebase's `settings_menu` button idiom.
macro_rules! prop_button {
    ($p:expr, $theme:expr, $fonts:expr, $spec:expr) => {
        $p.spawn(button_visual($theme))
            .with_children(|b| {
                b.spawn(text($theme, $fonts, $spec.label, $theme.font_body));
            })
            .observe(
                |_: On<Activate>,
                 mut c: Commands,
                 a: Res<AssetServer>,
                 d: Res<Dungeon>,
                 mut s: ResMut<EditorState>| {
                    let q = s.quantity();
                    for _ in 0..q {
                        spawn_prop(&mut c, &a, &d, &mut s, $spec);
                    }
                },
            );
    };
}

fn spawn_palette(commands: &mut Commands, theme: &UiTheme, fonts: &FontAssets) {
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(theme.space_md),
        top: Val::Px(theme.space_md),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(theme.space_sm),
        padding: UiRect::all(Val::Px(theme.space_md)),
        ..default()
    };
    commands
        .spawn((EditorRoot, panel(theme, node), GlobalZIndex(Z_MENU), TabGroup::new(0)))
        .with_children(|p| {
            p.spawn(text_colored(theme, fonts, "RESEARCH ROOM — CELL-9191", theme.font_body, theme.accent));
            p.spawn(text_colored(theme, fonts, "F6 close · drops at cell centre", theme.font_body * 0.85, theme.text_muted));

            // Pause/resume — stage a scene frozen, then run it (Space also toggles). The label tracks it.
            p.spawn((
                text_colored(theme, fonts, "RUNNING (Space to pause)", theme.font_body * 0.85, theme.accent),
                PauseLabel,
            ));
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text(theme, fonts, "PAUSE / RESUME", theme.font_body));
                })
                .observe(|_: On<Activate>, mut paused: ResMut<crate::time_control::UserPaused>| {
                    paused.0 = !paused.0;
                });

            // Quantity — how many each spawn button drops at once (cycles 1/5/10/25/50/100).
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn((text(theme, fonts, "Quantity: 1", theme.font_body), QuantityLabel));
                })
                .observe(|_: On<Activate>, mut s: ResMut<EditorState>| {
                    s.quantity_idx = (s.quantity_idx + 1) % QUANTITIES.len();
                });

            // Live, behaving creatures — spawned through the game's real runtime paths, so they run their
            // AI + animation on FixedUpdate (a crab hunts the unit; the unit auto-fires back).
            p.spawn(text_colored(theme, fonts, "— LIVE (behaving) —", theme.font_body * 0.85, theme.warn));
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text(theme, fonts, "Crab (hunts)", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut c: Commands,
                     graph: Res<crate::crab::SurfaceGraph>,
                     ca: Res<crate::crab::CrabAssets>,
                     canim: Res<crate::crab::CrabAnim>,
                     d: Res<Dungeon>,
                     sim: Res<crate::sim::SimTuning>,
                     beh: Res<crate::behavior_tuning::BehaviorTuning>,
                     mut s: ResMut<EditorState>| {
                        let q = s.quantity();
                        for _ in 0..q {
                            spawn_live_crab(&mut c, &graph, &ca, &canim, &d, &sim, &beh, &mut s);
                        }
                    },
                );
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text(theme, fonts, "Squad Unit (shoots)", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut c: Commands,
                     a: Res<AssetServer>,
                     valk: Res<crate::squad::ValkyrieAnim>,
                     d: Res<Dungeon>,
                     sim: Res<crate::sim::SimTuning>,
                     beh: Res<crate::behavior_tuning::BehaviorTuning>,
                     mut s: ResMut<EditorState>| {
                        let q = s.quantity();
                        for _ in 0..q {
                            spawn_live_unit(&mut c, &a, &valk, &d, &sim, &beh, &mut s);
                        }
                    },
                );
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text(theme, fonts, "SCP-150 Manca (stalks)", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut c: Commands,
                     graph: Res<crate::crab::SurfaceGraph>,
                     ma: Res<crate::parasite::MancaAssets>,
                     manim: Res<crate::parasite::MancaAnim>,
                     d: Res<Dungeon>,
                     sim: Res<crate::sim::SimTuning>,
                     beh: Res<crate::behavior_tuning::BehaviorTuning>,
                     mut s: ResMut<EditorState>| {
                        let q = s.quantity();
                        for _ in 0..q {
                            spawn_live_manca(&mut c, &graph, &ma, &manim, &d, &sim, &beh, &mut s);
                        }
                    },
                );
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text(theme, fonts, "SCP-999 (comforts)", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut c: Commands,
                     a: Res<AssetServer>,
                     d: Res<Dungeon>,
                     mut s: ResMut<EditorState>| {
                        let q = s.quantity();
                        for _ in 0..q {
                            spawn_live_scp999(&mut c, &a, &d, &mut s);
                        }
                    },
                );

            // Furniture — cycles through the whole placement manifest on repeat clicks.
            p.spawn(text_colored(theme, fonts, "— FURNITURE —", theme.font_body * 0.85, theme.text_muted));
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text(theme, fonts, "Next Furniture piece", theme.font_body));
                })
                .observe(
                    |_: On<Activate>,
                     mut c: Commands,
                     a: Res<AssetServer>,
                     m: Res<crate::placement::furnish::Manifest>,
                     d: Res<Dungeon>,
                     mut s: ResMut<EditorState>| {
                        let q = s.quantity();
                        for _ in 0..q {
                            spawn_furniture(&mut c, &a, &m, &d, &mut s);
                        }
                    },
                );

            // Static GLB props for art / scale inspection (no AI, bind pose). Unrolled one button per
            // `PROPS` index because the observer closure must be non-capturing (const index, not a loop
            // var); the `PROPS.len() == 5` assert above fails the build if this list drifts.
            p.spawn(text_colored(theme, fonts, "— STATIC (art) —", theme.font_body * 0.85, theme.text_muted));
            prop_button!(p, theme, fonts, PROPS[0]);
            prop_button!(p, theme, fonts, PROPS[1]);
            prop_button!(p, theme, fonts, PROPS[2]);
            prop_button!(p, theme, fonts, PROPS[3]);
            prop_button!(p, theme, fonts, PROPS[4]);

            // Clear Room — the no-legacy-state reset (despawn every RoomSpawned entity).
            p.spawn(button_visual(theme))
                .with_children(|b| {
                    b.spawn(text_colored(theme, fonts, "CLEAR ROOM", theme.font_body, theme.danger));
                })
                .observe(
                    |_: On<Activate>,
                     mut c: Commands,
                     q: Query<Entity, With<RoomSpawned>>,
                     mut s: ResMut<EditorState>| {
                        let mut n = 0;
                        for e in &q {
                            c.entity(e).despawn();
                            n += 1;
                        }
                        s.spawn_count = 0;
                        info!("research_room: cleared {n} spawned entit(ies)");
                    },
                );
        });
}

/// Spawn a prop's GLB scene as a `RoomSpawned` static actor, fanned out across the floor by the running
/// spawn counter so drops don't stack. Spawns `Visibility::default()` (Inherited → visible at a root), so
/// unlike the fog-gated dungeon tiles it shows immediately — no reveal step needed.
fn spawn_prop(
    commands: &mut Commands,
    assets: &AssetServer,
    dungeon: &Dungeon,
    state: &mut EditorState,
    spec: PropSpec,
) {
    let n = state.spawn_count;
    state.spawn_count += 1;
    // A simple 5-wide grid of cells around the chamber spawn keeps drops inside the room.
    let dx = (n % 5) as i32 - 2;
    let dy = (n / 5 % 4) as i32 - 2;
    let cell = nearest_floor(dungeon, dungeon.spawn + IVec2::new(dx, dy));
    let pos = dungeon.cell_center(cell);
    commands.spawn((
        RoomSpawned,
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(spec.glb))),
        Transform::from_translation(pos)
            .with_scale(Vec3::splat(spec.scale))
            .with_rotation(Quat::from_rotation_y(spec.yaw)),
        Visibility::default(),
    ));
    info!("research_room: spawned {} at cell {:?}", spec.label, cell);
}

/// Fan-out cell for the Nth spawn, spread across the chamber interior (12x8 distinct cells before it
/// wraps) so successive drops don't stack, then snapped to the nearest floor so nothing spawns in a wall.
fn fan_cell(dungeon: &Dungeon, n: u32) -> IVec2 {
    let dx = (n % 12) as i32 - 6;
    let dy = (n / 12 % 8) as i32 - 4;
    nearest_floor(dungeon, dungeon.spawn + IVec2::new(dx, dy))
}

/// Search radius (Chebyshev) for [`nearest_floor`]. The fan grid reaches ≤6 cells from the dungeon spawn
/// (which is floor), so any target's nearest floor sits well inside this; the margin also covers thick walls.
const FLOOR_SNAP_RADIUS: i32 = 16;

/// Nearest floor cell to `target`, searched in growing Chebyshev rings (nearest first, fixed scan order),
/// so a fanned-out spawn that lands in a wall snaps onto real, walkable game space. The dungeon spawn is
/// floor and within the fan radius, so a hit is guaranteed — the trailing `dungeon.spawn` only makes the
/// function total and is itself floor, never a wall.
fn nearest_floor(dungeon: &Dungeon, target: IVec2) -> IVec2 {
    for r in 0..=FLOOR_SNAP_RADIUS {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue; // scan only the ring at exactly Chebyshev radius r
                }
                let c = target + IVec2::new(dx, dy);
                if dungeon.is_floor(c) {
                    return c;
                }
            }
        }
    }
    dungeon.spawn
}

/// Spawn a live crab through the game's real runtime path (`crate::crab::spawn_crab_on_patch`), so it
/// runs its full AI + animation on `FixedUpdate`. Tagged `RoomSpawned` for "Clear Room".
fn spawn_live_crab(
    commands: &mut Commands,
    graph: &crate::crab::SurfaceGraph,
    crab_assets: &crate::crab::CrabAssets,
    crab_anim: &crate::crab::CrabAnim,
    dungeon: &Dungeon,
    sim: &crate::sim::SimTuning,
    beh: &crate::behavior_tuning::BehaviorTuning,
    state: &mut EditorState,
) {
    let n = state.spawn_count;
    state.spawn_count += 1;
    let cell = fan_cell(dungeon, n);
    match crate::crab::pick_patch(graph, dungeon, cell, false) {
        Some(patch) => {
            let seed = room_spawn_seed(n, ROOM_SPECIES_CRAB);
            let e = crate::crab::spawn_crab_on_patch(
                commands,
                graph,
                patch,
                &crab_assets.collider,
                &crab_assets.scene,
                crab_anim,
                seed,
                sim,
                beh.crab,
            );
            commands.entity(e).insert(RoomSpawned);
            info!("research_room: spawned live crab at cell {cell:?}");
        }
        None => warn!("research_room: no surface patch at cell {cell:?} for a crab"),
    }
}

/// F6-spawned units live in a reserved `SquadMember` namespace far above the native squad's `0..N` (which
/// `squad::spawn_squad` assigns at `Startup`). Without the offset the first F6 unit would take
/// `SquadMember(0)` and collide with the squad leader, panicking `laser::fire`'s `sort_total!` the instant
/// both fired on one tick. `spawn_count` keeps F6 units unique *within* the namespace; the base keeps them
/// disjoint from the squad. The per-unit decision seed (`= squad_member + 1`, which seeds `CyanideSmell`,
/// another total-sort tiebreak) derives from `squad_member`, so it is offset the same way and stays disjoint too.
const RESEARCH_ROOM_MEMBER_BASE: usize = 1_000_000;

/// The same reservation, for the **creature** seed streams. `CrabSeed`/`MancaSeed` are total-sort keys
/// (`re_role_crabs`'s flip budget, `crab_despawn_dead`, `manca_embed`'s greedy host claim,
/// `manca_despawn_dead`), and those sorts PANIC on a duplicate under `debug_assertions` — which is the
/// only build the Research Room exists in, so a collision takes the whole session down.
///
/// The previous `n·salt + 1` mapping did not prevent that: it decorrelated the two dev streams from
/// *each other*, but it evaluates to `1` at `n = 0` for every salt, so the very first F6 crab or manca
/// minted seed `1` — exactly what the native `CrabSpawnSeq`/`MancaSpawnSeq` counters hand their second
/// creature.
/// Adding a base far above those counters (they are click/population bounded, never near a million) makes
/// the dev seeds disjoint from the native streams, and the monotonic `n` keeps them unique within.
/// Downstream draws stay decorrelated because every consumer mixes the seed through `hash01_u32`.
const RESEARCH_ROOM_SEED_BASE: u32 = 1_000_000;

/// Width of each species' reserved seed band.
const RESEARCH_ROOM_SEED_SPAN: u32 = 1_000_000;

/// Species bands within the reserved range — one per creature stream, so a dev crab and a dev manca
/// spawned on the same click never share a raw seed either.
const ROOM_SPECIES_CRAB: u32 = 0;
const ROOM_SPECIES_MANCA: u32 = 1;
const ROOM_SPECIES_SCP999: u32 = 2;

/// A dev-spawn seed for the Nth F6 click, inside `species`' reserved band. Unique within the band by
/// construction (`n` is monotonic and click counts never approach the span), and disjoint both from the
/// other species' band and from the native `CrabSpawnSeq`/`MancaSpawnSeq` counters.
fn room_spawn_seed(n: u32, species: u32) -> u32 {
    RESEARCH_ROOM_SEED_BASE + species * RESEARCH_ROOM_SEED_SPAN + (n % RESEARCH_ROOM_SEED_SPAN)
}

/// Spawn a live squad unit through the real path (`crate::squad::spawn_unit`) so it behaves and auto-fires.
fn spawn_live_unit(
    commands: &mut Commands,
    assets: &AssetServer,
    valk: &crate::squad::ValkyrieAnim,
    dungeon: &Dungeon,
    sim: &crate::sim::SimTuning,
    beh: &crate::behavior_tuning::BehaviorTuning,
    state: &mut EditorState,
) {
    let n = state.spawn_count;
    state.spawn_count += 1;
    let cell = fan_cell(dungeon, n);
    match crate::squad_ai::persona::load_personas() {
        Ok(personas) => {
            // `role` (0..5) picks the RoleId + outfit. The `SquadMember` id must be unique across ALL
            // units — including the native squad's `0..N` from `spawn_squad` — or `laser::fire`'s
            // `sort_total!` panics when two share it, so F6 units take a reserved high namespace
            // (`RESEARCH_ROOM_MEMBER_BASE + n`) that the monotonic `n` keeps unique within.
            let role = (n as usize) % 5;
            let e = crate::squad::spawn_unit(
                commands,
                assets,
                valk,
                sim,
                beh,
                personas[role].clone(),
                dungeon.cell_center(cell),
                role,
                RESEARCH_ROOM_MEMBER_BASE + n as usize,
            );
            commands.entity(e).insert(RoomSpawned);
            info!("research_room: spawned live squad unit (role {role}) at cell {cell:?}");
        }
        Err(err) => error!("research_room: personas.ron: {err}"),
    }
}

/// Spawn a live SCP-150 manca through the real path (`crate::parasite::spawn_manca_on_patch`), so it
/// stalks + leaps on `FixedUpdate`. Tagged `RoomSpawned`. Uses a unique per-spawn seed (it drives
/// `MancaSeed` + `CyanideSmell`, both total-sort tiebreaks — so several never collide).
fn spawn_live_manca(
    commands: &mut Commands,
    graph: &crate::crab::SurfaceGraph,
    manca_assets: &crate::parasite::MancaAssets,
    manca_anim: &crate::parasite::MancaAnim,
    dungeon: &Dungeon,
    sim: &crate::sim::SimTuning,
    beh: &crate::behavior_tuning::BehaviorTuning,
    state: &mut EditorState,
) {
    let n = state.spawn_count;
    state.spawn_count += 1;
    let cell = fan_cell(dungeon, n);
    match crate::crab::pick_patch(graph, dungeon, cell, false) {
        Some(patch) => {
            let seed = room_spawn_seed(n, ROOM_SPECIES_MANCA);
            let home = dungeon.cell_center(cell);
            let e = crate::parasite::spawn_manca_on_patch(
                commands,
                graph,
                patch,
                &manca_assets.collider,
                &manca_assets.scene,
                manca_anim,
                seed,
                &sim.parasite,
                beh,
                home,
                0.0,
                None,
            );
            commands.entity(e).insert(RoomSpawned);
            info!("research_room: spawned live SCP-150 manca at cell {cell:?}");
        }
        None => warn!("research_room: no surface patch at cell {cell:?} for a manca"),
    }
}

/// Spawn a live SCP-999 comfort blob through the real path (`crate::scp999::spawn_scp999_at`), so it
/// seeks + tickle-calms on `FixedUpdate` and grows its eyes + jiggle via the windowed cosmetic plugin.
/// Tagged `RoomSpawned`. Uses a decorrelated per-spawn seed (drives only the cosmetic idle/blink phase).
fn spawn_live_scp999(
    commands: &mut Commands,
    assets: &AssetServer,
    dungeon: &Dungeon,
    state: &mut EditorState,
) {
    let n = state.spawn_count;
    state.spawn_count += 1;
    let cell = fan_cell(dungeon, n);
    let seed = room_spawn_seed(n, ROOM_SPECIES_SCP999);
    let e = crate::scp999::spawn_scp999_at(commands, assets, seed, dungeon.cell_center(cell));
    commands.entity(e).insert(RoomSpawned);
    info!("research_room: spawned live SCP-999 comfort blob at cell {cell:?}");
}

/// Spawn the next furniture piece from the placement manifest (cycles the whole catalogue on repeat
/// clicks) as a static `RoomSpawned` GLB actor at `FURNITURE_SCALE` (1.0, so no rescale).
fn spawn_furniture(
    commands: &mut Commands,
    assets: &AssetServer,
    manifest: &crate::placement::furnish::Manifest,
    dungeon: &Dungeon,
    state: &mut EditorState,
) {
    let items = &manifest.0.items;
    if items.is_empty() {
        warn!("research_room: furniture manifest is empty");
        return;
    }
    let idx = state.furniture_idx % items.len();
    state.furniture_idx += 1;
    let n = state.spawn_count;
    state.spawn_count += 1;
    let item = &items[idx];
    let cell = fan_cell(dungeon, n);
    commands.spawn((
        RoomSpawned,
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(item.glb.clone()))),
        Transform::from_translation(dungeon.cell_center(cell)),
        Visibility::default(),
    ));
    info!(
        "research_room: spawned furniture '{}' [{}/{}] ({})",
        item.key,
        idx + 1,
        items.len(),
        item.glb
    );
}

/// Marker on the palette's pause-status text, kept in sync by [`refresh_pause_label`].
#[derive(Component)]
pub(super) struct PauseLabel;

/// Marker on the palette's Quantity button label, kept in sync by [`refresh_quantity_label`].
#[derive(Component)]
pub(super) struct QuantityLabel;

/// Keep the Quantity button label showing the current per-click spawn count.
pub(super) fn refresh_quantity_label(
    state: Res<EditorState>,
    mut labels: Query<&mut Text, With<QuantityLabel>>,
) {
    for mut t in &mut labels {
        let want = format!("Quantity: {}", state.quantity());
        if t.0 != want {
            t.0 = want;
        }
    }
}

/// Space toggles the sim pause via the game's single-writer `UserPaused`, so you can stage a scene
/// (spawn + arrange) with everything frozen, then resume to watch it run.
///
/// Space is **not** an exclusive hotkey: it is also `bevy_ui_widgets::Button`'s activation key (see
/// `ui::widgets`), and the Ctrl+P note box takes raw text. Unguarded, one press both clicked the focused
/// palette button and toggled the pause — two unrelated actions from one keystroke. When something else
/// owns the keyboard, the press belongs to it.
pub(super) fn toggle_pause_hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    note_input: Option<Res<crate::NoteInputActive>>,
    mut paused: ResMut<crate::time_control::UserPaused>,
) {
    if focus.get().is_some() || note_input.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        paused.0 = !paused.0;
    }
}

/// Keep the palette's pause status label in sync with `UserPaused`.
pub(super) fn refresh_pause_label(
    paused: Res<crate::time_control::UserPaused>,
    mut labels: Query<&mut Text, With<PauseLabel>>,
) {
    for mut t in &mut labels {
        let want = if paused.0 {
            "PAUSED (Space to resume)"
        } else {
            "RUNNING (Space to pause)"
        };
        if t.0 != want {
            t.0 = want.to_string();
        }
    }
}
