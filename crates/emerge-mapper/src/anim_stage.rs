//! **The staged figure** — the selected rig, alive, driven by the real machinery.
//!
//! Everything else in the bench inspects the asset; this is the one surface that plays it, and it
//! plays it faithfully: every clip resident on one `AnimationPlayer`, one `AnimationGraph` from the
//! same `emerge_anim::rigs::build` the game spawns with, weights and one shared phase moved by the
//! same `apply_pose_blenders` pass — no transitions, nothing rewound, ever. What a conventional
//! "play clip N" preview cannot show is exactly what this can: whether the *set* reads together.
//!
//! Scrubbing pins the phase through [`emerge_anim::PoseBlender::hold_phase`] — the runtime's cadence
//! clamp floors at half nominal, so zero ground speed alone would still walk — and the one
//! `set_seek_time` formula stays the only author of clip time.
//!
//! The stage sits at its own far corner: `tiles::STAGE` owns `(-4096, 0, 4096)` and
//! `thumbs::BOOTH` owns `(4096, 0, 4096)`, so no two subjects are ever in shot together. The main
//! camera parks here (an arm of `tiles::stage_camera` — never a second `Camera3d`, which silently
//! breaks every `Single<_, With<Camera3d>>` in the crate).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use emerge_core::rigs::Playback;

use crate::anim_tab::BenchState;
use crate::chrome::{DIM, LABEL, ROW_BG, ROW_SELECTED, TEXT};
use crate::tiles::Mode;

/// The bench's staging corner — the third one.
pub const BENCH_STAGE: Vec3 = crate::stages::BENCH;

/// Orthographic viewport height on the stage, metres — a ~2 m figure with air, the same reasoning
/// as `tiles::TILE_VIEW_HEIGHT`.
pub const BENCH_VIEW_HEIGHT: f32 = 4.0;

/// Cycles per second a held scrub key sweeps, before the Shift divisor.
const SCRUB_RATE: f32 = 0.25;

/// How much slower Shift makes the sweep.
const FINE_DIVISOR: f32 = 5.0;

/// The staged preview's root.
#[derive(Component)]
pub struct BenchStage;

/// Which mesh the staged model is — keyed by path, the `drive_preview` idiom, so a re-selection of
/// a rig sharing the same GLB keeps the model (and its streamed-in player) instead of respawning.
#[derive(Component)]
pub(crate) struct BenchStageOf(String);

/// The scrub state.
#[derive(Resource)]
pub struct BenchScrub {
    /// The shared phase, `[0, 1)` — written by the keys while scrubbing, read back from the
    /// blender while playing so toggling is seamless.
    pub phase: f32,
    /// `true` = the figure advances at the mixture's own authored speed; `false` = held, keys move
    /// the phase.
    pub playing: bool,
    /// A soloed slot, if any — its weight is 1 and everything else 0.
    pub solo: Option<usize>,
    /// The slots in the equal mix when nothing is soloed.
    pub mixed: Vec<usize>,
}

impl Default for BenchScrub {
    fn default() -> Self {
        BenchScrub {
            phase: 0.0,
            // Alive by default: the figure walks in place the moment the tab opens. Space pauses
            // into a scrub.
            playing: true,
            solo: None,
            mixed: Vec::new(),
        }
    }
}

/// One weight chip in the pane, carrying its slot index.
#[derive(Component, Clone, Copy)]
pub(crate) struct BenchSlotChip(pub usize);

/// The `phase 0.372  playing` line under the chips.
#[derive(Component)]
pub(crate) struct ScrubLine;

/// **The stage camera preset** — judging foot contact requires seeing feet, and the default
/// framing is a three-quarter view of a whole figure. Cycled with `V`; applied by
/// `tiles::stage_camera`'s `Mode::Anim` arm (the one arbiter of the saved map view — a sibling
/// system would race the snapshot).
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct BenchCamera(pub CamPreset);

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum CamPreset {
    /// The whole figure, the tab's original framing.
    #[default]
    Figure,
    /// Framed on the contact joints — which sit at y ≈ 0 by construction, so no per-rig math.
    Feet,
    /// A low three-quarter profile.
    Side,
    /// Nearly ground level — the skate-judging view.
    Ground,
}

