//! **The animation bench** — what a rig's clips are, and whether the asset still agrees.
//!
//! The editor's third tab. `emerge_core::clips` measures a GLB and `assets/emerge/rigs.ron` records
//! what the game plays; this is where an author sees both at once. `docs/animation.md` names the gap
//! it closes: getting a gait's `(duration, phase_offset, cycle_distance)` was *"a manual offline step,
//! not a repo tool"*, and `src/site/staff_anim.rs` calls that measuring *"the largest hidden cost in
//! animating a new character"*.
//!
//! # It reads the manifest, and re-measures the asset beside it
//!
//! Every row shows the declared number and the measured one. That pairing is the point — a manifest
//! agreeing with itself proves nothing, and the failure this exists to catch is an artist re-exporting
//! a rig while the numbers stay where they were.
//!
//! # Same two panels as the other tabs
//!
//! Controls left, list right, both from [`crate::chrome`]. A third tab costs a `Mode` row and this
//! file rather than another copy of the panel furniture — which is why `chrome` was extracted first.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton};
use emerge_core::rigs::{Playback, Rigs};

use crate::chrome::{ACCENT, DANGER, DIM, LABEL, ROW_BG, ROW_SELECTED, TEXT};
use crate::keys::Context;
use crate::tiles::{AnimRoot, Mode};

/// What the bench is looking at.
#[derive(Resource, Default)]
pub struct BenchState {
    /// The manifest, read on first entry to the tab.
    pub rigs: Option<Rigs>,
    /// Which rig, as an index into the manifest's sorted names.
    pub selected: usize,
    /// Whether the read has happened. Separate from `rigs.is_none()`, which is also true of a read
    /// that failed — and those two want different words on screen.
    pub loaded: bool,
    pub status: String,
    /// The selected rig's measurements. Recomputed on selection change only: `measure` reads the GLB
    /// off disk and runs forward kinematics over every keyframe, which is not per-frame work.
    pub findings: Vec<Finding>,
    /// Which rig `findings` describes, so a re-measure happens exactly once per selection.
    measured: Option<usize>,
}

impl BenchState {
    /// The rig names, in manifest order. `Rigs` keeps a `BTreeMap`, so this is stable across runs —
    /// a list that reordered itself between sessions is one nobody can build a memory of
    /// (`docs/ui.md` §3.5, Samp 2011).
    pub fn names(&self) -> Vec<&str> {
        self.rigs
            .as_ref()
            .map(|r| r.rigs.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

/// One rig row, carrying its index.
#[derive(Component, Clone, Copy)]
struct RigRow(usize);

/// The node the rig list is rebuilt into.
#[derive(Component)]
struct RigList;

/// The node the selected rig's slot table is rebuilt into.
#[derive(Component)]
struct SlotPane;

/// The transient line.
#[derive(Component)]
struct BenchLine;

pub struct AnimTabPlugin;

impl Plugin for AnimTabPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BenchState>()
            .add_systems(Startup, spawn_panels)
            .add_systems(
                Update,
                (
                    load_on_entry,
                    // Ungated: `keys::just_pressed` now refuses an `Anim` binding unless the Anim
                    // tab owns the keyboard, so a run condition here would be the second census
                    // this module exists to prevent.
                    move_selection.in_set(crate::keys::Phase::Act),
                    keep_selection_visible,
                    measure_selected,
                    rebuild_list.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::filter::Filters>),
                    ),
                    rebuild_slots.run_if(resource_changed::<BenchState>),
                    refresh_line,
                ),
            )
            .add_observer(on_rig_click);
    }
}

