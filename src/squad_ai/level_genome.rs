//! The **level genome**: the game's evolvable *level-generation* config viewed as a mixed
//! structural + continuous parameter set.
//!
//! Sibling of [`super::world_genome`] (which searches the world-*dynamics* config as a flat `f32`
//! vector). A level is not all continuous — some choices are discrete (grid vs graph topology, which
//! coarse-block factorisation, which room types are present), so this genome is a **struct of typed
//! genes** rather than a flat `Vec<f32>`: continuous `Real` knobs, integer `Int` counts, and
//! `Cat`/`bool` structural switches. Each field mutates by its own kernel and clamps to a hard bound,
//! so children are feasible by construction (Skalse et al., arXiv:2209.13085 — "restrict the admissible
//! set"), the same feasibility discipline `world_genome` uses.
//!
//! A genome does not carry the *entire* config — only the evolved knobs. [`decode`] overlays them onto
//! a shipped **base** ([`LevelBase`]), so non-evolved fields (the furniture manifest, the dungeon seed,
//! each room type's area/aspect ranges, the mycelia GPU field parameters) pass through untouched. That
//! keeps one source of truth (`assets/config/config.ron`) and shrinks the search surface to the knobs
//! that actually shape architecture, furniture amount, and mushroom amount.
//!
//! **Readable elites.** [`decode`] produces the same `DungeonConfig` / `MetropolisWeights` /
//! `PlacementDensity` / `MyceliaConfig` a designer authors by hand, so an elite is a diff of level
//! dials a human can read and reject — the reward-hacking guard, exactly as `world_genome` intends.

use rand_chacha::ChaCha8Rng;

use crate::config::PlacementDensity;
use crate::dungeon::{DungeonConfig, NotchConfig, RoomType, Topology, WfcWeights};
use crate::mycelia::MyceliaConfig;
use crate::placement::solvers::metropolis::MetropolisWeights;
use crate::rng::DetRng;

use super::genome::gaussian;

/// The coarse-grid factorisations the search may pick between. Every pair multiplies to 192 tiles per
/// side — the fixed world extent the mycelia field (`CONTROL_SIZE = 192`) and the dungeon both assume —
/// so the dungeon is always 192×192 however the search trades block size against block count. This is
/// the one *architectural* structural gene: few large blocks vs many small ones.
const FACTORS: [(usize, usize); 4] = [(6, 32), (4, 48), (8, 24), (12, 16)];

// ── Hard per-gene bounds (min, max). Every shipped value sits inside its range; the extremes are
//    playable-but-different, never degenerate. Non-rock WFC weights and room weights are floored above
//    zero so their sums are provably positive (the dungeon always has a floor set). ──
const LIMINALITY: (f32, f32) = (0.0, 1.0);
const CORRIDOR_WIDTH: (i32, i32) = (1, 6);
const CORRIDOR_EXTRA: (i32, i32) = (0, 6);
// Doorway width as a fraction of each corridor's carved width (see `dungeon::doorway_width`). Lower
// bound stays > 0 so `dungeon::validate_config`'s `(0,1]` gate always holds after clamp; 1.0 lets a
// mouth open as wide as its corridor. The search tunes how open vs. pinched the map's doorways read.
const DOORWAY_RATIO: (f32, f32) = (0.2, 1.0);
const WFC_ROCK: (f32, f32) = (0.0, 4.0);
const WFC_OTHER: (f32, f32) = (0.05, 4.0);
const NOTCH_CHANCE: (f32, f32) = (0.0, 1.0);
const NOTCH_CORNERS: (i32, i32) = (1, 4);
const NOTCH_DEPTH_MIN: (f32, f32) = (0.0, 0.9);
const NOTCH_DEPTH_EXTRA: (f32, f32) = (0.0, 1.0);
const NOTCH_MIN_SIDE: (i32, i32) = (2, 8);
const GRAPH_SPACING: (f32, f32) = (6.0, 40.0);
const GRAPH_LINK: (f32, f32) = (0.05, 4.0);
const ROOM_WEIGHT: (f32, f32) = (0.05, 3.0);
const TILED: (i32, i32) = (0, 5);
const FREESTANDING: (i32, i32) = (0, 5);
const SCATTER: (i32, i32) = (0, 5);
// Total sconce budget per room. Widened from (0,3) when the furnish pass moved from "one sconce per
// room" to a 3-to-X row along every wall (see furnish.rs Pass 1b): the search now spans unlit rooms
// (0) through fully-lit rooms (a few walls × a handful each), so brightness is a real QD axis.
const WALL_LIGHTS: (i32, i32) = (0, 16);
const MIN_GAP: (f32, f32) = (0.5, 3.0);
const GROUP_NEAR: (f32, f32) = (0.3, 2.5);
const COHERENCE: (f32, f32) = (0.0, 1.0);
const HABITAT_COVERAGE: (f32, f32) = (0.02, 0.6);
const PATCH_SPACING: (f32, f32) = (2.0, 8.0);
const PATCH_RADIUS_MIN: (f32, f32) = (1.0, 5.0);
const PATCH_RADIUS_EXTRA: (f32, f32) = (0.0, 4.0);
const CORRIDOR_INFEST: (f32, f32) = (0.0, 0.5);
const CLUSTER_SPACING: (f32, f32) = (1.5, 6.0);
const CLUSTER_RADIUS: (f32, f32) = (0.3, 1.5);
/// Minimum margin by which a decoded `cluster_spacing` must clear `cluster_radius` — their bounds
/// overlap at 1.5, so `decode` floors the spacing this far past the radius to satisfy the strict
/// `cluster_spacing > cluster_radius` rule (`mycelia::validate_config`) for every mutated child.
const CLUSTER_GAP_MIN: f32 = 0.1;
const CLUSTER_SIZE_MAX: (i32, i32) = (2, 12);
const MAX_FRUIT: (i32, i32) = (5, 120);
const EDGE_NOISE_AMP: (f32, f32) = (0.0, 1.0);

