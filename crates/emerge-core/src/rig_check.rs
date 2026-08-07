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
//! gait moves the root not at all; a gait's cycle distance agrees with the declared one; and — the
//! one comparison against the *game* rather than the asset — a gait's authored speed against the
//! rig's declared drive range (the skate check, [`skate_report`]). Plus the rule the old copies both
//! broke: **a check that cannot run says so loudly.** The FK checks anchor on named nodes, and a rig
//! without them used to be skipped in silence — which looks exactly like a pass, the same failure
//! class as an empty rig list that looks like "this project has no rigs".

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

/// When two contact joints' cycle distances for the SAME clip disagree by this fraction, one
/// foot's stance labels are suspect. Measured (valkyrie foot_l vs foot_r at its declared 0.35,
/// 2026-08-06): walk 2.4%, walk_back 2.1%, strafe_l 4.1%, run 7.0% — honest asymmetry stays
/// under 10%; strafe_r's 31% spread (and run_back's unmeasurable right foot) are exactly the
/// labelling failures the note exists to surface.
pub const CYCLE_JOINT_SPREAD_TOL: f32 = 0.10;

/// A looping clip whose last pose sits at or past this angle from its first pops every cycle.
///
/// Chosen from the measured histogram over all 16 shipped rigs (2026-08-06): every humanoid,
/// crab and SCP-1048 loop closes within **0.1 deg**; the manca's three open loops measure 4.9
/// (Idle_Snug @L6_coxa), 6.9 (Attack2 @L6_femur) and 18.8 deg (Idle_Alert @R_antS1 — the
/// unambiguous antenna pop). 2 deg sits in the empty gap between the closed mode and the open one,
/// so it separates the modes rather than splitting either. Translation closure measured 0.0000
/// file units on every shipped loop.
pub const LOOP_TOL_DEG: f32 = 2.0;

/// A one-shot ending at or past this angle from the idle pose pops when its weight fades back.
///
/// Same sweep: end states are bimodal — clips that return to idle end within **0.0 deg** of it;
/// deliberately terminal ones end 80–100 deg away (SCP-1048 sit_down 80 @upper_leg_l, scrap
/// whip/rage 82 @upper_arm_r, scp610 death 80 @torso, manca BurrowOut 99.6 @L6_femur). 15 deg
/// separates the modes with headroom; the terminal clips carry a permanent, truthful Note —
/// "this state is meant to be terminal" is in the finding text.
pub const ONESHOT_END_TOL_DEG: f32 = 15.0;

/// The foot-skate noise floor, in cm per frame at 60 Hz — **real motion capture is not skate-free**,
/// averaging about 0.10 cm/frame under the metric of Ling et al., "Character Controllers Using
/// Motion VAEs", SIGGRAPH 2020 (arXiv:2103.14274). Their `s = d·(2 − 2^(h/H))` weights slide by
/// foot height; our clips are authored in place with the game owning the transform, so what the
/// skate check computes is the **geometric** skate rate `|drive − realized|` — the speed difference
/// between the body (the sim's transform) and the legs (the cadence clamp's best effort). Zero is
/// not the target; this floor is.
pub const SKATE_FLOOR_CM_PER_FRAME: f32 = 0.10;

/// [`SKATE_FLOOR_CM_PER_FRAME`] in world units per second (u ≈ m): `0.10 cm/frame × 60 frames/s ÷
/// 100 cm/m`.
pub const SKATE_FLOOR: f32 = SKATE_FLOOR_CM_PER_FRAME * 60.0 / 100.0;