fn spawn_panels(mut commands: Commands) {
    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Left,
        crate::chrome::TILES_CONTROLS_W,
        true,
        true,
    )
    .insert(AnimRoot)
    .with_children(|p| {
        crate::chrome::title(p, "ANIMATION");
        crate::chrome::key_census(p, &[Context::Anim, Context::Global]);
        p.spawn((
            Text::new(""),
            TextColor(DIM),
            TextFont::from_font_size(10.0),
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            BenchLine,
        ));
        // The slot table scrolls: a rig has up to ten slots and each is two lines with its measured
        // number under the declared one.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                margin: UiRect::top(Val::Px(8.0)),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            bevy::ui_widgets::ScrollArea::default(),
            SlotPane,
        ));
    });

    crate::chrome::panel_root(
        &mut commands,
        crate::chrome::Side::Right,
        crate::chrome::LIST_W,
        true,
        true,
    )
    .insert(AnimRoot)
    .with_children(|p| {
        p.spawn((
            Text::new("RIGS"),
            TextColor(LABEL),
            TextFont::from_font_size(10.0),
        ));
        crate::filter::spawn(p, crate::filter::Pane::Rigs);
        crate::chrome::scroll_list(p, RigList);
    });
}

/// Read the manifest the first time the tab is opened.
///
/// Lazy, on the precedent the tiles scan sets: a session that never opens this tab should not pay for
/// it. Loud on failure — a bench with no manifest has nothing to show, and saying so beats an empty
/// list that looks like "this project has no rigs".
fn load_on_entry(mode: Res<Mode>, project: Res<crate::project::Project>, mut bench: ResMut<BenchState>) {
    if *mode != Mode::Anim || bench.loaded {
        return;
    }
    bench.loaded = true;
    let path = project.root.join("assets/emerge/rigs.ron");
    match std::fs::read_to_string(&path) {
        Ok(text) => match Rigs::parse(&text) {
            Ok(rigs) => {
                bench.status = format!("{} rig(s) in {}", rigs.rigs.len(), path.display());
                bench.rigs = Some(rigs);
            }
            Err(e) => bench.status = format!("{}: {e}", path.display()),
        },
        Err(e) => bench.status = format!("cannot read {}: {e}", path.display()),
    }
}

/// **Keep the selection inside the filtered list.**
///
/// The selection indexes the WHOLE manifest, so filtering to `val` left the panel describing
/// `cipher_field` while the list offered only `valkyrie` — a detail pane about a row that is not on
/// screen. Narrowing to one row should land on it.
fn keep_selection_visible(filters: Res<crate::filter::Filters>, mut bench: ResMut<BenchState>) {
    if !filters.is_changed() {
        return;
    }
    let pane = crate::filter::Pane::Rigs;
    let names: Vec<String> = bench.names().iter().map(|s| (*s).to_owned()).collect();
    if names.is_empty() {
        return;
    }
    let visible = |i: usize| names.get(i).is_some_and(|n| filters.keeps(pane, n));
    if visible(bench.selected) {
        return;
    }
    // The first row that survives — and if none does, leave the selection alone rather than jumping
    // it somewhere arbitrary the moment a filter matches nothing mid-word.
    if let Some(first) = (0..names.len()).find(|i| visible(*i)) {
        bench.selected = first;
    }
}

/// Re-measure when the selection moves.
fn measure_selected(
    project: Res<crate::project::Project>,
    mut bench: ResMut<BenchState>,
) {
    if bench.measured == Some(bench.selected) || bench.rigs.is_none() {
        return;
    }
    let names: Vec<String> = bench.names().iter().map(|s| (*s).to_owned()).collect();
    let Some(name) = names.get(bench.selected).cloned() else {
        return;
    };
    let rig = bench.rigs.as_ref().and_then(|r| r.get(&name)).cloned();
    let Some(rig) = rig else { return };
    let found = measure(&project.root, &rig);
    bench.findings = found;
    bench.measured = Some(bench.selected);
}

fn move_selection(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut bench: ResMut<BenchState>,
) {
    let n = bench.names().len();
    if n == 0 {
        return;
    }
    let step = if crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::NextRig) {
        1
    } else if crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::PrevRig) {
        n - 1
    } else {
        return;
    };
    bench.selected = (bench.selected + step) % n;
}

fn on_rig_click(
    activate: On<Activate>,
    rows: Query<&RigRow>,
    mut bench: ResMut<BenchState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    bench.selected = row.0;
}