impl CamPreset {
    pub fn next(self) -> CamPreset {
        match self {
            CamPreset::Figure => CamPreset::Feet,
            CamPreset::Feet => CamPreset::Side,
            CamPreset::Side => CamPreset::Ground,
            CamPreset::Ground => CamPreset::Figure,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CamPreset::Figure => "figure",
            CamPreset::Feet => "feet",
            CamPreset::Side => "side",
            CamPreset::Ground => "ground",
        }
    }

    /// `(focus height above the stage, viewport height, elevation, yaw snap)`. The yaw snap puts
    /// the profile presets square to the figure's travel axis; the iso presets leave yaw alone so
    /// `Q`/`E` turning survives a preset cycle. Public so the headless test asserts against the
    /// same table the camera applies.
    pub fn framing(self) -> (f32, f32, f32, Option<f32>) {
        match self {
            CamPreset::Figure => (1.0, BENCH_VIEW_HEIGHT, crate::view::ISO_ELEVATION, None),
            CamPreset::Feet => (0.35, 1.6, crate::view::ISO_ELEVATION, None),
            CamPreset::Side => (1.0, 2.6, 0.26, Some(-std::f32::consts::FRAC_PI_4)),
            CamPreset::Ground => (0.3, 1.2, 0.09, Some(-std::f32::consts::FRAC_PI_4)),
        }
    }
}

/// `V`: the next preset.
pub(crate) fn cycle_cam_preset(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut cam: ResMut<BenchCamera>,
) {
    if crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::CycleCamPreset) {
        cam.0 = cam.0.next();
    }
}

/// **The A/B toggle**: stage a second, translucent figure playing the MEASURED numbers over the
/// declared one — judging an adopt becomes *look, then write* instead of write-and-undo. Its own
/// resource, never a `BenchScrub` field: the scrub is rewritten every playing frame, so anything
/// gated on `resource_changed::<BenchScrub>` fires constantly.
#[derive(Resource, Default)]
pub struct BenchAb(pub bool);

/// The ghost figure's root. Deliberately NOT `BenchStage` — `drive_bench_scrub` used to
/// `single_mut()` the stage, and a second match there kills scrubbing silently; `drive_bench_stage`'s
/// keyed respawn check would likewise mistake the ghost for the subject.
#[derive(Component)]
pub struct BenchGhost;

/// What the ghost was built from. A key mismatch respawns it; a manifest write clears the reports,
/// which retires it until the re-measure lands — so it can never play numbers the pane no longer
/// shows.
#[derive(Component, Clone, PartialEq)]
pub(crate) struct BenchGhostOf {
    mesh: String,
    /// The measured report's file fingerprint.
    fingerprint: u64,
    /// The slots excluded from adopt when it was built (the ghost stages exactly what adopt would
    /// write).
    excluded: std::collections::BTreeSet<usize>,
}

/// The one translucent material every ghost mesh shares — an overlay, not a second subject, so it
/// is unlit and reads wherever the two skeletons diverge.
#[derive(Resource)]
pub struct GhostMaterial(pub Handle<StandardMaterial>);

