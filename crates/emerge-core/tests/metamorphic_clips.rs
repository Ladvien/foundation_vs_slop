//! **Metamorphic relations for the clip measurer** — the answer to a measurer with no oracle.
//!
//! There is no ground-truth `cycle_distance` for a GLB to assert against: `measure(GLB)` is what
//! the testing literature calls a *non-testable program* (Segura, Fraser, Sanchez & Ruiz-Cortes,
//! "A Survey on Metamorphic Testing", IEEE TSE 2016, doi 10.1109/TSE.2016.2532875). The remedy is
//! metamorphic testing (Chen, Cheung & Yiu, "Metamorphic testing: a new approach for generating
//! next test cases", HKUST-CS98-01, 1998): transform the INPUT and assert how the output must
//! change. A violated relation proves a bug; a satisfied one does not prove correctness — but the
//! relations are cheap, and they catch exactly the class of error a sample size of one cannot.
//!
//! The fixture is the real, committed valkyrie GLB, mutated **in memory** — `Glb.json` and
//! `Glb.bin` are public — so every relation runs against real keyframe density rather than a
//! four-key toy. Node indices stay valid because every mutation appends; nothing is reordered.
//!
//! The transform helpers at the bottom re-state the accessor byte layout (base/stride/f32-LE)
//! that `clips.rs` reads. That duplication is deliberate: the helpers must not be the code under
//! test.

use emerge_core::clips::{self, PHASE_BINS};
use emerge_core::glb::Glb;
use emerge_core::rig_check::{self, Level};
use emerge_core::rigs::Rigs;

/// The valkyrie's clip indices, per `assets/emerge/rigs.ron`: walk is the phase reference.
const WALK: usize = 5;
const WALK_BACK: usize = 8;

/// Relative tolerance for float relations; phase comparisons get one grid bin.
const REL: f32 = 1.0e-3;
const ONE_BIN: f32 = 1.0 / PHASE_BINS as f32;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn valkyrie() -> Glb {
    Glb::open(&root().join("assets/characters/valkyrie.glb")).unwrap_or_else(|e| panic!("{e}"))
}

fn valkyrie_rig() -> emerge_core::rigs::Rig {
    let text = std::fs::read_to_string(root().join("assets/emerge/rigs.ron"))
        .unwrap_or_else(|e| panic!("{e}"));
    Rigs::parse(&text)
        .unwrap_or_else(|e| panic!("{e}"))
        .get("valkyrie")
        .cloned()
        .unwrap_or_else(|| panic!("no valkyrie rig"))
}

fn foot(glb: &Glb) -> usize {
    clips::node_index(glb, "foot_l").unwrap_or_else(|| panic!("no foot_l"))
}

fn rel_eq(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol * a.abs().max(b.abs()).max(1.0e-6)
}

/// Wrap-aware distance between two phase fractions.
fn phase_dist(a: f32, b: f32) -> f32 {
    clips::signed_offset((a - b).rem_euclid(1.0)).abs()
}

// ── relation 1: uniform scale ────────────────────────────────────────────────────────────────────

/// Scaling the whole rig by k scales cycle distance by k, keeps duration, keeps the CONTACT
/// LABELS bit-identical (the threshold is relative to the clip's own stance speed — an absolute
/// threshold would relabel a scaled rig, which is how a wrong `FIGURINE_SCALE` once hid), and
/// keeps phase.
#[test]
fn uniform_scale_scales_cycle_distance_and_nothing_else() {
    let plain = valkyrie();
    let f = foot(&plain);
    let base_cd = clips::cycle_distance(&plain, WALK, f, None).unwrap_or_else(|| panic!("no cd"));
    let base_dur = clips::clips(&plain)[WALK].duration;
    let base_contact = clips::contact_track(&plain, WALK, f, None)
        .unwrap_or_else(|| panic!("no track"))
        .contact;
    let base_phase = clips::phase_offset(&plain, WALK, WALK_BACK, f)
        .unwrap_or_else(|| panic!("no phase"));

    let mut scaled = valkyrie();
    wrap_scene_root(&mut scaled, [0.0; 3], [0.0, 0.0, 0.0, 1.0], [2.5; 3]);
    let f2 = foot(&scaled);
    let cd = clips::cycle_distance(&scaled, WALK, f2, None).unwrap_or_else(|| panic!("no cd"));
    assert!(rel_eq(cd, base_cd * 2.5, REL), "{cd} vs {}", base_cd * 2.5);
    assert_eq!(clips::clips(&scaled)[WALK].duration, base_dur);
    let contact = clips::contact_track(&scaled, WALK, f2, None)
        .unwrap_or_else(|| panic!("no track"))
        .contact;
    assert_eq!(contact, base_contact, "contact labels must be scale-invariant");
    let phase = clips::phase_offset(&scaled, WALK, WALK_BACK, f2)
        .unwrap_or_else(|| panic!("no phase"));
    assert!(phase_dist(phase, base_phase) <= ONE_BIN, "{phase} vs {base_phase}");
}

