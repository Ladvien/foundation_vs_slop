//! **The four checks, one policy, both consumers.**
//!
//! The editor's animation bench (`crates/emerge-mapper/src/anim_tab.rs`) and the CI drift guard
//! (`crates/emerge-core/tests/rigs_match_assets.rs`) used to carry two independent copies of the same
//! four checks — same primitives, separately written loops, thresholds and wording. Two copies of a
//! policy drift exactly the way two copies of a measurement do, and when CI went red the editor could
//! not be trusted to reproduce it. This module is the single copy: the editor renders what CI panics
//! on, in the same words.
//!
//! The checks, per slot: the clip exists; a gait's duration matches the asset within one frame; a
//! gait moves the root not at all; a gait's cycle distance agrees with the declared one. Plus the rule
//! the old copies both broke: **a check that cannot run says so loudly.** The FK checks anchor on
//! named nodes, and a rig without them used to be skipped in silence — which looks exactly like a
//! pass, the same failure class as an empty rig list that looks like "this project has no rigs".

use crate::clips::{self, ClipInfo};
use crate::glb::Glb;
use crate::rigs::{Playback, Rig};

/// One 24 fps frame — the duration tolerance the shared-phase seek mapping needs.
pub const FRAME: f32 = 1.0 / 24.0;

/// A gait must be authored in place; per-axis root travel at or above this is root motion.
pub const ROOT_MOTION_EPS: f32 = 1.0e-4;

/// Cycle-distance tolerance, as a fraction of the declared value. Loose on purpose:
/// `docs/artist_guide.md` §4's hand-measured back and strafe numbers are themselves rough, so a tight
/// bound would assert their error rather than the asset's truth. 20% catches a re-export that changed
/// a stride; `clips.rs`'s own test pins the reference gaits to 3%.
pub const CYCLE_TOL_DEFAULT: f32 = 0.20;

/// The tolerance once a rig's numbers are **measured-and-adopted and the asset unchanged**: the
/// same deterministic instrument re-measuring the same bytes should agree with itself to noise.
/// The loose default compensates for hand-entry error; adoption removes the hand.
pub const CYCLE_TOL_MEASURED: f32 = 0.02;

/// **Which cycle tolerance governs a slot** — the alert-fatigue policy, in one place.
///
/// An explicit per-slot `tolerance:` wins (a documented reason to disagree by a known margin). A
/// kept slot stays loose — its numbers are deliberately not the asset's. Otherwise: tight when the
/// provenance says these numbers were measured off exactly the bytes on disk, loose when a hand
/// may have been involved. A check that never fires stops being read, and a loose bound that
/// swallows real errors is how that happens — so the bound tightens exactly when it can.
pub fn cycle_tolerance(slot: &crate::rigs::SlotDef, provenance_current: bool) -> f32 {
    match (slot.tolerance, &slot.keep, provenance_current) {
        (Some(t), _, _) => t,
        (None, Some(_), _) => CYCLE_TOL_DEFAULT,
        (None, None, true) => CYCLE_TOL_MEASURED,
        (None, None, false) => CYCLE_TOL_DEFAULT,
    }
}

/// Phase agreement (in fractions of a cycle) below this is `Ok`; at or above it, a `Note`.
///
/// Never a `Bad` — yet. The manifest's back/strafe offsets are documented as rough hand
/// measurements (`docs/artist_guide.md` §4 owns up to a 0.14 spread), and a red build on
/// known-rough data teaches people to ignore red builds. The tier tightens when a slot's numbers
/// are measured-and-adopted, at which point measured and declared should agree to the grid.
pub const PHASE_TOL_OK: f32 = 0.05;


/// One measurement result, with the fix in the text — *"a warning that does not say what to do about
/// it is a warning that gets read once."*
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// The slot the finding is about, when it is about one.
    pub slot: Option<usize>,
    pub level: Level,
    pub text: String,
}