fn rebuild_list(
    mut commands: Commands,
    bench: Res<BenchState>,
    filters: Res<crate::filter::Filters>,
    lists: Query<Entity, With<RigList>>,
) {
    let pane = crate::filter::Pane::Rigs;
    let names = bench.names();
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|p| {
            for (ix, name) in names.iter().enumerate() {
                if !filters.keeps(pane, name) {
                    continue;
                }
                p.spawn((
                    UiButton,
                    Hovered::default(),
                    RigRow(ix),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(if ix == bench.selected {
                        ROW_SELECTED
                    } else {
                        ROW_BG
                    }),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new((*name).to_owned()),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
            }
        });
    }
}

/// The selected rig's slot table: what the manifest declares, slot by slot.
fn rebuild_slots(
    mut commands: Commands,
    bench: Res<BenchState>,
    panes: Query<Entity, With<SlotPane>>,
) {
    let names = bench.names();
    let rig = names
        .get(bench.selected)
        .and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)));
    for pane in &panes {
        commands.entity(pane).despawn_related::<Children>();
        commands.entity(pane).with_children(|p| {
            let Some(rig) = rig else {
                p.spawn((
                    Text::new("no rig selected"),
                    TextColor(DIM),
                    TextFont::from_font_size(11.0),
                ));
                return;
            };
            p.spawn((
                Text::new(rig.mesh.clone()),
                TextColor(ACCENT),
                TextFont::from_font_size(11.0),
            ));
            for (i, slot) in rig.slots.iter().enumerate() {
                let (kind, detail) = match slot.playback {
                    Playback::Free { speed } => ("free", format!("x{speed:.2}")),
                    Playback::OneShot { speed } => ("once", format!("x{speed:.2}")),
                    Playback::Gait {
                        duration,
                        phase_offset,
                        cycle_distance,
                    } => (
                        "gait",
                        format!("{duration:.3}s  ph {phase_offset:+.3}  {cycle_distance:.3}m"),
                    ),
                };
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            // Wide enough for "0 - clip 14" on one line. At 52 px it wrapped after
                            // "clip" and every row became two, which made a ten-slot rig unreadable.
                            min_width: Val::Px(84.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(format!("{i} - clip {}", slot.clip)),
                        TextColor(LABEL),
                        TextFont::from_font_size(10.0),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                    ));
                    row.spawn((
                        Node {
                            width: Val::Px(30.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Text::new(kind.to_owned()),
                        // A gait is the only kind carrying measured numbers, so it is the only one
                        // worth picking out of the column.
                        TextColor(if kind == "gait" { ACCENT } else { DIM }),
                        TextFont::from_font_size(10.0),
                    ));
                    row.spawn((
                        Text::new(detail),
                        TextColor(TEXT),
                        TextFont::from_font_size(10.0),
                    ));
                });
                if let Some(note) = &slot.note {
                    p.spawn((
                        Text::new(note.clone()),
                        TextColor(DIM),
                        TextFont::from_font_size(9.0),
                        Node {
                            margin: UiRect::left(Val::Px(84.0)),
                            ..default()
                        },
                    ));
                }
            }

            // **Measured against the asset, under the table it is measuring.** A finding with no fix
            // is a finding that gets read once, so each says what to do.
            if bench.findings.is_empty() {
                return;
            }
            p.spawn((
                Text::new("MEASURED".to_owned()),
                TextColor(LABEL),
                TextFont::from_font_size(10.0),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
            ));
            for f in &bench.findings {
                p.spawn((
                    Text::new(f.text.clone()),
                    TextColor(match f.level {
                        Level::Ok => DIM,
                        Level::Note => LABEL,
                        Level::Bad => DANGER,
                    }),
                    TextFont::from_font_size(9.0),
                ));
            }
        });
    }
}