// ── relation 2: uniform retime ───────────────────────────────────────────────────────────────────

/// Retiming every keyframe by k scales duration by k and stance speed by 1/k; the distance the
/// foot covers per cycle — a length, not a rate — must not move, and neither must phase.
#[test]
fn retiming_scales_duration_and_speed_but_not_distance() {
    let plain = valkyrie();
    let f = foot(&plain);
    let base_cd = clips::cycle_distance(&plain, WALK, f, None).unwrap_or_else(|| panic!("no cd"));
    let base_speed = clips::stance_speed(&plain, WALK, f, None).unwrap_or_else(|| panic!("no speed"));
    let base_dur = clips::clips(&plain)[WALK].duration;
    let base_phase = clips::phase_offset(&plain, WALK, WALK_BACK, f)
        .unwrap_or_else(|| panic!("no phase"));

    let mut slow = valkyrie();
    retime_clip(&mut slow, WALK, 2.0);
    retime_clip(&mut slow, WALK_BACK, 2.0);
    let f2 = foot(&slow);
    let dur = clips::clips(&slow)[WALK].duration;
    assert!((dur - base_dur * 2.0).abs() < 1.0e-4, "{dur} vs {}", base_dur * 2.0);
    let speed = clips::stance_speed(&slow, WALK, f2, None).unwrap_or_else(|| panic!("no speed"));
    assert!(rel_eq(speed, base_speed * 0.5, REL), "{speed} vs {}", base_speed * 0.5);
    let cd = clips::cycle_distance(&slow, WALK, f2, None).unwrap_or_else(|| panic!("no cd"));
    assert!(rel_eq(cd, base_cd, REL), "{cd} vs {base_cd}");
    let phase = clips::phase_offset(&slow, WALK, WALK_BACK, f2)
        .unwrap_or_else(|| panic!("no phase"));
    assert!(phase_dist(phase, base_phase) <= ONE_BIN, "{phase} vs {base_phase}");
}

// ── relation 3: yaw ──────────────────────────────────────────────────────────────────────────────

/// Turning the whole rig 90 degrees about +Y turns the measured travel with it and changes no
/// magnitude: (vx, vz) -> (vz, -vx), |v| and cycle distance fixed. The relation that de-risks the
/// next gaited rig authored on a different facing.
#[test]
fn a_yawed_rig_measures_the_same_gait_travelling_the_turned_direction() {
    let plain = valkyrie();
    let f = foot(&plain);
    let base = clips::contact_track(&plain, WALK, f, None).unwrap_or_else(|| panic!("no track"));
    let base_cd = clips::cycle_distance(&plain, WALK, f, None).unwrap_or_else(|| panic!("no cd"));

    let mut turned = valkyrie();
    let half = std::f32::consts::FRAC_PI_4; // 90 deg about Y as a quaternion: sin(45), cos(45)
    wrap_scene_root(&mut turned, [0.0; 3], [0.0, half.sin(), 0.0, half.cos()], [1.0; 3]);
    let f2 = foot(&turned);
    let t = clips::contact_track(&turned, WALK, f2, None).unwrap_or_else(|| panic!("no track"));
    let cd = clips::cycle_distance(&turned, WALK, f2, None).unwrap_or_else(|| panic!("no cd"));
    assert!(rel_eq(cd, base_cd, REL), "{cd} vs {base_cd}");
    let [vx, vz] = base.body_velocity;
    let [tx, tz] = t.body_velocity;
    // R_y(90): x' = z, z' = -x.
    assert!(
        (tx - vz).abs() < 1.0e-2 && (tz + vx).abs() < 1.0e-2,
        "({tx}, {tz}) vs rotated ({vz}, {})",
        -vx
    );
}

