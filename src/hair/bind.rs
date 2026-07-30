//! Root binding — attaching a guide strand's root to a **skinned host surface**, so it rides the mesh
//! the hair grows out of rather than a single bone.
//!
//! # Why this exists at all
//!
//! The superseded version anchored every guide to the one glTF joint named `head`
//! (`HEAD_BONE_NAME`/`locate_head_bone`). On the Valkyrie that happens to be almost exact — every
//! vertex of the `hair_cap_valkyrie` scalp cap is ~100% weighted to `head`. It stops being exact the
//! moment the host surface spans a *weight seam*: a root bound to one joint `j` moves as
//! `M_j · invBind_j · p_rest`, so across a seam between two joints the single-joint answer and the true
//! skinned answer diverge by roughly the joint separation times `sin(θ/2)` under a bend of `θ`.
//!
//! On a 25-joint crab carapace that error is centimetres — more than the fur is long, which is why the
//! fur would visibly detach or sink into the shell. This is the failure the SIGGRAPH 2025 *Indiana
//! Jones* talk reports as the reason they kept hair **cards** for dogs: single-joint scalp binding and
//! density-volume shadows both broke on animals that bend.
//! [`tests::single_joint_binding_is_far_worse_than_four_joint_across_a_bending_seam`] measures that
//! claim on a synthetic seam rather than restating it.
//!
//! # The scheme, and its one honest approximation
//!
//! At bind time, per root: pick a host triangle, take barycentrics, interpolate the **bind-space**
//! position and normal, barycentrically blend the three vertices' `JOINTS_0`/`WEIGHTS_0` into one
//! influence set, keep the four heaviest, renormalise, and remap the joint indices into a compact
//! per-rig palette. Per frame: compose the palette once per host, then [`eval_root`] evaluates
//!
//! ```text
//! p = Σᵢ wᵢ · (M_jointᵢ · invBindᵢ) · p_rest
//! ```
//!
//! — four weighted affine accumulations per root, *not* three vertex skins. This is exactly Bevy's own
//! `skin_model` (`bevy_pbr/src/render/skinning.wgsl`) evaluated on the CPU for a single point, so the
//! root lands on the same surface the GPU draws.
//!
//! **The approximation:** blending influences and *then* skinning one point,
//! `Σᵢ (Σₖ bₖwᵢₖ) Aᵢ · Σₖ bₖvₖ`, is not algebraically identical to skinning three vertices and *then*
//! interpolating, `Σₖ bₖ Σᵢ wᵢₖ Aᵢvₖ`. They agree exactly when all three vertices share one influence
//! set — true across the interior of the scalp cap and most fur patches. Across a seam under extreme
//! bend the error is bounded by the intra-triangle weight variation.
//! [`tests::blend_then_skin_matches_skin_then_blend_on_a_uniform_triangle`] pins the exact case and
//! [`tests::blend_then_skin_stays_close_to_true_skinning_across_a_bending_seam`] bounds the inexact one.
//! That is the price of 4 affine ops instead of 3 vertex skins, and it is measured, not assumed.
//!
//! Pure math over slices: no ECS types, no queries, no `Assets`. Same reason as [`super::sim`] — it
//! keeps the whole binding layer inside the GPU-free `cargo test` hard gate.

use bevy::math::Affine3A;
use bevy::prelude::*;

/// Number of joint influences kept per root. glTF's `JOINTS_0`/`WEIGHTS_0` carry exactly four per
/// vertex, and keeping four after blending three of them means a root is never *less* skinned than the
/// vertices it sits between.
pub(super) const INFLUENCES: usize = 4;

/// One host-mesh vertex's skinning influences, as read from `Mesh::ATTRIBUTE_JOINT_INDEX` (`Uint16x4`)
/// and `Mesh::ATTRIBUTE_JOINT_WEIGHT` (`Float32x4`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct VertexSkin {
    pub joints: [u16; INFLUENCES],
    pub weights: [f32; INFLUENCES],
}

