//! **The bench notices a re-export instead of waiting to be pointed at it.**
//!
//! Three pieces, all frame-driven ECS in the crate's deliberate no-threads style:
//!
//! - [`RigWatch`] polls the loaded rigs' GLB mtimes on a coarse timer. A change counts only when
//!   the new mtime is seen EQUAL on two consecutive polls — an exporter mid-write never triggers a
//!   measure of half a file. The `devshot` sentinel poll is the precedent; `notify` and a thread
//!   are exactly what this crate does not do.
//! - [`MeasureQueue`] + [`step_measure_queue`] re-measure **one rig per frame**, the `thumbs::bake`
//!   shape — a re-export that touches sixteen GLBs costs sixteen frames, never one stalled one.
//!   Selection, the watcher, and check-all all feed the same queue: one measurement path.
//! - [`BenchReports`] holds the results keyed by rig NAME, and [`BenchGeneration`] bumps only when
//!   a stored report actually changed — the `ThumbGeneration` idiom, because `resource_changed` on
//!   a resource a system takes as `ResMut` fires every frame.
//!
//! The stale count is painted onto the tab strip's `ANIM` label in place, so it survives tab
//! switches: a fact about the project does not stop being true because a different tab is open.

use std::collections::{HashMap, VecDeque};
use std::time::SystemTime;

use bevy::prelude::*;

use emerge_core::rig_check::{Finding, Level, Staleness};

use crate::anim_tab::BenchState;
use crate::tiles::Mode;

/// Seconds between mtime polls. Coarse on purpose: the point is "within a breath of the export",
/// not "within a frame", and sixteen `stat` calls a second is already generous.
const POLL_SECS: f32 = 1.0;

/// The mtime watcher's memory, per mesh path.
#[derive(Resource)]
pub(crate) struct RigWatch {
    timer: Timer,
    seen: HashMap<String, MtimeState>,
}

impl Default for RigWatch {
    fn default() -> Self {
        RigWatch {
            timer: Timer::from_seconds(POLL_SECS, TimerMode::Repeating),
            seen: HashMap::new(),
        }
    }
}

/// What the watcher knows about one file.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct MtimeState {
    /// The mtime the current measurements describe (or the first one ever seen).
    known: Option<SystemTime>,
    /// A changed mtime seen once — promoted only if the next poll sees it again unchanged.
    candidate: Option<SystemTime>,
}

/// **The debounce, as a pure function.** Returns `true` exactly when a changed mtime has held
/// still across two consecutive polls — the moment a re-measure is warranted.
pub(crate) fn promote(state: &mut MtimeState, observed: SystemTime) -> bool {
    match state.known {
        // First sight of the file: remember it, nothing changed.
        None => {
            state.known = Some(observed);
            state.candidate = None;
            false
        }
        Some(known) if known == observed => {
            // Back to (or still at) the known mtime — including an export that was reverted
            // between polls. Drop any half-seen candidate.
            state.candidate = None;
            false
        }
        Some(_) => {
            if state.candidate == Some(observed) {
                // Seen changed, then seen again unchanged: the file has settled.
                state.known = Some(observed);
                state.candidate = None;
                true
            } else {
                // Changed and possibly still being written. Wait one more poll.
                state.candidate = Some(observed);
                false
            }
        }
    }
}

/// Rig names awaiting a measure. Fed by selection (front), the watcher, and check-all (back).
#[derive(Resource, Default)]
pub(crate) struct MeasureQueue(VecDeque<String>);

impl MeasureQueue {
    pub(crate) fn push_back_unique(&mut self, name: &str) {
        if !self.0.iter().any(|n| n == name) {
            self.0.push_back(name.to_owned());
        }
    }

    /// The selected rig jumps the line — it is the one on screen.
    pub(crate) fn push_front_unique(&mut self, name: &str) {
        self.0.retain(|n| n != name);
        self.0.push_front(name.to_owned());
    }
}

