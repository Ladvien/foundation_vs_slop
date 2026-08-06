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
use emerge_core::rig_check::{Level, Staleness};
use emerge_core::rigs::{Playback, Rigs};

use crate::chrome::{ACCENT, DANGER, DIM, LABEL, ROW_BG, ROW_SELECTED, TEXT};
use crate::tiles::{AnimRoot, Mode};

/// How many manifest snapshots the bench keeps. The undo unit is the whole file's text — writes are
/// rare and a manifest is ~10 KB, so the cost of a text snapshot is nothing next to the cost of a
/// write that cannot be taken back.
const BENCH_HISTORY: usize = 64;

/// Which face the left pane shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// The selected rig's slot table, findings, plots and stage controls.
    #[default]
    One,
    /// The project-wide summary — every rig's worst finding, jump-to-detail.
    All,
}

/// What the bench is looking at.
#[derive(Resource, Default)]
pub struct BenchState {
    /// The manifest, read on first entry to the tab.
    pub rigs: Option<Rigs>,
    /// The manifest's TEXT as loaded or last written — what `RigDoc` edits, and the undo unit.
    /// Kept beside `rigs` because the parsed value cannot reproduce the file's comments.
    pub text: Option<String>,
    /// Manifest texts to restore, newest last. Restored THROUGH `commit_text`, so an undo re-runs
    /// the same validation and atomic save as the write it takes back.
    undo: Vec<String>,
    redo: Vec<String>,
    /// Which rig, as an index into the manifest's sorted names.
    pub selected: usize,
    /// Whether the read has happened. Separate from `rigs.is_none()`, which is also true of a read
    /// that failed — and those two want different words on screen.
    pub loaded: bool,
    pub status: String,
    /// One rig, or the whole project.
    pub view: View,
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

    /// Push the pre-write text onto the undo stack. Called only after a write actually landed —
    /// the tiles idiom: `record` on `Ok`, never on refusal.
    fn record(&mut self, before: String) {
        if self.undo.len() >= BENCH_HISTORY {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }
}

/// One rig row, carrying its index.
#[derive(Component, Clone, Copy)]
struct RigRow(usize);

/// One summary row in the check-all view, carrying the rig index it jumps to.
#[derive(Component, Clone, Copy)]
struct JumpRow(usize);

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
            .init_resource::<crate::anim_watch::RigWatch>()
            .init_resource::<crate::anim_watch::MeasureQueue>()
            .init_resource::<crate::anim_watch::BenchReports>()
            .init_resource::<crate::anim_watch::BenchGeneration>()
            .init_resource::<crate::anim_stage::BenchScrub>()
            // The editor registers the game's blend pass once, here — the staged figure is driven
            // by the REAL machinery, which is the whole reason emerge-anim is a crate.
            .add_plugins(emerge_anim::PoseBlendPlugin)
            .add_systems(Startup, (spawn_panels, crate::anim_plots::create_plot_images))
            .add_systems(
                Update,
                (
                    load_on_entry,
                    // Ungated: `keys::just_pressed` now refuses an `Anim` binding unless the Anim
                    // tab owns the keyboard, so a run condition here would be the second census
                    // this module exists to prevent.
                    move_selection.in_set(crate::keys::Phase::Act),
                    check_all_keys.in_set(crate::keys::Phase::Act),
                    adopt_measured.in_set(crate::keys::Phase::Act),
                    bench_history_keys.in_set(crate::keys::Phase::Act),
                    keep_selection_visible,
                    // The one measurement path: selection, the watcher and check-all feed the
                    // queue; the queue steps one rig per frame; the pane reads the reports.
                    crate::anim_watch::poll_mtimes,
                    crate::anim_watch::queue_selected,
                    crate::anim_watch::step_measure_queue,
                    crate::anim_watch::paint_stale_badge
                        .run_if(resource_changed::<crate::anim_watch::BenchGeneration>),
                    rebuild_list.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::filter::Filters>),
                    ),
                    rebuild_slots.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::anim_watch::BenchGeneration>),
                    ),
                    crate::anim_plots::render_plots.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::anim_watch::BenchGeneration>),
                    ),
                    refresh_line,
                    // The staged figure: spawned per selection, driven exactly like a game
                    // creature — after attach, before the blend pass writes the player.
                    crate::anim_stage::drive_bench_stage,
                    crate::anim_stage::drive_bench_scrub
                        .in_set(crate::keys::Phase::Act)
                        .after(emerge_anim::PoseAttachSet)
                        .before(emerge_anim::PoseBlendSet),
                    crate::anim_stage::refresh_scrub_ui,
                ),
            )
            .add_observer(on_rig_click)
            .add_observer(on_jump_click)
            .add_observer(crate::anim_stage::on_chip_click);
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
        crate::chrome::shortcut_hint(p);
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
fn load_on_entry(
    mode: Res<Mode>,
    project: Res<crate::project::Project>,
    mut bench: ResMut<BenchState>,
    mut queue: ResMut<crate::anim_watch::MeasureQueue>,
) {
    if *mode != Mode::Anim || bench.loaded {
        return;
    }
    bench.loaded = true;
    let path = project.root.join("assets/emerge/rigs.ron");
    match std::fs::read_to_string(&path) {
        Ok(text) => match Rigs::parse(&text) {
            Ok(rigs) => {
                bench.status = format!("{} rig(s) in {}", rigs.rigs.len(), path.display());
                // **Check-all runs on open.** The audit should not wait to be asked for: every rig
                // enters the queue now (one measures per frame), so the stale badge and the C
                // summary are warm from the first entry. The selected rig still jumps the line via
                // `queue_selected`. Lazy-load stands — a session that never opens the tab pays
                // nothing.
                for name in rigs.rigs.keys() {
                    queue.push_back_unique(name);
                }
                bench.rigs = Some(rigs);
                bench.text = Some(text);
            }
            Err(e) => bench.status = format!("{}: {e}", path.display()),
        },
        Err(e) => bench.status = format!("cannot read {}: {e}", path.display()),
    }
}