/// Startup. `Option`: the headless harness runs without the render half, and a bare
/// `ResMut<Assets<StandardMaterial>>` would panic the system there.
pub(crate) fn create_ghost_material(
    mut commands: Commands,
    mats: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let Some(mut mats) = mats else { return };
    let handle = mats.add(StandardMaterial {
        base_color: crate::chrome::ACCENT.with_alpha(0.35),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.insert_resource(GhostMaterial(handle));
}

/// **The rig as adopt would write it**: measured duration/phase/cycle over the declared slots,
/// skipping `keep:` slots and the transient excludes — so the ghost (and nothing else) answers
/// "what would I get?". Distances on `SlotMeasure` are file units; the rig's scale converts them
/// to the world units the manifest declares. A field that could not be measured keeps its declared
/// value — the same fallback-free rule adopt itself applies by only writing what was measured.
pub(crate) fn measured_rig(
    rig: &emerge_core::rigs::Rig,
    measured: &[emerge_core::rig_check::SlotMeasure],
    excluded: &std::collections::BTreeSet<usize>,
) -> emerge_core::rigs::Rig {
    let mut out = rig.clone();
    let scale = out.scale;
    for (i, slot) in out.slots.iter_mut().enumerate() {
        if slot.keep.is_some() || excluded.contains(&i) {
            continue;
        }
        let Playback::Gait {
            duration,
            phase_offset,
            cycle_distance,
        } = &mut slot.playback
        else {
            continue;
        };
        let Some(m) = measured.iter().find(|m| m.slot == i) else {
            continue;
        };
        *duration = m.duration;
        if let Some(ph) = m.phase_offset {
            *phase_offset = ph;
        }
        if let Some(cd) = m.cycle_distance {
            *cycle_distance = cd * scale;
        }
    }
    out
}

/// **Spawn and retire the ghost.** Wants one iff: on the Anim tab, the toggle is on, the selected
/// rig has gaits, and its report carries measured slots. Same shape as `drive_bench_stage`, keyed
/// by [`BenchGhostOf`].
pub(crate) fn drive_bench_ghost(
    mut commands: Commands,
    mode: Res<Mode>,
    ab: Option<Res<BenchAb>>,
    bench: Option<Res<BenchState>>,
    reports: Option<Res<crate::anim_watch::BenchReports>>,
    exclude: Option<Res<crate::anim_tab::AdoptExclude>>,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    ghosts: Query<(Entity, &BenchGhostOf), With<BenchGhost>>,
) {
    let want = (*mode == Mode::Anim && ab.as_ref().is_some_and(|a| a.0))
        .then(|| {
            let bench = bench.as_ref()?;
            let names = bench.names();
            let name = names.get(bench.selected)?;
            let rig = bench.rigs.as_ref()?.get(*name)?;
            if !rig.has_gaits() {
                return None;
            }
            let report = reports.as_ref()?.by_rig.get(*name)?;
            let fingerprint = report.fingerprint?;
            let excluded = exclude
                .as_ref()
                .map(|e| e.for_rig(name))
                .unwrap_or_default();
            (!report.slots.is_empty())
                .then(|| (rig.clone(), report.slots.clone(), fingerprint, excluded))
        })
        .flatten();

    let Some((rig, measured, fingerprint, excluded)) = want else {
        for (e, _) in &ghosts {
            commands.entity(e).despawn();
        }
        return;
    };
    let key = BenchGhostOf {
        mesh: rig.mesh.clone(),
        fingerprint,
        excluded: excluded.clone(),
    };
    if ghosts.iter().any(|(_, of)| *of == key) {
        return;
    }
    for (e, _) in &ghosts {
        commands.entity(e).despawn();
    }

    let staged = measured_rig(&rig, &measured, &excluded);
    let (graph, slots) = emerge_anim::rigs::build(&staged, &assets, &mut graphs);
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(rig.mesh.clone()));
    commands
        .spawn((
            BenchGhost,
            key,
            emerge_anim::BlendSource { graph, slots },
            Transform::from_translation(BENCH_STAGE).with_scale(Vec3::splat(rig.scale)),
            Visibility::Inherited,
        ))
        .with_children(|ghost| {
            // Same scene handle as the primary — the asset server dedupes the load — facing the
            // same way. No lights: the primary's booth lights are in range, and the tint is unlit
            // anyway.
            ghost
                .spawn((
                    WorldAssetRoot(scene),
                    Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                ))
                .observe(tint_ghost_scene);
        });
}

#[cfg(test)]
mod measured_rig_tests {
    use super::*;
    use emerge_core::rig_check::SlotMeasure;
    use emerge_core::rigs::{Rig, SlotDef};

    fn gait_slot(clip: usize) -> SlotDef {
        SlotDef {
            clip,
            playback: Playback::Gait {
                duration: 1.0,
                phase_offset: 0.1,
                cycle_distance: 2.0,
            },
            mask: None,
            note: None,
            state: None,
            keep: None,
            tolerance: None,
        }
    }

    #[test]
    fn measured_rig_stages_what_adopt_would_write() {
        let mut rig = Rig {
            mesh: "characters/test.glb".to_owned(),
            scale: 2.0,
            drive_speed: None,
            contact_eps: None,
            root_node: None,
            contact_joints: Vec::new(),
            provenance: None,
            slots: vec![
                gait_slot(0),
                gait_slot(1),
                gait_slot(2),
                SlotDef {
                    playback: Playback::Free { speed: 1.0 },
                    ..gait_slot(3)
                },
            ],
        };
        rig.slots[1].keep = Some("authored feel".to_owned());
        let measured = vec![
            SlotMeasure {
                slot: 0,
                duration: 1.5,
                cycle_distance: Some(1.1),
                phase_offset: Some(-0.2),
            },
            SlotMeasure {
                slot: 1,
                duration: 9.0,
                cycle_distance: Some(9.0),
                phase_offset: Some(0.9),
            },
            SlotMeasure {
                slot: 2,
                duration: 1.5,
                cycle_distance: None,
                phase_offset: None,
            },
        ];
        let excluded: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        let out = measured_rig(&rig, &measured, &excluded);
        // Slot 0: everything measured lands — cycle distance scaled into world units.
        assert_eq!(
            out.slots[0].playback,
            Playback::Gait {
                duration: 1.5,
                phase_offset: -0.2,
                cycle_distance: 2.2,
            }
        );
        // Slot 1 is kept: untouched, exactly as adopt would leave it.
        assert_eq!(out.slots[1].playback, rig.slots[1].playback);
        // Slot 2: unmeasured fields keep their declared values.
        assert_eq!(
            out.slots[2].playback,
            Playback::Gait {
                duration: 1.5,
                phase_offset: 0.1,
                cycle_distance: 2.0,
            }
        );
        // The free slot is not a gait and is never rewritten.
        assert_eq!(out.slots[3].playback, rig.slots[3].playback);

        // An excluded slot is skipped like a kept one.
        let excluded: std::collections::BTreeSet<usize> = [0usize].into_iter().collect();
        let out = measured_rig(&rig, &measured, &excluded);
        assert_eq!(out.slots[0].playback, rig.slots[0].playback);
    }
}

/// When the ghost's scene instance is ready, swap every mesh's material for the translucent tint.
/// An observer on the scene root, so it fires exactly once per spawned instance (the
/// `mixed_lighting` pattern) — no per-frame `Added` sweep.
fn tint_ghost_scene(
    ready: On<bevy::world_serialization::WorldInstanceReady>,
    tint: Option<Res<GhostMaterial>>,
    children: Query<&Children>,
    meshes: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
    mut commands: Commands,
) {
    let Some(tint) = tint else { return };
    let mut stack = vec![ready.entity];
    while let Some(e) = stack.pop() {
        if let Ok(kids) = children.get(e) {
            stack.extend(kids.iter());
        }
        if meshes.get(e).is_ok() {
            commands.entity(e).insert(MeshMaterial3d(tint.0.clone()));
        }
    }
}

/// **Spawn and retire the staged figure.** Early-outs like `drive_preview`: not on the Anim tab →
/// no stage; the selected rig's mesh differs from the staged one → replace it.
pub(crate) fn drive_bench_stage(
    mut commands: Commands,
    mode: Res<Mode>,
    bench: Option<Res<BenchState>>,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut scrub: ResMut<BenchScrub>,
    staged: Query<(Entity, &BenchStageOf), With<BenchStage>>,
) {
    let want = (*mode == Mode::Anim)
        .then(|| {
            bench.as_ref().and_then(|b| {
                let names = b.names();
                let name = names.get(b.selected)?;
                let rig = b.rigs.as_ref()?.get(name)?;
                Some((rig.clone(), rig.mesh.clone()))
            })
        })
        .flatten();

    let Some((rig, mesh)) = want else {
        for (e, _) in &staged {
            commands.entity(e).despawn();
        }
        return;
    };
    if staged.iter().any(|(_, of)| of.0 == mesh) {
        return;
    }
    for (e, _) in &staged {
        commands.entity(e).despawn();
    }
    // A fresh subject starts from the default mix: every gait slot equally, or — for a rig with no
    // gaits — its first slot solo, so a crab idles instead of standing in bind pose.
    let gaits: Vec<usize> = rig
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s.playback, Playback::Gait { .. }))
        .map(|(i, _)| i)
        .collect();
    scrub.mixed = gaits.clone();
    scrub.solo = if gaits.is_empty() { Some(0) } else { None };

    let (graph, slots) = emerge_anim::rigs::build(&rig, &assets, &mut graphs);
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    commands
        .spawn((
            BenchStage,
            BenchStageOf(mesh),
            emerge_anim::BlendSource { graph, slots },
            Transform::from_translation(BENCH_STAGE).with_scale(Vec3::splat(rig.scale)),
            Visibility::Inherited,
        ))
        .with_children(|stage| {
            // Facing the parked camera the way the game faces its figures — glTF +Z forward,
            // half-turned.
            stage.spawn((
                WorldAssetRoot(scene),
                Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            ));
            // The booth-light recipe (`thumbs::erect_booth`): point lights so nothing beyond their
            // range is touched, no shadows. Children, so retiring the stage is one despawn.
            for (offset, intensity) in [
                (Vec3::new(2.0, 3.0, 2.0), 400_000.0),
                (Vec3::new(-3.0, 2.0, 1.0), 150_000.0),
                (Vec3::new(0.0, 2.0, -3.0), 120_000.0),
            ] {
                stage.spawn((
                    PointLight {
                        intensity,
                        range: 20.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(offset),
                ));
            }
        });
}