/// The shipped config slices a [`LevelGenome`] evolves against — the band origin for `encode`/`authored`
/// and the base [`decode`] overlays onto. Cloned once from `GameConfig` by the caller.
#[derive(Clone, Debug)]
pub struct LevelBase {
    pub dungeon: DungeonConfig,
    pub metropolis: MetropolisWeights,
    pub density: PlacementDensity,
    pub mycelia: MyceliaConfig,
}

/// The decoded, ready-to-generate level config — the four slices the evaluator hands to
/// `Dungeon::generate`, `furnish_all`, and `habitat::build`.
#[derive(Clone, Debug)]
pub struct LevelPhenotype {
    pub dungeon: DungeonConfig,
    pub metropolis: MetropolisWeights,
    pub density: PlacementDensity,
    pub mycelia: MyceliaConfig,
}

/// The evolvable level config as typed genes. `Clone` so `coevolve::Population` can store it; not
/// serialised directly — an elite is written by [`decode`]-ing to the readable config slices.
#[derive(Clone, Debug)]
pub struct LevelGenome {
    // ── Dungeon architecture ──
    /// Index into [`FACTORS`] — the coarse-block factorisation (few big blocks vs many small).
    pub block_factor: usize,
    pub liminality: f32,
    pub corridor_width: i32,
    /// Corridor width spread: `corridor_width_max = clamp(corridor_width + extra, cw, block)`.
    pub corridor_extra: i32,
    /// Doorway width as a fraction of each corridor's carved width, in `(0,1]` (see [`DOORWAY_RATIO`]).
    pub doorway_ratio: f32,
    /// WFC prototype weights: [rock, dead_end, corridor, corner, tee, cross].
    pub wfc: [f32; 6],
    pub notch_present: bool,
    pub notch_chance: f32,
    pub notch_max_corners: i32,
    pub notch_depth_min: f32,
    /// `depth_max = clamp(depth_min + extra, depth_min, 1)`.
    pub notch_depth_extra: f32,
    pub notch_min_side: i32,
    /// Grid (false) or Poisson/Delaunay Graph (true) topology.
    pub graph: bool,
    pub graph_spacing: f32,
    pub graph_links: [f32; 6],
    /// Per authored room type: whether it may be generated, and its relative weight. Length matches
    /// `base.dungeon.room_types`. At least one is forced present at decode.
    pub room_present: Vec<bool>,
    pub room_weight: Vec<f32>,
    // ── Furniture amount ──
    pub tiled: i32,
    pub freestanding: i32,
    pub scatter: i32,
    pub wall_lights: i32,
    pub min_gap: f32,
    pub group_near: f32,
    pub coherence: f32,
    // ── Mushroom amount ──
    pub habitat_coverage: f32,
    pub patch_spacing: f32,
    pub patch_radius_min: f32,
    /// `patch_radius_max = patch_radius_min + extra`.
    pub patch_radius_extra: f32,
    pub corridor_infest: f32,
    pub cluster_spacing: f32,
    pub cluster_radius: f32,
    pub cluster_size_max: i32,
    pub max_fruit: i32,
    pub edge_noise_amp: f32,
}