// ── relation 4: mirror ───────────────────────────────────────────────────────────────────────────

/// Mirroring the rig (x -> -x) negates the sideways component of travel and nothing else: the
/// heights, speeds, distances and contact labels are all isometry-invariant.
#[test]
fn a_mirrored_rig_negates_sideways_travel_only() {
    let plain = valkyrie();
    let f = foot(&plain);
    // The strafes carry the sideways travel worth mirroring.
    let base = clips::contact_track(&plain, 13, f, None).unwrap_or_else(|| panic!("no track"));
    let base_cd = clips::cycle_distance(&plain, 13, f, None).unwrap_or_else(|| panic!("no cd"));

    let mut mirrored = valkyrie();
    wrap_scene_root(&mut mirrored, [0.0; 3], [0.0, 0.0, 0.0, 1.0], [-1.0, 1.0, 1.0]);
    let f2 = foot(&mirrored);
    let m = clips::contact_track(&mirrored, 13, f2, None).unwrap_or_else(|| panic!("no track"));
    let cd = clips::cycle_distance(&mirrored, 13, f2, None).unwrap_or_else(|| panic!("no cd"));
    assert!(rel_eq(cd, base_cd, REL), "{cd} vs {base_cd}");
    assert!(
        (m.body_velocity[0] + base.body_velocity[0]).abs() < 1.0e-2,
        "{} vs {}",
        m.body_velocity[0],
        -base.body_velocity[0]
    );
    assert!(
        (m.body_velocity[1] - base.body_velocity[1]).abs() < 1.0e-2,
        "forward travel must survive a mirror"
    );
    assert_eq!(m.contact, base.contact, "contact labels are isometry-invariant");
}

// ── relation 5: reverse ──────────────────────────────────────────────────────────────────────────

/// Playing both clips backwards flips the travel and negates the phase lag between them.
#[test]
fn reversing_both_clips_flips_travel_and_negates_phase() {
    let plain = valkyrie();
    let f = foot(&plain);
    let base = clips::contact_track(&plain, WALK, f, None).unwrap_or_else(|| panic!("no track"));
    let base_phase = clips::phase_offset(&plain, WALK, WALK_BACK, f)
        .unwrap_or_else(|| panic!("no phase"));

    let mut rev = valkyrie();
    reverse_clip(&mut rev, WALK);
    reverse_clip(&mut rev, WALK_BACK);
    let f2 = foot(&rev);
    let t = clips::contact_track(&rev, WALK, f2, None).unwrap_or_else(|| panic!("no track"));
    assert!(
        (t.body_velocity[0] + base.body_velocity[0]).abs() < 1.0e-2
            && (t.body_velocity[1] + base.body_velocity[1]).abs() < 1.0e-2,
        "reversed travel {:?} vs -{:?}",
        t.body_velocity,
        base.body_velocity
    );
    let phase = clips::phase_offset(&rev, WALK, WALK_BACK, f2)
        .unwrap_or_else(|| panic!("no phase"));
    // Reversing both clips negates the correlation lag; wrap-aware, two bins of grace because
    // both curves were independently re-binned.
    assert!(
        phase_dist(phase, -base_phase) <= 2.0 * ONE_BIN,
        "{phase} vs {}",
        -base_phase
    );
}

// ── relation 6: duplicate cycle ──────────────────────────────────────────────────────────────────

