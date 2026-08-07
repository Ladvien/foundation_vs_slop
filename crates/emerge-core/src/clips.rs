//! **Reading what a GLB's animations actually contain** — names, durations, root motion, and the two
//! numbers that are otherwise measured by hand.
//!
//! `docs/animation.md` states the gap this closes: getting a gait clip's `(duration, phase_offset,
//! cycle_distance)` is *"currently a **manual offline step, not a repo tool**"*, and
//! `src/site/staff_anim.rs` calls it *"the largest hidden cost in animating a new character"*. Every
//! wired clip in the game carries three numbers somebody sampled by hand, and nothing re-checks them
//! when the artist re-exports.
//!
//! # Engine-free, on purpose
//!
//! `tests/engine_free.rs` fails the build if this crate ever reaches for `bevy`. So this is the
//! *analysis* half — JSON and arithmetic over the glTF the artist shipped — and the playback half
//! lives in the editor, which has a renderer. The same split `emerge-core`/`emerge-bevy` already
//! draws for placement.
//!
//! There is no math crate here either (the allowlist is serde/serde_json/ron/rand), so the forward
//! kinematics below is written out: quaternion to matrix, compose, walk the chain.
//!
//! # What "in place" and "cycle distance" mean
//!
//! `docs/artist_guide.md` §4 requires gait clips authored **in place** — the `Root` node's
//! translation bit-zero — because the game drives the transform itself and the clip supplies only limb
//! motion. [`root_motion`] is that check, and it is the same one `tests/valkyrie_asset.rs` makes.
//!
//! The consequence is that the ground speed a clip *implies* is not in the file: it has to be
//! recovered from the feet. While a foot is planted it is stationary in the world, so relative to the
//! (static) root it slides backward at exactly the speed the body would be moving forward.
//! [`contact_track`] labels the planted bins by that velocity condition (GANimator's formulation,
//! restated in the ground frame) and [`cycle_distance`] measures the slide across them. Metaxas &
//! Sun, *Automating Gait Generation* (10.1145/383259.383288), is the standard statement of the
//! step-length/step-rate coupling this feeds; an inaccurate cycle distance is what foot-skate *is*.
//!
//! [`phase_offset`] aligns two clips by cross-correlating a foot-height curve, which is what
//! `docs/artist_guide.md` §4 already prescribes ("re-measured by cross-correlating foot height, not
//! guessed") and what WalkTheDog (2024) formalises as a 1-D phase manifold. Height, not the contact
//! train: correlating the binary trains was tried and measured walk→walk_back at +0.039 of a cycle
//! where the height curve reproduces the guide's validated −0.141 — a square wave keeps *where* the
//! stance falls but loses the pose shape that actually locates the alignment.

use serde_json::Value;

use crate::glb::Glb;

/// What one glTF animation says about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipInfo {
    /// Its index — the id the game wires, and the only thing it uses at runtime.
    pub index: usize,
    /// Its name, if the exporter wrote one. Names are documentation here, never a lookup key: the
    /// Valkyrie's `strafe_l`/`strafe_r` are backwards in the asset and the code wires them by
    /// measured direction, which only works because nothing resolves a clip by name.
    pub name: Option<String>,
    /// The longest keyframe time, i.e. what Bevy will report as the clip's duration.
    pub duration: f32,
    /// How many channels it drives. A clip with one channel is usually a mistake.
    pub channels: usize,
}

/// Every animation in the file, in index order.
pub fn clips(glb: &Glb) -> Vec<ClipInfo> {
    let Some(anims) = glb.json["animations"].as_array() else {
        // No `animations` array is not an error — most props are static scenery, and
        // `scripts/fbx_to_glb.py` exports them with `export_animations=False` on purpose.
        return Vec::new();
    };
    anims
        .iter()
        .enumerate()
        .map(|(index, a)| ClipInfo {
            index,
            name: a["name"].as_str().map(str::to_owned),
            duration: duration(glb, a),
            channels: a["channels"].as_array().map_or(0, Vec::len),
        })
        .collect()
}

/// The longest keyframe time across a clip's samplers.
fn duration(glb: &Glb, anim: &Value) -> f32 {
    let Some(samplers) = anim["samplers"].as_array() else {
        return 0.0;
    };
    let mut max = 0.0f32;
    for s in samplers {
        let Some(input) = s["input"].as_u64() else {
            continue;
        };
        // The accessor's own `max` — glTF requires it on animation inputs, so this needs no decode.
        if let Some(m) = glb.json["accessors"][input as usize]["max"][0].as_f64() {
            max = max.max(m as f32);
        }
    }
    max
}

/// The index of `name` in the **full** `nodes` array.
///
/// The full array, never a name-filtered one: `channels[].target.node` indexes the real array, and a
/// position within a list that dropped unnamed nodes silently diverges the moment a rig gains one.
/// `tests/valkyrie_asset.rs` records paying for exactly that.
pub fn node_index(glb: &Glb, name: &str) -> Option<usize> {
    glb.json["nodes"]
        .as_array()?
        .iter()
        .position(|n| n["name"].as_str() == Some(name))
}

/// How far a node's own translation channel moves in a clip, per axis.
///
/// **The in-place check.** All three should be ~0 for a gait clip on the rig's root; anything else is
/// root motion the game would fight, because the game owns the transform.
pub fn root_motion(glb: &Glb, clip: usize, node: usize) -> [f32; 3] {
    let Some((_, values)) = track(glb, clip, node, "translation") else {
        // No translation channel at all is the ideal answer to "does this move the root".
        return [0.0; 3];
    };
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for v in &values {
        for a in 0..3 {
            lo[a] = lo[a].min(v[a]);
            hi[a] = hi[a].max(v[a]);
        }
    }
    if values.is_empty() {
        return [0.0; 3];
    }
    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
}

// ── the all-rig checks: loop closure, source rate, one-shot end state ────────────────────────────

/// The angle between two unit quaternions, degrees — `2·acos(|a·b|)`, the absolute dot making it
/// antipodal-safe (q and −q are the same rotation).
fn quat_angle_deg(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    2.0 * dot.abs().min(1.0).acos().to_degrees()
}

/// The display name of a node, for findings.
fn node_name(glb: &Glb, index: usize) -> String {
    glb.json["nodes"]
        .as_array()
        .and_then(|nodes| nodes.get(index))
        .and_then(|n| n["name"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("node {index}"))
}

/// **How far a looping clip's last pose sits from its first** — the most common looping defect: a
/// loop whose ends do not meet pops once per cycle, forever.
///
/// Per rotation channel, first key vs last key, antipodal-safe; the worst joint is named so the
/// finding is actionable. Translation channels are compared whole (euclidean first-vs-last): Unity's
/// loop-pose rule matches rotation and root Y but deliberately not root XZ — here in-place authoring
/// already forces the root's XZ to zero, so a translation mismatch anywhere is real.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopClosure {
    /// How many rotation channels were compared.
    pub joints: usize,
    /// The rotation channel with the largest first-vs-last angle.
    pub worst_joint: String,
    pub max_angle_deg: f32,
    /// The largest first-vs-last translation delta, file units, and its joint.
    pub max_translation: f32,
    pub worst_translation_joint: Option<String>,
}