// ── Mutation kernels: one per gene kind. Continuous knobs get a range-relative Gaussian kick and clamp
//    to hard bounds; integers round; switches flip with probability `sigma`. No band around an authored
//    value is needed because the clamp is the physical bound, so children are feasible by construction. ──

fn mut_real(v: f32, (lo, hi): (f32, f32), sigma: f32, rng: &mut ChaCha8Rng) -> f32 {
    (v + gaussian(rng) * sigma * (hi - lo)).clamp(lo, hi)
}

fn mut_int(v: i32, (lo, hi): (i32, i32), sigma: f32, rng: &mut ChaCha8Rng) -> i32 {
    let step = (gaussian(rng) * sigma * (hi - lo) as f32).round() as i32;
    (v + step).clamp(lo, hi)
}

fn mut_flip(b: bool, p: f64, rng: &mut ChaCha8Rng) -> bool {
    if rng.unit() < p {
        !b
    } else {
        b
    }
}

fn mut_cat(idx: usize, k: usize, p: f64, rng: &mut ChaCha8Rng) -> usize {
    if k > 1 && rng.unit() < p {
        rng.below(k)
    } else {
        idx.min(k - 1)
    }
}

/// The shipped level as a genome — the mutation origin and the co-evolution's baseline. Lossy in one
/// direction only: `f64` config knobs (WFC + graph weights) are captured as `f32`, so `decode(authored)`
/// matches the shipped config to `f32` precision, not bit-for-bit (see the round-trip test).
pub fn authored(base: &LevelBase) -> LevelGenome {
    let d = &base.dungeon;
    let block_factor = FACTORS
        .iter()
        .position(|&(c, b)| c == d.coarse_w && b == d.block)
        .unwrap_or(0);
    let corridor_width = d.corridor_width as i32;
    let corridor_extra = d.corridor_width_max.map_or(0, |m| m as i32 - corridor_width).max(0);
    let w = &d.wfc_weights;
    let wfc = [
        w.rock as f32,
        w.dead_end as f32,
        w.corridor as f32,
        w.corner as f32,
        w.tee as f32,
        w.cross as f32,
    ];
    let (notch_present, notch_chance, notch_max_corners, notch_depth_min, notch_depth_extra, notch_min_side) =
        match &d.notch {
            Some(n) => (
                true,
                n.chance as f32,
                n.max_corners as i32,
                n.depth_min,
                (n.depth_max - n.depth_min).max(0.0),
                n.min_side as i32,
            ),
            None => (false, 0.5, 2, 0.3, 0.3, 4),
        };
    let (graph, graph_spacing, graph_links) = match &d.topology {
        Topology::Graph { site_spacing, link_weights } => (
            true,
            *site_spacing,
            std::array::from_fn(|i| link_weights[i] as f32),
        ),
        Topology::Grid => (false, 12.0, [1.0; 6]),
    };
    let room_present = vec![true; d.room_types.len()];
    let room_weight = d.room_types.iter().map(|t| t.weight as f32).collect();

    let m = &base.mycelia;
    LevelGenome {
        block_factor,
        liminality: d.liminality,
        corridor_width,
        corridor_extra,
        doorway_ratio: d.doorway_ratio,
        wfc,
        notch_present,
        notch_chance,
        notch_max_corners,
        notch_depth_min,
        notch_depth_extra,
        notch_min_side,
        graph,
        graph_spacing,
        graph_links,
        room_present,
        room_weight,
        tiled: base.density.tiled_per_room as i32,
        freestanding: base.density.freestanding_per_room as i32,
        scatter: base.density.scatter_per_room as i32,
        wall_lights: base.density.wall_lights_per_room as i32,
        min_gap: base.density.freestanding_min_gap,
        group_near: base.density.group_near_max,
        coherence: base.metropolis.coherence as f32,
        habitat_coverage: m.habitat_coverage,
        patch_spacing: m.patch_spacing,
        patch_radius_min: m.patch_radius_min,
        patch_radius_extra: (m.patch_radius_max - m.patch_radius_min).max(0.0),
        corridor_infest: m.corridor_infest_chance,
        cluster_spacing: m.cluster_spacing,
        cluster_radius: m.cluster_radius,
        cluster_size_max: m.cluster_size_max as i32,
        max_fruit: m.max_fruit_bodies as i32,
        edge_noise_amp: m.edge_noise_amp,
    }
}