/// **The skate arithmetic for one gait slot against the rig's declared drive range** — the check
/// that measures the clip against the *game* rather than against itself.
///
/// Every gait implies an authored speed (`cycle_distance / duration`). The runtime's cadence
/// ([`crate::gait::gait_cycles_per_sec`] — the exact function, never a restatement) clamps to
/// [`crate::gait::PHASE_RATE_CLAMP`] × the mixture's authored cadence, so with this slot carrying
/// the phase the legs realize `clamp(drive, authored × 0.5, authored × 2.0)` and the remainder is
/// foot slide. Reported two ways, per the literature's conventions: **magnitude** (worst skate over
/// the drive range, in u/s and cm/frame at 60 Hz — comparable to the mocap floor above) and
/// **prevalence** (the fraction of the drive range skating past the floor — the *skating ratio*
/// convention of Siyao et al., "Duolando", ICLR 2024, arXiv:2403.18811).
///
/// Worst-case by construction: it assumes this slot alone carries the phase. The runtime blends
/// neighbouring gaits, which widens the true skate-free band between two authored speeds — so a
/// per-slot `Note` here is a boundary statement, and the set-level coverage note is the one that
/// holds regardless of blend.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkateReport {
    pub slot: usize,
    /// `cycle_distance / duration`, world u/s — the speed this clip was authored for.
    pub authored_speed: f32,
    /// Below this drive speed the clamp floor overdrives the feet (`authored × 0.5`).
    pub onset_low: f32,
    /// Above this drive speed the clamp ceiling can no longer keep up (`authored × 2.0`).
    pub onset_high: f32,
    /// Cadence the top of the drive range asks for, cycles/s.
    pub required_cps_at_max: f32,
    /// Cadence the runtime actually delivers there, cycles/s — via the shared clamp.
    pub clamped_cps_at_max: f32,
    /// Worst `|drive − realized|` over the drive range, u/s.
    pub max_skate: f32,
    /// [`SkateReport::max_skate`] in the literature's units: cm/frame at 60 Hz.
    pub max_skate_cm_frame: f32,
    /// Fraction of the drive range where skate exceeds [`SKATE_FLOOR`].
    pub skating_ratio: f32,
}

/// See [`SkateReport`]. `duration`/`cycle_distance` are the slot's **declared** numbers (world
/// units) — the ones the game plays with; `drive` is the rig's declared `(min, max)` drive range.
/// Everything is closed-form because the skate function is piecewise linear in drive speed.
pub fn skate_report(
    slot: usize,
    duration: f32,
    cycle_distance: f32,
    drive: (f32, f32),
) -> SkateReport {
    let (lo, hi) = drive;
    let authored = cycle_distance / duration;
    // The realized leg speed at drive speed v, through the runtime's own cadence function — a
    // single slot at full weight, so weight_sum = 1 and the mixture terms collapse to this slot's.
    let realized = |v: f32| {
        crate::gait::gait_cycles_per_sec(v, 1.0, cycle_distance, 1.0 / duration) * cycle_distance
    };
    let (clamp_lo, clamp_hi) = crate::gait::PHASE_RATE_CLAMP;
    let onset_low = authored * clamp_lo;
    let onset_high = authored * clamp_hi;
    let skate = |v: f32| (v - realized(v)).abs();
    // Piecewise linear and zero on [onset_low, onset_high]: the maximum sits at an endpoint.
    let max_skate = skate(lo).max(skate(hi));
    // Length of the drive range where skate exceeds the floor: the sub-range below
    // `onset_low − floor` plus the sub-range above `onset_high + floor`.
    let span = hi - lo;
    let skating_ratio = if span > 0.0 {
        let below = (hi.min(onset_low - SKATE_FLOOR) - lo).max(0.0);
        let above = (hi - lo.max(onset_high + SKATE_FLOOR)).max(0.0);
        ((below + above) / span).clamp(0.0, 1.0)
    } else if max_skate > SKATE_FLOOR {
        1.0
    } else {
        0.0
    };
    SkateReport {
        slot,
        authored_speed: authored,
        onset_low,
        onset_high,
        required_cps_at_max: hi / cycle_distance,
        clamped_cps_at_max: crate::gait::gait_cycles_per_sec(hi, 1.0, cycle_distance, 1.0 / duration),
        max_skate,
        max_skate_cm_frame: max_skate * 100.0 / 60.0,
        skating_ratio,
    }
}


/// One measurement result, with the fix in the text — *"a warning that does not say what to do about
/// it is a warning that gets read once."*
/// Serde (here and on the types below) because the editor's measurement cache persists reports
/// between sessions.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    /// The slot the finding is about, when it is about one.
    pub slot: Option<usize>,
    pub level: Level,
    pub text: String,
}

