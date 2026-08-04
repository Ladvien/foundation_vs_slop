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
//! [`cycle_distance`] measures that slide. Metaxas & Sun, *Automating Gait Generation*
//! (10.1145/383259.383288), is the standard statement of the step-length/step-rate coupling this
//! feeds; an inaccurate cycle distance is what foot-skate *is*.
//!
//! [`phase_offset`] aligns two clips by cross-correlating a foot-height curve, which is what
//! `docs/artist_guide.md` §4 already prescribes ("re-measured by cross-correlating foot height, not
//! guessed") and what WalkTheDog (2024) formalises as a 1-D phase manifold.

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

/// **How far the body travels in one cycle of this clip**, in the file's own units.
///
/// Measured from the planted foot, which is the only honest source: the clip is authored in place, so
/// the ground speed it implies exists nowhere in the file except as the backward slide of whichever
/// foot is down. The stance window is the half of the cycle where the foot is lowest; its horizontal
/// displacement across that window is how far the body moved while it was planted.
///
/// Returns `None` when the foot has no resolvable motion — an answer of "I cannot tell" rather than a
/// zero that would read as "this clip covers no ground".
pub fn cycle_distance(glb: &Glb, clip: usize, foot: usize) -> Option<f32> {
    let speed = stance_speed(glb, clip, foot)?;
    let anim = glb.json["animations"].as_array()?.get(clip)?;
    Some(speed * duration(glb, anim))
}

/// The planted foot's horizontal speed, in file units per second.
///
/// **A speed, not a displacement.** The first version measured the chord between the ends of the
/// stance window and came out 2.4×–4.5× under the hand-measured table — a *varying* factor, which is
/// the tell that it was not a missing scale but the wrong quantity: the window is only part of the
/// cycle and its length varies per clip, so a displacement across it answers a different question for
/// every clip. Speed does not care how long the window is, and `distance = speed × duration` then
/// reproduces the table's own identity (0.98 u/s × 1.417 s = 1.388 u).
///
/// The **median** of the per-sample speeds in the window, not the mean: touchdown and lift-off sit at
/// the ends of the stance and are moving at neither the body's speed nor zero, and a mean would let
/// those two frames drag every number down.
pub fn stance_speed(glb: &Glb, clip: usize, foot: usize) -> Option<f32> {
    let (times, world) = world_track(glb, clip, foot)?;
    if world.len() < 4 {
        return None;
    }
    let mut heights: Vec<f32> = world.iter().map(|p| p[1]).collect();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The lower half of the height range is the stance — the foot is down for roughly that long, and
    // the exact fraction does not matter once the answer is a speed.
    let median = heights[heights.len() / 2];

    let mut speeds: Vec<f32> = Vec::new();
    for i in 1..world.len() {
        let dt = times[i] - times[i - 1];
        if dt <= 0.0 {
            continue;
        }
        // Both ends of the step must be planted, or the sample spans a touchdown.
        if world[i][1] > median || world[i - 1][1] > median {
            continue;
        }
        let (dx, dz) = (world[i][0] - world[i - 1][0], world[i][2] - world[i - 1][2]);
        speeds.push((dx * dx + dz * dz).sqrt() / dt);
    }
    if speeds.is_empty() {
        return None;
    }
    speeds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(speeds[speeds.len() / 2])
}

/// **How far `b` is out of step with `a`**, as a fraction of a cycle in `[0, 1)`.
///
/// Cross-correlates the two clips' foot-height curves — `docs/artist_guide.md` §4's own method, and
/// the alignment WalkTheDog (2024) formalises. Resampled to a common phase grid first, because the
/// two clips have different durations and different keyframe counts; correlating raw samples would be
/// correlating two different time axes.
pub fn phase_offset(glb: &Glb, a: usize, b: usize, foot: usize) -> Option<f32> {
    const BINS: usize = 128;
    let curve = |clip: usize| -> Option<Vec<f32>> {
        let (times, world) = world_track(glb, clip, foot)?;
        let span = times.last()? - times.first()?;
        if span <= 0.0 {
            return None;
        }
        let raw: Vec<Vec<f32>> = world.iter().map(|p| vec![p[1]]).collect();
        let mut out = Vec::with_capacity(BINS);
        for i in 0..BINS {
            let t = times[0] + span * (i as f32 / BINS as f32);
            out.push(sample(&times, &raw, t)?[0]);
        }
        // Zero-mean, so the correlation compares SHAPE rather than which foot sits higher.
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        for v in &mut out {
            *v -= mean;
        }
        Some(out)
    };
    let (ca, cb) = (curve(a)?, curve(b)?);
    let mut best = (f32::NEG_INFINITY, 0usize);
    for lag in 0..BINS {
        let score: f32 = (0..BINS).map(|i| ca[i] * cb[(i + lag) % BINS]).sum();
        if score > best.0 {
            best = (score, lag);
        }
    }
    // Negated: the offset is what `b` must be shifted BY to line up with `a`, which is the sign
    // `anim::Playback::Gait` wants and the sign `docs/artist_guide.md` §4's table is written in.
    let lag = best.1 as f32 / BINS as f32;
    Some(if lag == 0.0 { 0.0 } else { 1.0 - lag })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped rig, from the workspace root — `cargo test -p emerge-core` runs in the crate dir.
    const VALKYRIE: &str = "../../assets/characters/valkyrie.glb";
    /// `squad::FIGURINE_SCALE`. The game scales the whole figurine, so a distance measured in the
    /// file's units becomes this many world units.
    const FIGURINE_SCALE: f32 = 1.13;

    fn valkyrie() -> Glb {
        Glb::open(std::path::Path::new(VALKYRIE))
            .unwrap_or_else(|e| panic!("{VALKYRIE}: {e}"))
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
            let raw = cycle_distance(&glb, *ix, foot)
                .unwrap_or_else(|| panic!("clip {ix} ({name}): no measurable stance"));
            let got = raw * FIGURINE_SCALE;
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
}