/// Perturb a level genome: every gene gets its own kernel and clamps to its hard bound, so the child is
/// feasible by construction (no rejection loop). `sigma` scales continuous kicks and doubles as the
/// per-switch flip probability. Reuses `genome::gaussian` — one Gaussian kernel across all three genomes.
pub fn mutate(parent: &LevelGenome, sigma: f32, rng: &mut ChaCha8Rng) -> LevelGenome {
    let p = (sigma as f64).clamp(0.0, 1.0);
    let mut wfc = parent.wfc;
    wfc[0] = mut_real(wfc[0], WFC_ROCK, sigma, rng);
    for w in wfc.iter_mut().skip(1) {
        *w = mut_real(*w, WFC_OTHER, sigma, rng);
    }
    let graph_links = std::array::from_fn(|i| mut_real(parent.graph_links[i], GRAPH_LINK, sigma, rng));
    // Room presence/weight mutate per type; at least one stays present is enforced at decode.
    let room_present: Vec<bool> = parent.room_present.iter().map(|&b| mut_flip(b, p, rng)).collect();
    let room_weight: Vec<f32> = parent.room_weight.iter().map(|&w| mut_real(w, ROOM_WEIGHT, sigma, rng)).collect();

    LevelGenome {
        block_factor: mut_cat(parent.block_factor, FACTORS.len(), p, rng),
        liminality: mut_real(parent.liminality, LIMINALITY, sigma, rng),
        corridor_width: mut_int(parent.corridor_width, CORRIDOR_WIDTH, sigma, rng),
        corridor_extra: mut_int(parent.corridor_extra, CORRIDOR_EXTRA, sigma, rng),
        doorway_ratio: mut_real(parent.doorway_ratio, DOORWAY_RATIO, sigma, rng),
        wfc,
        notch_present: mut_flip(parent.notch_present, p, rng),
        notch_chance: mut_real(parent.notch_chance, NOTCH_CHANCE, sigma, rng),
        notch_max_corners: mut_int(parent.notch_max_corners, NOTCH_CORNERS, sigma, rng),
        notch_depth_min: mut_real(parent.notch_depth_min, NOTCH_DEPTH_MIN, sigma, rng),
        notch_depth_extra: mut_real(parent.notch_depth_extra, NOTCH_DEPTH_EXTRA, sigma, rng),
        notch_min_side: mut_int(parent.notch_min_side, NOTCH_MIN_SIDE, sigma, rng),
        graph: mut_flip(parent.graph, p, rng),
        graph_spacing: mut_real(parent.graph_spacing, GRAPH_SPACING, sigma, rng),
        graph_links,
        room_present,
        room_weight,
        tiled: mut_int(parent.tiled, TILED, sigma, rng),
        freestanding: mut_int(parent.freestanding, FREESTANDING, sigma, rng),
        scatter: mut_int(parent.scatter, SCATTER, sigma, rng),
        wall_lights: mut_int(parent.wall_lights, WALL_LIGHTS, sigma, rng),
        min_gap: mut_real(parent.min_gap, MIN_GAP, sigma, rng),
        group_near: mut_real(parent.group_near, GROUP_NEAR, sigma, rng),
        coherence: mut_real(parent.coherence, COHERENCE, sigma, rng),
        habitat_coverage: mut_real(parent.habitat_coverage, HABITAT_COVERAGE, sigma, rng),
        patch_spacing: mut_real(parent.patch_spacing, PATCH_SPACING, sigma, rng),
        patch_radius_min: mut_real(parent.patch_radius_min, PATCH_RADIUS_MIN, sigma, rng),
        patch_radius_extra: mut_real(parent.patch_radius_extra, PATCH_RADIUS_EXTRA, sigma, rng),
        corridor_infest: mut_real(parent.corridor_infest, CORRIDOR_INFEST, sigma, rng),
        cluster_spacing: mut_real(parent.cluster_spacing, CLUSTER_SPACING, sigma, rng),
        cluster_radius: mut_real(parent.cluster_radius, CLUSTER_RADIUS, sigma, rng),
        cluster_size_max: mut_int(parent.cluster_size_max, CLUSTER_SIZE_MAX, sigma, rng),
        max_fruit: mut_int(parent.max_fruit, MAX_FRUIT, sigma, rng),
        edge_noise_amp: mut_real(parent.edge_noise_amp, EDGE_NOISE_AMP, sigma, rng),
    }
}