/// A guide root, baked once against the host primitive's rest (bind-space) mesh.
#[derive(Clone, Debug, Default)]
pub(super) struct RootBind {
    /// Bind-space position, barycentrically interpolated from the host triangle.
    pub rest: Vec3,
    /// Bind-space surface normal — the strand's growth direction before any styling lift.
    pub normal: Vec3,
    /// Indices into the rig's compact joint palette (NOT raw glTF joint indices).
    pub slots: [u16; INFLUENCES],
    /// Renormalised influence weights, summing to 1 (or all zero for an unskinned host).
    pub weights: [f32; INFLUENCES],
}

/// A root evaluated against this frame's pose.
#[derive(Clone, Copy, Debug)]
pub(super) struct RootFrame {
    pub pos: Vec3,
    pub normal: Vec3,
}

/// Accumulate `dst += src * w` over an affine's four columns.
///
/// `Affine3A` has no scalar multiply, and going through `Mat4` per influence would cost a conversion
/// per joint per root per frame. Blending the columns directly is the same arithmetic linear-blend
/// skinning does anyway.
#[inline]
fn accumulate(dst: &mut Affine3A, src: &Affine3A, w: f32) {
    dst.matrix3.x_axis += src.matrix3.x_axis * w;
    dst.matrix3.y_axis += src.matrix3.y_axis * w;
    dst.matrix3.z_axis += src.matrix3.z_axis * w;
    dst.translation += src.translation * w;
}

/// Evaluate a bound root against a composed joint palette.
///
/// `palette[k]` must already be `joint_global.affine() * inverse_bindpose[k]` — the per-host
/// composition is hoisted out of this function on purpose, because a groom's roots share far fewer
/// joints than they have influences (a Valkyrie scalp cap: 936 roots over ~4 joints), and composing
/// per root would multiply the sparse `GlobalTransform` lookups by four.
pub(super) fn eval_root(bind: &RootBind, palette: &[Affine3A]) -> RootFrame {
    let mut a = Affine3A::ZERO;
    let mut total = 0.0f32;
    for i in 0..INFLUENCES {
        let w = bind.weights[i];
        if w <= 0.0 {
            continue;
        }
        // A slot outside the palette means the bake and the palette disagree — skip rather than index
        // out of bounds. `bake_root` cannot produce this, but a future groom-table edit could.
        let Some(m) = palette.get(bind.slots[i] as usize) else { continue };
        accumulate(&mut a, m, w);
        total += w;
    }
    if total <= 0.0 {
        // An unskinned host (or a fully-zero weight set): the bind pose IS the live pose.
        return RootFrame { pos: bind.rest, normal: bind.normal };
    }
    RootFrame {
        pos: a.transform_point3(bind.rest),
        // The normal is a direction, so it takes the linear part only, never the translation.
        normal: (a.matrix3 * Vec3A::from(bind.normal)).normalize_or_zero().into(),
    }
}

/// Blend three vertices' influence sets by barycentric weight, keep the four heaviest, renormalise.
///
/// Returns raw glTF joint indices paired with weights; [`bake_root`] is what remaps them into palette
/// slots. Duplicate joints across the three vertices are accumulated, not double-counted — that is the
/// whole reason this cannot be a naive concatenate-and-truncate.
pub(super) fn blend_influences(tri: [&VertexSkin; 3], bary: Vec3) -> ([u16; INFLUENCES], [f32; INFLUENCES]) {
    // At most 3 vertices x 4 influences distinct joints. A fixed array with a linear scan beats a
    // HashMap at this size and, unlike a HashMap, has no iteration-order question at all.
    let mut joints = [0u16; 12];
    let mut weights = [0.0f32; 12];
    let mut used = 0usize;

    let bary = [bary.x, bary.y, bary.z];
    for (v, b) in tri.iter().zip(bary) {
        if b <= 0.0 {
            continue;
        }
        for i in 0..INFLUENCES {
            let w = v.weights[i] * b;
            if w <= 0.0 {
                continue;
            }
            let j = v.joints[i];
            match (0..used).find(|&k| joints[k] == j) {
                Some(k) => weights[k] += w,
                None => {
                    joints[used] = j;
                    weights[used] = w;
                    used += 1;
                }
            }
        }
    }

    select_top(&joints[..used], &weights[..used])
}