/// A clip doubled end-to-end keeps its RATES (stance speed) while its per-clip-loop quantities
/// (duration, cycle distance — a per-loop length under this tool's definition) double; and a
/// doubled walk against a single-cycle reference is exactly what "phase ambiguous" exists to say.
#[test]
fn a_doubled_clip_doubles_per_loop_quantities_and_reads_ambiguous() {
    let plain = valkyrie();
    let f = foot(&plain);
    let base_dur = clips::clips(&plain)[WALK].duration;
    let base_speed = clips::stance_speed(&plain, WALK, f, None).unwrap_or_else(|| panic!("no speed"));
    let base_cd = clips::cycle_distance(&plain, WALK, f, None).unwrap_or_else(|| panic!("no cd"));

    let mut doubled = valkyrie();
    duplicate_cycle(&mut doubled, WALK);
    let f2 = foot(&doubled);
    let dur = clips::clips(&doubled)[WALK].duration;
    assert!((dur - base_dur * 2.0).abs() < 1.0e-3, "{dur} vs {}", base_dur * 2.0);
    // 128 bins now span two cycles, so the resample is twice as coarse: 2e-2, not 1e-3.
    let speed = clips::stance_speed(&doubled, WALK, f2, None).unwrap_or_else(|| panic!("no speed"));
    assert!(rel_eq(speed, base_speed, 2.0e-2), "{speed} vs {base_speed}");
    let cd = clips::cycle_distance(&doubled, WALK, f2, None).unwrap_or_else(|| panic!("no cd"));
    assert!(rel_eq(cd, base_cd * 2.0, 2.0e-2), "{cd} vs {}", base_cd * 2.0);
    // A clip that repeats inside its own cycle correlates with itself equally at lag 0 and at
    // half a cycle — the periodic-in-lag score landscape `ambiguous` exists to report. The
    // self-match is the cleanest positive-correlation reference (a cross-frequency pair can
    // score near zero, and the guard rightly refuses to call a zero-height landscape ambiguous).
    let m = clips::phase_match(&doubled, WALK, WALK, f2).unwrap_or_else(|| panic!("no match"));
    assert!(
        m.ambiguous,
        "a doubled cycle must read ambiguous against itself, not confidently aligned"
    );
    // The control: the single-cycle walk's self-match is NOT ambiguous.
    let m = clips::phase_match(&plain, WALK, WALK, f).unwrap_or_else(|| panic!("no match"));
    assert!(!m.ambiguous, "a single cycle must not read ambiguous against itself");
}

// ── relation 7: constant placement offset ────────────────────────────────────────────────────────

/// Where the rig stands in the scene must not matter: a constant offset moves nothing per-cycle,
/// so every measurement — including the in-place check, which measures a RANGE — is unchanged.
#[test]
fn a_constant_placement_offset_changes_no_measurement() {
    let plain = valkyrie();
    let f = foot(&plain);
    let root_node = clips::node_index(&plain, "Root").unwrap_or_else(|| panic!("no Root"));
    let base_cd = clips::cycle_distance(&plain, WALK, f, None).unwrap_or_else(|| panic!("no cd"));
    let base_phase = clips::phase_offset(&plain, WALK, WALK_BACK, f)
        .unwrap_or_else(|| panic!("no phase"));

    let mut moved = valkyrie();
    wrap_scene_root(&mut moved, [3.0, 0.0, -2.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
    let f2 = foot(&moved);
    let motion = clips::root_motion(&moved, WALK, root_node);
    assert!(
        motion.iter().all(|v| *v < rig_check::ROOT_MOTION_EPS),
        "a placement offset is not root motion: {motion:?}"
    );
    let cd = clips::cycle_distance(&moved, WALK, f2, None).unwrap_or_else(|| panic!("no cd"));
    assert!(rel_eq(cd, base_cd, REL), "{cd} vs {base_cd}");
    let phase = clips::phase_offset(&moved, WALK, WALK_BACK, f2)
        .unwrap_or_else(|| panic!("no phase"));
    assert!(phase_dist(phase, base_phase) <= ONE_BIN, "{phase} vs {base_phase}");
}

// ── relation 8: root drift ───────────────────────────────────────────────────────────────────────

/// A linear drift written onto the root's own translation channel IS root motion, and the policy
/// says so as the Bad "authored in place" finding. No cycle-distance claim here: the drift
/// velocity contaminates the stance cluster by construction.
#[test]
fn root_drift_fires_the_in_place_check() {
    let mut drifting = valkyrie();
    let root_node = clips::node_index(&drifting, "Root").unwrap_or_else(|| panic!("no Root"));
    // The ramp spans the root CHANNEL's own key range, which may end short of the clip's
    // duration (another channel can carry the last key).
    let span = add_root_ramp(&mut drifting, WALK, root_node, 0.5);
    let motion = clips::root_motion(&drifting, WALK, root_node);
    assert!(
        (motion[0] - 0.5 * span).abs() < 1.0e-3,
        "the ramp must read back as its own span: {motion:?} vs 0.5 x {span}"
    );
    let report = rig_check::check_rig(&drifting, &valkyrie_rig(), false);
    assert!(
        report
            .findings
            .iter()
            .any(|fd| fd.level == Level::Bad && fd.text.contains("authored in place")),
        "{:?}",
        report.findings
    );
}

// ── relation 9: renamed anchors ──────────────────────────────────────────────────────────────────

/// Renaming the anchor nodes must produce the two LOUD findings with candidate suggestions —
/// never a silent skip that looks like a pass. The regression test on the failure mode this
/// policy module exists to prevent.
#[test]
fn renamed_anchors_are_loud_never_silent() {
    let mut renamed = valkyrie();
    rename_node(&mut renamed, "Root", "Pelvis");
    rename_node(&mut renamed, "foot_l", "lf_foot");
    let report = rig_check::check_rig(&renamed, &valkyrie_rig(), false);
    let bad: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.level == Level::Bad)
        .collect();
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad[0].text.contains("Root"), "{}", bad[0].text);
    assert!(
        bad[1].text.contains("foot_l") && bad[1].text.contains("candidate contact joints:"),
        "{}",
        bad[1].text
    );
}