/// Overlay the genome's evolved knobs onto the shipped `base`, producing a ready-to-generate config.
/// Couplings the genes can't express independently (corridor spread, notch depth range, patch radius
/// range, "≥1 room type present", and the damp table naming exactly the present room tags) are resolved
/// here. `Err` only on a length mismatch between the genome's room genes and the base room types.
pub fn decode(g: &LevelGenome, base: &LevelBase) -> Result<LevelPhenotype, String> {
    if g.room_present.len() != base.dungeon.room_types.len()
        || g.room_weight.len() != base.dungeon.room_types.len()
    {
        return Err(format!(
            "level genome room genes ({}/{}) do not match base room types ({})",
            g.room_present.len(),
            g.room_weight.len(),
            base.dungeon.room_types.len()
        ));
    }

    let (coarse, block) = FACTORS[g.block_factor.min(FACTORS.len() - 1)];
    let cw = (g.corridor_width.clamp(CORRIDOR_WIDTH.0, block as i32)) as usize;
    let cw_max = (g.corridor_width + g.corridor_extra).clamp(cw as i32, block as i32) as usize;

    let clamp_w = |v: f32, b: (f32, f32)| (v.clamp(b.0, b.1)) as f64;
    let wfc_weights = WfcWeights {
        rock: clamp_w(g.wfc[0], WFC_ROCK),
        dead_end: clamp_w(g.wfc[1], WFC_OTHER),
        corridor: clamp_w(g.wfc[2], WFC_OTHER),
        corner: clamp_w(g.wfc[3], WFC_OTHER),
        tee: clamp_w(g.wfc[4], WFC_OTHER),
        cross: clamp_w(g.wfc[5], WFC_OTHER),
    };

    let notch = if g.notch_present {
        let depth_min = g.notch_depth_min.clamp(NOTCH_DEPTH_MIN.0, NOTCH_DEPTH_MIN.1);
        let depth_max = (depth_min + g.notch_depth_extra.max(0.0)).clamp(depth_min, 1.0);
        Some(NotchConfig {
            chance: g.notch_chance.clamp(NOTCH_CHANCE.0, NOTCH_CHANCE.1) as f64,
            max_corners: g.notch_max_corners.clamp(NOTCH_CORNERS.0, NOTCH_CORNERS.1) as usize,
            depth_min,
            depth_max,
            min_side: g.notch_min_side.clamp(NOTCH_MIN_SIDE.0, NOTCH_MIN_SIDE.1) as usize,
        })
    } else {
        None
    };

    let level = (coarse * block) as f32;
    let topology = if g.graph {
        let site_spacing = g.graph_spacing.clamp(GRAPH_SPACING.0, (level - 2.0).max(GRAPH_SPACING.0));
        Topology::Graph {
            site_spacing,
            link_weights: std::array::from_fn(|i| clamp_w(g.graph_links[i], GRAPH_LINK)),
        }
    } else {
        Topology::Grid
    };

    // Room types: keep the present ones (force ≥1 so the dungeon can populate), carrying each type's
    // authored area/aspect ranges but the evolved weight. A weight floor keeps the sum positive.
    let any_present = g.room_present.iter().any(|&p| p);
    let mut room_types: Vec<RoomType> = Vec::new();
    for (i, base_rt) in base.dungeon.room_types.iter().enumerate() {
        if any_present && !g.room_present[i] {
            continue;
        }
        let mut rt = base_rt.clone();
        rt.weight = g.room_weight[i].clamp(ROOM_WEIGHT.0, ROOM_WEIGHT.1) as f64;
        room_types.push(rt);
    }

    let dungeon = DungeonConfig {
        coarse_w: coarse,
        coarse_h: coarse,
        block,
        corridor_width: cw,
        corridor_width_max: Some(cw_max),
        doorway_ratio: g.doorway_ratio.clamp(DOORWAY_RATIO.0, DOORWAY_RATIO.1),
        seed: base.dungeon.seed,
        max_attempts: base.dungeon.max_attempts,
        liminality: g.liminality.clamp(LIMINALITY.0, LIMINALITY.1),
        wfc_weights,
        room_types,
        notch,
        topology,
    };

    let mut metropolis = base.metropolis.clone();
    metropolis.coherence = g.coherence.clamp(COHERENCE.0, COHERENCE.1) as f64;

    let density = PlacementDensity {
        tiled_per_room: g.tiled.clamp(TILED.0, TILED.1) as usize,
        freestanding_per_room: g.freestanding.clamp(FREESTANDING.0, FREESTANDING.1) as usize,
        scatter_per_room: g.scatter.clamp(SCATTER.0, SCATTER.1) as usize,
        wall_lights_per_room: g.wall_lights.clamp(WALL_LIGHTS.0, WALL_LIGHTS.1) as usize,
        freestanding_min_gap: g.min_gap.clamp(MIN_GAP.0, MIN_GAP.1),
        group_near_max: g.group_near.clamp(GROUP_NEAR.0, GROUP_NEAR.1),
    };

    // Mushroom amount: overlay the evolved knobs, and rebuild the damp table to name EXACTLY the present
    // room tags so `validate_damp_coverage` holds (a dropped room type must drop from the damp table too).
    let mut mycelia = base.mycelia.clone();
    let radius_min = g.patch_radius_min.clamp(PATCH_RADIUS_MIN.0, PATCH_RADIUS_MIN.1);
    mycelia.habitat_coverage = g.habitat_coverage.clamp(HABITAT_COVERAGE.0, HABITAT_COVERAGE.1);
    mycelia.patch_spacing = g.patch_spacing.clamp(PATCH_SPACING.0, PATCH_SPACING.1);
    mycelia.patch_radius_min = radius_min;
    mycelia.patch_radius_max = radius_min + g.patch_radius_extra.clamp(PATCH_RADIUS_EXTRA.0, PATCH_RADIUS_EXTRA.1);
    mycelia.corridor_infest_chance = g.corridor_infest.clamp(CORRIDOR_INFEST.0, CORRIDOR_INFEST.1);
    // cluster_spacing must strictly EXCEED cluster_radius (`mycelia::validate_config`: neighbouring
    // flushes merge otherwise). The two genes have overlapping hard bounds — both reach 1.5 — so a
    // mutated child can collide them. Resolve the coupling in decode, exactly like the patch-radius and
    // notch-depth ranges above: floor the spacing to a small margin past the radius so every child is
    // feasible by construction (no rejection loop). Leaves the shipped 3.0 > 0.7 untouched.
    let cluster_radius = g.cluster_radius.clamp(CLUSTER_RADIUS.0, CLUSTER_RADIUS.1);
    mycelia.cluster_radius = cluster_radius;
    mycelia.cluster_spacing = g
        .cluster_spacing
        .clamp(CLUSTER_SPACING.0, CLUSTER_SPACING.1)
        .max(cluster_radius + CLUSTER_GAP_MIN);
    mycelia.cluster_size_max = g.cluster_size_max.clamp(CLUSTER_SIZE_MAX.0, CLUSTER_SIZE_MAX.1) as u32;
    mycelia.max_fruit_bodies = g.max_fruit.clamp(MAX_FRUIT.0, MAX_FRUIT.1) as u32;
    mycelia.edge_noise_amp = g.edge_noise_amp.clamp(EDGE_NOISE_AMP.0, EDGE_NOISE_AMP.1);
    let present_tags: Vec<&str> = dungeon.room_types.iter().map(|t| t.tag.as_str()).collect();
    mycelia
        .damp_weights
        .retain(|row| present_tags.contains(&row.tag.as_str()));
    // A dropped room type must ALSO drop from every species' `room_affinity` — the same reason the damp
    // table is pruned above, and the half that was missed: `mycelia::validate_species` rejects an affinity
    // naming a type the dungeon never emits, and `config::load_game_config` runs it. So a genome that
    // dropped, say, "living" while a species still named it decoded to a config the REAL loader rejects —
    // `train apply` baked exactly that and left `config.ron` unloadable.
    //
    // Pruning is the right move rather than forbidding the drop: it is semantically neutral. Per
    // `fruit::species_weight`, an unlisted room is neutral (weight 1.0), so a species that loses its
    // preferred tag simply stops preferring a room that no longer exists — it never becomes ineligible.
    for s in &mut mycelia.species {
        s.room_affinity.retain(|a| present_tags.contains(&a.tag.as_str()));
    }

    Ok(LevelPhenotype { dungeon, metropolis, density, mycelia })
}

