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

use crate::chrome::{ACCENT, DANGER, DIM, LABEL, PAD, ROW_BG, ROW_SELECTED, TEXT};
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
    /// See [`crate::chrome::Status`]. This tab used to colour its one line by whether `rigs.ron` had
    /// loaded — a fact about a file, not about the sentence being shown — so `NOT WRITTEN:` rendered
    /// in the same grey as `adopted` on every session where the manifest had parsed.
    pub status: crate::chrome::Status,
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

/// **Transient per-slot adopt exclusion** — "adopt everything except slot 3, this once". The
/// durable opt-out is `keep:` in rigs.ron (a recorded decision with a why); this is the trial
/// form, living only until the next manifest write. Its own resource, never a `BenchState` field:
/// the adopt path holds `BenchState` mutably. Keyed by rig NAME so a selection change cannot leak
/// one rig's excludes onto another.
#[derive(Resource, Default)]
pub struct AdoptExclude {
    pub rig: Option<String>,
    pub slots: std::collections::BTreeSet<usize>,
}

impl AdoptExclude {
    /// The exclusion set for `name` — empty unless the key matches.
    pub fn for_rig(&self, name: &str) -> std::collections::BTreeSet<usize> {
        if self.rig.as_deref() == Some(name) {
            self.slots.clone()
        } else {
            std::collections::BTreeSet::new()
        }
    }

    fn clear(&mut self) {
        self.rig = None;
        self.slots.clear();
    }
}

/// The per-gait-slot `[skip]` chip, carrying its slot index.
#[derive(Component, Clone, Copy)]
pub(crate) struct SkipChip(pub usize);

/// A skip-chip click toggles the slot in the transient exclude set. NOT mod-click on the row —
/// mod-click already means "mix in/out" on the stage chips, and one gesture with two meanings in
/// one panel is how muscle memory betrays people.
pub(crate) fn on_skip_click(
    activate: On<bevy::ui_widgets::Activate>,
    chips: Query<&SkipChip>,
    bench: Res<BenchState>,
    mut exclude: ResMut<AdoptExclude>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    let Some(name) = bench.names().get(bench.selected).map(|s| (*s).to_owned()) else {
        return;
    };
    if exclude.rig.as_deref() != Some(name.as_str()) {
        exclude.rig = Some(name);
        exclude.slots.clear();
    }
    if !exclude.slots.remove(&chip.0) {
        exclude.slots.insert(chip.0);
    }
}

/// The node the selected rig's slot table is rebuilt into.
#[derive(Component)]
struct SlotPane;

/// The transient line.
#[derive(Component)]
struct BenchLine;

pub struct AnimTabPlugin;

/// **The rig list follows its selection**, the same way the palette and the mesh lists do.
///
/// It was the one scrollable list with no follower at all: the arrows moved `BenchState::selected`
/// and the highlight walked off the bottom while the list stood still. Added when the other two were
/// re-keyed — *"can we fix that and get it pinned across the board?"*, 2026-08-16 — because "across
/// the board" is only true if the list that never had one gets one too.
///
/// Keyed on the selection through `chrome::Follow`, not on `BenchState::is_changed`: this resource
/// carries a status line and a measurement queue, both written most frames, which is precisely the
/// churn that made the other two followers dead code.
fn keep_rig_selection_on_screen(
    state: Res<BenchState>,
    rows: Query<(&RigRow, &ComputedNode, &UiGlobalTransform)>,
    mut lists: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<RigList>, Without<RigRow>),
    >,
    mut follow: Local<crate::chrome::Follow<usize>>,
) {
    if !follow.should_scroll(Some(state.selected)) {
        return;
    }
    // A UI node's transform is its CENTRE, so the edges are the half-size either side.
    let Some((row_mid, row_half)) = rows
        .iter()
        .find(|(r, _, _)| r.0 == state.selected)
        .map(|(_, n, t)| (t.translation.y, n.size.y * 0.5))
    else {
        return;
    };
    for (list, list_tf, mut scroll) in &mut lists {
        // Physical in, logical out — `ComputedNode` and `UiGlobalTransform` are physical pixels,
        // `ScrollPosition` is logical.
        if let Some(want) = crate::chrome::scroll_to_reveal(
            (row_mid, row_half),
            (list_tf.translation.y, list.size.y * 0.5),
            scroll.0.y,
            list.inverse_scale_factor,
        ) {
            scroll.0.y = want;
        }
    }
}