// ── relation 10: bilateral symmetry ──────────────────────────────────────────────────────────────

/// A biped's feet strike half a cycle apart — the untransformed invariant the multi-joint
/// machinery must reproduce: foot_r's stance onsets sit ~0.5 of a cycle from foot_l's on the
/// walk. (The relation phase-locking two gaits leans on, stated where it can fail loudly.)
#[test]
fn the_feet_strike_half_a_cycle_apart_on_the_walk() {
    let glb = valkyrie();
    let fl = foot(&glb);
    let fr = clips::node_index(&glb, "foot_r").unwrap_or_else(|| panic!("no foot_r"));
    let onset_of = |joint: usize| -> f32 {
        let t = clips::contact_track(&glb, WALK, joint, None)
            .unwrap_or_else(|| panic!("no track for joint {joint}"));
        let onsets = clips::stance_onsets(&t);
        assert_eq!(onsets.len(), 1, "one footstrike per walk cycle, got {onsets:?}");
        onsets[0] as f32 / t.bins as f32
    };
    let gap = phase_dist(onset_of(fl), onset_of(fr));
    assert!(
        (gap - 0.5).abs() <= 0.06,
        "footstrikes {gap} of a cycle apart; a biped walk alternates at 0.5"
    );
    // And the multi-joint scorer agrees with the single-joint one on an unambiguous pair — the
    // wrapper and the set form are the same path, so this pins their equivalence.
    let single = clips::phase_match(&glb, WALK, WALK_BACK, fl)
        .unwrap_or_else(|| panic!("no single match"));
    let pair = clips::phase_match_joints(&glb, WALK, WALK_BACK, &[fl, fr])
        .unwrap_or_else(|| panic!("no pair match"));
    assert!(
        phase_dist(single.offset, pair.offset) <= 2.0 * ONE_BIN,
        "single {} vs pair {}",
        single.offset,
        pair.offset
    );
}

// ── the transform helpers ────────────────────────────────────────────────────────────────────────

/// Append a new scene root carrying `t`/`r`/`s`, adopting every currently-parentless node. The FK
/// in `clips.rs` composes ancestor rest TRS, so this one node applies a rigid transform (or scale)
/// to every measurement without touching a single keyframe. Append-only: indices stay valid.
fn wrap_scene_root(glb: &mut Glb, t: [f32; 3], r: [f32; 4], s: [f32; 3]) {
    let nodes = glb.json["nodes"].as_array().cloned().unwrap_or_default();
    let mut is_child = vec![false; nodes.len()];
    for n in &nodes {
        if let Some(kids) = n["children"].as_array() {
            for k in kids.iter().filter_map(serde_json::Value::as_u64) {
                if let Some(slot) = is_child.get_mut(k as usize) {
                    *slot = true;
                }
            }
        }
    }
    let roots: Vec<usize> = (0..nodes.len()).filter(|i| !is_child[*i]).collect();
    let new_node = serde_json::json!({
        "name": "MetaRoot",
        "translation": t,
        "rotation": r,
        "scale": s,
        "children": roots,
    });
    if let Some(arr) = glb.json["nodes"].as_array_mut() {
        arr.push(new_node);
    }
}