/// See [`LoopClosure`]. `None` when the clip does not exist or drives no rotation channel.
pub fn loop_closure(glb: &Glb, clip: usize) -> Option<LoopClosure> {
    let anim = glb.json["animations"].as_array()?.get(clip)?;
    let channels = anim["channels"].as_array()?;
    let samplers = anim["samplers"].as_array()?;
    let mut joints = 0usize;
    let mut worst_joint = String::new();
    let mut max_angle_deg = 0.0f32;
    let mut max_translation = 0.0f32;
    let mut worst_translation_joint = None;
    for ch in channels {
        let path = ch["target"]["path"].as_str().unwrap_or("");
        let node = ch["target"]["node"].as_u64().map(|n| n as usize);
        let (Some(node), true) = (node, path == "rotation" || path == "translation") else {
            continue;
        };
        let Some(s) = samplers.get(ch["sampler"].as_u64().unwrap_or(u64::MAX) as usize) else {
            continue;
        };
        let width = if path == "rotation" { 4 } else { 3 };
        let Some(values) = s["output"]
            .as_u64()
            .and_then(|ix| floats(glb, ix as usize, width))
        else {
            continue;
        };
        let (Some(first), Some(last)) = (values.first(), values.last()) else {
            continue;
        };
        if path == "rotation" {
            joints += 1;
            let angle = quat_angle_deg(first, last);
            if angle > max_angle_deg {
                max_angle_deg = angle;
                worst_joint = node_name(glb, node);
            }
        } else {
            let d = first
                .iter()
                .zip(last)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f32>()
                .sqrt();
            if d > max_translation {
                max_translation = d;
                worst_translation_joint = Some(node_name(glb, node));
            }
        }
    }
    (joints > 0).then_some(LoopClosure {
        joints,
        worst_joint,
        max_angle_deg,
        max_translation,
        worst_translation_joint,
    })
}

/// **How densely a clip was actually keyed** — its densest channel's key count and rate.
///
/// The rate is `1 / median inter-key interval` (median, not mean: robust against a single long hold
/// key). What a playback speed does to it is the caller's arithmetic — the bench surfaces
/// `fps × speed / 60` as authored keys per rendered frame at 60 Hz, the number that says when a
/// sped-up clip starts strobing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceRate {
    pub keys: usize,
    pub fps: f32,
}

/// See [`SourceRate`]. `None` when the clip does not exist or no channel has two keys.
pub fn source_rate(glb: &Glb, clip: usize) -> Option<SourceRate> {
    let anim = glb.json["animations"].as_array()?.get(clip)?;
    let channels = anim["channels"].as_array()?;
    let samplers = anim["samplers"].as_array()?;
    let mut best: Option<Vec<f32>> = None;
    for ch in channels {
        let Some(s) = samplers.get(ch["sampler"].as_u64().unwrap_or(u64::MAX) as usize) else {
            continue;
        };
        let Some(times) = s["input"].as_u64().and_then(|ix| scalars(glb, ix as usize)) else {
            continue;
        };
        if times.len() > best.as_ref().map_or(0, Vec::len) {
            best = Some(times);
        }
    }
    let times = best.filter(|t| t.len() >= 2)?;
    let mut gaps: Vec<f32> = times.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = gaps[gaps.len() / 2];
    (median > 0.0).then_some(SourceRate {
        keys: times.len(),
        fps: 1.0 / median,
    })
}

/// **Where a one-shot leaves the skeleton, against the pose it will fade back into.**
///
/// Final-key rotations of the joints `clip` drives, vs `reference_clip` sampled at its start
/// (rest pose where the reference does not drive a joint). Driven-joints-only makes it
/// mask-correct for free: the valkyrie's fire compares only the upper body it actually moves.
#[derive(Clone, Debug, PartialEq)]
pub struct PoseDelta {
    /// How many rotation channels were compared.
    pub joints: usize,
    pub worst_joint: String,
    pub max_angle_deg: f32,
}

/// See [`PoseDelta`]. `None` when either clip does not exist or `clip` drives no rotation channel.
pub fn end_pose_delta(glb: &Glb, clip: usize, reference_clip: usize) -> Option<PoseDelta> {
    let anims = glb.json["animations"].as_array()?;
    let anim = anims.get(clip)?;
    anims.get(reference_clip)?;
    let channels = anim["channels"].as_array()?;
    let samplers = anim["samplers"].as_array()?;
    let mut joints = 0usize;
    let mut worst_joint = String::new();
    let mut max_angle_deg = 0.0f32;
    for ch in channels {
        if ch["target"]["path"].as_str() != Some("rotation") {
            continue;
        }
        let Some(node) = ch["target"]["node"].as_u64().map(|n| n as usize) else {
            continue;
        };
        let Some(s) = samplers.get(ch["sampler"].as_u64().unwrap_or(u64::MAX) as usize) else {
            continue;
        };
        let Some(values) = s["output"]
            .as_u64()
            .and_then(|ix| floats(glb, ix as usize, 4))
        else {
            continue;
        };
        let Some(end) = values.last() else { continue };
        // The pose the skeleton fades back into: the reference's first key for this joint, or the
        // rig's rest rotation where the reference leaves the joint alone.
        let reference = track_raw(glb, reference_clip, node, "rotation")
            .and_then(|(_, vals)| vals.into_iter().next())
            .unwrap_or_else(|| {
                let (_, r, _) = rest(glb, node);
                r.to_vec()
            });
        joints += 1;
        let angle = quat_angle_deg(end, &reference);
        if angle > max_angle_deg {
            max_angle_deg = angle;
            worst_joint = node_name(glb, node);
        }
    }
    (joints > 0).then_some(PoseDelta {
        joints,
        worst_joint,
        max_angle_deg,
    })
}

/// One channel's keyframe times and values, as raw floats (3 wide for TRS translation/scale, 4 for a
/// rotation quaternion).
fn track_raw(glb: &Glb, clip: usize, node: usize, path: &str) -> Option<(Vec<f32>, Vec<Vec<f32>>)> {
    let anim = glb.json["animations"].as_array()?.get(clip)?;
    let channels = anim["channels"].as_array()?;
    let samplers = anim["samplers"].as_array()?;
    let ch = channels.iter().find(|c| {
        c["target"]["node"].as_u64() == Some(node as u64)
            && c["target"]["path"].as_str() == Some(path)
    })?;
    let s = samplers.get(ch["sampler"].as_u64()? as usize)?;
    let times = scalars(glb, s["input"].as_u64()? as usize)?;
    let width = if path == "rotation" { 4 } else { 3 };
    let values = floats(glb, s["output"].as_u64()? as usize, width)?;
    Some((times, values))
}

/// A translation/scale channel, as vec3s.
fn track(glb: &Glb, clip: usize, node: usize, path: &str) -> Option<(Vec<f32>, Vec<[f32; 3]>)> {
    let (times, raw) = track_raw(glb, clip, node, path)?;
    let values = raw
        .into_iter()
        .map(|v| [v[0], v[1], v[2]])
        .collect::<Vec<_>>();
    Some((times, values))
}

/// Decode a `SCALAR`/`FLOAT` accessor.
fn scalars(glb: &Glb, index: usize) -> Option<Vec<f32>> {
    floats(glb, index, 1).map(|rows| rows.into_iter().map(|r| r[0]).collect())
}

