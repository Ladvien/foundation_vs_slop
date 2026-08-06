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

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton};

use emerge_core::rigs::Playback;

use crate::anim_tab::BenchState;
use crate::chrome::{DIM, LABEL, ROW_BG, ROW_SELECTED, TEXT};
use crate::tiles::Mode;

/// The bench's staging corner — the third one.
pub const BENCH_STAGE: Vec3 = Vec3::new(-4096.0, 0.0, -4096.0);

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
    mut blenders: Query<&mut emerge_anim::PoseBlender, With<BenchStage>>,
) {
    // Space toggles; the phase carries across the toggle in both directions.
    if crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::PlayPause) {
        scrub.playing = !scrub.playing;
    }
    let back = crate::keys::pressed(&keyboard, live.0, crate::keys::Action::ScrubBack);
    let fwd = crate::keys::pressed(&keyboard, live.0, crate::keys::Action::ScrubFwd);
    if !scrub.playing && (back || fwd) {
        let fine = if crate::keys::shift_held(&keyboard) {
            FINE_DIVISOR
        } else {
            1.0
        };
        let dir = (fwd as i8 - back as i8) as f32;
        scrub.phase = emerge_anim::wrap01(scrub.phase + dir * SCRUB_RATE / fine * time.delta_secs());
    }

    let Ok(mut blender) = blenders.single_mut() else {
        return;
    };
    let Some(bench) = bench else { return };
    let names = bench.names();
    let Some(rig) = names
        .get(bench.selected)
        .and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)))
    else {
        return;
    };

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
        return;
    }

    if scrub.playing {
        blender.release_phase();
        // The mixture's own authored speed, so cadence sits at 1× nominal: mean cycle distance ×
        // nominal cadence, both weighted the way `gait_cycles_per_sec` weighs them.
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

/// Repaint the chips' pressed-state and the scrub line, in place.
pub(crate) fn refresh_scrub_ui(
    scrub: Option<Res<BenchScrub>>,
    mut chips: Query<(&BenchSlotChip, &mut BackgroundColor)>,
    mut lines: Query<&mut Text, With<ScrubLine>>,
) {
    let Some(scrub) = scrub else { return };
    for (chip, mut bg) in &mut chips {
        let lit = match scrub.solo {
            Some(s) => s == chip.0,
            None => scrub.mixed.contains(&chip.0),
        };
        let want = if lit { ROW_SELECTED } else { ROW_BG };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    let want = format!(
        "phase {:.3}  {}",
        scrub.phase,
        if scrub.playing { "playing" } else { "scrub (left/right, Shift: fine)" }
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
        column_gap: Val::Px(4.0),
        flex_wrap: FlexWrap::Wrap,
        row_gap: Val::Px(3.0),
        ..default()
    })
    .with_children(|row| {
        for (i, slot) in rig.slots.iter().enumerate() {
            let label = slot
                .note
                .as_deref()
                .map(|n| n.split([' ', '—']).next().unwrap_or(n).to_owned())
                .unwrap_or_else(|| format!("slot {i}"));
            row.spawn((
                UiButton,
                Hovered::default(),
                BenchSlotChip(i),
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(ROW_BG),
            ))
            .with_children(|chip| {
                chip.spawn((
                    Text::new(format!("{i} {label}")),
                    TextColor(TEXT),
                    TextFont::from_font_size(10.0),
                ));
            });
        }
    });
    p.spawn((
        Text::new("click: solo · mod-click: mix in/out"),
        TextColor(LABEL),
        TextFont::from_font_size(9.0),
    ));
    p.spawn((
        Text::new(String::new()),
        TextColor(DIM),
        TextFont::from_font_size(9.0),
        ScrubLine,
    ));
}