/// Everything one measuring pass said about one rig — what the pane renders.
/// Serde because `anim_cache` persists reports between sessions.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RigReport {
    /// The file fingerprint the findings describe. `None` when the GLB could not be read.
    pub fingerprint: Option<u64>,
    /// Whether the manifest's recorded measurement still describes the asset.
    pub staleness: Option<Staleness>,
    pub findings: Vec<Finding>,
    /// Asset clip names by clip index.
    pub clip_names: Vec<Option<String>>,
    /// The provenance stamp's date, when there is one.
    pub date: Option<String>,
    /// The clip-list diff, when stale — the diagnosis lines.
    pub diff: Vec<String>,
    /// Phase-gridded curves per gait slot `(slot index, curves)` — the plots' input, a by-product
    /// of the FK the checks already ran. Empty for a rig with no gaits (or no contact joint).
    pub curves: Vec<(usize, emerge_core::clips::GaitCurves)>,
    /// The measured numbers per gait slot, straight from `check_rig` — what the ghost stages, the
    /// plots overlay, and the measured sub-line prints. Used to be dropped on the floor here while
    /// `adopt` re-measured from scratch.
    pub slots: Vec<emerge_core::rig_check::SlotMeasure>,
    /// The skate arithmetic per gait slot — empty when the rig declares no `drive_speed`.
    pub skates: Vec<emerge_core::rig_check::SkateReport>,
    /// The most severe finding level, for summaries and the badge.
    pub worst: Level,
}

/// The measured state of the project, keyed by rig name. Name-keyed so a report can never describe
/// a different rig than the pane thinks it does — the index-raced flash the old per-selection cache
/// had to guard against cannot be expressed.
#[derive(Resource, Default)]
pub struct BenchReports {
    pub by_rig: HashMap<String, RigReport>,
}

impl BenchReports {
    /// How many rigs' recorded measurements no longer describe their assets.
    pub fn stale_count(&self) -> usize {
        self.by_rig
            .values()
            .filter(|r| r.staleness == Some(Staleness::Stale))
            .count()
    }
}

/// Bumped only when a stored report actually changed.
#[derive(Resource, Default)]
pub struct BenchGeneration(pub u32);

/// **One rig, measured.** The policy is `rig_check::check_rig` — the same call CI makes.
pub(crate) fn measure_rig(root: &std::path::Path, rig: &emerge_core::rigs::Rig) -> RigReport {
    let path = root.join("assets").join(&rig.mesh);
    let (glb, hash) = match emerge_core::glb::Glb::open_fingerprinted(&path) {
        Ok(pair) => pair,
        Err(e) => {
            let findings = vec![Finding::bad(format!("cannot read {}: {e}", rig.mesh))];
            let worst = emerge_core::rig_check::worst(&findings);
            return RigReport {
                fingerprint: None,
                staleness: None,
                findings,
                clip_names: Vec::new(),
                date: rig.provenance.as_ref().map(|p| p.date.clone()),
                diff: Vec::new(),
                curves: Vec::new(),
                slots: Vec::new(),
                skates: Vec::new(),
                worst,
            };
        }
    };
    let clips = emerge_core::clips::clips(&glb);
    let staleness = emerge_core::rig_check::staleness(rig, hash);
    let diff = match (staleness, &rig.provenance) {
        (Staleness::Stale, Some(p)) => {
            emerge_core::rig_check::clip_list_diff(&p.clip_names, &clips)
        }
        _ => Vec::new(),
    };
    let report =
        emerge_core::rig_check::check_rig(&glb, rig, staleness == Staleness::Current);
    let worst = emerge_core::rig_check::worst(&report.findings);
    // The plots' curves — same FK, same anchors, gathered while the file is open.
    let mut curves = Vec::new();
    if rig.has_gaits() {
        if let Some(foot) = emerge_core::clips::node_index(&glb, rig.contact_joint()) {
            let root = emerge_core::clips::node_index(&glb, rig.root_node());
            for (i, slot) in rig.slots.iter().enumerate() {
                if !matches!(slot.playback, emerge_core::rigs::Playback::Gait { .. }) {
                    continue;
                }
                if let Some(c) =
                    emerge_core::clips::gait_curves(&glb, slot.clip, foot, root, rig.contact_eps)
                {
                    curves.push((i, c));
                }
            }
        }
    } else {
        // A gait-less rig still deserves curves — height and speed of its most foot-like joint,
        // with no contact claim. One ordered selection rule: the conventional contact joint when
        // the asset has it, else the best measured candidate, else no curve.
        for (i, slot) in rig.slots.iter().enumerate() {
            if !matches!(slot.playback, emerge_core::rigs::Playback::Free { .. }) {
                continue;
            }
            let joint = emerge_core::clips::node_index(&glb, rig.contact_joint()).or_else(|| {
                emerge_core::clips::contact_candidates(&glb, slot.clip)
                    .first()
                    .and_then(|(name, _)| emerge_core::clips::node_index(&glb, name))
            });
            let Some(joint) = joint else { continue };
            if let Some(c) = emerge_core::clips::joint_curves(&glb, slot.clip, joint) {
                curves.push((i, c));
            }
        }
    }
    RigReport {
        fingerprint: Some(hash),
        staleness: Some(staleness),
        findings: report.findings,
        clip_names: clips.into_iter().map(|c| c.name).collect(),
        date: rig.provenance.as_ref().map(|p| p.date.clone()),
        diff,
        curves,
        slots: report.slots,
        skates: report.skates,
        worst,
    }
}