/// Pick the `INFLUENCES` heaviest entries, renormalised.
///
/// Written as a fixed-size selection rather than a sort, deliberately: `tests/determinism_lint.rs`
/// forbids unannotated sorts, and a selection with an explicit tiebreak needs no annotation because it
/// has no tie to resolve ambiguously. Equal weights resolve by **lower input index**, so the result is
/// a pure function of the input order rather than of whatever the comparator happened to see first.
fn select_top(joints: &[u16], weights: &[f32]) -> ([u16; INFLUENCES], [f32; INFLUENCES]) {
    let mut out_j = [0u16; INFLUENCES];
    let mut out_w = [0.0f32; INFLUENCES];
    let mut taken = [false; 12];

    for slot in 0..INFLUENCES {
        let mut best: Option<usize> = None;
        for k in 0..weights.len() {
            if taken[k] {
                continue;
            }
            match best {
                // Strictly-greater only, so a tie keeps the earlier index.
                Some(b) if weights[k] <= weights[b] => {}
                _ => best = Some(k),
            }
        }
        let Some(b) = best else { break };
        taken[b] = true;
        out_j[slot] = joints[b];
        out_w[slot] = weights[b];
    }

    let sum: f32 = out_w.iter().sum();
    if sum > 0.0 {
        for w in out_w.iter_mut() {
            *w /= sum;
        }
    }
    (out_j, out_w)
}