/// **The one door to disk.** Parse and validate the candidate text, write it atomically, and only
/// then adopt it in memory — the `tiles::commit_measured` shape. A failure leaves both the file and
/// the in-memory state exactly as they were.
fn commit_text(
    root: &std::path::Path,
    bench: &mut BenchState,
    new_text: String,
) -> Result<(), String> {
    let parsed = Rigs::parse(&new_text)?;
    let path = root.join("assets/emerge/rigs.ron");
    emerge_core::ron_surgery::save_atomic(&path, &new_text)?;
    bench.rigs = Some(parsed);
    bench.text = Some(new_text);
    Ok(())
}

/// **Adopt measured values for the selected rig** — the explicit write-back, and the only one.
///
/// Measures the asset NOW (hash, clips, checks in one motion, so the provenance stamps exactly the
/// bytes the numbers came from), rewrites each unkept gait slot's measured fields through
/// [`emerge_core::rigs_edit::RigDoc`], stamps provenance, and refuses the whole write unless the
/// edited text parses back to precisely the value it built in memory — `replace_field`'s key search
/// is textual, and corruption must be refused, never written.
fn adopt(root: &std::path::Path, bench: &mut BenchState) -> Result<String, String> {
    let text = bench.text.clone().ok_or("no manifest loaded")?;
    let name = bench
        .names()
        .get(bench.selected)
        .map(|s| (*s).to_owned())
        .ok_or("no rig selected")?;
    let rigs = bench.rigs.as_ref().ok_or("no manifest loaded")?;
    let rig = rigs
        .get(&name)
        .cloned()
        .ok_or_else(|| format!("no rig named `{name}`"))?;
    if !rig.has_gaits() {
        return Err(format!("'{name}' has no gait slots — nothing to adopt"));
    }

    let path = root.join("assets").join(&rig.mesh);
    let (glb, hash) = emerge_core::glb::Glb::open_fingerprinted(&path)?;
    let clip_infos = emerge_core::clips::clips(&glb);
    let current = emerge_core::rig_check::staleness(&rig, hash)
        == emerge_core::rig_check::Staleness::Current;
    let report = emerge_core::rig_check::check_rig(&glb, &rig, current);

    let mut doc = emerge_core::rigs_edit::RigDoc::open(&text, &name)?;
    // The in-memory expectation the written text must parse back to.
    let mut trial = rigs.clone();
    let trial_rig = trial
        .rigs
        .get_mut(&name)
        .ok_or_else(|| format!("no rig named `{name}`"))?;

    let mut wrote = 0usize;
    let mut kept = 0usize;
    for (i, slot) in rig.slots.iter().enumerate() {
        let Playback::Gait {
            duration: d0,
            phase_offset: p0,
            cycle_distance: c0,
        } = slot.playback
        else {
            continue;
        };
        if slot.keep.is_some() {
            kept += 1;
            continue;
        }
        let Some(m) = report.slots.iter().find(|m| m.slot == i) else {
            continue;
        };
        let mut adopted = (d0, p0, c0);
        let dur = emerge_core::ron_surgery::fmt_f32(m.duration);
        doc.edit_slot_field(i, "duration", &dur)?;
        adopted.0 = m.duration;
        wrote += 1;
        if let Some(ph) = m.phase_offset {
            doc.edit_slot_field(i, "phase_offset", &emerge_core::ron_surgery::fmt_f32(ph))?;
            adopted.1 = ph;
            wrote += 1;
        }
        if let Some(raw) = m.cycle_distance {
            let world = raw * rig.scale;
            doc.edit_slot_field(i, "cycle_distance", &emerge_core::ron_surgery::fmt_f32(world))?;
            adopted.2 = world;
            wrote += 1;
        }
        if let Some(s) = trial_rig.slots.get_mut(i) {
            s.playback = Playback::Gait {
                duration: adopted.0,
                phase_offset: adopted.1,
                cycle_distance: adopted.2,
            };
        }
    }
    if wrote == 0 {
        return Err(if kept > 0 {
            format!("all of '{name}'s gait slots are kept — nothing to write")
        } else {
            format!("nothing measurable on '{name}' — nothing to write")
        });
    }

    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "the system clock is before 1970")?
        .as_secs();
    let stamp = emerge_core::rigs::Provenance {
        glb_fnv1a: emerge_core::rigs::fingerprint_string(hash),
        clips: clip_infos.len(),
        clip_names: clip_infos
            .iter()
            .map(|c| c.name.clone().unwrap_or_default())
            .collect(),
        tool: emerge_core::rigs::BENCH_TOOL_VERSION,
        date: emerge_core::rig_check::civil_date_utc(secs),
    };
    trial_rig.provenance = Some(stamp.clone());
    doc.set_rig_field(
        "provenance",
        &emerge_core::rigs_edit::provenance_value(&stamp),
    )?;

    let new_text = doc.render();
    // The parse-back equality guard. `fmt_f32` round-trips exactly, so equality here is exact.
    let parsed = Rigs::parse(&new_text)?;
    if parsed != trial {
        return Err("the edit did not land where intended — refusing to write".to_owned());
    }
    commit_text(root, bench, new_text)?;
    bench.record(text);
    Ok(format!(
        "wrote {wrote} value(s) + provenance for '{name}'{}",
        if kept > 0 { format!(" ({kept} kept)") } else { String::new() }
    ))
}