/// **Drive the staged blender** — the bench's creature driver, ordered exactly like the game's
/// (`.after(PoseAttachSet).before(PoseBlendSet)`).
pub(crate) fn drive_bench_scrub(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    time: Res<Time>,
    bench: Option<Res<BenchState>>,
    mut scrub: ResMut<BenchScrub>,
    mut blenders: Query<
        (&mut emerge_anim::PoseBlender, Has<BenchGhost>),
        Or<(With<BenchStage>, With<BenchGhost>)>,
    >,
) {
    // Space toggles; the phase carries across the toggle in both directions.
    if crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::PlayPause) {
        scrub.playing = !scrub.playing;
    }
    let back = crate::keys::pressed(&keyboard, *live, crate::keys::Action::ScrubBack);
    let fwd = crate::keys::pressed(&keyboard, *live, crate::keys::Action::ScrubFwd);
    if !scrub.playing && (back || fwd) {
        let fine = if crate::keys::shift_held(&keyboard) {
            FINE_DIVISOR
        } else {
            1.0
        };
        let dir = (fwd as i8 - back as i8) as f32;
        scrub.phase = emerge_anim::wrap01(scrub.phase + dir * SCRUB_RATE / fine * time.delta_secs());
    }

    let Some(bench) = bench else { return };
    let names = bench.names();
    let Some(rig) = names
        .get(bench.selected)
        .and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)))
    else {
        return;
    };

    // Primary and ghost take the same weights; only the phase authority differs. The ghost is
    // ALWAYS held to the shared scrub phase — same φ into slots with measured numbers is exactly
    // the A/B semantics, and the two figures can never decohere.
    for (mut blender, is_ghost) in &mut blenders {
        // The target weights: a solo, or the equal mix.
        let len = blender.len();
        let mut targets = vec![0.0f32; len];
        match scrub.solo {
            Some(s) if s < len => targets[s] = 1.0,
            _ => {
                let n = scrub.mixed.iter().filter(|i| **i < len).count();
                if n > 0 {
                    for &i in scrub.mixed.iter().filter(|i| **i < len) {
                        targets[i] = 1.0 / n as f32;
                    }
                }
            }
        }
        if blender.set_targets(&targets).is_err() {
            continue;
        }

        if is_ghost {
            blender.set_ground_speed(0.0);
            blender.hold_phase(scrub.phase);
        } else if scrub.playing {
            blender.release_phase();
            // The mixture's own authored speed, so cadence sits at 1× nominal: mean cycle
            // distance × nominal cadence, both weighted the way `gait_cycles_per_sec` weighs them.
            let mut w_sum = 0.0f32;
            let mut w_dist = 0.0f32;
            let mut w_cad = 0.0f32;
            for (i, slot) in rig.slots.iter().enumerate() {
                if let Playback::Gait {
                    duration,
                    cycle_distance,
                    ..
                } = slot.playback
                {
                    let w = targets.get(i).copied().unwrap_or(0.0);
                    w_sum += w;
                    w_dist += w * cycle_distance;
                    if duration > 1.0e-6 {
                        w_cad += w / duration;
                    }
                }
            }
            let authored = if w_sum > 1.0e-6 {
                (w_dist / w_sum) * (w_cad / w_sum)
            } else {
                0.0
            };
            blender.set_ground_speed(authored);
            // Read the phase back so toggling into a scrub starts where the figure stands.
            scrub.phase = blender.phase();
        } else {
            blender.set_ground_speed(0.0);
            blender.hold_phase(scrub.phase);
        }
    }
}