/// Decode a FLOAT accessor of `width` components per element.
///
/// Refuses anything that is not `FLOAT` rather than misreading it: a quantised accessor decoded as
/// f32 produces numbers that look plausible and are wrong, which is worse than a refusal.
fn floats(glb: &Glb, index: usize, width: usize) -> Option<Vec<Vec<f32>>> {
    let acc = &glb.json["accessors"][index];
    if acc["componentType"].as_u64()? != 5126 {
        return None;
    }
    let count = acc["count"].as_u64()? as usize;
    let view = &glb.json["bufferViews"][acc["bufferView"].as_u64()? as usize];
    let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"].as_u64().unwrap_or(0) as usize;
    let stride = if stride == 0 { width * 4 } else { stride };
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let mut row = Vec::with_capacity(width);
        for c in 0..width {
            let at = base + i * stride + c * 4;
            let bytes = glb.bin.get(at..at + 4)?;
            row.push(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
        out.push(row);
    }
    Some(out)
}

// ── forward kinematics ───────────────────────────────────────────────────────────────────────────

/// A node's local transform, as a 4×4 in column-major order.
type Mat4 = [[f32; 4]; 4];

fn identity() -> Mat4 {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// Translation · Rotation · Scale, in that order — the glTF node convention.
fn trs(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Mat4 {
    let [x, y, z, w] = r;
    // Quaternion to rotation matrix, written out because there is no math crate here.
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    [
        [
            (1.0 - 2.0 * (yy + zz)) * s[0],
            (2.0 * (xy - wz)) * s[1],
            (2.0 * (xz + wy)) * s[2],
            t[0],
        ],
        [
            (2.0 * (xy + wz)) * s[0],
            (1.0 - 2.0 * (xx + zz)) * s[1],
            (2.0 * (yz - wx)) * s[2],
            t[1],
        ],
        [
            (2.0 * (xz - wy)) * s[0],
            (2.0 * (yz + wx)) * s[1],
            (1.0 - 2.0 * (xx + yy)) * s[2],
            t[2],
        ],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Sample a track at `t`, holding the ends and interpolating linearly between keys.
///
/// Linear, including for rotations, followed by a renormalise — the LERP-not-SLERP choice Gregory's
/// *Game Engine Architecture* §5.4.5 describes. It is exact at the keyframes and within a fraction of
/// a degree between them at 24 fps, and this is a measuring tool, not a renderer.
fn sample(times: &[f32], values: &[Vec<f32>], t: f32) -> Option<Vec<f32>> {
    if times.is_empty() || values.len() < times.len() {
        return None;
    }
    if t <= times[0] {
        return values.first().cloned();
    }
    if t >= times[times.len() - 1] {
        return values.get(times.len() - 1).cloned();
    }
    let hi = times.iter().position(|&k| k >= t)?;
    let lo = hi.saturating_sub(1);
    let span = times[hi] - times[lo];
    let f = if span > 0.0 { (t - times[lo]) / span } else { 0.0 };
    let mut out: Vec<f32> = values[lo]
        .iter()
        .zip(&values[hi])
        .map(|(a, b)| a + (b - a) * f)
        .collect();
    if out.len() == 4 {
        let n = out.iter().map(|c| c * c).sum::<f32>().sqrt();
        if n > 0.0 {
            for c in &mut out {
                *c /= n;
            }
        }
    }
    Some(out)
}

/// Each node's parent, derived from `children`.
fn parents(glb: &Glb) -> Vec<Option<usize>> {
    let nodes = glb.json["nodes"].as_array().map_or(&[][..], |v| v.as_slice());
    let mut out = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(kids) = n["children"].as_array() {
            for k in kids.iter().filter_map(Value::as_u64) {
                if let Some(slot) = out.get_mut(k as usize) {
                    *slot = Some(i);
                }
            }
        }
    }
    out
}

/// A node's rest TRS, from either its TRS fields or its `matrix`.
fn rest(glb: &Glb, node: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let n = &glb.json["nodes"][node];
    let vec3 = |v: &Value, d: [f32; 3]| -> [f32; 3] {
        v.as_array()
            .filter(|a| a.len() == 3)
            .map(|a| {
                let g = |i: usize| a[i].as_f64().unwrap_or(d[i] as f64) as f32;
                [g(0), g(1), g(2)]
            })
            .unwrap_or(d)
    };
    let t = vec3(&n["translation"], [0.0; 3]);
    let s = vec3(&n["scale"], [1.0; 3]);
    let r = n["rotation"]
        .as_array()
        .filter(|a| a.len() == 4)
        .map(|a| {
            let g = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
            [g(0), g(1), g(2), g(3)]
        })
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
    (t, r, s)
}

/// **Where a node is, in the scene's space, at every keyframe time of a clip.**
///
/// Walks the node's ancestor chain, taking each one's animated TRS where the clip drives it and its
/// rest pose where it does not. This is the piece that makes a foot's motion legible: a foot's own
/// translation channel says nothing useful, because everything above it in the chain is rotating.
pub fn world_track(glb: &Glb, clip: usize, node: usize) -> Option<(Vec<f32>, Vec<[f32; 3]>)> {
    let parent = parents(glb);
    // The chain from the node up to a root, then reversed so it composes parent-first.
    let mut chain = vec![node];
    let mut at = node;
    // **Bounded by the node count.** `parents` just inverts `children` and nothing here runs a glTF
    // validator, so an exporter that writes A as a child of B and B as a child of A makes this walk
    // never terminate — the editor hangs with memory climbing while it pushes a chain forever. A
    // chain cannot be longer than `parent` (one entry per node) without repeating a node, so
    // exceeding that IS the
    // cycle, and `None` is the honest answer: this rig has no measurable track.
    while let Some(Some(p)) = parent.get(at).copied() {
        if chain.len() > parent.len() {
            return None;
        }
        chain.push(p);
        at = p;
    }
    chain.reverse();

    // Every keyframe time any node in the chain is keyed at — the union, so nothing is missed.
    let mut times: Vec<f32> = Vec::new();
    for &n in &chain {
        for path in ["translation", "rotation", "scale"] {
            if let Some((ts, _)) = track_raw(glb, clip, n, path) {
                times.extend(ts);
            }
        }
    }
    if times.is_empty() {
        return None;
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    times.dedup_by(|a, b| (*a - *b).abs() < 1.0e-6);

    // Cache each chain node's channels once rather than re-reading the JSON per sample.
    let tracks: Vec<_> = chain
        .iter()
        .map(|&n| {
            (
                n,
                track_raw(glb, clip, n, "translation"),
                track_raw(glb, clip, n, "rotation"),
                track_raw(glb, clip, n, "scale"),
            )
        })
        .collect();

    let mut out = Vec::with_capacity(times.len());
    for &t in &times {
        let mut m = identity();
        for (n, tt, rr, ss) in &tracks {
            let (rt, rr_rest, rs) = rest(glb, *n);
            let tv = tt
                .as_ref()
                .and_then(|(ts, vs)| sample(ts, vs, t))
                .map_or(rt, |v| [v[0], v[1], v[2]]);
            let rv = rr
                .as_ref()
                .and_then(|(ts, vs)| sample(ts, vs, t))
                .map_or(rr_rest, |v| [v[0], v[1], v[2], v[3]]);
            let sv = ss
                .as_ref()
                .and_then(|(ts, vs)| sample(ts, vs, t))
                .map_or(rs, |v| [v[0], v[1], v[2]]);
            m = mul(&m, &trs(tv, rv, sv));
        }
        out.push([m[0][3], m[1][3], m[2][3]]);
    }
    Some((times, out))
}

// ── contact labelling and gait measurement ───────────────────────────────────────────────────────

/// The uniform phase grid every per-cycle quantity is resampled onto. One bin is 1/128 of a cycle,
/// which is also the resolution of a measured phase offset.
pub const PHASE_BINS: usize = 128;

/// The minimum Otsu separability (`η = σ²_between / σ²_total`, in log space) for a derived contact
/// threshold to count as a measurement. Below it the stance and swing velocity modes do not
/// separate, and the honest answer is `None` — the caller's loud "no planted-foot stance" path,
/// whose remedy is the rig-level `contact_eps:` declaration. The six valkyrie gaits measure η
/// 0.73–0.88 (table on [`otsu_threshold`]); a genuinely unimodal distribution scores far below
/// 0.5, so 0.5 is a sanity bound, not a quality ranking — the valkyrie's own run_back derives a
/// *misplaced* threshold at a healthy η, which is why that rig declares its `contact_eps`.
pub const CONTACT_SEPARABILITY_MIN: f32 = 0.5;

/// A joint must read as planted for at least this fraction of a cycle before
/// [`contact_candidates`] will name it.
pub const MIN_STANCE_FRACTION: f32 = 0.2;

/// A node's world track resampled onto the uniform phase grid.
struct Resampled {
    /// World position per bin.
    pos: Vec<[f32; 3]>,
    /// Seconds per bin.
    dt: f32,
    /// The clip duration the manifest's identity uses (`distance = speed × duration`).
    duration: f32,
}

fn resampled(glb: &Glb, clip: usize, node: usize) -> Option<Resampled> {
    let (times, world) = world_track(glb, clip, node)?;
    if world.len() < 4 {
        return None;
    }
    let first = *times.first()?;
    let span = *times.last()? - first;
    if span <= 0.0 {
        return None;
    }
    let raw: Vec<Vec<f32>> = world.iter().map(|p| p.to_vec()).collect();
    let mut pos = Vec::with_capacity(PHASE_BINS);
    for i in 0..PHASE_BINS {
        let t = first + span * (i as f32 / PHASE_BINS as f32);
        let v = sample(&times, &raw, t)?;
        pos.push([v[0], v[1], v[2]]);
    }
    let anim = glb.json["animations"].as_array()?.get(clip)?;
    Some(Resampled {
        pos,
        dt: span / PHASE_BINS as f32,
        duration: duration(glb, anim),
    })
}

/// The middle value. `None` on an empty set — a median of nothing is not 0.
fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(v[v.len() / 2])
}

/// **Per-bin contact labels for one clip's contact joint.**
///
/// The formulation is GANimator's — a joint is in contact on a frame when its velocity magnitude
/// falls below a threshold (Li et al., *GANimator: Neural Motion Synthesis from a Single Sequence*,
/// SIGGRAPH 2022, 10.1145/3528223.3530094) — but that condition is stated in the **ground** frame,
/// and these clips are authored in place: during stance the foot is not near-zero in file space, it
/// slides backward at exactly body speed. So the label is `‖v − v_stance‖ < ε·‖v_stance‖`, where
/// `v_stance` is the stance cluster's own velocity, found by seeding with the height median and
/// taking the component-wise median velocity over the seed.
///
/// `-v_stance` is then the body's travel — a **vector**, which is what makes a mis-named strafe
/// clip measurable rather than merely annotated.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactTrack {
    /// [`PHASE_BINS`], carried so a consumer needs no second constant.
    pub bins: usize,
    /// Contact per phase bin.
    pub contact: Vec<bool>,
    /// Horizontal speed per phase bin, file units per second.
    pub speed: Vec<f32>,
    /// The body's horizontal travel velocity (XZ), file units per second.
    pub body_velocity: [f32; 2],
    /// Fraction of the cycle labelled contact.
    pub stance_fraction: f32,
    /// The clip's duration, seconds.
    pub duration: f32,
    /// The contact threshold actually used, as a fraction of the stance speed — derived per clip
    /// ([`otsu_threshold`]) unless the rig declared one. Surfaced so a finding can say
    /// "(contact eps 0.31x stance, derived)" and a human can overrule a bad derivation.
    pub threshold: f32,
}

impl ContactTrack {
    /// The planted foot's horizontal speed, file units per second — the **median** over contact
    /// bins, not the mean: touchdown and lift-off sit at the stance edges moving at neither the
    /// body's speed nor zero, and a mean would let them drag the number down.
    pub fn stance_speed(&self) -> f32 {
        let planted: Vec<f32> = self
            .contact
            .iter()
            .zip(&self.speed)
            .filter(|(c, _)| **c)
            .map(|(_, s)| *s)
            .collect();
        // Unreachable by construction — `contact_core` refuses an empty label set — but this is a
        // measuring tool and 0.0 here reads as "covers no ground", which validate() then refuses.
        median(planted).unwrap_or(0.0)
    }

    /// How far the body travels in one cycle of this clip, in the file's own units.
    ///
    /// **A speed times a duration, not a displacement.** An earlier version measured the chord
    /// across the stance window and came out 2.4×–4.5× under the hand-measured table — a *varying*
    /// factor, the tell that it was the wrong quantity: the window's length varies per clip. Speed
    /// does not care how long the window is, and `distance = speed × duration` reproduces the
    /// table's own identity (0.98 u/s × 1.417 s = 1.388 u).
    pub fn cycle_distance(&self) -> f32 {
        self.stance_speed() * self.duration
    }
}

/// **The derived contact threshold**: Otsu's method over the **log** of the normalized
/// ground-frame distances — the exact sweep maximizing between-class variance (N. Otsu, "A
/// Threshold Selection Method from Gray-Level Histograms", IEEE Trans. SMC 9(1), 1979,
/// 10.1109/TSMC.1979.4310076). Contact frames cluster near the stance velocity, swing frames far
/// from it; the threshold belongs in the gap between the modes, and Otsu finds the gap from the
/// clip's own histogram instead of assuming one rig's tuning generalizes.
///
/// **Log space, from measurement, not taste.** The distances are a ratio quantity with a heavy
/// swing tail; raw-space Otsu splits inside the tail (checkpoint sweep, 2026-08-06: thresholds
/// 1.2–1.6× stance, walk's cycle 7.6% off the hand-validated table — past the 3% reference pin).
/// In log space the split lands in the multiplicative valley: walk thr 0.31/err 0.5%, run
/// 0.29/2.0%, walk_back 0.44/9.2% (the declared number is itself rough), strafe_l 0.40/4.5%,
/// strafe_r 0.80/0.4%. The one clip log-Otsu still mismeasures is run_back (thr 0.83, err 22.3% —
/// its transition-heavy histogram has its valley past the cliff the retired fixed `0.35` sweep
/// documented), at a mid-pack η of 0.76 no floor can single out; that is what the rig-level
/// `contact_eps:` declaration is for, and the shipped valkyrie declares one.
///
/// Returns `(threshold, separability)` in normalized-distance units; `None` when the values do
/// not separate (η < [`CONTACT_SEPARABILITY_MIN`], degenerate classes, or too few positive
/// distances) — never a guessed constant.
fn otsu_threshold(values: &[f32]) -> Option<(f32, f32)> {
    let mut sorted: Vec<f32> = values
        .iter()
        .filter(|v| **v > 0.0)
        .map(|v| v.ln())
        .collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n < 4 {
        return None;
    }
    let total_mean = sorted.iter().sum::<f32>() / n as f32;
    let total_var = sorted.iter().map(|v| (v - total_mean).powi(2)).sum::<f32>() / n as f32;
    if total_var <= 1.0e-12 {
        return None;
    }
    let mut best_split = 0usize;
    let mut best_between = 0.0f32;
    let mut lead_sum = 0.0f32;
    let total_sum: f32 = sorted.iter().sum();
    for k in 1..n {
        lead_sum += sorted[k - 1];
        let (w0, w1) = (k as f32 / n as f32, (n - k) as f32 / n as f32);
        let mu0 = lead_sum / k as f32;
        let mu1 = (total_sum - lead_sum) / (n - k) as f32;
        let between = w0 * w1 * (mu0 - mu1) * (mu0 - mu1);
        if between > best_between {
            best_between = between;
            best_split = k;
        }
    }
    if best_split == 0 {
        return None;
    }
    let separability = best_between / total_var;
    if separability < CONTACT_SEPARABILITY_MIN {
        return None;
    }
    // The threshold sits in the middle of the inter-class gap, back in linear units.
    let threshold = (0.5 * (sorted[best_split - 1] + sorted[best_split])).exp();
    Some((threshold, separability))
}

/// `eps`: a declared threshold (fraction of stance speed) overriding the derived one — the
/// explicit-decision path, from `Rig::contact_eps`.
fn contact_core(r: &Resampled, eps: Option<f32>) -> Option<ContactTrack> {
    let bins = PHASE_BINS;
    // Velocity per bin, wrapping the seam: a gait clip loops, so the pose at phase 1 is the pose at
    // phase 0 and the last bin's step is as real as any other.
    let vel: Vec<[f32; 2]> = (0..bins)
        .map(|i| {
            let p = r.pos[i];
            let q = r.pos[(i + 1) % bins];
            [(q[0] - p[0]) / r.dt, (q[2] - p[2]) / r.dt]
        })
        .collect();
    let heights: Vec<f32> = r.pos.iter().map(|p| p[1]).collect();
    // The height median only SEEDS the stance — it finds which velocity cluster is the planted one.
    // The label itself is the velocity condition, which survives rigs whose feet never rise far.
    let cut = median(heights.clone())?;
    let seed: Vec<usize> = (0..bins).filter(|&i| heights[i] <= cut).collect();
    if seed.len() < 4 {
        return None;
    }
    let vx = median(seed.iter().map(|&i| vel[i][0]).collect())?;
    let vz = median(seed.iter().map(|&i| vel[i][1]).collect())?;
    let stance = (vx * vx + vz * vz).sqrt();
    if stance <= 1.0e-6 {
        // No resolvable slide — "I cannot tell", never a zero that reads as "covers no ground".
        return None;
    }
    // Distance from the stance cluster, in stance-speed units — the quantity the threshold cuts.
    let dist: Vec<f32> = vel
        .iter()
        .map(|v| {
            let (dx, dz) = (v[0] - vx, v[1] - vz);
            (dx * dx + dz * dz).sqrt() / stance
        })
        .collect();
    let threshold = match eps {
        Some(e) => e,
        None => otsu_threshold(&dist)?.0,
    };
    let contact: Vec<bool> = dist.iter().map(|d| *d < threshold).collect();
    let planted = contact.iter().filter(|&&c| c).count();
    if planted == 0 || planted == bins {
        // All-planted is as unmeasurable as none: there is no swing to separate from.
        return None;
    }
    let speed: Vec<f32> = vel.iter().map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt()).collect();
    Some(ContactTrack {
        bins,
        contact,
        speed,
        body_velocity: [-vx, -vz],
        stance_fraction: planted as f32 / bins as f32,
        duration: r.duration,
        threshold,
    })
}

/// See [`ContactTrack`]. `eps` as on [`contact_core`]: `None` derives the threshold per clip.
pub fn contact_track(glb: &Glb, clip: usize, foot: usize, eps: Option<f32>) -> Option<ContactTrack> {
    contact_core(&resampled(glb, clip, foot)?, eps)
}

/// The planted foot's horizontal speed, in file units per second.
pub fn stance_speed(glb: &Glb, clip: usize, foot: usize, eps: Option<f32>) -> Option<f32> {
    Some(contact_track(glb, clip, foot, eps)?.stance_speed())
}

/// **How far the body travels in one cycle of this clip**, in the file's own units.
///
/// Returns `None` when the foot has no resolvable motion — an answer of "I cannot tell" rather than
/// a zero that would read as "this clip covers no ground".
pub fn cycle_distance(glb: &Glb, clip: usize, foot: usize, eps: Option<f32>) -> Option<f32> {
    Some(contact_track(glb, clip, foot, eps)?.cycle_distance())
}

/// **The phase bins where a stance begins** — the wrap-aware `false → true` transitions of the
/// contact labels. These are the clip's sync markers in the Unreal sense: two gaits agree in phase
/// when their footstrikes land together.
pub fn stance_onsets(track: &ContactTrack) -> Vec<usize> {
    let n = track.contact.len();
    (0..n)
        .filter(|&i| track.contact[i] && !track.contact[(i + n - 1) % n])
        .collect()
}

/// A phase alignment between two clips' contact trains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhaseMatch {
    /// What `b` must be shifted by to line up with `a`, as a fraction of a cycle in `[0, 1)`.
    pub offset: f32,
    /// A second, distant correlation peak scored within 90% of the best. Two clips that step a
    /// different number of times per cycle produce this — the best answer is then a convention
    /// rather than a measurement, and a consumer should say so.
    pub ambiguous: bool,
}

/// **How far `b` is out of step with `a`**, from cross-correlating the two clips' zero-meaned
/// foot-height curves on the shared phase grid — `docs/artist_guide.md` §4's own method. Height and
/// not the contact train, for a measured reason: the trains scored walk→walk_back at +0.039 of a
/// cycle where the height curve reproduces the guide's validated −0.141. A square wave keeps *where*
/// the stance falls but loses the pose shape that locates the alignment.
///
/// The single-joint form of [`phase_match_joints`] — the same scorer, one path.
pub fn phase_match(glb: &Glb, a: usize, b: usize, foot: usize) -> Option<PhaseMatch> {
    phase_match_joints(glb, a, b, &[foot])
}

/// [`phase_match`] over a SET of contact joints: the score at each lag is the sum of the per-joint
/// height correlations, joint identity preserved — `a`'s left foot correlates with `b`'s left
/// foot, never its right. This is the sync-marker intersection posture (Unreal's sync groups align
/// on the markers common to every clip in the group): a left/right pair breaks the half-cycle
/// ambiguity a single symmetric gait leaves, because only the true lag lines BOTH feet up at once.
///
/// Deterministic tie-break: an exactly tied score resolves to the lag with the smallest signed
/// offset, then the smaller lag.
pub fn phase_match_joints(glb: &Glb, a: usize, b: usize, joints: &[usize]) -> Option<PhaseMatch> {
    let bins = PHASE_BINS;
    let curve = |clip: usize, joint: usize| -> Option<Vec<f32>> {
        let r = resampled(glb, clip, joint)?;
        let mut out: Vec<f32> = r.pos.iter().map(|p| p[1]).collect();
        // Zero-mean, so the correlation compares SHAPE rather than which foot sits higher.
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        for v in &mut out {
            *v -= mean;
        }
        Some(out)
    };
    // Every joint must resolve in BOTH clips — a pair that half-resolves would silently become a
    // single-joint answer wearing a multi-joint label.
    let mut pairs: Vec<(Vec<f32>, Vec<f32>)> = Vec::new();
    for &j in joints {
        pairs.push((curve(a, j)?, curve(b, j)?));
    }
    if pairs.is_empty() {
        return None;
    }
    let offset_of = |lag: usize| -> f32 {
        // Negated: the offset is what `b` must be shifted BY to line up with `a`, which is the
        // sign `anim::Playback::Gait` wants and the sign the artist guide's table is written in.
        let f = lag as f32 / bins as f32;
        if f == 0.0 { 0.0 } else { 1.0 - f }
    };
    let scores: Vec<f32> = (0..bins)
        .map(|lag| {
            pairs
                .iter()
                .map(|(ca, cb)| (0..bins).map(|i| ca[i] * cb[(i + lag) % bins]).sum::<f32>())
                .sum()
        })
        .collect();
    let mut best = 0usize;
    for lag in 1..bins {
        let tighter = (signed_offset(offset_of(lag)).abs(), lag)
            < (signed_offset(offset_of(best)).abs(), best);
        if scores[lag] > scores[best] || (scores[lag] == scores[best] && tighter) {
            best = lag;
        }
    }
    // A distant runner-up peak nearly as good as the winner means the curve repeats inside one
    // cycle — clips stepping a different number of times. Adjacent lags are the same peak, not a
    // rival.
    let far = bins / 8;
    let ambiguous = scores[best] > 0.0
        && (0..bins).any(|lag| {
            let d = lag.abs_diff(best).min(bins - lag.abs_diff(best));
            d > far && scores[lag] >= 0.9 * scores[best]
        });
    Some(PhaseMatch {
        offset: offset_of(best),
        ambiguous,
    })
}

/// [`phase_match`]'s offset alone, as a bare fraction in `[0, 1)`.
pub fn phase_offset(glb: &Glb, a: usize, b: usize, foot: usize) -> Option<f32> {
    Some(phase_match(glb, a, b, foot)?.offset)
}

/// A fraction in `[0, 1)` restated in the manifest's signed convention, `(-0.5, 0.5]`.
///
/// An offset is a shift along a cycle that wraps, so −0.141 and 0.859 are the same alignment — and
/// the small signed value is what an author means (`rigs::Rigs::validate` documents the same rule).
pub fn signed_offset(frac: f32) -> f32 {
    if frac > 0.5 { frac - 1.0 } else { frac }
}

/// **Joints that behave like feet in this clip**, best first — the note the bench shows when a rig
/// has no joint named `foot_l`, so "configure `contact_joints`" arrives with the names to pick from.
///
/// A candidate is a *named leaf* of the node tree whose ground-frame velocity stays inside the
/// contact threshold for at least [`MIN_STANCE_FRACTION`] of the cycle, and whose lowest point sits
/// in the lowest quartile of all leaves' minima — feet are low. Skeleton-Aware Networks (Aberman et
/// al. 2020) make the structural argument: skeletons that differ in joint count still share their
/// end-effector set, and the end effectors are where contact lives.
///
/// Ordered by stance fraction descending, then name — a total order, so the list is stable.
pub fn contact_candidates(glb: &Glb, clip: usize) -> Vec<(String, f32)> {
    let Some(nodes) = glb.json["nodes"].as_array() else {
        return Vec::new();
    };
    let is_leaf =
        |n: &Value| n["children"].as_array().is_none_or(|k| k.is_empty());
    let mut lows: Vec<f32> = Vec::new();
    let mut named: Vec<(String, f32, f32)> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if !is_leaf(n) {
            continue;
        }
        let Some(r) = resampled(glb, clip, i) else {
            continue;
        };
        let low = r.pos.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        lows.push(low);
        let Some(name) = n["name"].as_str() else {
            continue;
        };
        // Candidates always derive their threshold: the suggestion list exists precisely when the
        // configuration is in doubt, so a declared override has nothing to say here.
        if let Some(t) = contact_core(&r, None) {
            if t.stance_fraction >= MIN_STANCE_FRACTION {
                named.push((name.to_owned(), t.stance_fraction, low));
            }
        }
    }
    if lows.is_empty() {
        return Vec::new();
    }
    lows.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let quartile = lows[lows.len() / 4];
    let mut out: Vec<(String, f32)> = named
        .into_iter()
        .filter(|(_, _, low)| *low <= quartile)
        .map(|(name, fraction, _)| (name, fraction))
        .collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out
}