impl Plugin for AnimTabPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            keep_rig_selection_on_screen
                .run_if(in_state(crate::screen::Screen::Editor)),
        )
        .init_resource::<BenchState>()
            .init_resource::<crate::anim_watch::RigWatch>()
            .init_resource::<crate::anim_watch::MeasureQueue>()
            .init_resource::<crate::anim_watch::BenchReports>()
            .init_resource::<crate::anim_watch::BenchGeneration>()
            .init_resource::<crate::anim_stage::BenchScrub>()
            .init_resource::<crate::anim_stage::BenchAb>()
            .init_resource::<AdoptExclude>()
            .init_resource::<crate::anim_cache::BenchCache>()
            .init_resource::<crate::anim_stage::BenchCamera>()
            // The editor registers the game's blend pass once, here — the staged figure is driven
            // by the REAL machinery, which is the whole reason emerge-anim is a crate.
            .add_plugins(emerge_anim::PoseBlendPlugin)
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                (
                    spawn_panels,
                    crate::anim_plots::create_plot_images,
                    crate::anim_stage::create_ghost_material,
                    // The persisted cache warms the reports before the first frame, so the STALE
                    // badge is truthful at startup rather than after the tab's first audit.
                    crate::anim_cache::load_bench_cache,
                ),
            )
            .add_systems(
                Update,
                ((
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
                    crate::anim_cache::save_bench_cache
                        .run_if(resource_changed::<crate::anim_watch::BenchGeneration>),
                    rebuild_list.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::filter::Filters>),
                    ),
                    rebuild_slots.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::anim_watch::BenchGeneration>)
                            .or_else(resource_changed::<AdoptExclude>),
                    ),
                    crate::anim_plots::render_plots.run_if(
                        resource_changed::<BenchState>
                            .or_else(resource_changed::<crate::anim_watch::BenchGeneration>)
                            .or_else(resource_changed::<crate::anim_stage::BenchAb>),
                    ),
                    refresh_line,
                    // The staged figure: spawned per selection, driven exactly like a game
                    // creature — after attach, before the blend pass writes the player.
                    // Nested: a flat `add_systems` tuple caps at 20 entries, the same
                    // `all_tuples!` ceiling as `add_plugins`' 15.
                    (
                        crate::anim_stage::drive_bench_stage,
                        crate::anim_stage::drive_bench_ghost,
                        crate::anim_stage::toggle_ghost_key.in_set(crate::keys::Phase::Act),
                        crate::anim_stage::cycle_cam_preset.in_set(crate::keys::Phase::Act),
                        crate::anim_stage::drive_bench_scrub
                            .in_set(crate::keys::Phase::Act)
                            .after(emerge_anim::PoseAttachSet)
                            .before(emerge_anim::PoseBlendSet),
                        crate::anim_stage::refresh_scrub_ui,
                        crate::anim_plots::drive_plot_hover,
                    ),
                ),)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            )
            .add_observer(on_rig_click)
            .add_observer(on_jump_click)
            .add_observer(crate::anim_stage::on_chip_click)
            .add_observer(crate::anim_stage::on_ghost_chip_click)
            .add_observer(on_skip_click);
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
        crate::chrome::back_button(p);
        crate::chrome::problem_banner(p, &[crate::tiles::Mode::Anim]);
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
        // number under the declared one. The declared-beside-measured table is the block an author
        // asking about a gait is looking at, and the one worth handing to somebody else verbatim —
        // hence the `CopyPane`.
        crate::chrome::scroll_list(
            p,
            (SlotPane, crate::notice::CopyPane(&[crate::tiles::Mode::Anim])),
        )
        .entry::<Node>()
        .and_modify(|mut n| n.margin.top = Val::Px(8.0));
        // **Last, and it must be.** `margin-top: auto` is what pins it to the bottom of
        // the panel, and an auto margin in a column absorbs the free space above it — so
        // placed any earlier it pushes every sibling after it down with it.
        crate::chrome::problem_log(p, &[crate::tiles::Mode::Anim]);
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
        crate::chrome::list_heading(p, "RIGS");
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
    reports: Res<crate::anim_watch::BenchReports>,
) {
    if *mode != Mode::Anim || bench.loaded {
        return;
    }
    bench.loaded = true;
    let path = project.root.join("assets/emerge/rigs.ron");
    match std::fs::read_to_string(&path) {
        Ok(text) => match Rigs::parse(&text) {
            Ok(rigs) => {
                bench.status.note(format!("{} rig(s) in {}", rigs.rigs.len(), path.display()));
                // **Check-all runs on open — for whatever is not already measured.** The audit
                // should not wait to be asked for: unmeasured rigs enter the queue now (one
                // measures per frame), so the stale badge and the C summary are warm from the
                // first entry. Already-measured rigs are the watcher's to keep fresh — measuring
                // them again would say nothing new. The selected rig still jumps the line via
                // `queue_selected`. Lazy-load stands — a session that never opens the tab pays
                // nothing.
                for name in rigs.rigs.keys() {
                    if !reports.by_rig.contains_key(name) {
                        queue.push_back_unique(name);
                    }
                }
                bench.rigs = Some(rigs);
                bench.text = Some(text);
            }
            // The manifest is on disk and unreadable — the tab has nothing to show and the author
            // has a file to fix. Exactly what the block is for.
            Err(e) => bench.status.problem(format!("{}: {e}", path.display())),
        },
        Err(e) => bench.status.problem(format!("cannot read {}: {e}", path.display())),
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
fn adopt(
    root: &std::path::Path,
    bench: &mut BenchState,
    exclude: &AdoptExclude,
) -> Result<String, String> {
    let text = bench.text.clone().ok_or("no manifest loaded")?;
    let name = bench
        .names()
        .get(bench.selected)
        .map(|s| (*s).to_owned())
        .ok_or("no rig selected")?;
    let excluded = exclude.for_rig(&name);
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
    let mut skipped = 0usize;
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
        // The transient exclude: skipped exactly like a kept slot, but only for this write.
        if excluded.contains(&i) {
            skipped += 1;
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
        return Err(if kept + skipped > 0 {
            format!(
                "all of '{name}'s gait slots are kept or skipped ({kept} kept, {skipped} \
                 skipped) — nothing to write"
            )
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
    let mut held = Vec::new();
    if kept > 0 {
        held.push(format!("{kept} kept"));
    }
    if skipped > 0 {
        held.push(format!("{skipped} skipped"));
    }
    Ok(format!(
        "wrote {wrote} value(s) + provenance for '{name}'{}",
        if held.is_empty() { String::new() } else { format!(" ({})", held.join(", ")) }
    ))
}

/// Enter, in the Anim context.
fn adopt_measured(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<crate::project::Project>,
    mut bench: ResMut<BenchState>,
    mut exclude: ResMut<AdoptExclude>,
    mut reports: ResMut<crate::anim_watch::BenchReports>,
    mut generation: ResMut<crate::anim_watch::BenchGeneration>,
) {
    if !crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::AdoptMeasured) {
        return;
    }
    // A failed write REPLACES the message — an author told "adopted" by a program that could not
    // write the file has been told something untrue (the `tiles::persist` rule). It now goes to a
    // different slot as well, so it survives whatever the author does next.
    match adopt(&project.root, &mut bench, &exclude) {
        Ok(said) => {
            crate::anim_watch::invalidate(&mut reports, &mut generation);
            // The excludes were about the text that just changed; a stale set silently shaping
            // the NEXT adopt would be the two-paths bug in miniature.
            exclude.clear();
            bench.status.note(said);
        }
        Err(e) => bench.status.problem(format!("NOT WRITTEN: {e}")),
    }
}

/// Cmd+Z / Shift+Cmd+Z, in the Anim context. One body for both directions, restored **through**
/// [`commit_text`] so an undo re-runs the same validation and atomic save as the write it takes
/// back — and a refused restore pushes the entry back rather than eating it.
fn bench_history_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Res<crate::project::Project>,
    mut bench: ResMut<BenchState>,
    mut exclude: ResMut<AdoptExclude>,
    mut reports: ResMut<crate::anim_watch::BenchReports>,
    mut generation: ResMut<crate::anim_watch::BenchGeneration>,
) {
    let undo = crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::UndoBench);
    let redo = crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::RedoBench);
    if !undo && !redo {
        return;
    }
    let popped = if undo { bench.undo.pop() } else { bench.redo.pop() };
    let Some(target) = popped else {
        bench.status.note(format!("nothing to {} on this tab", if undo { "undo" } else { "redo" }));
        return;
    };
    let now = bench.text.clone();
    match commit_text(&project.root, &mut bench, target.clone()) {
        Ok(()) => {
            crate::anim_watch::invalidate(&mut reports, &mut generation);
            // The manifest text changed under the excludes — same rule as adopt.
            exclude.clear();
            if let Some(now) = now {
                if undo {
                    bench.redo.push(now);
                } else {
                    bench.undo.push(now);
                }
            }
            bench.status.note(if undo {
                "undid the last bench write"
            } else {
                "put the bench write back"
            });
        }
        Err(e) => {
            if undo {
                bench.undo.push(target);
            } else {
                bench.redo.push(target);
            }
            bench.status.problem(format!("NOT WRITTEN: {e}"));
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
    time: Res<Time>,
    mut repeat: ResMut<crate::keys::Repeat>,
    mut bench: ResMut<BenchState>,
) {
    let n = bench.names().len();
    if n == 0 {
        return;
    }
    // Held arrows walk the list at the shared [`crate::keys::REPEAT_SECS`] cadence — the same
    // helper the aim keys ride, so every held key in the editor has one rhythm. Shift is the long
    // stride, five at a time, the same bargain the tiles lists strike.
    let dt = time.delta_secs();
    // Clamped below the list's length: this list wraps, and on a short list a stride of exactly
    // `n` is the identity wearing a jump's clothes.
    let stride = if crate::keys::SHIFT_KEYS.iter().any(|k| keyboard.pressed(*k)) {
        5.min(n.saturating_sub(1)).max(1)
    } else {
        1
    };
    let step = if crate::keys::repeating(
        &keyboard, *live, crate::keys::Action::NextRig, &mut repeat, dt,
    ) {
        stride
    } else if crate::keys::repeating(
        &keyboard, *live, crate::keys::Action::PrevRig, &mut repeat, dt,
    ) {
        n - stride
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
    reports: Res<crate::anim_watch::BenchReports>,
) {
    if !crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::CheckAllRigs) {
        return;
    }
    // Only what is unmeasured: reports persist for the session and the watcher re-measures on a
    // real change, so a second C is an instant summary, not a rescan that says nothing new.
    let missing: Vec<String> = bench
        .names()
        .iter()
        .filter(|n| !reports.by_rig.contains_key(**n))
        .map(|n| (*n).to_owned())
        .collect();
    for name in &missing {
        queue.push_back_unique(name);
    }
    bench.view = View::All;
    bench.status.note(if missing.is_empty() {
        "all rigs measured".to_owned()
    } else {
        format!("measuring {} rig(s)...", missing.len())
    });
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
                crate::chrome::list_row(p, ix == bench.selected, RigRow(ix)).with_children(|row| {
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
/// The measured half of a gait slot's row — `measured: 1.402s  ph +0.020  1.386m`, mirroring the
/// declared detail column so the pair reads as declared-above-measured. Distances arrive in FILE
/// units on `SlotMeasure`; the rig's scale converts them to the world units the manifest declares.
/// A field that could not be measured is simply absent rather than printed as a guess. The one
/// extension point for per-slot measured data — the skate summary rides here too.
fn slot_measured_line(
    m: &emerge_core::rig_check::SlotMeasure,
    skate: Option<&emerge_core::rig_check::SkateReport>,
    scale: f32,
) -> String {
    let mut line = format!("measured: {:.3}s", m.duration);
    if let Some(ph) = m.phase_offset {
        line.push_str(&format!("  ph {ph:+.3}"));
    }
    if let Some(cd) = m.cycle_distance {
        line.push_str(&format!("  {:.3}m", cd * scale));
    }
    if let Some(sk) = skate {
        if sk.max_skate > emerge_core::rig_check::SKATE_FLOOR {
            line.push_str(&format!(
                "  skate up to {:.2} u/s over {:.0}% of drive",
                sk.max_skate,
                sk.skating_ratio * 100.0
            ));
        }
    }
    line
}

fn rebuild_slots(
    mut commands: Commands,
    bench: Res<BenchState>,
    reports: Option<Res<crate::anim_watch::BenchReports>>,
    plots: Option<Res<crate::anim_plots::BenchPlots>>,
    exclude: Option<Res<AdoptExclude>>,
    panes: Query<Entity, With<SlotPane>>,
) {
    let names = bench.names();
    let rig = names
        .get(bench.selected)
        .and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)));
    let excluded = names
        .get(bench.selected)
        .zip(exclude.as_ref())
        .map(|(n, e)| e.for_rig(n))
        .unwrap_or_default();
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
                    // The one severity rail, through the one map — this used to be a 3 px dialect
                    // whose every non-blocking verdict rendered LABEL, so "worth checking" here and
                    // "worth checking" on the Tiles findings wore different inks (unified
                    // 2026-08-17). `rig_check`'s middle tier is worded "worth checking", so it IS
                    // the vocabulary's Warn.
                    let (tint, word) = crate::chrome::severity_style(match report.worst {
                        Level::Bad => emerge_core::import::Severity::Blocking,
                        _ => emerge_core::import::Severity::Warn,
                    });
                    let first = report
                        .findings
                        .iter()
                        .find(|f| f.level == report.worst)
                        .map(|f| f.text.clone())
                        .unwrap_or_default();
                    crate::chrome::severity_rail(
                        p,
                        tint,
                        (UiButton, Hovered::default(), JumpRow(ix), BackgroundColor(ROW_BG)),
                    )
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
            // **The staged figure's controls, directly under the rig they belong to.**
            //
            // These were the pane's last block — below the slot table, the plot legend, three
            // charts, the hover readout and the top-down trace. They are the one part of this pane
            // an author *drives* rather than reads: which clip is soloed, what is in the mix, where
            // the scrub sits. Everything they were buried under is a report about the choice they
            // make here, so the choice goes first and the evidence follows it.
            crate::anim_stage::spawn_chips(p, rig);
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
                                    // One panel inset of indent — `PAD`, not a bare 12, so the
                                    // provenance block steps in by the same unit the panel does.
                                    margin: UiRect::left(Val::Px(PAD)),
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
                    // The transient adopt-exclude chip — gaits only, since adopt writes nothing
                    // else. The durable form is `keep:` in the manifest; this is "not this once".
                    // `CHIP_PAD` like every other chip (its 4/1 was the last padding outlier;
                    // unified 2026-08-17), keeping its quieter 9 px word.
                    if matches!(slot.playback, Playback::Gait { .. }) {
                        crate::chrome::chip(
                            row,
                            SkipChip(i),
                            "skip",
                            9.0,
                            if excluded.contains(&i) { TEXT } else { DIM },
                            if excluded.contains(&i) { ROW_SELECTED } else { ROW_BG },
                            Color::NONE,
                        );
                    }
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
                // **Declared above, measured below** — the module's own promise, kept structurally
                // rather than buried in finding prose. Only gaits carry measured numbers.
                if matches!(slot.playback, Playback::Gait { .. }) {
                    if let Some(r) = report {
                        if let Some(m) = r.slots.iter().find(|m| m.slot == i) {
                            let skate = r.skates.iter().find(|s| s.slot == i);
                            let mut line = slot_measured_line(m, skate, rig.scale);
                            if excluded.contains(&i) {
                                line.push_str(" | excluded from adopt");
                            }
                            p.spawn((
                                Text::new(line),
                                TextColor(DIM),
                                TextFont::from_font_size(9.0),
                                Node {
                                    margin: UiRect::left(Val::Px(84.0)),
                                    ..default()
                                },
                            ));
                        }
                    }
                }
            }

            // **Measured against the asset, under the table it is measuring.** A finding with no
            // fix is a finding that gets read once, so each says what to do — and a finding that
            // says "fine" earns one line for ALL of them, not one each: the alert-fatigue rule the
            // tolerance policy already cites. Every Note and Bad still prints in full.
            let findings = report.map(|r| r.findings.as_slice()).unwrap_or_default();
            if !findings.is_empty() {
                // A real `section`, like "PLOTS" thirty lines down — two heading styles in one
                // pane was the 2026-08-17 audit's clearest type drift.
                crate::chrome::section(p, "MEASURED");
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
                    // The same rank walk `render_plots` colors by: gait slots when the rig has
                    // any, else every free slot — lockstep or the legend lies.
                    let mut rank = 0usize;
                    for (i, slot) in rig.slots.iter().enumerate() {
                        let plotted = match slot.playback {
                            Playback::Gait { .. } => true,
                            Playback::Free { .. } => !rig.has_gaits(),
                            _ => false,
                        };
                        if !plotted {
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
                    ("foot height / phase (contact ticks below; dim = measured, G)", &plots.height),
                    ("foot speed / phase (m/s; stance should sit flat; dim = measured, G)", &plots.speed),
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
                    // The plot in a relative wrapper, so the shared hover overlay can sit exactly
                    // on top of it; the cursor is read off the plot node itself.
                    p.spawn(Node {
                        width: Val::Px(crate::anim_plots::SHOW_W),
                        height: Val::Px(crate::anim_plots::SHOW_PLOT_H),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|wrap| {
                        wrap.spawn((
                            ImageNode::new(handle.clone()),
                            Node {
                                width: Val::Px(crate::anim_plots::SHOW_W),
                                height: Val::Px(crate::anim_plots::SHOW_PLOT_H),
                                ..default()
                            },
                            bevy::ui::RelativeCursorPosition::default(),
                            bevy::picking::hover::Hovered::default(),
                            crate::anim_plots::PhasePlotNode,
                        ));
                        wrap.spawn((
                            ImageNode::new(plots.hover.clone()),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(crate::anim_plots::SHOW_W),
                                height: Val::Px(crate::anim_plots::SHOW_PLOT_H),
                                ..default()
                            },
                            // The overlay must not eat the cursor from the plot beneath it.
                            bevy::picking::Pickable::IGNORE,
                        ));
                    });
                }
                // The hover readout — per-slot values at the cursor's phase, written by
                // `drive_plot_hover`.
                p.spawn((
                    Text::new(String::new()),
                    TextColor(DIM),
                    TextFont::from_font_size(9.0),
                    crate::anim_plots::PlotReadout,
                ));
                p.spawn((
                    Text::new("top-down trace (fwd = up; arrow = declared cycle along measured travel; dim arrow = measured, G)"),
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

        });
    }
}

/// The running commentary, and the block above it.
///
/// **The colour rule is gone rather than fixed.** It read `bench.rigs.is_none() && bench.loaded` —
/// a fact about whether a *file* had parsed, standing in for whether the *sentence* was bad news. So
/// `NOT WRITTEN:` from a failed adopt drew in the same grey as `adopted` whenever `rigs.ron` had
/// loaded, which is every session in which anyone gets far enough to adopt anything. The write site
/// says which it is now (`crate::chrome::Status`), and this line is only ever the receipt.
fn refresh_line(
    bench: Res<BenchState>,
    mut lines: Query<(&mut Text, &mut TextColor), With<BenchLine>>,
) {
    if !bench.is_changed() {
        return;
    }
    for (mut text, mut colour) in &mut lines {
        if text.0 != bench.status.note_text() {
            text.0 = bench.status.note_text().to_owned();
        }
        if colour.0 != DIM {
            colour.0 = DIM;
        }
    }
}

/// **The writer, proven against a disposable copy of the real project.** The model is
/// `tiles::write_library_tests`: a temp dir, the real files copied in, and assertions on the bytes.
#[cfg(test)]
mod measured_line_tests {
    use super::*;

    #[test]
    fn the_measured_line_scales_distances_and_omits_what_could_not_be_measured() {
        let full = emerge_core::rig_check::SlotMeasure {
            slot: 2,
            duration: 1.402,
            cycle_distance: Some(1.227),
            phase_offset: Some(0.02),
        };
        // 1.227 file units × 1.13 scale = 1.387 world units, the manifest's own frame.
        let line = slot_measured_line(&full, None, 1.13);
        assert_eq!(line, "measured: 1.402s  ph +0.020  1.387m");

        let bare = emerge_core::rig_check::SlotMeasure {
            slot: 0,
            duration: 0.75,
            cycle_distance: None,
            phase_offset: None,
        };
        assert_eq!(slot_measured_line(&bare, None, 1.13), "measured: 0.750s");

        // A skating slot says so on the same line; a skate-free one stays quiet.
        let skating = emerge_core::rig_check::skate_report(2, 1.402, 1.387, (0.9, 6.0));
        let line = slot_measured_line(&full, Some(&skating), 1.13);
        assert!(line.contains("skate up to"), "{line}");
        let quiet = emerge_core::rig_check::skate_report(2, 1.402, 1.387, (0.9, 1.2));
        let line = slot_measured_line(&full, Some(&quiet), 1.13);
        assert!(!line.contains("skate"), "{line}");
    }
}

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

        let said = adopt(&dir, &mut bench, &AdoptExclude::default()).unwrap_or_else(|e| panic!("{e}"));
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

    /// The transient exclude holds one slot's bytes still while its siblings adopt — and
    /// excluding everything refuses to write at all.
    #[test]
    fn an_excluded_slot_keeps_its_bytes_while_siblings_adopt() {
        let dir = temp_project();
        let manifest = dir.join("assets/emerge/rigs.ron");
        let before = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        let mut bench = bench_for(&dir);
        // Slot 2 is the walk — the phase reference, and the line we hold still.
        let walk_line = |t: &str| -> String {
            t.lines()
                .find(|l| l.contains("the phase reference"))
                .map(str::to_owned)
                .unwrap_or_else(|| panic!("no walk line"))
        };
        let mut exclude = AdoptExclude::default();
        exclude.rig = Some("valkyrie".to_owned());
        exclude.slots.insert(2);
        let said = adopt(&dir, &mut bench, &exclude).unwrap_or_else(|e| panic!("{e}"));
        assert!(said.contains("1 skipped"), "{said}");
        let after = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            walk_line(&before),
            walk_line(&after),
            "the excluded slot's numbers must be byte-identical"
        );
        assert_ne!(before, after, "the siblings still adopted");

        // A mismatched rig key means nothing is excluded — the set cannot leak across rigs.
        let mut bench = bench_for(&dir);
        let mut foreign = AdoptExclude::default();
        foreign.rig = Some("crab".to_owned());
        foreign.slots.insert(2);
        let said = adopt(&dir, &mut bench, &foreign).unwrap_or_else(|e| panic!("{e}"));
        assert!(!said.contains("skipped"), "{said}");

        // Excluding every gait slot refuses the write outright.
        let mut bench = bench_for(&dir);
        let mut all = AdoptExclude::default();
        all.rig = Some("valkyrie".to_owned());
        all.slots.extend([2usize, 3, 4, 5, 6, 7]);
        let refused = adopt(&dir, &mut bench, &all);
        assert!(refused.is_err(), "{refused:?}");
    }

    #[test]
    fn a_second_adopt_is_stable_where_the_asset_is() {
        // Adopt twice with nothing changing between: the second write must change only the
        // provenance date at most — measured values are deterministic, so the numbers hold still.
        let dir = temp_project();
        let manifest = dir.join("assets/emerge/rigs.ron");
        let mut bench = bench_for(&dir);
        adopt(&dir, &mut bench, &AdoptExclude::default()).unwrap_or_else(|e| panic!("{e}"));
        let once = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        adopt(&dir, &mut bench, &AdoptExclude::default()).unwrap_or_else(|e| panic!("{e}"));
        let twice = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(once, twice, "a repeated adopt of an unchanged asset must be a fixpoint");
    }
}