/// The ghost toggle chip in the pane.
#[derive(Component)]
pub(crate) struct GhostChip;

/// The ghost chip: toggle the A/B overlay.
pub(crate) fn on_ghost_chip_click(
    activate: On<Activate>,
    chips: Query<(), With<GhostChip>>,
    mut ab: ResMut<BenchAb>,
) {
    if chips.get(activate.entity).is_ok() {
        ab.0 = !ab.0;
    }
}

/// `G`: the same toggle, from the keyboard.
pub(crate) fn toggle_ghost_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut ab: ResMut<BenchAb>,
) {
    if crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::ToggleGhost) {
        ab.0 = !ab.0;
    }
}

/// A chip click: solo the slot; a mod-click toggles it in or out of the equal mix instead.
pub(crate) fn on_chip_click(
    activate: On<Activate>,
    keyboard: Res<ButtonInput<KeyCode>>,
    chips: Query<&BenchSlotChip>,
    mut scrub: ResMut<BenchScrub>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    if crate::keys::mod_held(&keyboard) {
        scrub.solo = None;
        if let Some(at) = scrub.mixed.iter().position(|i| *i == chip.0) {
            scrub.mixed.remove(at);
        } else {
            scrub.mixed.push(chip.0);
            scrub.mixed.sort_unstable();
        }
    } else if scrub.solo == Some(chip.0) {
        // Clicking the soloed chip un-solos back to the mix.
        scrub.solo = None;
    } else {
        scrub.solo = Some(chip.0);
    }
}