/// Enter, in the Anim context.
fn adopt_measured(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<crate::project::Project>,
    mut bench: ResMut<BenchState>,
    mut reports: ResMut<crate::anim_watch::BenchReports>,
    mut generation: ResMut<crate::anim_watch::BenchGeneration>,
) {
    if !crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::AdoptMeasured) {
        return;
    }
    // A failed write REPLACES the message — an author told "adopted" by a program that could not
    // write the file has been told something untrue (the `tiles::persist` rule).
    bench.status = match adopt(&project.root, &mut bench) {
        Ok(said) => {
            crate::anim_watch::invalidate(&mut reports, &mut generation);
            said
        }
        Err(e) => format!("NOT WRITTEN: {e}"),
    };
}

/// Cmd+Z / Shift+Cmd+Z, in the Anim context. One body for both directions, restored **through**
/// [`commit_text`] so an undo re-runs the same validation and atomic save as the write it takes
/// back — and a refused restore pushes the entry back rather than eating it.
fn bench_history_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<crate::project::Project>,
    mut bench: ResMut<BenchState>,
    mut reports: ResMut<crate::anim_watch::BenchReports>,
    mut generation: ResMut<crate::anim_watch::BenchGeneration>,
) {
    let undo = crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::UndoBench);
    let redo = crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::RedoBench);
    if !undo && !redo {
        return;
    }
    let popped = if undo { bench.undo.pop() } else { bench.redo.pop() };
    let Some(target) = popped else {
        bench.status = format!("nothing to {} on this tab", if undo { "undo" } else { "redo" });
        return;
    };
    let now = bench.text.clone();
    match commit_text(&project.root, &mut bench, target.clone()) {
        Ok(()) => {
            crate::anim_watch::invalidate(&mut reports, &mut generation);
            if let Some(now) = now {
                if undo {
                    bench.redo.push(now);
                } else {
                    bench.undo.push(now);
                }
            }
            bench.status = if undo {
                "undid the last bench write".to_owned()
            } else {
                "put the bench write back".to_owned()
            };
        }
        Err(e) => {
            if undo {
                bench.undo.push(target);
            } else {
                bench.redo.push(target);
            }
            bench.status = format!("NOT WRITTEN: {e}");
        }
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
    bench.view = View::One;
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
    bench.view = View::One;
}