/// The byte layout `clips.rs` reads: `bufferView.byteOffset + accessor.byteOffset`, tightly
/// packed f32-LE unless the view carries a stride.
fn accessor_layout(glb: &Glb, ix: usize, width: usize) -> (usize, usize, usize) {
    let acc = &glb.json["accessors"][ix];
    let count = acc["count"].as_u64().unwrap_or(0) as usize;
    let view = &glb.json["bufferViews"][acc["bufferView"].as_u64().unwrap_or(0) as usize];
    let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"].as_u64().unwrap_or(0) as usize;
    let stride = if stride == 0 { width * 4 } else { stride };
    (base, stride, count)
}

fn read_rows(glb: &Glb, ix: usize, width: usize) -> Vec<Vec<f32>> {
    let (base, stride, count) = accessor_layout(glb, ix, width);
    (0..count)
        .map(|i| {
            (0..width)
                .map(|c| {
                    let at = base + i * stride + c * 4;
                    let b = &glb.bin[at..at + 4];
                    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
                })
                .collect()
        })
        .collect()
}

/// An output accessor's element width, off its glTF `type`.
fn accessor_width(glb: &Glb, ix: usize) -> usize {
    match glb.json["accessors"][ix]["type"].as_str() {
        Some("VEC4") => 4,
        Some("SCALAR") => 1,
        _ => 3,
    }
}