/// **Which stage chips are lit, and what the scrub line says.**
///
/// The two chip loops used to write `BackgroundColor` directly, each restating
/// `chrome::style_list_rows`'s lit / hover / rest priority privately. Since 2026-09-03 that is a
/// **conflict** rather than a duplication: a `chrome::chip` carries [`crate::chrome::RowRest`], so
/// the shared repainter writes the same component every frame and the two would take turns at it
/// in whatever order the schedule happened to pick.
///
/// What only this module knows is which chip is *on* — soloed, mixed in, or ghosting — so that is
/// all it writes. `style_list_rows` derives hover, press, disabled and the lit fill from it, and
/// lit still beats hover there for the same reason it did here: hover says *this is a click
/// target*, not *this is playing*.
pub(crate) fn refresh_scrub_ui(
    scrub: Option<Res<BenchScrub>>,
    ab: Option<Res<BenchAb>>,
    cam: Option<Res<BenchCamera>>,
    mut chips: Query<(&BenchSlotChip, &mut crate::chrome::RowRest)>,
    mut ghost_chips: Query<
        &mut crate::chrome::RowRest,
        (With<GhostChip>, Without<BenchSlotChip>),
    >,
    mut lines: Query<&mut Text, With<ScrubLine>>,
) {
    let Some(scrub) = scrub else { return };
    for (chip, mut rest) in &mut chips {
        let lit = match scrub.solo {
            Some(s) => s == chip.0,
            None => scrub.mixed.contains(&chip.0),
        };
        let want = if lit { ROW_SELECTED } else { ROW_BG };
        if rest.0 != want {
            rest.0 = want;
        }
    }
    let ghost_on = ab.is_some_and(|a| a.0);
    for mut rest in &mut ghost_chips {
        let want = if ghost_on { ROW_SELECTED } else { ROW_BG };
        if rest.0 != want {
            rest.0 = want;
        }
    }
    let want = format!(
        "phase {:.3}  {}  ·  view: {} (V)",
        scrub.phase,
        if scrub.playing { "playing" } else { "scrub (left/right, Shift: fine)" },
        cam.map(|c| c.0.label()).unwrap_or("figure"),
    );
    for mut text in &mut lines {
        if text.0 != want {
            text.0 = want.clone();
        }
    }
}