/// Ordered: `Bad` outranks `Note` outranks `Ok`, so [`worst`] is a `max`.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
pub enum Level {
    Ok,
    Note,
    Bad,
}

impl Finding {
    pub fn ok(text: String) -> Finding {
        Finding { slot: None, level: Level::Ok, text }
    }
    pub fn note(text: String) -> Finding {
        Finding { slot: None, level: Level::Note, text }
    }
    pub fn bad(text: String) -> Finding {
        Finding { slot: None, level: Level::Bad, text }
    }
    /// Attach the slot the finding is about.
    pub fn at(mut self, slot: usize) -> Finding {
        self.slot = Some(slot);
        self
    }
}

/// The most severe level present. An empty report is `Ok` — nothing to say is a pass.
pub fn worst(findings: &[Finding]) -> Level {
    findings.iter().map(|f| f.level).max().unwrap_or(Level::Ok)
}

/// `"clip 11 (run)"` when the exporter wrote a name, `"clip 11"` otherwise. Names are documentation,
/// never a lookup key — the Valkyrie's strafe clips are named backwards in the asset — but a reader
/// resolving an index by hand deserves the label the asset carries.
pub fn clip_label(index: usize, clips: &[ClipInfo]) -> String {
    match clips.get(index).and_then(|c| c.name.as_deref()) {
        Some(name) => format!("clip {index} ({name})"),
        None => format!("clip {index}"),
    }
}

/// What the asset measures for one gait slot — the structured half of a check, and the input a
/// write-back adopts from. Distances are **file units**; multiply by the rig scale for world units.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotMeasure {
    pub slot: usize,
    /// The asset clip's duration, seconds.
    pub duration: f32,
    /// Foot-slide distance per cycle, file units. `None` when the clip has no measurable stance.
    pub cycle_distance: Option<f32>,
    /// Signed phase offset in the frame where the rig's FIRST gait slot sits at 0.0 — the frame an
    /// adopt writes, since it writes the reference as 0.0. `None` when it could not be measured.
    pub phase_offset: Option<f32>,
}

/// Everything one pass over the asset produced.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RigCheck {
    pub findings: Vec<Finding>,
    /// One entry per gait slot, in slot order.
    pub slots: Vec<SlotMeasure>,
}