fn refresh_line(bench: Res<BenchState>, mut lines: Query<(&mut Text, &mut TextColor), With<BenchLine>>) {
    if !bench.is_changed() {
        return;
    }
    for (mut text, mut colour) in &mut lines {
        if text.0 != bench.status {
            text.0 = bench.status.clone();
        }
        // A message naming a file that could not be read is a refusal, not a status.
        let want = if bench.rigs.is_none() && bench.loaded {
            DANGER
        } else {
            DIM
        };
        if colour.0 != want {
            colour.0 = want;
        }
    }
}

/// **What the asset says, against what the manifest claims.**
///
/// A manifest agreeing with itself proves nothing. This re-measures the GLB with
/// `emerge_core::clips` — the same functions `crates/emerge-core/tests/rigs_match_assets.rs` runs in
/// CI — and reports each disagreement with the fix, the rule `tiles.rs` states as *"a warning that
/// does not say what to do about it is a warning that gets read once."*
fn measure(root: &std::path::Path, rig: &emerge_core::rigs::Rig) -> Vec<Finding> {
    let mut out = Vec::new();
    let path = root.join("assets").join(&rig.mesh);
    let glb = match emerge_core::glb::Glb::open(&path) {
        Ok(g) => g,
        Err(e) => {
            out.push(Finding::bad(format!("cannot read {}: {e}", rig.mesh)));
            return out;
        }
    };
    let found = emerge_core::clips::clips(&glb);
    let root_node = emerge_core::clips::node_index(&glb, "Root");
    let foot = emerge_core::clips::node_index(&glb, "foot_l");

    for (i, slot) in rig.slots.iter().enumerate() {
        let Some(c) = found.get(slot.clip) else {
            out.push(Finding::bad(format!(
                "slot {i} names clip {} but the asset has {} — it was re-exported; re-measure and \
                 update rigs.ron",
                slot.clip,
                found.len()
            )));
            continue;
        };
        let Playback::Gait {
            duration,
            cycle_distance,
            ..
        } = slot.playback
        else {
            continue;
        };
        // One 24 fps frame is the tolerance the phase mapping needs.
        if (c.duration - duration).abs() >= 1.0 / 24.0 {
            out.push(Finding::bad(format!(
                "slot {i} is {:.3}s in the asset, {duration:.3}s here — the shared phase maps onto \
                 the wrong part of the clip and feet drift",
                c.duration
            )));
        }
        if let Some(r) = root_node {
            let m = emerge_core::clips::root_motion(&glb, slot.clip, r);
            if m.iter().any(|v| *v >= 1.0e-4) {
                out.push(Finding::bad(format!(
                    "slot {i} moves Root by {:?} — a gait must be authored in place; the game drives \
                     the transform itself",
                    m
                )));
            }
        }
        if let Some(f) = foot {
            match emerge_core::clips::cycle_distance(&glb, slot.clip, f) {
                Some(raw) => {
                    let measured = raw * FIGURINE_SCALE;
                    let err = (measured - cycle_distance).abs() / cycle_distance;
                    if err >= 0.20 {
                        out.push(Finding::bad(format!(
                            "slot {i} measures {measured:.3} m/cycle, manifest says \
                             {cycle_distance:.3} ({:.0}% out) — re-measure",
                            err * 100.0
                        )));
                    } else {
                        out.push(Finding::ok(format!(
                            "slot {i} measures {measured:.3} m/cycle vs {cycle_distance:.3} declared"
                        )));
                    }
                }
                None => out.push(Finding::note(format!(
                    "slot {i}: no planted-foot stance to measure"
                ))),
            }
        }
    }
    out
}

/// `squad::FIGURINE_SCALE` — the manifest is in world units, the GLB in its own.
const FIGURINE_SCALE: f32 = 1.13;

/// One measurement result.
pub struct Finding {
    pub text: String,
    pub level: Level,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Level {
    Ok,
    Note,
    Bad,
}

impl Finding {
    fn ok(text: String) -> Finding {
        Finding { text, level: Level::Ok }
    }
    fn note(text: String) -> Finding {
        Finding { text, level: Level::Note }
    }
    fn bad(text: String) -> Finding {
        Finding { text, level: Level::Bad }
    }
}
