# Research & Implementation Plan: A Rust Crate for Automated Grasp Baking (`grasp-*`)

*Suggested filename: `2026-08-07-grasp-crate-research-and-implementation-plan.md`*

## 1. Executive Summary

The plan is fundamentally sound and well-aligned with how industry actually solves this. The core architectural decisions — render-free core, offline baking, authored-pose + retrieval + IK-correction rather than runtime grasp synthesis, glTF read-only via the `gltf` crate, a `.grip` postcard format — are all correct and de-risked. The single most important validation from the literature: the "grasp baking" decomposition the user proposes is essentially the 1991 Rijpkema & Girard three-phase scheme (classify primitive → preshape → close to contact), and the runtime IK-correction step is exactly what Jason Gregory's *Game Engine Architecture* prescribes for the "left hand drifts off the weapon stock under LERP blending" problem. The user has independently reconstructed the canonical approach.

What should change:

1. **Grip-frame detection on arbitrary meshes (Question A) is the hard part, and the plan under-weights it.** VHACD→cylinder-fitting is a reasonable Tier C fallback but it is fragile on procedural guns, and it is by far the largest schedule risk. Lean much harder on Tier A (declared at generation time) and Tier B (named empties/conventions). Treat geometric detection as best-effort with a mandatory human-review escape hatch, not as a reliable automatic path.
2. **Adopt a synergy / eigengrasp low-dimensional pose space as the pose representation and the finger-closing search space**, not just normalized flexion angles. This is strongly supported (Santello 1998; Ciocarlie & Allen 2009) and makes both retargeting and naturalness materially better for very little extra code.
3. **Version reality: parry3d moved off nalgebra to glam via `glamx` at v0.26 (May 2025), but its glam is not the same glam Bevy 0.19 uses.** As of August 2026 the latest parry3d is 0.32 (glamx 0.1 → nalgebra 0.34 era) and Bevy 0.19 pins glam 0.30.x (bumped 0.29→0.30 via PRs #18130/#18474). This is the highest-priority technical unknown and forces a specific isolation strategy in `grasp-core` (below).
4. **`bevy_gltf` is not reliably render-free**; the plan's instinct to use the raw `gltf` crate for reads is correct and should be treated as a hard rule, not an option.

The realistic effort estimate is **4–7 months for one experienced engineer** to a production-quality v1, with grip-frame detection and rig retargeting being where the estimate is most likely to blow out.

## 2. Literature Synthesis (home-still corpus first)

### 2.1 The core method is a known 1991 result
`10.1145_122718.122754` (Rijpkema & Girard, *Computer Animation of Knowledge-Based Human Grasping*, SIGGRAPH 1991) is the direct ancestor of the whole plan. Its three-phase decomposition — **task initialization** (classify the object as a primitive: block/sphere/cylinder/cone/torus, choosing a grasp strategy from a knowledge base), **target approach** (filter feasible grasp positions, preshape the hand), and **grasp execution** (close fingers to actual contact via collision detection so fingertips touch but do not interpenetrate) — is exactly the pipeline. Their key insight — restrict the "astronomical search space of grasping techniques and grasping locations" to "the much smaller set of frequently used human grasping methods" — is the argument for Tier A/B grip declaration and for a small library of weapon grasp archetypes. **Take from it:** the phase structure, and the principle that primitive classification + preshaping beats blind search.

### 2.2 IK correction of blended poses (the load-bearing corpus item)
The Gregory *Game Engine Architecture, 3rd ed.* passage is the technical justification for the entire runtime design. Verbatim: *"As the character aims the weapon in various directions, we may notice that the left hand no longer aligns properly with the stock at certain aim angles. This kind of joint misalignment is caused by LERP blending. Even if the joints in question are aligned perfectly in clip A and in clip B, LERP blending does not guarantee that those joints will be in alignment when A and B are blended together."* The prescribed fix is a short (2–4 joint) IK chain on the support hand, applied *after* local/global pose computation but *before* the final matrix palette — i.e., after animation-target mutation and before transform propagation, which is exactly the plan's Bevy system ordering. Gregory also warns IK "solves only for the position of a joint" and that orientation alignment needs extra code — directly relevant to the support-hand-on-foregrip case where wrist roll matters.

### 2.3 Contact-based factorization (validates "fit cylinder → contact targets → pose" without ML)
`10.48550_arXiv.2210.09245` (Contact2Grasp) and the ContactOpt / ContactPose line (`10.48550_arXiv.2007.09545`) establish that **factorizing grasp synthesis into (object → contact map → hand pose) is smoother and better-conditioned than object → pose directly**, because "a small change in a valid contact map would likely produce another valid solution" — the contact space is a low-dimensional smooth manifold. This is the theoretical license for the plan's geometric approach: fit a cylinder to the grip, derive contact targets on its surface, and close fingers to those targets. You can implement the object→contact→pose factorization *without ML* using geometric contact targets from a fitted cylinder; the ML papers simply learn what geometry gives you for free on a known handle.

### 2.4 Grasp quality metrics for visual (not force-closure) plausibility
`10.1007_s10514-014-9402-3` (Roa & Suárez, *Grasp quality measures: review and performance*) is the reference for the plan's contact-scoring composite. The directly transferable, lightweight measures (no force-closure computation needed):
- **Orthogonality / alignment to principal axis** (their Q_O): "humans tend to align their hands with the main axis of inertia of the object" — the palm-normal-vs-principal-axis angle. Cheap and perceptually meaningful for a rifle.
- **Distance between object CoM and contact centroid** (Q_DCC).
- **Combination strategies**: they document both serial (use one measure to generate candidates, another to rank) and parallel (weighted sum of normalized measures) composition — precisely the plan's "composite quality score." Use the parallel normalized-weighted-sum form.

### 2.5 Postural synergies / eigengrasps — adopt this
Santello, Flanders & Soechting (1998), *Postural hand synergies for tool use* (J. Neurosci. 18(23):10105–10115, DOI 10.1523/JNEUROSCI.18-23-10105.1998): principal-components analysis of static grasp postures found, verbatim, that *"the first two components could account for >80% of the variance, implying a substantial reduction from the 15 degrees of freedom that were recorded."* Retaining three synergies pushes this to >84% (per the eLife 2016 summary: *"Three hand postural synergies were identified through a principal component analysis (PCA) that accounted for a high fraction (>84%) of variance in the kinematic data across all hand postures... (Santello et al., 1998)"*). Bicchi, Gabiccini & Santello (2011, DOI 10.1098/rstb.2011.0152, a corpus target) show these synergies are *generative*, not just descriptive. Ciocarlie & Allen (2009), *Hand Posture Subspaces for Dexterous Robotic Grasping* (IJRR 28(7):851–867, DOI 10.1177/0278364909105606) demonstrate the synthesis payoff: they optimize grasps over a **2-D "eigengrasp" subspace** with simulated annealing across multiple hand models, cutting the search dimensionality drastically. **Implication for the crate:** represent finger closing as a search over a 2–3-D synergy space rather than 15+ independent joint angles. This reduces the finger-closing solver's search space by roughly an order of magnitude, improves naturalness for free, and gives a natural rig-independent pose representation (synergy coefficients are unitless and retarget across proportions).

MANO (`10.48550_arXiv.2201.02610`, Embodied Hands) is the modern statistical realization of the same idea: it "provides a compact mapping from hand poses to pose blend shape corrections and a linear manifold of pose synergies," and reduces its pose space by a linear PCA embedding. It is compatible with standard LBS graphics engines. Relevant as a *target topology* if a learned refiner is added later, and as evidence that a PCA pose manifold is production-proven.

### 2.6 Retargeting across rigs
`Skeleton-Aware-Networks-for-Deep-Motion-Retargeting` (Aberman et al. 2020) directly addresses "retargeting of skeletons with the same structure but different proportions" — the FP↔TP rig case. Two takeaways usable *without* the neural network: (1) their strong baseline for intra-structural retargeting is simply **copying joint rotations then correcting end-effectors with IK** — exactly the plan (normalized flexion + wrist-local transform + support-hand IK); (2) they confirm naive rotation-copy plus an end-effector IK cleanup is a legitimate, if imperfect, method. The finger-specific retargeting literature is thin; MANO-topology→game-rig retargeting is essentially bespoke in industry, so the plan's rig-relative normalized representation is a reasonable in-house choice, and synergy coefficients are a better-conditioned interchange than raw angles.

### 2.7 Joint limits and swing-twist
`10.1016_j.gmod.2008.03.002` (Unzueta et al., Sequential IK) gives the swing-twist decomposition approach to joint limits: model swing with a spherical parameterization (circumduction angle θ + swing amplitude ψ) bounded by a cubic-spline boundary from measured limits (Kapandji), and limit twist against a reference orientation from the parent. Crucially they note finger/limb joint limits are *coupled* but they deliberately ignore couplings "to get visually pleasing results … to simplify calculations" — endorsement for the plan to use simple per-joint limits plus a DIP≈⅔·PIP coupling heuristic rather than a full biomechanical model. `Elasticity-Inspired-Deformers-for-Character-Articulation` gives the closed-form swing/twist quaternion decomposition (Q = Q_swing · Q_twist, twist about the bone axis) you will implement for both joint limits and any skinning-artifact mitigation.

### 2.8 Convex decomposition (Tier C detection backbone)
The corpus has two directly relevant items: `sig2024_Navigation-Driven_Approximate_Convex_Decomposition` (surveys V-HACD [Mamou 2016], CoACD [Wei et al. 2022], and concavity/volume metrics; notes V-HACD "is directly supported by popular game engines") and `10.1145_2461912.2461934` (VACD for fracture, with the volume-maximizing approximate-convex-hull algorithm). Key practical warnings: surface-based concavity metrics "can cause the method … to cover the opening of a cup with hulls generated from the cup's outer surface" (i.e., decomposition can miss the very concavity — a trigger guard — you care about), and clean/watertight input matters (V-HACD voxelizes to guarantee solidity at some accuracy cost). This is the empirical basis for treating cylinder-from-VHACD as fragile.

### 2.9 Anthropometric grip data (for the graspable-diameter filter)
Human power-grasp cylinder studies converge on a mid-30s-mm optimum. Kong & Lowe (2005), *Int. J. Industrial Ergonomics* 35:495–507, found participants "rated the mid-sized handles (30, 35 and 40 mm) as the most comfortable for maximum grip force exertions," with the comfort-maximizing diameter equal to **19.7% of the user's hand length**; they tested the **25–50 mm** range. Sancho-Bru et al. (*Optimum Tool Handle Diameter for a Cylinder Grip*, PubMed 14605652) estimate "a 33-mm optimum diameter tool handle for the general population," and Rossi et al. (2012) report maximal grip strength for handle diameters between 25 and 40 mm. This validates the plan's ~25–50 mm graspable-diameter filter for pistol grips/foregrips; note a rifle barrel (~15–25 mm) and a scope (~30–40 mm but differently isolated/elongated) will need the elongation/isolation/perpendicularity scoring to disambiguate, since barrel diameter can fall inside the graspable band. Anatomical finger ROM for joint-limit defaults: MCP ~0/90° (flexion), PIP ~0/100°, DIP ~0/70–80°, thumb IP ~0/80°; functional (not maximal) grasp flexions average roughly MCP 61°, PIP 60°, DIP 39° (Hume et al.). Finger closing order in real grips: PIP leads, then DIP/MCP; the thumb starts before the fingers and releases last.

### 2.10 Grasp taxonomy
Feix et al., *The GRASP Taxonomy of Human Grasp Types* (IEEE T-HMS 2016): 33 grasp types, reducible to 17 if object shape is ignored, organized by opposition type (palm/pad/side), virtual-finger assignment, power/precision/intermediate, and thumb position (abducted/adducted). For weapons you need only a small subset: a **power/wrap grasp** (pistol grip, foregrip), a **trigger-finger extension** variant, and a **precision/pad** grasp for small controls. Author a handful of archetypes indexed by this taxonomy rather than trying to synthesize arbitrary grasps.

### 2.11 Robotic grasp-detection methods (what transfers)
GraspNet-1Billion, Contact-GraspNet, AnyGrasp, GPD, Dex-Net all target *physical* parallel-jaw or dexterous grasping from partial point clouds with force-closure success as the metric. What transfers to the *visual* problem: the **contact-point-as-grasp-representation** idea (Contact-GraspNet roots a 4-DOF grasp in observed surface points) and the graspness/affordance-region heuristic. What does *not* transfer: force-closure objectives, gripper-collision-in-clutter reasoning, and the assumption of a depth sensor. For a known full mesh with a declared/fitted handle, these are overkill; the corpus's Roa & Suárez alignment/orthogonality measures plus contact count are the right lightweight surrogate.

### 2.12 Perceptual plausibility (what "looks right" means)
The literature gives **no quantitative perceptual threshold** for tolerable finger-object penetration in grasp animation — state this openly. The strongest sourced findings: viewers judge collision/contact plausibility primarily from *visual* cues and tolerance is context-dependent (ACM TAP, DOI 10.1145/1577755.1577758); there is a documented uncanny valley specifically for hands (Poliakoff et al. 2013, DOI 10.1068/p7569; D'Alonzo et al. 2019, DOI 10.1038/s41598-019-55478-z); and practitioners assert millimeter-scale hand-object errors are visually objectionable — ContactOpt (Grady et al., CVPR 2021, arXiv 2104.07267) states plainly that for grasp realism "millimeters matter." Practical takeaway: set the skin margin at ~1–2 mm and rely on human review for final acceptance.

## 3. Algorithm Specification

### Stage 1 — Grip frame resolution (the hard problem)
A grip frame is a `Transform` (position + basis: grip axis = the cylinder/long axis the palm wraps, palm normal, and thumb-side direction) plus a `graspable_radius` and a grasp archetype id.

- **Tier A — declared (strongly preferred).** When the weapon is procedurally generated (per the corpus item Deolikar & Lupiani, the user's gun generator in Blender), emit grip frames as part of generation. This makes the hardest problem disappear. *Push as much volume as possible through this tier.*
- **Tier B — named convention.** Read glTF nodes/empties named by convention (`grip.primary`, `grip.support`, `grip.trigger`). The `gltf` crate exposes the node hierarchy and names (enable the `names` feature). Cheap, robust, artist-controllable.
- **Tier C — geometric detection (best-effort).** Pipeline:
  1. `parry3d::transformation::vhacd::VHACD::decompose(&params, &vertices, &indices, true)` with `keep_voxel_to_primitives_map = true`, then `compute_exact_convex_hulls(&vertices, &indices)`. Tune `resolution` (start 64–128), `max_convex_hulls` (~32), `concavity` ~0.01.
  2. For each convex part, fit a cylinder: PCA for the principal axis, project points to get radius/length. (No cylinder-fit crate exists; implement from PCA — a few dozen lines with `glam`/`nalgebra` eigen-decomposition.)
  3. Filter by graspable diameter (25–50 mm) and score by **elongation** (length/radius), **isolation** (few neighboring hulls along the axis), and **perpendicularity to the weapon's global principal axis** (a pistol grip is roughly perpendicular to the barrel; a foregrip too).
  4. Emit candidates worst-first into the review queue. **Do not trust Tier C unattended.**

Honest assessment: Tier C will misfire on thin trigger guards (a concavity VHACD may bridge — see §2.8), on rails/accessories with graspable diameters, and on stylized/procedural geometry with non-manifold or open meshes. Budget for it to get ~50–70% of clean cases right and route the rest to review. This is the correct place to spend a human-in-the-loop, not to chase a perfect detector.

### Stage 2 — Wrist placement
Given the grip frame, place the wrist via a per-rig measured `palm_offset` (a rigid transform from grip frame to wrist joint, measured once per rig from a reference authored grasp). Deterministic, no search.

### Stage 3 — Per-finger closing solver
Two options; recommend the **synergy-space** variant.

- **Baseline (joint-space):** For each finger, flex proximal→distal. At each joint, sweep the attached capsule (`parry3d::shape::Capsule` with per-segment `capsule_radius`) along the rotation arc using `query::cast_shapes` with `ShapeCastOptions` (note the ≥0.13 rename from `time_of_impact`/`TOI` to `cast_shapes`/`ShapeCastHit`; set `target_distance` to a small skin margin and `compute_impact_geometry_on_penetration = true`). Stop at time-of-impact, clamp to joint limits (MCP 0/90, PIP 0/100, DIP 0/70, with DIP≈⅔·PIP coupling), advance to the next joint. Closing order PIP→DIP→MCP per §2.9; thumb first.
- **Recommended (synergy-space):** Parameterize the whole hand pose by 2–3 synergy coefficients (eigengrasp basis authored per archetype, or borrowed from MANO's PCA), and search those coefficients (line search / small argmin problem) until fingers reach contact without penetration. Fewer DOF, more natural intermediate poses, and the coefficients are the rig-independent interchange representation. Use `parry3d::query::cast_shapes` for the contact test as above.

```rust
// grasp-core (no bevy, no I/O)
pub struct RigDescription { pub joints: Vec<JointDesc>, pub flexion_axes: Vec<Vec3>,
    pub limits: Vec<(f32,f32)>, pub capsule_radii: Vec<f32>, pub palm_offset: Isometry }
pub struct GripFrame { pub frame: Isometry, pub grip_axis: Vec3, pub palm_normal: Vec3,
    pub graspable_radius: f32, pub archetype: GraspArchetype }
pub struct NormalizedPose { pub synergy_coeffs: [f32; 3], pub flexion: Vec<f32>,   // rig-relative, normalized 0..1
    pub wrist_local: Isometry }
pub struct ContactScore { pub contacts: u32, pub thumb_opposition_deg: f32,
    pub max_penetration_mm: f32, pub palm_contact: bool, pub spread_variance: f32, pub composite: f32 }
pub fn close_fingers(rig: &RigDescription, mesh: &TriMesh, grip: &GripFrame) -> (NormalizedPose, ContactScore);
pub fn two_bone_ik(root: Isometry, l1: f32, l2: f32, target: Vec3, pole: Vec3) -> (Quat, Quat);
```

### Stage 4 — Penetration relief
After closing, run `parry3d::query::contact` between each finger capsule and the mesh; where penetration exceeds the skin margin, back off the distal joint along its flexion axis by the penetration depth's angular equivalent. Iterate 1–2 passes. Failure mode to watch: relief on one joint reintroducing a gap on another; cap iterations and let the score/review catch residuals.

### Stage 5 — Contact scoring
Composite = weighted normalized sum (Roa & Suárez parallel form) of: contact count (want ≥ N per finger), thumb opposition angle (want opposition to the other virtual finger), max penetration (want ≈ skin margin, penalize both gap and overlap), palm contact boolean, and spread variance (penalize splayed/unnatural fingers). Emit per-bake in `bake_report.json`.

### Stage 6 — Human review gate
Threshold on composite; anything below goes to the `grasp-review` Bevy app worst-first. This is not optional given Tier C's fragility.

### Stage 7 — Rig-relative normalized output
Emit synergy coefficients + normalized flexion + wrist-local `Isometry` so poses retarget FP↔TP. Retargeting = copy normalized coeffs/angles to target rig, recompute wrist from target `palm_offset`, then support-hand two-bone IK cleanup (per Aberman baseline + Gregory).

## 4. Rust / Bevy Ecosystem Findings (verified, August 2026)

| Question | Finding | Date/Version | Impact |
|---|---|---|---|
| parry3d ↔ Bevy 0.19 glam compatibility | **The central risk.** parry migrated nalgebra→glam via `glamx` at **0.26.0 (2025-05-16)**. Latest parry3d **0.32.0 (2026-01-09)** depends on `glamx ^0.1` → `nalgebra ^0.34`; rapier3d 0.34 uses `glamx ^0.3`/glam 0.32. Bevy 0.19.0 (released 2026-06-19, 261 contributors / 1,185 PRs, MSRV Rust 1.95.0) pins **glam 0.30.x** (`bevy_math` depends on glam ^0.30.7; bumped 0.29→0.30 via PRs #18130/#18474). So parry's internal glam (via glamx) is a *different, newer* glam than Bevy's. | Aug 2026 | `grasp-core` must **not** leak parry/glam types across its API. Use glam in the public API at Bevy's version; convert to parry types internally. glamx↔nalgebra conversions exist; glam↔glam across minor versions may need `.to_array()`/`from_array()` bridging. Pin explicitly and build early — milestone 0. |
| Is `bevy_gltf` render-free in 0.19? | Not reliably. Long-standing reports (discussion #10775: "Requested resource `Assets<Shader>` does not exist") show glTF loading dragging in render types. 0.17 made meshes/animation/scenes more render-independent but glTF was conspicuously omitted. | Aug 2026 | **Confirmed: use the raw `gltf` crate for reads in `grasp-io`.** Hard rule, as planned. |
| `LoadTransformAndSave` / asset_processor + glTF | Bug #14189: `asset_processor` with `AssetMode::Processed` caused glTF to load only on first run in 0.14/0.15. Not confirmed fixed for 0.19. | Aug 2026 | Treat the `LoadTransformAndSave` processor path as **experimental/optional**. Do the bake in `grasp-cli` (headless, raw `gltf`), not the Bevy asset pipeline, for CI reliability. |
| Single-keyframe clip under `add_additive_blend` + mask holds a static pose? | Yes — additive blending landed in 0.15 (Add nodes), masks in 0.15; the canonical use case cited in the PR is literally "a character to hold a weapon while performing arbitrary poses." `AnimationGraph::add_clip_with_mask`, `add_additive_blend`, mask = u64 bitfield (1 = node cannot animate that group). Example `animation_masks.rs` exists. | 0.15+ (stable in 0.19) | Plan is viable. Mask locomotion off both hands; additive-blend a one-keyframe grip clip. Test mask+additive interaction (weight propagation was a historical bug class). |
| Rust IK crates tracking 0.19 | `bevy_mod_inverse_kinematics` (Kurble; positional + pole targets), `bevy_fabrik` (FABRIK, no multi-chain), `bevy_animation_graph` (native two-bone IK + masks + clip playback). None guaranteed on 0.19 same-day. | Aug 2026 | **Implement two-bone analytic IK yourself in `grasp-core`** (law-of-cosines, ~40 lines, deterministic, no deps). Do not put a Bevy-version-coupled IK dep on the critical path. |
| `parry3d` shape-casting API | `query::cast_shapes(pos12, vel12, g1, g2, ShapeCastOptions)` (renamed from `time_of_impact`; `TOI`→`ShapeCastHit`, `TOIStatus`→`ShapeCastStatus`). `ShapeCastOptions { max_time_of_impact, target_distance, stop_at_penetration, compute_impact_geometry_on_penetration }`. `cast_shapes_nonlinear` for rotational sweeps. `query::contact(pos12,g1,g2,prediction)`. | parry ≥0.13 rename; current in 0.32 | Use `cast_shapes` for the arc sweep; `compute_impact_geometry_on_penetration=true` for witness points when starting in contact. |
| `PointQuery` / TriMesh flags | `PointQuery::project_point` / `contains_point` available. `TriMeshFlags::ORIENTED` (assume outward orientation, compute pseudo-normals) + `FIX_INTERNAL_EDGES` (clamp contact normals) — set both for reliable signed distance / inside tests on weapon meshes. | current | Needed for penetration sign; set flags at TriMesh construction. |
| VHACD API | `VHACD::decompose(&VHACDParameters, &vertices, &indices, keep_voxel_to_primitives_map: bool)` then `compute_exact_convex_hulls(&vertices,&indices)` (panics if map not kept). `VHACDParameters { concavity, alpha, beta, resolution, plane_downsampling, convex_hull_downsampling, fill_mode, convex_hull_approximation, max_convex_hulls }`; `FillMode::FloodFill{detect_cavities}` for hollow shapes. | current | Directly usable for Tier C. Watch open parry panic issues (#50, #347-class) on degenerate input — wrap in `catch_unwind` or pre-validate watertightness. |
| Determinism | parry changelog: `enhanced-determinism` now enables `glamx/scalar-math` to disable arch-specific SIMD (NEON on arm64, SSE2 on x86_64) that caused float non-associativity in Vec3/Vec4/Quat dot products across platforms. | current | **Enable `enhanced-determinism` for the bake pipeline** so CI (x86) and local (possibly arm64 Mac) produce byte-identical `.grip` output. Accept the speed hit; baking is offline. |
| glTF writing in Rust (if ever needed) | `gltf-json` (part of `gltf-rs`, current `gltf` 1.4) supports building/serializing glTF JSON; `mesh-tools`, `awsm-renderer-glb-export` exist but are niche. | `gltf` 1.4 | Not needed for v1 (write path is `.grip`/postcard). If glTF export is later required, use `gltf-json`. |
| `gltf` crate (read) | `gltf` 1.4 (gltf-rs); `import()` for doc+buffers+images; supports skins, animations, node hierarchy; `names`/`extras` features off by default. | 1.4 | Core of `grasp-io`. Enable `names` + `extras` for Tier B grip conventions. |
| `argmin` | Latest 0.11.0; object-safe `Solver`, backends vec/ndarray/nalgebra, LBFGS/BFGS/Nelder-Mead/particle-swarm/simulated-annealing; `web-time` (WASM-friendly). | 0.11.0 | Use for synergy-space refinement (Nelder-Mead or simulated annealing over 2–3 coeffs, echoing Ciocarlie & Allen's SA-over-eigengrasp approach). Optional; a hand-rolled line search may suffice. |
| Rust ONNX / ML inference (later) | `ort` (ONNX Runtime wrapper, production-grade, execution providers, WASM via tract/candle backends), `candle` (HF, ONNX/TensorRT), `burn` (pure Rust, backend-agnostic), `tract` (pure Rust, portable). | ort docs current Mar 2026 | If a learned MANO-topology refiner is added, export to ONNX and run via `ort`; `tract`/`candle` for pure-Rust/portable. Not needed for v1. |
| PyO3 / Blender Python | Blender 4.5 LTS and 5.0/5.1 (5.1 released 2026-03-17) both ship **CPython 3.11**; `bpy` wheels are `cp311`. abi3 wheels tagged `cp311-abi3` work; targeting 3.11 specifically is safest. | Aug 2026 | `grasp-py` via maturin/PyO3 with `abi3-py311`. A subprocess fallback (CLI called from Blender) avoids ABI coupling entirely and is the lower-risk default. |
| Existing Rust crates doing parts of this | Convex decomposition: parry VHACD (in-tree). Skeletal animation: `bevy_animation`, `bevy_animation_graph`. Collision/geometry: parry. No Rust crate for medial-axis/curve-skeleton, cylinder/superquadric fitting, or handle detection — **implement these yourself.** | Aug 2026 | The novel IP (grip detection, finger solver, scoring) has no off-the-shelf Rust; parry provides the geometry primitives underneath. |

## 5. Revised Implementation Plan

**Milestone 0 — Dependency spike (1 week). DoD:** a workspace where `grasp-core` (glam at Bevy 0.19's version + parry3d 0.32 internal) and a minimal `grasp-bevy` (Bevy 0.19) both compile and share data via glam conversion, with `enhanced-determinism` on. *This validates the #1 risk before any real work.* If glam versions prove irreconcilable, fall back to keeping parry entirely internal with array-based (`[f32;3]`) boundaries.

**Milestone 1 — `grasp-core` geometry + solver (4–6 weeks). DoD:** given a `RigDescription`, a `TriMesh`, and a *declared* `GripFrame` (Tier A only), produce a scored `NormalizedPose` deterministically; two-bone IK unit-tested against closed-form cases; finger closing via `cast_shapes`; penetration relief via `query::contact`; synergy-space parameterization in place.

**Milestone 2 — `grasp-io` (2 weeks). DoD:** read skins/nodes/animations from GLB via `gltf` 1.4; parse Tier B named grip empties; read/write `.grip` via serde+postcard; round-trip test.

**Milestone 3 — `grasp-cli` batch bake (2 weeks). DoD:** clap + rayon headless bake over a directory; emits `bake_report.json` with per-asset composite scores; CI gate fails on regression; byte-identical output across platforms (determinism test).

**Milestone 4 — `grasp-bevy` runtime (3–4 weeks). DoD:** AssetLoader for `.grip`; ECS components; weapon attached to an `ik_hand_gun` carrier node; locomotion masked off both hands; grip pose additive-blended as a one-keyframe clip with animatable weight; support-hand two-bone IK system ordered after animation-target mutation, before transform propagation; visibly correct grip in FP and TP.

**Milestone 5 — `grasp-review` (2–3 weeks). DoD:** Bevy app with renderer; draggable grip-frame gizmo; live re-solve on drag; contact visualization; worst-first triage queue driven by `bake_report.json`.

**Milestone 6 — Tier C geometric detection (3–5 weeks, high variance). DoD:** VHACD→cylinder-fit→score produces grip-frame candidates that clear review for a majority of a test set of clean weapon meshes; everything else routed to review. *Explicitly scoped as best-effort.*

**Milestone 7 (optional) — `grasp-py` (1–2 weeks).** maturin/PyO3 `abi3-py311` or subprocess fallback.

**Changes from the sketched plan:** (a) added Milestone 0 dependency spike; (b) synergy-space solver promoted from "maybe" to recommended; (c) Tier C moved *after* the runtime and review tooling (so the human-in-the-loop exists before the unreliable detector); (d) `LoadTransformAndSave` demoted to optional, CLI is the canonical bake path; (e) IK implemented in-house rather than via a Bevy-coupled crate.

Total: **~4–7 months** for one experienced engineer. Effort most likely to blow out: Milestone 6 (grip detection) and rig retargeting quality (Milestone 1/4 interaction).

## 6. Test Strategy

- **Golden/regression tests for the solver:** commit a set of (rig, mesh, grip)→`.grip` golden outputs; CI re-bakes and diffs. Because bakes are deterministic (§4), diffs must be byte-identical; any drift is a real regression. Standard approach for geometric solvers.
- **Analytic unit tests:** two-bone IK against hand-computed law-of-cosines cases (reachable, unreachable/stretched, singular/straight); swing-twist decomposition against known quaternions; cylinder fit against synthetic cylinders with known axis/radius.
- **Property tests:** closed fingers never penetrate beyond skin margin (`max_penetration_mm ≤ ε`); pose stays within joint limits; synergy coeffs in range.
- **Determinism test:** bake the same input on x86 and arm64 CI runners; assert identical `.grip`. Requires `enhanced-determinism`.
- **Scoring calibration:** hand-label a set of bakes good/bad; tune composite weights so the review threshold has acceptable precision/recall; treat the labeled set as a fixture.
- **Perceptual validation (informal):** no quantitative perceptual penetration threshold exists in the literature (§2.12). Use the ContactOpt "millimeters matter" heuristic — set skin margin at ~1–2 mm and validate by eye in `grasp-review`. Cite Roa & Suárez metrics for objective scoring, but accept that final acceptance is a human judgment call.

## 7. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| parry glam ≠ Bevy glam version conflict blocks the build | Medium-High | High | Milestone 0 spike; keep parry internal to `grasp-core`, array-based public boundary; explicit pins; convert types at the seam. |
| Tier C grip detection unreliable on procedural/thin geometry | High | Medium | Lean on Tier A/B; scope Tier C as best-effort; mandatory review gate built *before* Tier C (Milestone 5 before 6). |
| `bevy_gltf`/asset_processor drags in render deps or has glTF processing bugs | Medium | Medium | Raw `gltf` crate; bake in CLI not asset pipeline; `LoadTransformAndSave` optional. |
| Retargeting FP↔TP looks wrong (proportions differ) | Medium | Medium | Synergy-space representation; wrist recomputed from per-rig `palm_offset`; support-hand IK cleanup; per-rig reference-grasp calibration. |
| Masks + additive blending edge cases in Bevy 0.19 | Low-Medium | Medium | Follow `animation_masks.rs`; integration-test weight propagation; "hold weapon" is the feature's canonical use. |
| VHACD panics on degenerate/non-watertight input (parry #50/#347-class) | Medium | Low-Medium | Pre-validate/repair meshes; `catch_unwind`; `FillMode::FloodFill`; fall back to review. |
| Non-deterministic bakes break CI gating | Medium | Medium | `enhanced-determinism`; cross-platform determinism test in CI. |
| Effort underestimate on detection + retargeting | High | Medium | Front-load Tier A/B; timebox Tier C; treat 7 months as a realistic upper bound. |
| PyO3/Blender ABI coupling breakage | Low | Low | Target `abi3-py311`; subprocess fallback as default. |

## 8. Reading List

**(a) home-still corpus — read these, with doc_ids:**
- `10.1145_122718.122754` (Rijpkema & Girard 1991) — three-phase grasp decomposition; primitive classification; search-space pruning. *The method's origin.*
- `Game Engine Architecture, Third Edition … Jason Gregory` — §12.6 (LERP blending, partial-skeleton blending), §12.7 (IK post-processing), §12.11 (attach points, reference locators, foot IK). *The runtime IK-correction justification; the LERP-misalignment argument verbatim.*
- `10.1007_s10514-014-9402-3` (Roa & Suárez) — grasp quality measures; use orthogonality (Q_O), CoM-centroid distance (Q_DCC), parallel weighted-sum combination for the composite score.
- `10.48550_arXiv.2210.09245` (Contact2Grasp) — object→contact→pose factorization; contact space as smooth manifold. Justifies the geometric contact-target approach.
- `10.48550_arXiv.2007.09545` (ContactPose) — thermal-capture contact maps; ground truth for contact-based scoring intuition.
- `10.48550_arXiv.2201.02610` (Embodied Hands / MANO) — PCA pose manifold / synergies; LBS-compatible; target topology for any later learned refiner.
- `Skeleton-Aware-Networks-for-Deep-Motion-Retargeting` (Aberman 2020) — intra-structural retargeting; rotation-copy + end-effector IK baseline.
- `10.1016_j.gmod.2008.03.002` (Unzueta, Sequential IK) — swing-twist joint limits via spherical parameterization; endorsement for ignoring joint couplings for visual results.
- `Elasticity-Inspired-Deformers-for-Character-Articulation` — closed-form swing/twist quaternion decomposition.
- `sig2024_Navigation-Driven_Approximate_Convex_Decomposition` and `10.1145_2461912.2461934` — VHACD/CoACD survey and approximate-convex-hull algorithm; concavity-metric pitfalls (cup-opening problem).
- `10.1016_j.robot.2011.07.016` (Sahbani et al.) — grasp synthesis survey (analytical vs. empirical); context for why full runtime synthesis is avoided.
- `book_fundamentals_computer_graphics_marschner` — Ch. 17 IK/Jacobian derivation (background for two-bone IK).
- `Interactive-Hand-Pose-Estimation-using-a-Stretch-Sensing-Soft-Glove` — hand pose data-representation reference.
- Corpus also holds GraspXL (`2403.19649`), FastGrasp (`2411.14786`), GOAL (`2112.11454`), Contact-consistency grasps (`2104.03304`), and physically-plausible full-body HOI (`2309.07907`) — read only if a learned later-stage refiner is pursued.

**(b) papers to acquire (not confirmed in corpus):**
- Santello, Flanders & Soechting (1998), *Postural hand synergies for tool use*, J. Neurosci., DOI 10.1523/JNEUROSCI.18-23-10105.1998 — >80% variance in first two PCs; >84% in three (per eLife 2016 summary).
- Bicchi, Gabiccini & Santello (2011), Phil. Trans. R. Soc. B, DOI 10.1098/rstb.2011.0152 — synergies as a generative model.
- Ciocarlie & Allen (2009), *Hand Posture Subspaces for Dexterous Robotic Grasping*, IJRR, DOI 10.1177/0278364909105606 — eigengrasp synthesis via SA over a 2-D subspace.
- Kong & Lowe (2005), *Int. J. Industrial Ergonomics* 35:495–507 — optimum cylinder diameter 30–40 mm ≈ 19.7% of hand length; 25–50 mm tested. Sancho-Bru et al. (PubMed 14605652) — 33-mm general-population optimum.
- Feix et al. (2016), *The GRASP Taxonomy of Human Grasp Types*, IEEE T-HMS — 33 grasp types, archetype selection.
- ContactOpt (Grady et al., CVPR 2021, arXiv 2104.07267) — "millimeters matter"; contact optimization.
- ManipNet (Zhang, Ye, Shiratori, Komura, SIGGRAPH 2021, DOI 10.1145/3450626.3459830; github.com/cghezhang/ManipNet) — wrist+object trajectory → finger motion; **license CC-BY-NC (Oculus project), SDFr portion MIT** — the NC license likely blocks commercial use; treat as a reference architecture only.
- Perceptual: ACM TAP DOI 10.1145/1577755.1577758 (error sensitivity in animation); Poliakoff et al. 2013 (uncanny valley for hands, DOI 10.1068/p7569); D'Alonzo et al. 2019 (DOI 10.1038/s41598-019-55478-z).

**(c) docs / repos / crates:**
- `gltf` 1.4 (gltf-rs) + `gltf-json`; parry3d 0.32 (docs.rs, CHANGELOG for the cast_shapes rename + enhanced-determinism note); Bevy 0.19 release notes + `animation_masks.rs` example; `argmin` 0.11; `ort` (ort.pyke.io) / `candle` / `burn` / `tract`; `bevy_animation_graph`, `bevy_mod_inverse_kinematics`, `bevy_fabrik` (reference only); V-HACD (kmammou/v-hacd) and CoACD upstreams; maturin/PyO3 abi3 docs; Blender `bpy` on PyPI (cp311).

---

### Bottom line
Build it. The architecture is right, the hard parts are known and bounded, and the corpus already contains the load-bearing references (Rijpkema & Girard for the method, Gregory for the runtime IK correction, Roa & Suárez for scoring, MANO/Santello for the synergy pose space). Do the parry↔glam dependency spike in week one; push grip-frame resolution into Tier A/B declaration wherever possible; adopt the synergy pose space; and treat geometric detection (Tier C) as an assist to a human reviewer, not an oracle. Plan for 4–7 months, and expect grip detection and cross-rig retargeting to consume the overrun if there is one.