/// **The four checks.** The rig's own `scale` converts the GLB's file units to the manifest's world
/// units, and its `root_node`/`contact_joints` (or the conventional defaults) anchor the FK.
/// `provenance_current` is whether the rig's stamp matches the bytes being checked
/// ([`staleness`] == [`Staleness::Current`]) — it selects the cycle tolerance, so the editor and CI
/// tighten together or not at all.
///
/// The caller opens the GLB — how an unreadable file is reported (a finding, a panic) is the
/// consumer's decision, but what agreement *means* is decided here, once.
pub fn check_rig(glb: &Glb, rig: &Rig, provenance_current: bool) -> RigCheck {
    let mut findings = Vec::new();
    let mut slots = Vec::new();
    let scale = rig.scale;
    let found = clips::clips(glb);
    let root_name = rig.root_node();
    let foot_name = rig.contact_joint();
    let root_node = clips::node_index(glb, root_name);
    let foot = clips::node_index(glb, foot_name);

    // **Loud when an anchor is missing.** Only rigs that declare a gait need the anchors — the crab
    // legitimately has neither, and scolding it would be noise. But a gait rig without them used to
    // lose checks 3 and 4 in silence, and silence looks like a pass.
    let gaits = rig
        .slots
        .iter()
        .filter(|s| matches!(s.playback, Playback::Gait { .. }))
        .count();
    if gaits > 0 {
        if root_node.is_none() {
            findings.push(Finding::bad(format!(
                "declares {gaits} gait slot(s) but the asset has no node named '{root_name}' — the \
                 in-place check cannot run; export the rig with its root bone named {root_name}, \
                 or set `root_node:` on the rig in rigs.ron"
            )));
        }
        if foot.is_none() {
            // Which joints DO behave like feet, measured off the reference gait — the difference
            // between "configure contact_joints" and "configure contact_joints to one of these".
            let reference_clip = rig.slots.iter().find_map(|s| {
                matches!(s.playback, Playback::Gait { .. }).then_some(s.clip)
            });
            let suggestion = match reference_clip
                .map(|c| clips::contact_candidates(glb, c))
                .filter(|c| !c.is_empty())
            {
                Some(cands) => format!(
                    "; candidate contact joints: {}",
                    cands
                        .iter()
                        .take(4)
                        .map(|(n, f)| format!("{n} {f:.2}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None => String::new(),
            };
            findings.push(Finding::bad(format!(
                "declares {gaits} gait slot(s) but the asset has no node named '{foot_name}' — \
                 cycle distance cannot be measured; export the rig with its contact foot named \
                 {foot_name}, or set `contact_joints:` on the rig in rigs.ron{suggestion}"
            )));
        }
    }

    for (i, slot) in rig.slots.iter().enumerate() {
        let Some(c) = found.get(slot.clip) else {
            findings.push(
                Finding::bad(format!(
                    "slot {i} names clip {} but the asset has {}: {} — it was re-exported; \
                     re-measure and update rigs.ron",
                    slot.clip,
                    found.len(),
                    roster(&found),
                ))
                .at(i),
            );
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
        let mut measure = SlotMeasure {
            slot: i,
            duration: c.duration,
            cycle_distance: None,
            phase_offset: None,
        };
        if (c.duration - duration).abs() >= FRAME {
            findings.push(
                Finding::bad(format!(
                    "slot {i} is {:.3}s in the asset, {duration:.3}s here — the shared phase maps \
                     onto the wrong part of the clip and feet drift",
                    c.duration
                ))
                .at(i),
            );
        }
        if let Some(r) = root_node {
            let m = clips::root_motion(glb, slot.clip, r);
            if m.iter().any(|v| *v >= ROOT_MOTION_EPS) {
                findings.push(
                    Finding::bad(format!(
                        "slot {i} moves {root_name} by {m:?} — a gait must be authored in place; \
                         the game drives the transform itself"
                    ))
                    .at(i),
                );
            }
        }
        if let Some(f) = foot {
            match clips::cycle_distance(glb, slot.clip, f) {
                Some(raw) => {
                    measure.cycle_distance = Some(raw);
                    let measured = raw * scale;
                    let err = (measured - cycle_distance).abs() / cycle_distance;
                    let tol = cycle_tolerance(slot, provenance_current);
                    // Why the tolerance is what it is, said inline — a bound whose provenance is
                    // invisible reads as arbitrary, and arbitrary bounds get argued with.
                    let why = match (slot.tolerance, &slot.keep, provenance_current) {
                        (Some(_), _, _) => "explicit",
                        (None, Some(_), _) => "kept",
                        (None, None, true) => "measured-and-adopted",
                        (None, None, false) => "hand-measured default",
                    };
                    if err >= tol {
                        findings.push(
                            Finding::bad(format!(
                                "slot {i} measures {measured:.3} m/cycle, manifest says \
                                 {cycle_distance:.3} ({:.0}% out, tolerance {:.0}% — {why}) — \
                                 re-measure or adopt",
                                err * 100.0,
                                tol * 100.0
                            ))
                            .at(i),
                        );
                    } else {
                        findings.push(
                            Finding::ok(format!(
                                "slot {i} measures {measured:.3} m/cycle vs {cycle_distance:.3} \
                                 declared (tolerance {:.0}%)",
                                tol * 100.0
                            ))
                            .at(i),
                        );
                    }
                }
                None => findings.push(
                    Finding::note(format!("slot {i}: no planted-foot stance to measure")).at(i),
                ),
            }
        }
        slots.push(measure);
    }

    // **Phase alignment**, each gait measured against the rig's FIRST gait slot — deterministic
    // because slot order is the manifest's stated contract, and for the Valkyrie that is the walk,
    // which the manifest itself annotates as "the phase reference all the others are measured
    // against".
    if let Some(f) = foot {
        let gait_slots: Vec<(usize, usize, f32)> = rig
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s.playback {
                Playback::Gait { phase_offset, .. } => Some((i, s.clip, phase_offset)),
                _ => None,
            })
            .collect();
        if let Some(&(ref_slot, ref_clip, ref_declared)) = gait_slots.first() {
            // The reference defines the frame; an adopt writes it as 0.0.
            if let Some(sm) = slots.iter_mut().find(|sm| sm.slot == ref_slot) {
                sm.phase_offset = Some(0.0);
            }
            for &(i, clip, declared) in gait_slots.iter().skip(1) {
                match clips::phase_match(glb, ref_clip, clip, f) {
                    Some(m) => {
                        // The slot's implied ABSOLUTE offset, in the frame where the reference
                        // sits at its declared value — directly comparable with what rigs.ron says.
                        let measured = clips::signed_offset(wrap01(ref_declared + m.offset));
                        let diff = clips::signed_offset(wrap01(measured - declared)).abs();
                        if let Some(sm) = slots.iter_mut().find(|sm| sm.slot == i) {
                            sm.phase_offset = Some(clips::signed_offset(m.offset));
                        }
                        if diff < PHASE_TOL_OK {
                            findings.push(
                                Finding::ok(format!(
                                    "slot {i} phase {measured:+.3} measured vs {declared:+.3} \
                                     declared"
                                ))
                                .at(i),
                            );
                        } else {
                            findings.push(
                                Finding::note(format!(
                                    "slot {i} phase {measured:+.3} measured vs {declared:+.3} \
                                     declared ({diff:.3} of a cycle apart) — the declared \
                                     alignment does not match the asset; adopt the measured set \
                                     once it looks right"
                                ))
                                .at(i),
                            );
                        }
                        if m.ambiguous {
                            findings.push(
                                Finding::note(format!(
                                    "slot {i}: phase is ambiguous against slot {ref_slot} — the \
                                     clips step a different number of times per cycle, so the \
                                     alignment is a convention, not a measurement"
                                ))
                                .at(i),
                            );
                        }
                    }
                    None => findings.push(
                        Finding::note(format!(
                            "slot {i}: phase cannot be measured against slot {ref_slot} — one of \
                             the two clips has no resolvable foot track"
                        ))
                        .at(i),
                    ),
                }
            }
        }
    }
    RigCheck { findings, slots }
}

/// Into `[0, 1)`, the wrap a phase fraction lives on.
fn wrap01(x: f32) -> f32 {
    x.rem_euclid(1.0)
}

/// Is a rig's recorded measurement still about the asset on disk?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staleness {
    /// No provenance stamp — the numbers were never measured by the bench.
    NeverMeasured,
    /// The stamp's fingerprint matches the live file.
    Current,
    /// The asset changed since the stamp was written. The strongest statement the bench can make:
    /// it catches re-exports the four value checks never look at, and it cannot false-alarm.
    Stale,
}

/// See [`Staleness`]. `live_hash` is the file fingerprint from `Glb::open_fingerprinted`.
pub fn staleness(rig: &Rig, live_hash: u64) -> Staleness {
    match &rig.provenance {
        None => Staleness::NeverMeasured,
        Some(p) if p.glb_fnv1a == crate::rigs::fingerprint_string(live_hash) => Staleness::Current,
        Some(_) => Staleness::Stale,
    }
}

/// **What a re-export did to the clip list, stated causally.** `recorded` is the provenance's
/// name-per-index snapshot; `live` is the asset now. Empty when nothing changed — and the common
/// real cause of a broken manifest ("index out of range") comes back as *"strafe_l added at index
/// 6; every index after it shifted"*, which is the diagnosis rather than the symptom.
pub fn clip_list_diff(recorded: &[String], live: &[ClipInfo]) -> Vec<String> {
    let live_names: Vec<String> = live
        .iter()
        .map(|c| c.name.clone().unwrap_or_default())
        .collect();
    if recorded == live_names.as_slice() {
        return Vec::new();
    }
    let show = |s: &String| -> String {
        if s.is_empty() { "(unnamed)".to_owned() } else { s.clone() }
    };
    // One insertion: everything before k matches, and the old tail reappears after the new entry.
    if live_names.len() == recorded.len() + 1 {
        if let Some(k) = (0..live_names.len())
            .find(|&k| recorded.get(k) != live_names.get(k))
            .filter(|&k| recorded[k..] == live_names[k + 1..])
        {
            return vec![format!(
                "{} added at index {k}; every clip index after it shifted up by one",
                show(&live_names[k])
            )];
        }
    }
    // One removal: the mirror case.
    if recorded.len() == live_names.len() + 1 {
        if let Some(k) = (0..recorded.len())
            .find(|&k| recorded.get(k) != live_names.get(k))
            .filter(|&k| live_names[k..] == recorded[k + 1..])
        {
            return vec![format!(
                "{} removed at index {k}; every clip index after it shifted down by one",
                show(&recorded[k])
            )];
        }
    }
    // Same count, renames only.
    if recorded.len() == live_names.len() {
        let mut out: Vec<String> = recorded
            .iter()
            .zip(&live_names)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(6)
            .map(|(i, (a, b))| format!("clip {i} renamed: {} -> {}", show(a), show(b)))
            .collect();
        let changed = recorded.iter().zip(&live_names).filter(|(a, b)| a != b).count();
        if changed > 6 {
            out.push(format!("... and {} more renames", changed - 6));
        }
        return out;
    }
    // Anything messier gets the honest summary plus where it first diverges.
    let first = (0..recorded.len().max(live_names.len()))
        .find(|&k| recorded.get(k) != live_names.get(k))
        .unwrap_or(0);
    vec![format!(
        "clip list changed: {} -> {} clips, first divergence at index {first}",
        recorded.len(),
        live_names.len()
    )]
}

/// `YYYY-MM-DD` (UTC) from unix seconds — Howard Hinnant's `civil_from_days` algorithm
/// (https://howardhinnant.github.io/date_algorithms.html), hand-rolled because the allowlist has
/// no date crate. The clock itself is injected by the CALLER (`SystemTime` at the call site), so
/// everything here is pure and testable.
pub fn civil_date_utc(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The asset's clips as `"0 idle, 1 walk, ..."`, capped so a pathological export cannot flood a
/// finding. What turns "index out of range" into "here is what to pick".
fn roster(found: &[ClipInfo]) -> String {
    const CAP: usize = 20;
    let mut names: Vec<String> = found
        .iter()
        .take(CAP)
        .map(|c| match &c.name {
            Some(n) => format!("{} {n}", c.index),
            None => format!("{}", c.index),
        })
        .collect();
    if found.len() > CAP {
        names.push("...".to_owned());
    }
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rigs::SlotDef;
    use serde_json::json;

    /// A minimal one-clip GLB: one node, one translation channel, duration from the accessor max.
    fn tiny_glb(node_names: &[&str], clip_name: &str, duration: f32) -> Glb {
        let nodes: Vec<_> = node_names.iter().map(|n| json!({ "name": n })).collect();
        Glb {
            json: json!({
                "nodes": nodes,
                "animations": [{
                    "name": clip_name,
                    "channels": [{ "sampler": 0, "target": { "node": 0, "path": "translation" } }],
                    "samplers": [{ "input": 0, "output": 1 }],
                }],
                "accessors": [{ "max": [duration] }, {}],
            }),
            bin: Vec::new(),
        }
    }

    fn gait_slot(clip: usize) -> SlotDef {
        SlotDef {
            clip,
            playback: Playback::Gait {
                duration: 1.417,
                phase_offset: 0.0,
                cycle_distance: 1.388,
            },
            mask: None,
            note: None,
            state: None,
            keep: None,
            tolerance: None,
        }
    }

    fn free_slot(clip: usize) -> SlotDef {
        SlotDef {
            clip,
            playback: Playback::Free { speed: 1.0 },
            mask: None,
            note: None,
            state: None,
            keep: None,
            tolerance: None,
        }
    }

    fn rig(slots: Vec<SlotDef>) -> Rig {
        Rig {
            mesh: "characters/test.glb".to_owned(),
            scale: 1.0,
            root_node: None,
            contact_joints: Vec::new(),
            provenance: None,
            slots,
        }
    }

    #[test]
    fn a_gait_rig_without_the_anchor_nodes_is_told_loudly() {
        let glb = tiny_glb(&["pelvis"], "walk", 1.417);
        let report = check_rig(&glb, &rig(vec![gait_slot(0)]), false);
        let bad: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.level == Level::Bad)
            .collect();
        assert_eq!(bad.len(), 2, "{:?}", report.findings);
        assert!(bad[0].text.contains(crate::rigs::DEFAULT_ROOT_NODE), "{}", bad[0].text);
        assert!(bad[1].text.contains(crate::rigs::DEFAULT_CONTACT_JOINT), "{}", bad[1].text);
    }

    #[test]
    fn a_configured_anchor_replaces_the_conventional_name() {
        // The same rig, but its manifest names the anchors the asset actually has — the loud
        // findings must go quiet without any node being renamed.
        let glb = tiny_glb(&["pelvis"], "walk", 1.417);
        let mut r = rig(vec![gait_slot(0)]);
        r.root_node = Some("pelvis".to_owned());
        r.contact_joints = vec!["pelvis".to_owned()];
        let report = check_rig(&glb, &r, false);
        let anchors_missing = report
            .findings
            .iter()
            .any(|f| f.text.contains("has no node named"));
        assert!(!anchors_missing, "{:?}", report.findings);
    }

    #[test]
    fn a_rig_with_no_gaits_owes_no_anchors() {
        // The crab's case: Free slots only, no Root, no foot_l — and nothing to scold.
        let glb = tiny_glb(&["shell"], "scuttle", 0.8);
        let report = check_rig(&glb, &rig(vec![free_slot(0)]), false);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn a_missing_clip_names_what_the_asset_does_have() {
        let glb = tiny_glb(&["pelvis"], "walk", 1.417);
        let report = check_rig(&glb, &rig(vec![free_slot(7)]), false);
        assert_eq!(report.findings.len(), 1);
        let f = &report.findings[0];
        assert_eq!(f.level, Level::Bad);
        assert_eq!(f.slot, Some(0));
        assert!(f.text.contains("0 walk"), "{}", f.text);
    }

    #[test]
    fn the_shipped_valkyrie_produces_no_bad_findings() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap_or_else(|e| panic!("workspace root: {e}"));
        let text = std::fs::read_to_string(root.join("assets/emerge/rigs.ron"))
            .unwrap_or_else(|e| panic!("rigs.ron: {e}"));
        let rigs = crate::rigs::Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        let valk = rigs
            .get("valkyrie")
            .unwrap_or_else(|| panic!("no valkyrie in the manifest"));
        let glb = Glb::open(&root.join("assets").join(&valk.mesh))
            .unwrap_or_else(|e| panic!("{e}"));
        let report = check_rig(&glb, valk, false);
        let bad: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.level == Level::Bad)
            .collect();
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(report.slots.len(), 6, "the valkyrie declares six gaits");
    }

    #[test]
    fn clip_labels_carry_the_asset_name_when_there_is_one() {
        let glb = tiny_glb(&["pelvis"], "walk", 1.0);
        let found = clips::clips(&glb);
        assert_eq!(clip_label(0, &found), "clip 0 (walk)");
        assert_eq!(clip_label(9, &found), "clip 9");
    }

    #[test]
    fn staleness_is_a_three_way_answer() {
        let mut r = rig(vec![free_slot(0)]);
        assert_eq!(staleness(&r, 7), Staleness::NeverMeasured);
        r.provenance = Some(crate::rigs::Provenance {
            glb_fnv1a: crate::rigs::fingerprint_string(7),
            clips: 0,
            clip_names: Vec::new(),
            tool: crate::rigs::BENCH_TOOL_VERSION,
            date: "2026-08-06".to_owned(),
        });
        assert_eq!(staleness(&r, 7), Staleness::Current);
        assert_eq!(staleness(&r, 8), Staleness::Stale);
    }

    fn infos(names: &[&str]) -> Vec<ClipInfo> {
        names
            .iter()
            .enumerate()
            .map(|(index, n)| ClipInfo {
                index,
                name: (!n.is_empty()).then(|| (*n).to_owned()),
                duration: 1.0,
                channels: 1,
            })
            .collect()
    }

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn a_clip_list_diff_states_the_cause_not_the_symptom() {
        let recorded = owned(&["idle", "walk", "run"]);
        assert!(clip_list_diff(&recorded, &infos(&["idle", "walk", "run"])).is_empty());

        let added = clip_list_diff(&recorded, &infos(&["idle", "strafe_l", "walk", "run"]));
        assert_eq!(added.len(), 1);
        assert!(
            added[0].contains("strafe_l added at index 1") && added[0].contains("shifted"),
            "{added:?}"
        );

        let removed = clip_list_diff(&recorded, &infos(&["idle", "run"]));
        assert_eq!(removed.len(), 1);
        assert!(removed[0].contains("walk removed at index 1"), "{removed:?}");

        let renamed = clip_list_diff(&recorded, &infos(&["idle", "walk", "sprint"]));
        assert_eq!(renamed.len(), 1);
        assert!(renamed[0].contains("clip 2 renamed: run -> sprint"), "{renamed:?}");

        let messy = clip_list_diff(&recorded, &infos(&["other"]));
        assert_eq!(messy.len(), 1);
        assert!(messy[0].contains("3 -> 1 clips"), "{messy:?}");
    }

    #[test]
    fn civil_dates_come_out_gregorian() {
        // Vectors computed with Python's datetime, mid-day so the day boundary is unambiguous.
        assert_eq!(civil_date_utc(45_000), "1970-01-01");
        assert_eq!(civil_date_utc(951_827_400), "2000-02-29");
        assert_eq!(civil_date_utc(1_786_019_400), "2026-08-06");
        assert_eq!(civil_date_utc(946_643_400), "1999-12-31");
    }

    #[test]
    fn the_cycle_tolerance_policy_is_a_truth_table() {
        let mut slot = gait_slot(0);
        // Hand-measured default: loose.
        assert_eq!(cycle_tolerance(&slot, false), CYCLE_TOL_DEFAULT);
        // Measured-and-adopted, asset unchanged: tight.
        assert_eq!(cycle_tolerance(&slot, true), CYCLE_TOL_MEASURED);
        // Kept: loose regardless — the numbers are deliberately not the asset's.
        slot.keep = Some("authored feel".to_owned());
        assert_eq!(cycle_tolerance(&slot, true), CYCLE_TOL_DEFAULT);
        // Explicit wins over everything.
        slot.tolerance = Some(0.07);
        assert_eq!(cycle_tolerance(&slot, true), 0.07);
        assert_eq!(cycle_tolerance(&slot, false), 0.07);
    }

    #[test]
    fn the_fingerprint_spelling_and_hash_are_pinned() {
        // FNV-1a 64 reference vectors; a fingerprint that drifts orphans every stored provenance.
        assert_eq!(crate::glb::fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(crate::glb::fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(
            crate::rigs::fingerprint_string(0xaf63_dc4c_8601_ec8c),
            "0xaf63dc4c8601ec8c"
        );
    }
}