/// Uniformly-distributed barycentrics from a point in the unit square.
///
/// The `sqrt` map is the standard area-preserving one; sampling `(s, t)` directly as `(b1, b2)` would
/// bunch roots toward one corner of every triangle.
#[inline]
pub(super) fn barycentric_from_unit_square(s: f32, t: f32) -> Vec3 {
    let u = s.clamp(0.0, 1.0).sqrt();
    let t = t.clamp(0.0, 1.0);
    Vec3::new(1.0 - u, u * (1.0 - t), u * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin(joints: [u16; 4], weights: [f32; 4]) -> VertexSkin {
        VertexSkin { joints, weights }
    }

    fn rigid(a: &Affine3A, p: Vec3) -> Vec3 {
        a.transform_point3(p)
    }

    /// True linear-blend skinning of one vertex, for the A/B comparisons below.
    fn skin_vertex(v: &VertexSkin, p: Vec3, palette: &[Affine3A]) -> Vec3 {
        let mut acc = Vec3::ZERO;
        for i in 0..INFLUENCES {
            let w = v.weights[i];
            if w > 0.0 {
                acc += rigid(&palette[v.joints[i] as usize], p) * w;
            }
        }
        acc
    }

    fn bind_of(rest: Vec3, normal: Vec3, j: [u16; 4], w: [f32; 4]) -> RootBind {
        RootBind { rest, normal, slots: j, weights: w }
    }

    #[test]
    fn blended_weights_are_a_partition_of_unity() {
        let a = skin([0, 1, 0, 0], [0.7, 0.3, 0.0, 0.0]);
        let b = skin([1, 2, 0, 0], [0.5, 0.5, 0.0, 0.0]);
        let c = skin([2, 3, 4, 5], [0.4, 0.3, 0.2, 0.1]);
        let (_, w) = blend_influences([&a, &b, &c], Vec3::new(0.2, 0.5, 0.3));
        let sum: f32 = w.iter().sum();
        assert!((sum - 1.0).abs() < 1.0e-6, "weights sum to {sum}, not 1");
        assert!(w.iter().all(|x| *x >= 0.0), "no weight may be negative: {w:?}");
    }

    /// At a vertex, blending must reproduce that vertex's own influences exactly.
    #[test]
    fn blending_at_a_vertex_reproduces_that_vertexs_influences() {
        let a = skin([7, 9, 0, 0], [0.6, 0.4, 0.0, 0.0]);
        let b = skin([1, 2, 3, 4], [0.25, 0.25, 0.25, 0.25]);
        let c = skin([5, 6, 0, 0], [0.5, 0.5, 0.0, 0.0]);
        let (j, w) = blend_influences([&a, &b, &c], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(j[0], 7);
        assert_eq!(j[1], 9);
        assert!((w[0] - 0.6).abs() < 1.0e-6, "got {:?}", w);
        assert!((w[1] - 0.4).abs() < 1.0e-6, "got {:?}", w);
    }

    /// A joint appearing on more than one vertex must ACCUMULATE, not appear twice and crowd out a
    /// genuinely distinct influence. This is the bug a concatenate-and-truncate would have.
    ///
    /// The barycentrics are deliberately NOT symmetric: at (0.25, 0.25, 0.5) the two joints would tie at
    /// 0.5 each and the assertion would only be testing the tiebreak. 0.6 vs 0.4 is what proves joint 3
    /// summed its two contributions rather than either one winning alone.
    #[test]
    fn a_joint_shared_by_two_vertices_accumulates_instead_of_duplicating() {
        let a = skin([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let b = skin([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let c = skin([8, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let (j, w) = blend_influences([&a, &b, &c], Vec3::new(0.3, 0.3, 0.4));
        assert_eq!(j[0], 3, "joint 3 totals 0.6 and must lead joint 8's 0.4");
        assert_eq!(j[1], 8);
        assert!((w[0] - 0.6).abs() < 1.0e-6, "joint 3 must total 0.3+0.3, got {w:?}");
        assert!((w[1] - 0.4).abs() < 1.0e-6, "got {w:?}");
        assert!(w[2] == 0.0 && w[3] == 0.0, "only two distinct joints exist: {w:?}");
    }

    #[test]
    fn blending_keeps_the_four_heaviest_of_twelve() {
        let a = skin([0, 1, 2, 3], [0.4, 0.3, 0.2, 0.1]);
        let b = skin([4, 5, 6, 7], [0.4, 0.3, 0.2, 0.1]);
        let c = skin([8, 9, 10, 11], [0.7, 0.2, 0.06, 0.04]);
        let (j, w) = blend_influences([&a, &b, &c], Vec3::new(1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0));
        // Heaviest four raw contributions: 8 (0.233), 0 (0.133), 4 (0.133), then 1 and 5 tie at 0.1 —
        // the tie resolves to the earlier input index, which is 1.
        assert_eq!(j[0], 8);
        assert!(j.contains(&0) && j.contains(&4), "the two 0.4-weighted leads must survive: {j:?}");
        let sum: f32 = w.iter().sum();
        assert!((sum - 1.0).abs() < 1.0e-6, "the kept subset must renormalise, got {sum}");
    }

    /// Equal weights must resolve deterministically, not by whichever the comparison saw first.
    #[test]
    fn tied_weights_resolve_to_the_lower_input_index() {
        let joints = [10u16, 11, 12, 13, 14];
        let weights = [0.2f32, 0.2, 0.2, 0.2, 0.2];
        let (j, _) = select_top(&joints, &weights);
        assert_eq!(j, [10, 11, 12, 13], "a five-way tie must keep the first four, in order");
    }

    #[test]
    fn eval_root_on_an_identity_palette_returns_the_bind_pose() {
        let bind = bind_of(Vec3::new(0.1, 0.2, 0.3), Vec3::Y, [0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let f = eval_root(&bind, &[Affine3A::IDENTITY]);
        assert!((f.pos - bind.rest).length() < 1.0e-6, "got {:?}", f.pos);
        assert!((f.normal - Vec3::Y).length() < 1.0e-6, "got {:?}", f.normal);
    }

    /// The Valkyrie case: one rigid joint at full weight. This must be exact, not approximate — the
    /// scalp cap is ~100% weighted to `head`, so any error here is error the superseded single-bone
    /// path did not have.
    #[test]
    fn eval_root_follows_a_single_rigid_joint_exactly() {
        let q = Quat::from_euler(EulerRot::XYZ, 0.3, -1.1, 0.7);
        let a = Affine3A::from_rotation_translation(q, Vec3::new(4.0, -2.0, 9.0));
        let rest = Vec3::new(0.02, 0.11, -0.06);
        let bind = bind_of(rest, Vec3::Z, [0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);

        let f = eval_root(&bind, &[a]);
        assert!((f.pos - a.transform_point3(rest)).length() < 1.0e-5, "pos {:?}", f.pos);
        assert!((f.normal - (q * Vec3::Z)).length() < 1.0e-5, "normal {:?}", f.normal);
    }

    /// A root with no live influences must fall back to its bind pose rather than collapsing to the
    /// origin — an unskinned host is a legitimate groom target, not an error.
    #[test]
    fn eval_root_with_zero_weights_returns_the_bind_pose() {
        let bind = bind_of(Vec3::new(1.0, 2.0, 3.0), Vec3::X, [0; 4], [0.0; 4]);
        let f = eval_root(&bind, &[Affine3A::from_translation(Vec3::splat(50.0))]);
        assert_eq!(f.pos, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(f.normal, Vec3::X);
    }

    /// A slot the palette does not contain must be skipped, not panic. `tests/panic_budget.rs` is a
    /// ratchet; indexing a palette by baked data is exactly where an out-of-bounds would come from.
    #[test]
    fn eval_root_ignores_a_slot_beyond_the_palette() {
        let bind = bind_of(Vec3::X, Vec3::Y, [0, 99, 0, 0], [0.5, 0.5, 0.0, 0.0]);
        let f = eval_root(&bind, &[Affine3A::IDENTITY]);
        assert!(f.pos.is_finite() && f.normal.is_finite(), "must degrade, not panic or NaN");
    }

    /// The theorem the whole scheme rests on: when all three vertices share one influence set,
    /// blend-then-skin and skin-then-blend are the same arithmetic. Asserted, not asserted-in-a-comment.
    #[test]
    fn blend_then_skin_matches_skin_then_blend_on_a_uniform_triangle() {
        let palette = [
            Affine3A::from_rotation_translation(Quat::from_rotation_y(0.9), Vec3::new(1.0, 0.0, 0.0)),
            Affine3A::from_rotation_translation(Quat::from_rotation_x(-0.4), Vec3::new(0.0, 2.0, 0.0)),
        ];
        // One shared influence set across all three vertices.
        let s = skin([0, 1, 0, 0], [0.6, 0.4, 0.0, 0.0]);
        let (v0, v1, v2) = (Vec3::ZERO, Vec3::new(0.1, 0.0, 0.0), Vec3::new(0.0, 0.1, 0.0));
        let bary = Vec3::new(0.2, 0.3, 0.5);
        let rest = v0 * bary.x + v1 * bary.y + v2 * bary.z;

        let (j, w) = blend_influences([&s, &s, &s], bary);
        let ours = eval_root(&bind_of(rest, Vec3::Y, j, w), &palette).pos;

        let truth = skin_vertex(&s, v0, &palette) * bary.x
            + skin_vertex(&s, v1, &palette) * bary.y
            + skin_vertex(&s, v2, &palette) * bary.z;

        assert!((ours - truth).length() < 1.0e-6, "ours {ours:?} != truth {truth:?}");
    }

    /// A seam fixture at a realistic distance from the joint it bends about.
    ///
    /// The offset matters and an earlier version of these tests got it wrong: with the triangle only
    /// 3 mm from the pivot the bend barely displaces it, both schemes look fine, and the A/B below
    /// measured 4x instead of the real 31x. 5 cm is crab-carapace scale — the case that actually ships.
    fn seam_fixture() -> ([Affine3A; 2], [VertexSkin; 3], [Vec3; 3], Vec3) {
        let bend = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        // Joint 0 is the parent (identity); joint 1 pivots 90 degrees about the origin.
        let palette = [Affine3A::IDENTITY, Affine3A::from_quat(bend)];
        // Weights swing 0.8 -> 0.3 across the triangle: a genuine seam, not a uniform patch.
        let skins = [
            skin([0, 1, 0, 0], [0.80, 0.20, 0.0, 0.0]),
            skin([0, 1, 0, 0], [0.55, 0.45, 0.0, 0.0]),
            skin([0, 1, 0, 0], [0.30, 0.70, 0.0, 0.0]),
        ];
        let verts = [
            Vec3::new(-0.005, 0.05, 0.0),
            Vec3::new(0.005, 0.05, 0.0),
            Vec3::new(0.000, 0.058, 0.0),
        ];
        let bary = Vec3::splat(1.0 / 3.0);
        (palette, skins, verts, bary)
    }

    fn seam_truth(skins: &[VertexSkin; 3], verts: &[Vec3; 3], bary: Vec3, palette: &[Affine3A]) -> Vec3 {
        let b = [bary.x, bary.y, bary.z];
        (0..3).map(|k| skin_vertex(&skins[k], verts[k], palette) * b[k]).sum()
    }

    /// The crab case: bound the approximation instead of pretending it is exact.
    ///
    /// Measured at 1.1 mm, and notably **independent of how far the triangle sits from the pivot** — the
    /// error is a function of the intra-triangle weight variation alone, which is why a 2 mm bound is a
    /// real bound and not a fit to one fixture.
    #[test]
    fn blend_then_skin_stays_close_to_true_skinning_across_a_bending_seam() {
        let (palette, skins, verts, bary) = seam_fixture();
        let rest: Vec3 = (0..3).map(|k| verts[k] * [bary.x, bary.y, bary.z][k]).sum();
        let truth = seam_truth(&skins, &verts, bary, &palette);

        let (j, w) = blend_influences([&skins[0], &skins[1], &skins[2]], bary);
        let err = (eval_root(&bind_of(rest, Vec3::Y, j, w), &palette).pos - truth).length();
        assert!(err < 2.0e-3, "seam error {err} m exceeds the 2 mm bound");
    }

    /// The A/B that justifies the design, and the research claim measured in this repo rather than
    /// restated: binding to the single heaviest joint is far worse across a bending seam. Measured at
    /// 31x (1.1 mm vs 34 mm) at crab-carapace scale; asserted at 10x for headroom.
    #[test]
    fn single_joint_binding_is_far_worse_than_four_joint_across_a_bending_seam() {
        let (palette, skins, verts, bary) = seam_fixture();
        let rest: Vec3 = (0..3).map(|k| verts[k] * [bary.x, bary.y, bary.z][k]).sum();
        let truth = seam_truth(&skins, &verts, bary, &palette);

        let (j, w) = blend_influences([&skins[0], &skins[1], &skins[2]], bary);
        let four = (eval_root(&bind_of(rest, Vec3::Y, j, w), &palette).pos - truth).length();

        // The superseded scheme: bind to the heaviest joint alone, at full weight.
        let one_hot = bind_of(rest, Vec3::Y, [j[0], 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let one = (eval_root(&one_hot, &palette).pos - truth).length();

        assert!(
            one > four * 10.0,
            "four-joint binding must be an order of magnitude better across a seam: four={four} one={one}"
        );
    }

    #[test]
    fn barycentrics_from_the_unit_square_are_valid_and_sum_to_one() {
        for (s, t) in [(0.0, 0.0), (1.0, 1.0), (0.5, 0.25), (0.13, 0.87), (1.0, 0.0), (0.0, 1.0)] {
            let b = barycentric_from_unit_square(s, t);
            let sum = b.x + b.y + b.z;
            assert!((sum - 1.0).abs() < 1.0e-6, "({s},{t}) summed to {sum}");
            assert!(b.x >= 0.0 && b.y >= 0.0 && b.z >= 0.0, "({s},{t}) gave negative {b:?}");
        }
    }

    /// The `sqrt` map exists to keep the distribution uniform over the triangle. Without it, roots
    /// bunch toward one corner of every triangle — visible as a clump at one edge of the scalp.
    #[test]
    fn barycentric_sampling_is_uniform_over_the_triangle() {
        // The mean barycentric of a uniform distribution over a triangle is (1/3, 1/3, 1/3).
        let n = 4096u32;
        let mut mean = Vec3::ZERO;
        for i in 0..n {
            let s = crate::util::hash01_u32(i.wrapping_mul(0x9E37_79B1));
            let t = crate::util::hash01_u32(i.wrapping_mul(0x85EB_CA6B) ^ 0x1234_5678);
            mean += barycentric_from_unit_square(s, t);
        }
        mean /= n as f32;
        for (axis, v) in [("x", mean.x), ("y", mean.y), ("z", mean.z)] {
            assert!((v - 1.0 / 3.0).abs() < 0.02, "mean barycentric {axis} = {v}, not ~1/3");
        }
    }
}