/// The chip row and scrub line, spawned into the pane by `rebuild_slots`.
pub(crate) fn spawn_chips(p: &mut ChildSpawnerCommands, rig: &emerge_core::rigs::Rig) {
    crate::chrome::section(p, "STAGE");
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(crate::chrome::GAP_TIGHT),
        flex_wrap: FlexWrap::Wrap,
        // A wrapping row of chips has to be allowed to narrow, or its last member is clipped by
        // the pane's right edge (the audit's F7).
        // CHROME-OK: zero is not a spacing step.
        min_width: Val::Px(0.0),
        row_gap: Val::Px(crate::chrome::GAP_TIGHT),
        ..default()
    })
    .with_children(|row| {
        for (i, slot) in rig.slots.iter().enumerate() {
            let label = slot
                .note
                .as_deref()
                .map(|n| n.split([' ', '—']).next().unwrap_or(n).to_owned())
                .unwrap_or_else(|| format!("slot {i}"));
            crate::chrome::chip(
                row,
                BenchSlotChip(i),
                &format!("{i} {label}"),
                crate::chrome::text::CONTROL,
                TEXT,
                ROW_BG,
                Color::NONE,
            );
        }
    });
    // The ghost toggle, only where there is something to A/B — a gait-less rig has no measured
    // numbers to stage.
    if rig.has_gaits() {
        p.spawn(Node {
            flex_direction: FlexDirection::Row,
            margin: UiRect::top(Val::Px(crate::chrome::GAP_TIGHT)),
            ..default()
        })
        .with_children(|row| {
            crate::chrome::chip(
                row,
                GhostChip,
                "ghost (G): play the measured values over the declared",
                crate::chrome::text::CONTROL,
                TEXT,
                ROW_BG,
                Color::NONE,
            );
        });
    }
    p.spawn((
        Text::new("click: solo · mod-click: mix in/out"),
        TextColor(LABEL),
        crate::chrome::font(crate::chrome::text::HINT),
    ));
    p.spawn((
        Text::new(String::new()),
        TextColor(DIM),
        crate::chrome::font(crate::chrome::text::HINT),
        ScrubLine,
    ));
}