/// The genome-level feasibility gate: decode, then run every real subsystem validator (dungeon
/// generation invariants, mycelia config + damp coverage, placement density). `mutate` produces
/// feasible children by construction; this catches genomes built any other way. One `Err`, no fallback.
pub fn is_feasible(g: &LevelGenome, base: &LevelBase) -> Result<(), String> {
    let p = decode(g, base)?;
    crate::dungeon::validate_config(&p.dungeon)?;
    crate::config::validate_density(&p.density)?;
    crate::mycelia::validate_config(&p.mycelia)?;
    crate::mycelia::validate_damp_coverage(&p.mycelia, &p.dungeon.room_types)?;
    // Mirror the REAL config load path exactly: `config::load_game_config` runs BOTH cross-slice mycelia
    // validators (damp coverage AND species). Running only the first is how an elite that dropped a room
    // type still named by a species' `room_affinity` passed this gate, got baked by `train apply`, and made
    // `config.ron` unloadable at the next startup. A feasibility gate that validates less than the loader
    // does is not a gate — every validator the loader runs on these slices belongs here.
    crate::mycelia::validate_species(&p.mycelia, &p.dungeon.room_types)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    fn base() -> LevelBase {
        let cfg = crate::config::load_game_config().expect("shipped game config");
        LevelBase {
            dungeon: cfg.dungeon,
            metropolis: cfg.placement.metropolis,
            density: cfg.placement.density,
            mycelia: cfg.mycelia,
        }
    }

    #[test]
    fn authored_decodes_to_the_shipped_config_within_f32_precision() {
        // encode ∘ decode is the identity on the shipped config up to f32 precision (the WFC/graph
        // weights are f64 in config but f32 in the genome), and every structural choice is exact.
        let base = base();
        let p = decode(&authored(&base), &base).expect("authored decodes");
        // Structural: exact.
        assert_eq!(p.dungeon.coarse_w, base.dungeon.coarse_w);
        assert_eq!(p.dungeon.coarse_h, base.dungeon.coarse_h);
        assert_eq!(p.dungeon.block, base.dungeon.block);
        assert_eq!(p.dungeon.corridor_width, base.dungeon.corridor_width);
        assert_eq!(p.dungeon.corridor_width_max, base.dungeon.corridor_width_max);
        assert!(
            (p.dungeon.doorway_ratio - base.dungeon.doorway_ratio).abs() < 1e-6,
            "doorway_ratio must round-trip: got {} vs {}",
            p.dungeon.doorway_ratio,
            base.dungeon.doorway_ratio
        );
        assert_eq!(p.dungeon.room_types.len(), base.dungeon.room_types.len());
        assert!(matches!(p.dungeon.topology, Topology::Grid));
        assert!(p.dungeon.notch.is_some());
        assert_eq!(p.density.tiled_per_room, base.density.tiled_per_room);
        assert_eq!(p.density.freestanding_per_room, base.density.freestanding_per_room);
        assert_eq!(p.mycelia.max_fruit_bodies, base.mycelia.max_fruit_bodies);
        assert_eq!(p.mycelia.cluster_size_max, base.mycelia.cluster_size_max);
        // Continuous: within f32 epsilon.
        assert!((p.dungeon.liminality - base.dungeon.liminality).abs() < 1e-4);
        assert!((p.dungeon.wfc_weights.rock - base.dungeon.wfc_weights.rock).abs() < 1e-4);
        assert!((p.dungeon.wfc_weights.tee - base.dungeon.wfc_weights.tee).abs() < 1e-4);
        assert!((p.mycelia.habitat_coverage - base.mycelia.habitat_coverage).abs() < 1e-6);
        assert!((p.mycelia.patch_radius_max - base.mycelia.patch_radius_max).abs() < 1e-4);
        // The authored genome must pass every real validator.
        is_feasible(&authored(&base), &base).expect("shipped level is feasible");
    }

    #[test]
    fn dropping_a_room_type_prunes_every_reference_to_it() {
        // REGRESSION. A levels elite dropped a room type ("living") that `mycelia.species[1].room_affinity`
        // still named. `decode` pruned the damp table but NOT the species affinity, and `is_feasible` ran
        // `validate_damp_coverage` but NOT `validate_species` — so the genome passed the gate, `train apply`
        // baked it, and the next startup died in `config::load_game_config`:
        //   "mycelia.species[1] ...: room_affinity names room type "living", which dungeon.room_types never
        //    emits"
        // leaving `config.ron` unloadable. Pin BOTH halves: decode prunes every cross-slice reference, and
        // the gate runs exactly the validators the real loader runs.
        let base = base();
        // Pick a type some species actually names — the case that used to break.
        let referenced = base
            .mycelia
            .species
            .iter()
            .flat_map(|s| s.room_affinity.iter())
            .map(|a| a.tag.clone())
            .find(|tag| base.dungeon.room_types.iter().any(|rt| &rt.tag == tag))
            .expect("some species names a room type the shipped dungeon emits");
        let idx = base
            .dungeon
            .room_types
            .iter()
            .position(|rt| rt.tag == referenced)
            .expect("the named type is emitted");

        let mut g = authored(&base);
        g.room_present[idx] = false; // the others stay present, so this one really is dropped

        let p = decode(&g, &base).expect("decodes");
        assert!(
            !p.dungeon.room_types.iter().any(|rt| rt.tag == referenced),
            "the room type must actually be dropped, or this proves nothing"
        );
        assert!(
            !p.mycelia.species.iter().any(|s| s.room_affinity.iter().any(|a| a.tag == referenced)),
            "no species may still name a dropped room type"
        );
        assert!(
            !p.mycelia.damp_weights.iter().any(|r| r.tag == referenced),
            "the damp table must drop it too"
        );

        // The gate, and the exact cross-slice validators `config::load_game_config` runs on these slices.
        is_feasible(&g, &base).expect("dropping a referenced room type must stay feasible");
        crate::mycelia::validate_species(&p.mycelia, &p.dungeon.room_types)
            .expect("the loader's species check must pass on a decoded elite");
        crate::mycelia::validate_damp_coverage(&p.mycelia, &p.dungeon.room_types)
            .expect("the loader's damp check must pass on a decoded elite");
    }

    #[test]
    fn mutation_stays_feasible_across_many_draws() {
        // Feasibility by construction: every clamped child passes all subsystem validators. Guards the
        // couplings (corridor spread, notch depth, patch radius, ≥1 room, damp table) hold under mutation.
        let base = base();
        let authored = authored(&base);
        let mut rng = seeded(0x1E4E1_C0DE);
        for _ in 0..300 {
            let child = mutate(&authored, 0.3, &mut rng);
            is_feasible(&child, &base)
                .unwrap_or_else(|e| panic!("a clamped level child must be feasible, got: {e}"));
        }
    }

    #[test]
    fn a_mutation_actually_moves_something() {
        let base = base();
        let authored = authored(&base);
        let mut rng = seeded(0xF00D);
        // Over a few draws at least one gene must change (guards a frozen kernel / zero-scale bug).
        let moved = (0..8).any(|_| {
            let c = mutate(&authored, 0.5, &mut rng);
            c.liminality != authored.liminality
                || c.block_factor != authored.block_factor
                || c.habitat_coverage != authored.habitat_coverage
                || c.max_fruit != authored.max_fruit
                || c.wfc != authored.wfc
        });
        assert!(moved, "mutation changed nothing across 8 draws");
    }

    #[test]
    fn dropping_all_room_types_keeps_at_least_one() {
        // The "≥1 present" guard: a genome with every room type switched off still decodes to a dungeon
        // with a non-empty room set (and a matching damp table), rather than an invalid empty config.
        let base = base();
        let mut g = authored(&base);
        g.room_present = vec![false; g.room_present.len()];
        let p = decode(&g, &base).expect("decodes");
        assert!(!p.dungeon.room_types.is_empty(), "must keep ≥1 room type");
        assert_eq!(
            p.mycelia.damp_weights.len(),
            p.dungeon.room_types.len(),
            "damp table must name exactly the present room tags"
        );
        is_feasible(&g, &base).expect("all-off genome is still feasible");
    }
}