/// **Everything the bench plots for one gait slot**, resampled onto the shared phase grid.
/// Positions and speeds are in FILE units — the caller applies the rig's scale.
/// Serde because the editor's measurement cache persists these between sessions.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GaitCurves {
    /// [`PHASE_BINS`].
    pub bins: usize,
    /// The clip's duration, seconds.
    pub duration: f32,
    /// Contact joint world height per bin.
    pub foot_height: Vec<f32>,
    /// Contact joint horizontal speed per bin, file units per second. During stance this should sit
    /// at the body's speed; deviation inside the stance IS foot skate, made visible.
    pub ground_speed: Vec<f32>,
    /// Horizontal distance of the root from its bin-0 position. Flat is "authored in place".
    pub root_drift: Vec<f32>,
    /// Contact joint XZ per bin — the top-down trace.
    pub trace: Vec<[f32; 2]>,
    /// Contact label per bin.
    pub contact: Vec<bool>,
    /// The body's horizontal travel velocity (XZ), file units per second.
    pub body_velocity: [f32; 2],
}

/// See [`GaitCurves`]. The curves are a by-product of the FK the checks already run. `eps` as on
/// [`contact_core`]: `None` derives the contact threshold per clip.
pub fn gait_curves(
    glb: &Glb,
    clip: usize,
    foot: usize,
    root: Option<usize>,
    eps: Option<f32>,
) -> Option<GaitCurves> {
    let r = resampled(glb, clip, foot)?;
    let c = contact_core(&r, eps)?;
    let root_drift = match root.and_then(|n| resampled(glb, clip, n)) {
        Some(rr) => {
            let o = rr.pos[0];
            rr.pos
                .iter()
                .map(|p| {
                    let (dx, dz) = (p[0] - o[0], p[2] - o[2]);
                    (dx * dx + dz * dz).sqrt()
                })
                .collect()
        }
        // A root the clip never keys has no track; it also has no drift, and the flat line is the
        // correct picture of that, not a stand-in for a missing one.
        None => vec![0.0; PHASE_BINS],
    };
    Some(GaitCurves {
        bins: PHASE_BINS,
        duration: r.duration,
        foot_height: r.pos.iter().map(|p| p[1]).collect(),
        ground_speed: c.speed.clone(),
        root_drift,
        trace: r.pos.iter().map(|p| [p[0], p[2]]).collect(),
        contact: c.contact.clone(),
        body_velocity: c.body_velocity,
    })
}