/// Forget everything measured — the manifest's text just changed under the reports (an adopt, an
/// undo), so every one of them describes a file that no longer exists. The selected rig re-enters
/// the queue on the next `queue_selected` pass, and the badge recounts to zero meanwhile.
pub(crate) fn invalidate(reports: &mut BenchReports, generation: &mut BenchGeneration) {
    if !reports.by_rig.is_empty() {
        reports.by_rig.clear();
        generation.0 = generation.0.wrapping_add(1);
    }
}

/// Poll the loaded rigs' GLB mtimes. Only rigs already loaded — the tab's lazy load stands, and a
/// session that never opens the bench never stats a file.
pub(crate) fn poll_mtimes(
    time: Res<Time>,
    project: Option<Res<crate::project::Project>>,
    bench: Option<Res<BenchState>>,
    mut watch: ResMut<RigWatch>,
    mut queue: ResMut<MeasureQueue>,
) {
    let (Some(project), Some(bench)) = (project, bench) else {
        return;
    };
    let Some(rigs) = &bench.rigs else {
        return;
    };
    if !watch.timer.tick(time.delta()).just_finished() {
        return;
    }
    // Meshes first, so two rigs sharing a GLB cost one stat — and both re-measure when it changes.
    let mut by_mesh: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, rig) in &rigs.rigs {
        by_mesh.entry(rig.mesh.as_str()).or_default().push(name);
    }
    for (mesh, names) in by_mesh {
        // An unreadable file is not the watcher's to report: the measure pass says "cannot read"
        // loudly, and it will run as soon as the file reappears with a settled mtime.
        let Ok(meta) = std::fs::metadata(project.root.join("assets").join(mesh)) else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let state = watch.seen.entry(mesh.to_owned()).or_default();
        if promote(state, mtime) {
            for name in names {
                queue.push_back_unique(name);
            }
        }
    }
}

/// Keep the selected rig measured. Cheap: a map lookup per frame until the report exists.
pub(crate) fn queue_selected(
    bench: Option<Res<BenchState>>,
    reports: Option<Res<BenchReports>>,
    mut queue: ResMut<MeasureQueue>,
) {
    let (Some(bench), Some(reports)) = (bench, reports) else {
        return;
    };
    let names = bench.names();
    let Some(name) = names.get(bench.selected) else {
        return;
    };
    if reports.by_rig.contains_key(*name) {
        return;
    }
    queue.push_front_unique(name);
}