/// Ordered: `Bad` outranks `Note` outranks `Ok`, so [`worst`] is a `max`.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// One entry per gait slot, in slot order — empty when the rig declares no `drive_speed`.
    pub skates: Vec<SkateReport>,
}

/// **The checks.** The rig's own `scale` converts the GLB's file units to the manifest's world
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
    let mut skates = Vec::new();
    let scale = rig.scale;
    let found = clips::clips(glb);
    let root_name = rig.root_node();
    let root_node = clips::node_index(glb, root_name);
    let eps = rig.contact_eps;
    // Every declared contact joint (or the conventional default), resolved against the asset. The
    // FIRST resolved entry is THE anchor — cycle distance, phase, `SlotMeasure` and adopt all hang
    // off it; the rest corroborate.
    let foot_name = rig.contact_joint();
    let joint_names: Vec<&str> = if rig.contact_joints.is_empty() {
        vec![foot_name]
    } else {
        rig.contact_joints.iter().map(String::as_str).collect()
    };
    let resolved: Vec<usize> = joint_names
        .iter()
        .filter_map(|n| clips::node_index(glb, n))
        .collect();
    let missing: Vec<&str> = joint_names
        .iter()
        .filter(|n| clips::node_index(glb, n).is_none())
        .copied()
        .collect();
    let foot = joint_names.first().and_then(|n| clips::node_index(glb, n));

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
        if !missing.is_empty() {
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
            let listed = missing.join("', '");
            let consequence = if foot.is_none() {
                "cycle distance cannot be measured"
            } else {
                "the multi-joint corroboration is short-handed"
            };
            findings.push(Finding::bad(format!(
                "declares {gaits} gait slot(s) but the asset has no node named '{listed}' — \
                 {consequence}; export the rig with its contact feet under these names, or fix \
                 `contact_joints:` on the rig in rigs.ron{suggestion}"
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
        // **The all-rig checks** — the ones that owe nothing to a gait, so the other fifteen rigs
        // get more than "the clip exists".
        if let Playback::Free { speed } | Playback::OneShot { speed } = slot.playback {
            // Keying density, display-only: the team picks the floor once the numbers are lived
            // with — `keys per rendered frame` is what says when a sped-up clip strobes.
            if let Some(sr) = clips::source_rate(glb, slot.clip) {
                findings.push(
                    Finding::ok(format!(
                        "slot {i} keyed at {:.0} fps ({} keys); at x{speed:.2} = {:.1} authored \
                         keys per rendered frame at 60 Hz",
                        sr.fps,
                        sr.keys,
                        sr.fps * speed / 60.0
                    ))
                    .at(i),
                );
            }
            match slot.playback {
                Playback::Free { .. } => match clips::loop_closure(glb, slot.clip) {
                    Some(lc) if lc.max_angle_deg >= LOOP_TOL_DEG => {
                        let translation = if lc.max_translation >= 1.0e-3 {
                            match &lc.worst_translation_joint {
                                Some(j) => format!(
                                    "; {j} also translates {:.3} file units",
                                    lc.max_translation
                                ),
                                None => String::new(),
                            }
                        } else {
                            String::new()
                        };
                        findings.push(
                            Finding::note(format!(
                                "slot {i} loop pops: {} ends {:.1} deg from its start (worst of \
                                 {} rotated joints, tolerance {LOOP_TOL_DEG} deg){translation} — \
                                 re-export with the last key matching the first",
                                lc.worst_joint, lc.max_angle_deg, lc.joints
                            ))
                            .at(i),
                        );
                    }
                    Some(lc) => {
                        findings.push(
                            Finding::ok(format!(
                                "slot {i} loop closes ({} joints within {LOOP_TOL_DEG} deg)",
                                lc.joints
                            ))
                            .at(i),
                        );
                    }
                    None => findings.push(
                        Finding::note(format!(
                            "slot {i}: loop closure cannot be measured — the clip drives no \
                             rotation channel"
                        ))
                        .at(i),
                    ),
                },
                _ => {
                    // A one-shot's weight fades back into whatever the mix holds — the idle, by
                    // the slot-0 convention this file's own rigs all follow.
                    let idle = rig
                        .slots
                        .first()
                        .filter(|s0| matches!(s0.playback, Playback::Free { .. }))
                        .map(|s0| s0.clip);
                    match idle.and_then(|r| clips::end_pose_delta(glb, slot.clip, r)) {
                        Some(pd) if pd.max_angle_deg >= ONESHOT_END_TOL_DEG => {
                            findings.push(
                                Finding::note(format!(
                                    "slot {i} ends {:.1} deg from the idle pose at {} (slot 0 \
                                     first frame is the idle convention) — the fade back to idle \
                                     pops; end the clip nearer the idle pose, or this state is \
                                     meant to be terminal",
                                    pd.max_angle_deg, pd.worst_joint
                                ))
                                .at(i),
                            );
                        }
                        Some(pd) => {
                            findings.push(
                                Finding::ok(format!(
                                    "slot {i} ends on the idle pose ({} joints within \
                                     {ONESHOT_END_TOL_DEG} deg)",
                                    pd.joints
                                ))
                                .at(i),
                            );
                        }
                        None => findings.push(
                            Finding::note(format!(
                                "slot {i}: no idle reference to compare the one-shot's end state \
                                 against — slot 0 is not a free loop"
                            ))
                            .at(i),
                        ),
                    }
                }
            }
            continue;
        }
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
        // **The skate check** — the one comparison against the game rather than the asset. Declared
        // numbers on purpose: they are what the runtime plays with, wrong or not.
        if let Some(drive) = rig.drive_speed {
            let sk = skate_report(i, duration, cycle_distance, drive);
            let (lo, hi) = drive;
            if sk.max_skate <= SKATE_FLOOR {
                findings.push(
                    Finding::ok(format!(
                        "slot {i} authored {:.2} u/s; drive {lo:.1}-{hi:.1} u/s stays inside the \
                         x0.5-x2 cadence clamp (skate-free band {:.2}-{:.2} u/s)",
                        sk.authored_speed, sk.onset_low, sk.onset_high
                    ))
                    .at(i),
                );
            } else {
                let mut sides = Vec::new();
                if hi > sk.onset_high + SKATE_FLOOR {
                    sides.push(format!(
                        "drive above {:.2} u/s outruns the x2 cadence clamp (needs {:.2} cps, \
                         capped at {:.2})",
                        sk.onset_high, sk.required_cps_at_max, sk.clamped_cps_at_max
                    ));
                }
                if lo < sk.onset_low - SKATE_FLOOR {
                    sides.push(format!(
                        "drive below {:.2} u/s underruns the x0.5 clamp floor — the legs cannot \
                         slow further",
                        sk.onset_low
                    ));
                }
                findings.push(
                    Finding::note(format!(
                        "slot {i} authored {:.2} u/s: {} — feet slide up to {:.2} u/s \
                         (~{:.2} cm/frame at 60 Hz; mocap's own floor is ~0.10) over {:.0}% of \
                         the {lo:.1}-{hi:.1} u/s drive range, when this gait carries the phase \
                         alone; author a gait nearer the drive speed or narrow the range",
                        sk.authored_speed,
                        sides.join("; "),
                        sk.max_skate,
                        sk.max_skate_cm_frame,
                        sk.skating_ratio * 100.0
                    ))
                    .at(i),
                );
            }
            skates.push(sk);
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
            match clips::contact_track(glb, slot.clip, f, eps) {
                Some(track) => {
                    let raw = track.cycle_distance();
                    measure.cycle_distance = Some(raw);
                    let measured = raw * scale;
                    let err = (measured - cycle_distance).abs() / cycle_distance;
                    let tol = cycle_tolerance(slot, provenance_current);
                    // Why the tolerance is what it is, said inline — a bound whose provenance is
                    // invisible reads as arbitrary, and arbitrary bounds get argued with. The
                    // contact threshold gets the same treatment: derived per clip or declared on
                    // the rig, and which one is in the text.
                    let why = match (slot.tolerance, &slot.keep, provenance_current) {
                        (Some(_), _, _) => "explicit",
                        (None, Some(_), _) => "kept",
                        (None, None, true) => "measured-and-adopted",
                        (None, None, false) => "hand-measured default",
                    };
                    let eps_why = if eps.is_some() { "declared" } else { "derived" };
                    let thr = track.threshold;
                    if err >= tol {
                        findings.push(
                            Finding::bad(format!(
                                "slot {i} measures {measured:.3} m/cycle, manifest says \
                                 {cycle_distance:.3} ({:.0}% out, tolerance {:.0}% — {why}; \
                                 contact eps {thr:.2}x stance, {eps_why}) — re-measure or adopt",
                                err * 100.0,
                                tol * 100.0
                            ))
                            .at(i),
                        );
                    } else {
                        findings.push(
                            Finding::ok(format!(
                                "slot {i} measures {measured:.3} m/cycle vs {cycle_distance:.3} \
                                 declared (tolerance {:.0}%; contact eps {thr:.2}x stance, \
                                 {eps_why})",
                                tol * 100.0
                            ))
                            .at(i),
                        );
                    }
                    // **Multi-joint corroboration.** One foot's labels can lie (a partial stance,
                    // a threshold landing badly); a second foot measuring the same body is the
                    // cheap cross-check. The FIRST joint stays the anchor — this only speaks when
                    // the corroborators disagree with it.
                    if resolved.len() > 1 {
                        let per_joint: Vec<f32> = resolved
                            .iter()
                            .filter_map(|&jx| {
                                Some(clips::contact_track(glb, slot.clip, jx, eps)?.cycle_distance())
                            })
                            .collect();
                        if per_joint.len() > 1 {
                            let hi = per_joint.iter().fold(f32::MIN, |a, &b| a.max(b));
                            let lo = per_joint.iter().fold(f32::MAX, |a, &b| a.min(b));
                            let spread = (hi - lo) / hi.max(1.0e-6);
                            if spread >= CYCLE_JOINT_SPREAD_TOL {
                                let listing = resolved
                                    .iter()
                                    .zip(joint_names.iter())
                                    .filter_map(|(&jx, name)| {
                                        let cd = clips::contact_track(glb, slot.clip, jx, eps)?
                                            .cycle_distance();
                                        Some(format!("{name} {:.3}", cd * scale))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                findings.push(
                                    Finding::note(format!(
                                        "slot {i} cycle distance disagrees across contact \
                                         joints: {listing} ({:.0}% spread) — one foot's stance \
                                         labels are suspect; check the contact plots",
                                        spread * 100.0
                                    ))
                                    .at(i),
                                );
                            }
                        }
                    }
                }
                None => findings.push(
                    Finding::note(format!(
                        "slot {i}: no planted-foot stance to measure — the stance and swing \
                         speeds do not separate; declare `contact_eps:` on the rig if a human \
                         can see the stance"
                    ))
                    .at(i),
                ),
            }
        }
        slots.push(measure);
    }

    // **Set-level skate coverage** — the statement that survives blending. A per-slot note assumes
    // that slot carries the phase alone; blending widens the skate-free band *between* authored
    // speeds, but nothing the blender does can realize a speed beyond the fastest gait's clamp
    // ceiling or beneath the slowest gait's clamp floor. (Direction is pooled: back and strafe
    // gaits count toward coverage they can only provide when travel points their way — a
    // direction-aware coverage note is a known refinement.)
    if !skates.is_empty() {
        if let Some((lo, hi)) = rig.drive_speed {
            let top = skates.iter().map(|s| s.onset_high).fold(f32::MIN, f32::max);
            let bottom = skates.iter().map(|s| s.onset_low).fold(f32::MAX, f32::min);
            if hi > top + SKATE_FLOOR {
                let fastest = skates
                    .iter()
                    .map(|s| s.authored_speed)
                    .fold(f32::MIN, f32::max);
                findings.push(Finding::note(format!(
                    "no gait covers the top of the drive range: fastest authored {fastest:.2} u/s \
                     x2 = {top:.2} < drive max {hi:.1} — everything above {top:.2} u/s skates \
                     regardless of blend"
                )));
            }
            if lo < bottom - SKATE_FLOOR {
                let slowest = skates
                    .iter()
                    .map(|s| s.authored_speed)
                    .fold(f32::MAX, f32::min);
                findings.push(Finding::note(format!(
                    "no gait covers the bottom of the drive range: slowest authored {slowest:.2} \
                     u/s x0.5 = {bottom:.2} > drive min {lo:.1} — everything below {bottom:.2} u/s \
                     skates regardless of blend"
                )));
            }
        }
    }

    // **Phase alignment**, each gait measured against the rig's FIRST gait slot — deterministic
    // because slot order is the manifest's stated contract, and for the Valkyrie that is the walk,
    // which the manifest itself annotates as "the phase reference all the others are measured
    // against". Scored over every resolved contact joint (`phase_match_joints`): a left/right
    // pair is what breaks the half-cycle tie a single symmetric gait leaves.
    if foot.is_some() {
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
                match clips::phase_match_joints(glb, ref_clip, clip, &resolved) {
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
                            let tried = if resolved.len() > 1 {
                                format!(
                                    " (marker intersection over {} joints did not break the tie)",
                                    resolved.len()
                                )
                            } else {
                                String::new()
                            };
                            findings.push(
                                Finding::note(format!(
                                    "slot {i}: phase is ambiguous against slot {ref_slot} — the \
                                     clips step a different number of times per cycle, so the \
                                     alignment is a convention, not a measurement{tried}"
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
    RigCheck { findings, slots, skates }
}

/// Into `[0, 1)`, the wrap a phase fraction lives on.
fn wrap01(x: f32) -> f32 {
    x.rem_euclid(1.0)
}

/// Is a rig's recorded measurement still about the asset on disk?
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
            drive_speed: None,
            contact_eps: None,
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
        // The crab's case: Free slots only, no Root, no foot_l — and no anchor scolding. (The
        // all-rig loop-closure check still speaks; what must NOT appear is anything Bad or
        // anything about the missing anchor nodes.)
        let glb = tiny_glb(&["shell"], "scuttle", 0.8);
        let report = check_rig(&glb, &rig(vec![free_slot(0)]), false);
        assert!(
            report.findings.iter().all(|f| f.level != Level::Bad),
            "{:?}",
            report.findings
        );
        assert!(
            !report.findings.iter().any(|f| f.text.contains("no node named")),
            "{:?}",
            report.findings
        );
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
        // The skate check runs against the shipped drive range and stays a Note: the run gait is
        // authored at 2.85 u/s, so the top of the 6.0 drive outruns its x2 clamp by ~5% — a truth
        // about today's tuning, documented here rather than reddening CI.
        assert_eq!(report.skates.len(), 6, "one skate report per gait");
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.level == Level::Note && f.text.contains("cadence clamp")),
            "the shipped drive range should produce at least one skate note"
        );
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

    /// The all-rig checks refuse to guess: `tiny_glb`'s clips carry no rotation channel with real
    /// bytes, so loop closure says it cannot be measured — a Note, never a silent pass.
    #[test]
    fn unmeasurable_loop_closure_is_a_note_not_a_silent_pass() {
        let glb = tiny_glb(&["pelvis"], "idle", 1.0);
        let report = check_rig(&glb, &rig(vec![free_slot(0)]), false);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.level == Level::Note && f.text.contains("loop closure cannot")),
            "{:?}",
            report.findings
        );
    }

    /// A one-shot with no free slot 0 has no idle to compare against, and says so.
    #[test]
    fn a_one_shot_without_an_idle_reference_is_told() {
        let glb = tiny_glb(&["pelvis"], "fire", 1.0);
        let mut shot = free_slot(0);
        shot.playback = Playback::OneShot { speed: 1.0 };
        let report = check_rig(&glb, &rig(vec![shot]), false);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.level == Level::Note && f.text.contains("no idle reference")),
            "{:?}",
            report.findings
        );
    }

    /// The skate arithmetic, pinned closed-form. Walk-shaped numbers: authored 1.0 u/s exactly
    /// (duration 1.4, cycle 1.4), so the skate-free band is 0.5–2.0 u/s by construction.
    #[test]
    fn the_skate_report_is_a_truth_table() {
        // Entirely inside the band: no skate, no ratio.
        let sk = skate_report(2, 1.4, 1.4, (0.6, 1.8));
        assert_eq!(sk.slot, 2);
        assert!((sk.authored_speed - 1.0).abs() < 1.0e-6);
        assert!((sk.onset_low - 0.5).abs() < 1.0e-6 && (sk.onset_high - 2.0).abs() < 1.0e-6);
        assert_eq!(sk.max_skate, 0.0);
        assert_eq!(sk.skating_ratio, 0.0);

        // Top of the range outruns the clamp: skate = hi − onset_high, ratio = the overshoot
        // past (onset_high + floor) over the span.
        let sk = skate_report(0, 1.4, 1.4, (1.0, 3.0));
        assert!((sk.max_skate - 1.0).abs() < 1.0e-5, "{}", sk.max_skate);
        assert!((sk.max_skate_cm_frame - 1.0 * 100.0 / 60.0).abs() < 1.0e-4);
        let expect = (3.0 - (2.0 + SKATE_FLOOR)) / 2.0;
        assert!((sk.skating_ratio - expect).abs() < 1.0e-5, "{}", sk.skating_ratio);

        // Bottom of the range underruns the clamp floor.
        let sk = skate_report(0, 1.4, 1.4, (0.1, 0.6));
        assert!((sk.max_skate - 0.4).abs() < 1.0e-5, "{}", sk.max_skate);
        let expect = ((0.5 - SKATE_FLOOR) - 0.1) / 0.5;
        assert!((sk.skating_ratio - expect).abs() < 1.0e-5, "{}", sk.skating_ratio);

        // A degenerate one-speed range: all skate or none.
        assert_eq!(skate_report(0, 1.4, 1.4, (3.0, 3.0)).skating_ratio, 1.0);
        assert_eq!(skate_report(0, 1.4, 1.4, (1.0, 1.0)).skating_ratio, 0.0);

        // The clamped cadence is the runtime's own function, not a restatement.
        let sk = skate_report(0, 1.4, 1.4, (1.0, 3.0));
        assert_eq!(
            sk.clamped_cps_at_max,
            crate::gait::gait_cycles_per_sec(3.0, 1.0, 1.4, 1.0 / 1.4)
        );
        assert!((sk.required_cps_at_max - 3.0 / 1.4).abs() < 1.0e-6);
    }

    /// A drive range inside the clamp band yields the Ok wording; one outside yields the Note with
    /// the chain (cps needed vs capped) — and never anything Bad, because today's content skates
    /// and a red build on known content teaches people to ignore red builds.
    #[test]
    fn the_skate_check_notes_but_never_reddens() {
        let glb = tiny_glb(&["Root", "foot_l"], "walk", 1.417);
        let mut r = rig(vec![gait_slot(0)]);
        r.drive_speed = Some((0.9, 6.0));
        let report = check_rig(&glb, &r, false);
        let skate_notes: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.level == Level::Note && f.text.contains("cadence clamp"))
            .collect();
        assert!(!skate_notes.is_empty(), "{:?}", report.findings);
        assert!(skate_notes[0].text.contains("cps"), "{}", skate_notes[0].text);
        assert_eq!(report.skates.len(), 1);
        // The set-level coverage note fires too: 0.98 × 2 < 6.0.
        assert!(
            report.findings.iter().any(|f| f.text.contains("top of the drive range")),
            "{:?}",
            report.findings
        );

        // No drive_speed declared: no skate opinion at all.
        let quiet = check_rig(&glb, &rig(vec![gait_slot(0)]), false);
        assert!(quiet.skates.is_empty());
        assert!(!quiet.findings.iter().any(|f| f.text.contains("clamp")));
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