/// Rewrite one clip's sampler data through `f`, repointing the samplers at freshly appended
/// accessors — the asset SHARES time accessors across clips (the valkyrie's clip 5 and clip 18
/// share one), so mutating in place would silently corrupt a neighbour. `f` maps
/// `(times, values)` to their replacements; deduplicated per accessor pair so shared-within-clip
/// inputs transform once.
fn repoint_clip_samplers(
    glb: &mut Glb,
    clip: usize,
    f: impl Fn(Vec<Vec<f32>>, Vec<Vec<f32>>) -> (Vec<Vec<f32>>, Vec<Vec<f32>>),
) {
    let samplers = glb.json["animations"][clip]["samplers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut input_map: Vec<(usize, usize)> = Vec::new();
    let mut new_samplers = Vec::new();
    for s in &samplers {
        let mut ns = s.clone();
        let (Some(input), Some(output)) = (
            s["input"].as_u64().map(|v| v as usize),
            s["output"].as_u64().map(|v| v as usize),
        ) else {
            new_samplers.push(ns);
            continue;
        };
        let times = read_rows(glb, input, 1);
        let values = read_rows(glb, output, accessor_width(glb, output));
        let (new_times, new_values) = f(times, values);
        let new_input = match input_map.iter().find(|(o, _)| *o == input) {
            Some((_, n)) => *n,
            None => {
                let n = append_accessor(glb, &new_times);
                input_map.push((input, n));
                n
            }
        };
        let new_output = append_accessor(glb, &new_values);
        ns["input"] = serde_json::json!(new_input);
        ns["output"] = serde_json::json!(new_output);
        new_samplers.push(ns);
    }
    glb.json["animations"][clip]["samplers"] = serde_json::json!(new_samplers);
}

/// Multiply every keyframe time of a clip by `k`. Durations follow, because `clips()` reads them
/// off the (freshly written) input accessor `max`.
fn retime_clip(glb: &mut Glb, clip: usize, k: f32) {
    repoint_clip_samplers(glb, clip, |times, values| {
        (
            times.into_iter().map(|r| vec![r[0] * k]).collect(),
            values,
        )
    });
}

/// Reverse a clip in time: times map to `T − t` (re-ascending), outputs reverse row order.
fn reverse_clip(glb: &mut Glb, clip: usize) {
    let duration = clips::clips(glb)[clip].duration;
    repoint_clip_samplers(glb, clip, |times, mut values| {
        let mut new_times: Vec<Vec<f32>> =
            times.into_iter().map(|r| vec![duration - r[0]]).collect();
        new_times.reverse();
        values.reverse();
        (new_times, values)
    });
}

/// Append a doubled copy of every sampler's data: times `[0..T, T..2T]`, outputs repeated. New
/// accessors and views point into fresh bytes at the end of `bin`; only this clip's samplers are
/// repointed, so sharing cannot leak.
fn duplicate_cycle(glb: &mut Glb, clip: usize) {
    let duration = clips::clips(glb)[clip].duration;
    repoint_clip_samplers(glb, clip, |times, values| {
        let mut new_times = times.clone();
        let mut new_values = values.clone();
        for (t, v) in times.iter().zip(&values) {
            // Skip an exact duplicate of the seam key.
            if t[0] <= 1.0e-6 {
                continue;
            }
            new_times.push(vec![t[0] + duration]);
            new_values.push(v.clone());
        }
        (new_times, new_values)
    });
}

/// Rewrite the clip's root-translation channel as `original + ramp(t)` along X, repointing that
/// one sampler's output at fresh bytes. Returns the channel's own key span — the range the ramp
/// covers, which can end short of the clip's duration.
fn add_root_ramp(glb: &mut Glb, clip: usize, node: usize, drift_per_sec: f32) -> f32 {
    let anim = glb.json["animations"][clip].clone();
    let channels = anim["channels"].as_array().cloned().unwrap_or_default();
    let samplers = anim["samplers"].as_array().cloned().unwrap_or_default();
    let at = channels
        .iter()
        .position(|c| {
            c["target"]["node"].as_u64() == Some(node as u64)
                && c["target"]["path"].as_str() == Some("translation")
        })
        .unwrap_or_else(|| panic!("the root has no translation channel to drift"));
    let sampler_ix = channels[at]["sampler"].as_u64().unwrap_or(0) as usize;
    let input = samplers[sampler_ix]["input"].as_u64().unwrap_or(0) as usize;
    let output = samplers[sampler_ix]["output"].as_u64().unwrap_or(0) as usize;
    let times = read_rows(glb, input, 1);
    let values = read_rows(glb, output, 3);
    let ramped: Vec<Vec<f32>> = times
        .iter()
        .zip(&values)
        .map(|(t, v)| vec![v[0] + drift_per_sec * t[0], v[1], v[2]])
        .collect();
    let new_output = append_accessor(glb, &ramped);
    glb.json["animations"][clip]["samplers"][sampler_ix]["output"] =
        serde_json::json!(new_output);
    let span = times.last().map_or(0.0, |t| t[0]) - times.first().map_or(0.0, |t| t[0]);
    span
}

/// Append rows as a fresh FLOAT accessor + bufferView over bytes appended to `bin`.
fn append_accessor(glb: &mut Glb, rows: &[Vec<f32>]) -> usize {
    let width = rows.first().map_or(1, Vec::len);
    let offset = glb.bin.len();
    for row in rows {
        for v in row {
            glb.bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let views = glb.json["bufferViews"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("no bufferViews"));
    views.push(serde_json::json!({
        "byteOffset": offset,
        "byteLength": rows.len() * width * 4,
    }));
    let view_ix = views.len() - 1;
    let min: Vec<f32> = (0..width)
        .map(|c| rows.iter().map(|r| r[c]).fold(f32::MAX, f32::min))
        .collect();
    let max: Vec<f32> = (0..width)
        .map(|c| rows.iter().map(|r| r[c]).fold(f32::MIN, f32::max))
        .collect();
    let kind = match width {
        1 => "SCALAR",
        3 => "VEC3",
        _ => "VEC4",
    };
    let accessors = glb.json["accessors"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("no accessors"));
    accessors.push(serde_json::json!({
        "bufferView": view_ix,
        "componentType": 5126,
        "count": rows.len(),
        "type": kind,
        "min": min,
        "max": max,
    }));
    accessors.len() - 1
}

fn rename_node(glb: &mut Glb, from: &str, to: &str) {
    let Some(nodes) = glb.json["nodes"].as_array_mut() else {
        return;
    };
    for n in nodes {
        if n["name"].as_str() == Some(from) {
            n["name"] = serde_json::json!(to);
        }
    }
}