/// **One rig per frame** — the `thumbs::bake` shape. The FK over every keyframe of every clip is
/// real work, and sixteen of it in one frame is a stalled editor.
pub(crate) fn step_measure_queue(
    project: Option<Res<crate::project::Project>>,
    bench: Option<Res<BenchState>>,
    mut queue: ResMut<MeasureQueue>,
    mut reports: ResMut<BenchReports>,
    mut generation: ResMut<BenchGeneration>,
) {
    let (Some(project), Some(bench)) = (project, bench) else {
        return;
    };
    let Some(name) = queue.0.pop_front() else {
        return;
    };
    // A rig that vanished from the manifest between enqueue and now is simply no longer measurable.
    let Some(rig) = bench.rigs.as_ref().and_then(|r| r.get(&name)) else {
        return;
    };
    let report = measure_rig(&project.root, rig);
    if reports.by_rig.get(&name) != Some(&report) {
        reports.by_rig.insert(name, report);
        generation.0 = generation.0.wrapping_add(1);
    }
}

/// Repaint the ANIM tab's badge with the stale count: nothing, or `2 STALE`.
///
/// In place, never rebuilt — the strip is shared chrome — and on the strip precisely so the fact
/// survives tab switches: the developer who re-exported from Blender is on no particular tab when
/// the bench notices.
///
/// The badge is its own text child, not a suffix on the label: `style_tabs` owns every
/// [`crate::tiles::TabLabel`]'s colour per frame, so a `DANGER` written into the label is stomped
/// a frame later — the word rendered in the tab's ordinary grey, which the pane's own doc calls
/// the one word here allowed to shout, whispering. See [`crate::tiles::TabBadge`] for the colour
/// argument (Lewandowska et al. 2022: persistent peripheral signal at medium intensity).
pub(crate) fn paint_stale_badge(
    reports: Option<Res<BenchReports>>,
    tabs: Query<(&crate::tiles::Tab, &Children)>,
    mut badges: Query<&mut Text, With<crate::tiles::TabBadge>>,
) {
    let Some(reports) = reports else {
        return;
    };
    let stale = reports.stale_count();
    let want = if stale == 0 {
        String::new()
    } else {
        format!("{stale} STALE")
    };
    for (tab, children) in &tabs {
        if tab.0 != Mode::Anim {
            continue;
        }
        for child in children {
            if let Ok(mut text) = badges.get_mut(*child) {
                if text.0 != want {
                    text.0 = want.clone();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn a_change_promotes_only_after_holding_still_for_two_polls() {
        let mut s = MtimeState::default();
        assert!(!promote(&mut s, t(100)), "first sight is baseline, not change");
        assert!(!promote(&mut s, t(100)), "unchanged");
        assert!(!promote(&mut s, t(200)), "changed once — might still be mid-write");
        assert!(promote(&mut s, t(200)), "held still: promote");
        assert!(!promote(&mut s, t(200)), "and it is the new baseline");
    }

    #[test]
    fn a_file_still_being_written_never_promotes() {
        let mut s = MtimeState::default();
        assert!(!promote(&mut s, t(100)));
        // The exporter keeps touching the file; every poll sees a different mtime.
        assert!(!promote(&mut s, t(200)));
        assert!(!promote(&mut s, t(300)));
        assert!(!promote(&mut s, t(400)));
        // It settles.
        assert!(promote(&mut s, t(400)));
    }

    #[test]
    fn a_revert_between_polls_is_no_change_at_all() {
        let mut s = MtimeState::default();
        assert!(!promote(&mut s, t(100)));
        assert!(!promote(&mut s, t(200)), "changed once");
        assert!(!promote(&mut s, t(100)), "back to known — the candidate must die");
        assert!(!promote(&mut s, t(100)), "and stay dead");
    }

    #[test]
    fn the_queue_deduplicates_and_the_front_wins() {
        let mut q = MeasureQueue::default();
        q.push_back_unique("a");
        q.push_back_unique("b");
        q.push_back_unique("a");
        assert_eq!(q.0.len(), 2);
        q.push_front_unique("b");
        assert_eq!(q.0.front().map(String::as_str), Some("b"));
        assert_eq!(q.0.len(), 2);
    }
}