/// A summary row click: land on the rig it names.
fn on_jump_click(
    activate: On<Activate>,
    rows: Query<&JumpRow>,
    mut bench: ResMut<BenchState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    bench.selected = row.0;
    bench.view = View::One;
}

/// C, in the Anim context: measure every rig through the one queue and show the summary. The audit
/// nobody performs when it costs sixteen clicks costs one key.
fn check_all_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mut bench: ResMut<BenchState>,
    mut queue: ResMut<crate::anim_watch::MeasureQueue>,
) {
    if !crate::keys::just_pressed(&keyboard, live.0, crate::keys::Action::CheckAllRigs) {
        return;
    }
    let names: Vec<String> = bench.names().iter().map(|s| (*s).to_owned()).collect();
    for name in &names {
        queue.push_back_unique(name);
    }
    bench.view = View::All;
    bench.status = format!("measuring {} rig(s)...", names.len());
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
    reports: Option<Res<crate::anim_watch::BenchReports>>,
    plots: Option<Res<crate::anim_plots::BenchPlots>>,
    panes: Query<Entity, With<SlotPane>>,
) {
    let names = bench.names();
    let rig = names
        .get(bench.selected)
        .and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)));
    // Reports are keyed by rig NAME, so this lookup can never pair a slot table with another
    // rig's measurements — the race the old per-selection index cache had to guard against.
    // `None` simply means the queue has not reached this rig yet.
    let report = names
        .get(bench.selected)
        .and_then(|n| reports.as_ref().and_then(|r| r.by_rig.get(*n)));
    for pane in &panes {
        commands.entity(pane).despawn_related::<Children>();
        commands.entity(pane).with_children(|p| {
            // **The project-wide summary** — every rig's verdict at a glance, worst first in the
            // reader's eye because only the offenders get a rail. One keystroke reproduces what CI
            // sees, which is what makes a red build cheap instead of mysterious.
            if bench.view == View::All {
                let names = bench.names();
                let (mut ok, mut note, mut bad, mut pending) = (0usize, 0usize, 0usize, 0usize);
                for n in &names {
                    match reports.as_ref().and_then(|r| r.by_rig.get(*n)).map(|r| r.worst) {
                        None => pending += 1,
                        Some(Level::Ok) => ok += 1,
                        Some(Level::Note) => note += 1,
                        Some(Level::Bad) => bad += 1,
                    }
                }
                p.spawn((
                    Text::new(format!(
                        "{bad} bad, {note} with notes, {ok} ok{}",
                        if pending > 0 {
                            format!(", {pending} measuring...")
                        } else {
                            String::new()
                        }
                    )),
                    TextColor(if bad > 0 { DANGER } else { TEXT }),
                    TextFont::from_font_size(11.0),
                ));
                for (ix, n) in names.iter().enumerate() {
                    let Some(report) = reports.as_ref().and_then(|r| r.by_rig.get(*n)) else {
                        continue;
                    };
                    if report.worst == Level::Ok {
                        continue;
                    }
                    // The severity-rail shape (`tiles.rs`): a tinted left border, the severity as
                    // a WORD as well as a hue, and the first finding as the remedy line.
                    let (word, tint) = match report.worst {
                        Level::Bad => ("blocking", DANGER),
                        _ => ("worth checking", LABEL),
                    };
                    let first = report
                        .findings
                        .iter()
                        .find(|f| f.level == report.worst)
                        .map(|f| f.text.clone())
                        .unwrap_or_default();
                    p.spawn((
                        UiButton,
                        Hovered::default(),
                        JumpRow(ix),
                        Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                            border: UiRect::left(Val::Px(3.0)),
                            margin: UiRect::top(Val::Px(4.0)),
                            ..default()
                        },
                        BorderColor::all(tint),
                        BackgroundColor(ROW_BG),
                    ))
                    .with_children(|row| {
                        row.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|head| {
                            head.spawn((
                                Text::new((*n).to_owned()),
                                TextColor(TEXT),
                                TextFont::from_font_size(11.0),
                            ));
                            head.spawn((
                                Text::new(word),
                                TextColor(tint),
                                TextFont::from_font_size(9.0),
                            ));
                        });
                        row.spawn((
                            Text::new(first),
                            TextColor(LABEL),
                            TextFont::from_font_size(9.0),
                        ));
                    });
                }
                return;
            }

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
            // **The provenance line** — is the recorded measurement about the file on disk? A
            // standing fact about the rig, so it sits with the mesh path rather than among the
            // findings, and STALE is the one word here allowed to shout.
            match report {
                None => {
                    p.spawn((
                        Text::new("measuring..."),
                        TextColor(DIM),
                        TextFont::from_font_size(9.0),
                    ));
                }
                Some(report) => {
                    if let Some(st) = report.staleness {
                        let date = report.date.as_deref().unwrap_or("an unknown date");
                        let (line, colour) = match st {
                            Staleness::NeverMeasured => ("never measured".to_owned(), DIM),
                            Staleness::Current => (format!("measured {date}, current"), DIM),
                            Staleness::Stale => {
                                (format!("STALE: asset changed since {date}"), DANGER)
                            }
                        };
                        p.spawn((
                            Text::new(line),
                            TextColor(colour),
                            TextFont::from_font_size(9.0),
                        ));
                        for d in &report.diff {
                            p.spawn((
                                Text::new(d.clone()),
                                TextColor(DANGER),
                                TextFont::from_font_size(9.0),
                                Node {
                                    margin: UiRect::left(Val::Px(12.0)),
                                    ..default()
                                },
                            ));
                        }
                    }
                }
            }
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
                        match slot.tolerance {
                            // An explicit tolerance is a decision; it reads beside the number it
                            // governs.
                            Some(t) => format!(
                                "{duration:.3}s  ph {phase_offset:+.3}  {cycle_distance:.3}m +-{:.0}%",
                                t * 100.0
                            ),
                            None => format!(
                                "{duration:.3}s  ph {phase_offset:+.3}  {cycle_distance:.3}m"
                            ),
                        },
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
                // **One sub-line per slot, not three.** The note, the asset's own clip name
                // (which is what makes the LEFTWARD mismatch self-evidencing rather than
                // folklore), and a kept-reason each used to be their own line — on a ten-slot rig
                // that is twenty lines of annotation around ten of data. Joined with an ASCII
                // separator; the line wraps if it must.
                let asset_name = report
                    .and_then(|r| r.clip_names.get(slot.clip))
                    .and_then(Option::as_deref);
                let mut sub: Vec<String> = Vec::new();
                if let Some(note) = &slot.note {
                    sub.push(note.clone());
                }
                if let Some(name) = asset_name {
                    sub.push(format!("asset: {name}"));
                }
                if let Some(reason) = &slot.keep {
                    sub.push(format!("kept: {reason}"));
                }
                if !sub.is_empty() {
                    p.spawn((
                        Text::new(sub.join(" | ")),
                        TextColor(DIM),
                        TextFont::from_font_size(9.0),
                        Node {
                            margin: UiRect::left(Val::Px(84.0)),
                            ..default()
                        },
                    ));
                }
            }

            // **Measured against the asset, under the table it is measuring.** A finding with no
            // fix is a finding that gets read once, so each says what to do — and a finding that
            // says "fine" earns one line for ALL of them, not one each: the alert-fatigue rule the
            // tolerance policy already cites. Every Note and Bad still prints in full.
            let findings = report.map(|r| r.findings.as_slice()).unwrap_or_default();
            if !findings.is_empty() {
                p.spawn((
                    Text::new("MEASURED".to_owned()),
                    TextColor(LABEL),
                    TextFont::from_font_size(10.0),
                    Node {
                        margin: UiRect::top(Val::Px(8.0)),
                        ..default()
                    },
                ));
                let ok = findings.iter().filter(|f| f.level == Level::Ok).count();
                if ok > 0 {
                    p.spawn((
                        Text::new(format!("{ok} measurement(s) agree with the manifest")),
                        TextColor(DIM),
                        TextFont::from_font_size(9.0),
                    ));
                }
                for f in findings.iter().filter(|f| f.level != Level::Ok) {
                    p.spawn((
                        Text::new(f.text.clone()),
                        TextColor(match f.level {
                            Level::Note => LABEL,
                            _ => DANGER,
                        }),
                        TextFont::from_font_size(9.0),
                    ));
                }
            }

            // **The plots**, under the findings they explain — a scalar verdict says a problem
            // exists; the curves say where in the cycle it lives. The images are the stable
            // handles `render_plots` repaints; captions and legends are Text nodes, never pixels,
            // per the ASCII-only rule.
            let has_curves = report.is_some_and(|r| !r.curves.is_empty());
            if let (true, Some(plots)) = (has_curves, plots.as_ref()) {
                crate::chrome::section(p, "PLOTS");
                // The legend: which color is which gait slot, labelled by the asset's clip name
                // when it has one.
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|legend| {
                    let mut rank = 0usize;
                    for (i, slot) in rig.slots.iter().enumerate() {
                        if !matches!(slot.playback, Playback::Gait { .. }) {
                            continue;
                        }
                        let label = report
                            .and_then(|r| r.clip_names.get(slot.clip))
                            .and_then(Option::as_deref)
                            .map(|n| format!("{i} {n}"))
                            .unwrap_or_else(|| format!("{i} clip {}", slot.clip));
                        legend.spawn((
                            Text::new(label),
                            TextColor(crate::anim_plots::slot_ui_color(rank)),
                            TextFont::from_font_size(9.0),
                        ));
                        rank += 1;
                    }
                });
                let charts = [
                    ("foot height / phase (contact ticks below)", &plots.height),
                    ("foot speed / phase (m/s; stance should sit flat)", &plots.speed),
                    ("root drift / phase (m; red line = in-place limit)", &plots.drift),
                ];
                for (caption, handle) in charts {
                    p.spawn((
                        Text::new(caption),
                        TextColor(LABEL),
                        TextFont::from_font_size(9.0),
                        Node {
                            margin: UiRect::top(Val::Px(4.0)),
                            ..default()
                        },
                    ));
                    p.spawn((
                        ImageNode::new(handle.clone()),
                        Node {
                            width: Val::Px(crate::anim_plots::SHOW_W),
                            height: Val::Px(crate::anim_plots::SHOW_PLOT_H),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                }
                p.spawn((
                    Text::new("top-down trace (fwd = up; arrow = declared cycle along measured travel)"),
                    TextColor(LABEL),
                    TextFont::from_font_size(9.0),
                    Node {
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },
                ));
                p.spawn((
                    ImageNode::new(plots.trace.clone()),
                    Node {
                        width: Val::Px(crate::anim_plots::SHOW_W),
                        height: Val::Px(crate::anim_plots::SHOW_TRACE_H),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
            }

            // The staged figure's controls — weight chips and the scrub line.
            crate::anim_stage::spawn_chips(p, rig);
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

/// **The writer, proven against a disposable copy of the real project.** The model is
/// `tiles::write_library_tests`: a temp dir, the real files copied in, and assertions on the bytes.
#[cfg(test)]
mod write_back_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// A disposable project root holding copies of the real manifest and the real valkyrie GLB.
    fn temp_project() -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anim_bench_write_back_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let ws = workspace_root();
        std::fs::create_dir_all(dir.join("assets/emerge")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(dir.join("assets/characters")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::copy(
            ws.join("assets/emerge/rigs.ron"),
            dir.join("assets/emerge/rigs.ron"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        std::fs::copy(
            ws.join("assets/characters/valkyrie.glb"),
            dir.join("assets/characters/valkyrie.glb"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        dir
    }

    fn bench_for(dir: &std::path::Path) -> BenchState {
        let text = std::fs::read_to_string(dir.join("assets/emerge/rigs.ron"))
            .unwrap_or_else(|e| panic!("{e}"));
        let rigs = Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        let selected = rigs
            .rigs
            .keys()
            .position(|k| k == "valkyrie")
            .unwrap_or_else(|| panic!("no valkyrie"));
        let mut bench = BenchState::default();
        bench.rigs = Some(rigs);
        bench.text = Some(text);
        bench.selected = selected;
        bench.loaded = true;
        bench
    }

    #[test]
    fn adopting_writes_values_and_provenance_and_keeps_every_comment() {
        let dir = temp_project();
        let manifest = dir.join("assets/emerge/rigs.ron");
        let before = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        let mut bench = bench_for(&dir);

        let said = adopt(&dir, &mut bench).unwrap_or_else(|e| panic!("{e}"));
        assert!(said.contains("provenance"), "{said}");

        let after = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(before, after, "an adopt that changes nothing wrote nothing");
        // Every comment line survives verbatim — the whole reason RigDoc exists.
        let comments = |t: &str| -> Vec<String> {
            t.lines()
                .filter(|l| l.trim_start().starts_with("//"))
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(comments(&before), comments(&after));
        // The slot notes — the LEFTWARD one above all — survive too.
        assert!(after.contains("carries the body LEFTWARD"), "the note died");
        // The stamp landed, parses, and matches the asset's actual fingerprint.
        let parsed = Rigs::parse(&after).unwrap_or_else(|e| panic!("{e}"));
        let stamp = parsed
            .get("valkyrie")
            .and_then(|r| r.provenance.as_ref())
            .unwrap_or_else(|| panic!("no provenance written"));
        let (_, live) = emerge_core::glb::Glb::open_fingerprinted(
            &dir.join("assets/characters/valkyrie.glb"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(stamp.glb_fnv1a, emerge_core::rigs::fingerprint_string(live));
        assert_eq!(stamp.clips, 20);

        // Undo restores the file byte-identically, through the same commit door.
        let target = bench.undo.pop().unwrap_or_else(|| panic!("no undo entry recorded"));
        commit_text(&dir, &mut bench, target).unwrap_or_else(|e| panic!("{e}"));
        let restored = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, restored);
    }

    #[test]
    fn a_text_that_does_not_validate_is_refused_and_the_file_untouched() {
        let dir = temp_project();
        let manifest = dir.join("assets/emerge/rigs.ron");
        let before = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        let mut bench = bench_for(&dir);
        let refused = commit_text(&dir, &mut bench, "(version: 99, rigs: {})".to_owned());
        assert!(refused.is_err());
        let still = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, still, "a refused write must not touch the file");
        assert_eq!(bench.text.as_deref(), Some(before.as_str()), "nor the memory");
    }

    #[test]
    fn a_second_adopt_is_stable_where_the_asset_is() {
        // Adopt twice with nothing changing between: the second write must change only the
        // provenance date at most — measured values are deterministic, so the numbers hold still.
        let dir = temp_project();
        let manifest = dir.join("assets/emerge/rigs.ron");
        let mut bench = bench_for(&dir);
        adopt(&dir, &mut bench).unwrap_or_else(|e| panic!("{e}"));
        let once = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        adopt(&dir, &mut bench).unwrap_or_else(|e| panic!("{e}"));
        let twice = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(once, twice, "a repeated adopt of an unchanged asset must be a fixpoint");
    }
}