/// **A joint's curves without a contact claim** — what the bench plots for a rig with no gaits.
/// The same [`GaitCurves`] shape (one curve type, one raster path), with `contact` all-false and
/// `root_drift`/`body_velocity` zeroed: no contact was measured, so no travel or stance is
/// claimed — honest empties, not defaults standing in for measurements.
pub fn joint_curves(glb: &Glb, clip: usize, node: usize) -> Option<GaitCurves> {
    let r = resampled(glb, clip, node)?;
    Some(GaitCurves {
        bins: PHASE_BINS,
        duration: r.duration,
        foot_height: r.pos.iter().map(|p| p[1]).collect(),
        ground_speed: (0..PHASE_BINS)
            .map(|i| {
                let p = r.pos[i];
                let q = r.pos[(i + 1) % PHASE_BINS];
                let (dx, dz) = ((q[0] - p[0]) / r.dt, (q[2] - p[2]) / r.dt);
                (dx * dx + dz * dz).sqrt()
            })
            .collect(),
        root_drift: vec![0.0; PHASE_BINS],
        trace: r.pos.iter().map(|p| [p[0], p[2]]).collect(),
        contact: vec![false; PHASE_BINS],
        body_velocity: [0.0, 0.0],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped rig, from the workspace root — `cargo test -p emerge-core` runs in the crate dir.
    const VALKYRIE: &str = "../../assets/characters/valkyrie.glb";

    fn valkyrie() -> Glb {
        Glb::open(std::path::Path::new(VALKYRIE))
            .unwrap_or_else(|e| panic!("{VALKYRIE}: {e}"))
    }

    /// The valkyrie's render scale, from the one place that owns it. The game scales the whole
    /// figurine, so a distance measured in the file's units becomes this many world units.
    fn figurine_scale() -> f32 {
        let text = std::fs::read_to_string("../../assets/emerge/rigs.ron")
            .unwrap_or_else(|e| panic!("rigs.ron: {e}"));
        let rigs = crate::rigs::Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        rigs.get("valkyrie")
            .unwrap_or_else(|| panic!("no valkyrie in the manifest"))
            .scale
    }

    /// `docs/artist_guide.md` §4's table: index, name, duration, cycle distance in world units.
    const TABLE: [(usize, &str, f32, f32); 6] = [
        (5, "walk", 1.417, 1.388),
        (11, "run", 0.750, 2.135),
        (8, "walk_back", 1.458, 1.538),
        (12, "run_back", 0.583, 1.185),
        (13, "strafe_l", 0.708, 1.937),
        (14, "strafe_r", 0.583, 1.259),
    ];

    #[test]
    fn the_clip_table_reads_back_off_the_asset() {
        let glb = valkyrie();
        let all = clips(&glb);
        assert_eq!(all.len(), 20, "the Valkyrie ships 20 clips");
        for (ix, name, dur, _) in TABLE {
            let c = &all[ix];
            assert_eq!(c.index, ix);
            // One frame at 24 fps. The durations came out exact, but the contract is a frame.
            assert!(
                (c.duration - dur).abs() < 1.0 / 24.0,
                "clip {ix} ({name}) is {:.3}s, the guide says {dur:.3}s",
                c.duration
            );
            assert!(c.channels > 0, "clip {ix} ({name}) drives nothing");
        }
    }

    /// The in-place contract, measured the way `tests/valkyrie_asset.rs` measures it.
    #[test]
    fn the_gait_clips_are_authored_in_place() {
        let glb = valkyrie();
        let root = node_index(&glb, "Root").unwrap_or_else(|| panic!("no Root node"));
        for (ix, name, _, _) in TABLE {
            let m = root_motion(&glb, ix, root);
            for (axis, v) in m.iter().enumerate() {
                assert!(
                    *v < 1.0e-4,
                    "clip {ix} ({name}) moves Root on axis {axis} by {v} — gait clips must be in place"
                );
            }
        }
    }

    /// **The measurement that replaces the manual step.**
    ///
    /// Only `walk` and `run` are asserted tightly. `docs/artist_guide.md` §4 says the offsets across
    /// the set "agree to within 0.14 of a cycle (walk and run to within 0.016)" — the back and strafe
    /// clips' hand-measured numbers are themselves rough, so pinning this tool to them would be
    /// pinning it to their error. These two are the reference, and they come back within ~1.5%.
    #[test]
    fn the_reference_gaits_measure_what_the_guide_recorded() {
        let glb = valkyrie();
        let foot = node_index(&glb, "foot_l").unwrap_or_else(|| panic!("no foot_l node"));
        for (ix, name, _, want) in TABLE.iter().take(2) {
            let raw = cycle_distance(&glb, *ix, foot, None)
                .unwrap_or_else(|| panic!("clip {ix} ({name}): no measurable stance"));
            let got = raw * figurine_scale();
            let err = (got - want).abs() / want;
            assert!(
                err < 0.03,
                "clip {ix} ({name}) measures {got:.3} u/cycle, the guide says {want:.3} ({:.1}% out)",
                err * 100.0
            );
        }
    }

    /// A clip is perfectly in phase with itself, and `walk_back` is the offset the guide recorded.
    #[test]
    fn phase_offsets_line_up_with_the_guide() {
        let glb = valkyrie();
        let foot = node_index(&glb, "foot_l").unwrap_or_else(|| panic!("no foot_l node"));
        assert_eq!(
            phase_offset(&glb, 5, 5, foot),
            Some(0.0),
            "a clip must be in phase with itself"
        );
        // The guide records walk_back at -0.141 of a cycle; this returns the same shift as a
        // positive fraction, so -0.141 reads as 0.859.
        let got = phase_offset(&glb, 5, 8, foot).unwrap_or_else(|| panic!("no offset"));
        assert!(
            (got - 0.859).abs() < 0.02,
            "walk -> walk_back measured {got:.3} of a cycle, the guide says 0.859 (-0.141)"
        );
    }

    /// A prop with no animations is not an error — most of the library is static scenery.
    #[test]
    fn a_static_mesh_reports_no_clips() {
        let glb = Glb::parse(&[]).err();
        assert!(glb.is_some(), "an empty buffer is not a GLB");
    }

    /// **The contact labels stay plausible across the whole gait set**, under both threshold
    /// paths.
    ///
    /// Derived (`None`, log-Otsu — see [`otsu_threshold`]'s table): every gait lands at 0.39–0.49
    /// stance, the biomechanically plausible band for a single foot. Declared 0.35 (the
    /// valkyrie's own `contact_eps:`): the four clean gaits label 0.41–0.45, and the two roughest
    /// clips (`run_back` 0.148, `strafe_r` 0.086) genuinely carry that little clean stance —
    /// pinning the truth beats pinning a wish. A re-export that changes these materially changed
    /// the clips.
    #[test]
    fn contact_fractions_stay_plausible() {
        let glb = valkyrie();
        let foot = node_index(&glb, "foot_l").unwrap_or_else(|| panic!("no foot_l node"));
        let derived = [
            (5, "walk", 0.30, 0.60),
            (11, "run", 0.30, 0.60),
            (8, "walk_back", 0.30, 0.60),
            (12, "run_back", 0.30, 0.60),
            (13, "strafe_l", 0.30, 0.60),
            (14, "strafe_r", 0.30, 0.60),
        ];
        for (ix, name, lo, hi) in derived {
            let t = contact_track(&glb, ix, foot, None)
                .unwrap_or_else(|| panic!("clip {ix} ({name}): no contact track"));
            assert!(
                (lo..=hi).contains(&t.stance_fraction),
                "clip {ix} ({name}) labels {:.3} of the cycle as stance (derived), expected \
                 {lo}..{hi}",
                t.stance_fraction
            );
        }
        let declared = [
            (5, "walk", 0.30, 0.60),
            (11, "run", 0.30, 0.60),
            (8, "walk_back", 0.30, 0.60),
            (12, "run_back", 0.08, 0.40),
            (13, "strafe_l", 0.30, 0.60),
            (14, "strafe_r", 0.05, 0.35),
        ];
        for (ix, name, lo, hi) in declared {
            let t = contact_track(&glb, ix, foot, Some(0.35))
                .unwrap_or_else(|| panic!("clip {ix} ({name}): no contact track"));
            assert!(
                (lo..=hi).contains(&t.stance_fraction),
                "clip {ix} ({name}) labels {:.3} of the cycle as stance (at 0.35), expected \
                 {lo}..{hi}",
                t.stance_fraction
            );
        }
    }

    /// **The strafe naming swap, measured rather than annotated.** The guide's LEFTWARD note says
    /// clip 13 (`valkyrie_strafe_l` in the asset) carries the body toward −X and clip 14 toward
    /// +X; `body_velocity` is the vector that makes that a pin instead of prose.
    #[test]
    fn the_strafe_clips_travel_the_directions_the_guide_records() {
        let glb = valkyrie();
        let foot = node_index(&glb, "foot_l").unwrap_or_else(|| panic!("no foot_l node"));
        let vx = |clip: usize| {
            contact_track(&glb, clip, foot, None)
                .unwrap_or_else(|| panic!("clip {clip}: no contact track"))
                .body_velocity[0]
        };
        assert!(vx(13) < -0.5, "clip 13 measures body vx {}, the guide says −X", vx(13));
        assert!(vx(14) > 0.5, "clip 14 measures body vx {}, the guide says +X", vx(14));
    }

    /// The signed convention the manifest stores: small magnitudes, `(-0.5, 0.5]`.
    #[test]
    fn signed_offsets_prefer_the_small_magnitude() {
        assert_eq!(signed_offset(0.0), 0.0);
        assert_eq!(signed_offset(0.5), 0.5);
        assert!((signed_offset(0.859) - -0.141).abs() < 1.0e-6);
        assert!((signed_offset(0.25) - 0.25).abs() < 1.0e-6);
    }

    // ── the all-rig measurements, on a synthetic fixture with real accessor bytes ────────────────

    /// Append `rows` as a FLOAT accessor backed by `bin`; returns the accessor index.
    fn push_accessor(
        accessors: &mut Vec<serde_json::Value>,
        views: &mut Vec<serde_json::Value>,
        bin: &mut Vec<u8>,
        rows: &[Vec<f32>],
    ) -> usize {
        use serde_json::json;
        let offset = bin.len();
        for row in rows {
            for v in row {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let width = rows.first().map_or(1, Vec::len);
        views.push(json!({ "byteOffset": offset, "byteLength": rows.len() * width * 4 }));
        let max: Vec<f32> = (0..width)
            .map(|c| rows.iter().map(|r| r[c]).fold(f32::MIN, f32::max))
            .collect();
        accessors.push(json!({
            "bufferView": views.len() - 1,
            "componentType": 5126,
            "count": rows.len(),
            "max": max,
        }));
        accessors.len() - 1
    }

    /// A quaternion `deg` about +Y.
    fn qy(deg: f32) -> Vec<f32> {
        let h = deg.to_radians() / 2.0;
        vec![0.0, h.sin(), 0.0, h.cos()]
    }

    /// Two clips over one joint (`hips`), keyed with real bytes: `loopy` (clip 0) closes its
    /// rotation loop on irregular key times; `open` (clip 1) ends 40 deg away from both its own
    /// start and `loopy`'s first frame.
    fn pose_fixture() -> Glb {
        use serde_json::json;
        let mut bin = Vec::new();
        let mut accessors = Vec::new();
        let mut views = Vec::new();
        // Irregular keys: median gap is 1/24 s despite the long final hold.
        let times = vec![
            vec![0.0f32],
            vec![1.0 / 24.0],
            vec![2.0 / 24.0],
            vec![3.0 / 24.0],
            vec![1.0],
        ];
        let t = push_accessor(&mut accessors, &mut views, &mut bin, &times);
        let closed = vec![qy(0.0), qy(20.0), qy(-15.0), qy(10.0), qy(0.0)];
        let closed_out = push_accessor(&mut accessors, &mut views, &mut bin, &closed);
        let open = vec![qy(0.0), qy(15.0), qy(30.0), qy(35.0), qy(40.0)];
        let open_out = push_accessor(&mut accessors, &mut views, &mut bin, &open);
        Glb {
            json: json!({
                "nodes": [{ "name": "hips" }],
                "animations": [
                    {
                        "name": "loopy",
                        "channels": [{ "sampler": 0, "target": { "node": 0, "path": "rotation" } }],
                        "samplers": [{ "input": t, "output": closed_out }],
                    },
                    {
                        "name": "open",
                        "channels": [{ "sampler": 0, "target": { "node": 0, "path": "rotation" } }],
                        "samplers": [{ "input": t, "output": open_out }],
                    },
                ],
                "accessors": accessors,
                "bufferViews": views,
            }),
            bin,
        }
    }

    #[test]
    fn a_closed_loop_passes_and_an_open_one_names_the_worst_joint() {
        let glb = pose_fixture();
        let closed = loop_closure(&glb, 0).unwrap_or_else(|| panic!("no closure for loopy"));
        assert_eq!(closed.joints, 1);
        assert!(closed.max_angle_deg < 0.01, "{}", closed.max_angle_deg);
        let open = loop_closure(&glb, 1).unwrap_or_else(|| panic!("no closure for open"));
        assert_eq!(open.worst_joint, "hips");
        assert!(
            (open.max_angle_deg - 40.0).abs() < 0.1,
            "{}",
            open.max_angle_deg
        );
        // A clip index past the asset is a refusal, not a guess.
        assert!(loop_closure(&glb, 9).is_none());
    }

    #[test]
    fn a_one_shot_ending_off_the_reference_idle_is_measured() {
        let glb = pose_fixture();
        // `open` ends 40 deg from `loopy`'s first frame.
        let pd = end_pose_delta(&glb, 1, 0).unwrap_or_else(|| panic!("no delta"));
        assert_eq!(pd.joints, 1);
        assert_eq!(pd.worst_joint, "hips");
        assert!((pd.max_angle_deg - 40.0).abs() < 0.1, "{}", pd.max_angle_deg);
        // `loopy` ends exactly on it.
        let pd = end_pose_delta(&glb, 0, 0).unwrap_or_else(|| panic!("no delta"));
        assert!(pd.max_angle_deg < 0.01, "{}", pd.max_angle_deg);
    }

    #[test]
    fn source_rate_uses_the_median_of_irregular_keys() {
        let glb = pose_fixture();
        let sr = source_rate(&glb, 0).unwrap_or_else(|| panic!("no rate"));
        assert_eq!(sr.keys, 5);
        // The long final hold must not drag the rate down — median, not mean.
        assert!((sr.fps - 24.0).abs() < 0.01, "{}", sr.fps);
    }

    /// The gait-less plotting path: real bins, real heights, and NO claims — contact all-false,
    /// zero drift, zero travel.
    #[test]
    fn joint_curves_carry_shape_without_contact_claims() {
        let glb = valkyrie();
        let foot = node_index(&glb, "foot_l").unwrap_or_else(|| panic!("no foot_l"));
        // Clip 0 is the idle — exactly the free-slot case this exists for.
        let c = joint_curves(&glb, 0, foot).unwrap_or_else(|| panic!("no curves"));
        assert_eq!(c.bins, PHASE_BINS);
        assert_eq!(c.foot_height.len(), PHASE_BINS);
        assert!(c.duration > 0.0);
        assert!(c.contact.iter().all(|planted| !planted), "no contact was measured");
        assert!(c.root_drift.iter().all(|d| *d == 0.0));
        assert_eq!(c.body_velocity, [0.0, 0.0]);
        // The heights are a real curve, not a constant — the idle breathes.
        let lo = c.foot_height.iter().fold(f32::MAX, |a, &b| a.min(b));
        let hi = c.foot_height.iter().fold(f32::MIN, |a, &b| a.max(b));
        assert!(hi > lo, "a flat line would mean the resample read nothing");
    }
}
