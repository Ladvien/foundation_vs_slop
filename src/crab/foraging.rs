//! `crab::foraging` — meat-seeking, carrying, scent/field deposits, and nest reproduction (split out of the former `crab.rs`, 2026-07-19 review Finding: large files).

use super::*;

/// World height a hauled chunk rides at — the crew's mouth height (crab seat ~0.05 + model ~0.11 + tooth
/// bone), so the chunk is gripped at the mouths rather than floating overhead.
pub(crate) const CARRY_HEIGHT: f32 = 0.15;

/// Shared handles kept so `nest_reproduce` can spawn new crabs at runtime.
#[derive(Resource)]
pub(crate) struct CrabAssets {
    pub(crate) collider: Handle<Mesh>,
    pub(crate) scene: Handle<WorldAsset>,
}

/// Panic locomotion: crawl *down* the THREAT gradient (away from danger) across the floor, with free
/// cell-by-cell transfer (unlike surface pursuit, flight isn't along the field's gate). Returns the
/// animation state. This is the movement half of the emergent frenzy→scatter.
pub(crate) fn crab_flee(
    motion: &mut CrabMotion,
    stig: &crate::ai::field::Stig,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    // Flee down the THREAT gradient (away from danger); if the field is flat, keep the current heading
    // so the crab keeps moving rather than freezing. `steer_surface` routes this along the graph, so a
    // cornered crab climbs a wall to escape instead of clipping through it.
    let g = stig.gradient(crate::ai::field::FieldId::THREAT_GUN, dungeon, motion.pos);
    let away = Vec3::new(-g.x, 0.0, -g.y);
    let desired = if away.length_squared() > 1.0e-6 {
        away
    } else {
        motion.heading
    };
    if steer_surface(
        motion,
        graph,
        dungeon,
        desired,
        None,
        bc.speed * bc.flee_speed_mul,
        sep,
        dt,
        t,
        bc,
    ) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Mass on a scout's marked sighting by following the **local vectorial rally pheromone** (Tang et al.
/// 2019): the sampled vector already points toward the (moving) prey — the swarm reads it and steers
/// straight along it, routed around walls by `steer_surface`. This is the paper's map-guided "tracking"
/// mode (robots move according to the pheromone map). A crab only enters Rally when the local magnitude
/// clears `RALLY_MIN`, so the vector is non-zero here; a vanishing vector just holds heading.
pub(crate) fn crab_rally(
    motion: &mut CrabMotion,
    rally: &crate::ai::field::RallyField,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    let v = rally.sample(dungeon, motion.pos);
    let desired = if v.length_squared() > 1.0e-6 {
        Vec3::new(v.x, 0.0, v.y)
    } else {
        motion.heading
    };
    if steer_surface(motion, graph, dungeon, desired, None, bc.speed, sep, dt, t, bc) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Aggressive scout roam: range across floor + walls on a heading re-rolled every `bc.scout_wander_interval`
/// (copies `enemy::enemy_seek`'s wander, routed through the wall-aware `steer_surface` so scouts climb).
/// Faster than the swarm forages so scouts cover ground and find prey to report.
pub(crate) fn crab_scout_roam(
    motion: &mut CrabMotion,
    scout: &mut Scout,
    graph: &crate::surface_nav::SurfaceGraph,
    dungeon: &Dungeon,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    scout.wander_timer -= dt;
    if scout.wander_timer <= 0.0 || scout.wander_dir == Vec3::ZERO {
        scout.wander_timer = bc.scout_wander_interval;
        let angle = rand01(&mut scout.rng) * std::f32::consts::TAU;
        scout.wander_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
    }
    if steer_surface(
        motion,
        graph,
        dungeon,
        scout.wander_dir,
        None,
        bc.speed * bc.scout_speed_mul,
        sep,
        dt,
        t,
        bc,
    ) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Move a crab one step along the surface graph toward a `desired` world direction, transferring between
/// patches ONLY through graph gates (`SurfaceGraph::neighbors`) — the same wall-respecting mechanic as
/// flow-field pursuit. It therefore never clips through a wall and can climb onto a wall patch when that
/// is the best-aligned escape. `home` is an exact point the crab walks straight to when that point lies
/// on its current patch (the final approach onto / hold on a gib). Returns whether the crab moved.
#[allow(clippy::too_many_arguments)]
pub(crate) fn steer_surface(
    motion: &mut CrabMotion,
    graph: &crate::surface_nav::SurfaceGraph,
    dungeon: &Dungeon,
    desired: Vec3,
    home: Option<Vec3>,
    speed: f32,
    sep: Vec3,
    dt: f32,
    _t: f32,
    bc: CrabTuning,
) -> bool {
    // Final approach: if the homing point sits on THIS patch's cell, walk straight to it (no gate). Some
    // ⇒ the shared core skips the neighbour-gate scan and steers straight at this point (the on-patch
    // final approach onto / hold on a gib).
    let on_patch_home =
        home.filter(|h| graph.floor_patch_cell(dungeon.world_to_cell(*h)) == Some(motion.patch));

    // Reynolds separation, projected onto the current surface and scaled here (the core adds it as-is, so
    // the `project_tangent(sep, n) * bc.sep_strength` arithmetic stays bit-identical to the old copy).
    let n = graph.patch(motion.patch).normal;
    let push = project_tangent(sep, n) * bc.sep_strength;

    // `_t` is ignored: the shared core recomputes `(NORMAL_EASE * dt).min(1.0)` internally (same formula,
    // same `dt`), so the eased normal/heading are bit-identical. Kept in the signature so the 6 call sites
    // and the intermediate steer helpers (which also use `t` for their own lerps) stay untouched.
    crate::surface_nav::steer_surface_core(
        &mut motion.pos,
        &mut motion.patch,
        &mut motion.normal,
        &mut motion.heading,
        graph,
        desired,
        push,
        speed,
        dt,
        on_patch_home,
    )
}

/// Foraging locomotion: crawl toward meat along walkable floor. Long-range navigation follows the MEAT
/// stigmergy gradient, which — because the field lives only on floor cells and diffuses only between
/// them — flows *around* walls (a proper floor-topology potential field; ACO trail ascent, Dorigo).
/// Only within line-of-sight (`bc.los_range`) of the committed chunk does the crab straight-line home onto
/// the exact gib, then hold within `bc.grab_range` for the lift. A flat local field falls back to steering
/// at the coarse target (the MEAT hotspot for a forager, the chunk for a committed crab) so a crab out
/// of the field's reach still heads the right way instead of freezing. Free cell-by-cell floor transfer.
pub(crate) fn crab_seek_meat(
    motion: &mut CrabMotion,
    stig: &crate::ai::field::Stig,
    dungeon: &Dungeon,
    graph: &crate::surface_nav::SurfaceGraph,
    target: Option<Vec3>,
    coarse: Option<Vec3>,
    hauling: bool,
    sep: Vec3,
    dt: f32,
    t: f32,
    bc: CrabTuning,
) -> CrabState {
    // A hauling carrier hugs the chunk (mouth on it); a gathering crab holds a grab-range away.
    let hold = if hauling { bc.carry_hold } else { bc.grab_range };

    // Committed to a chunk that's within reach: hold position and keep the mouth turned onto it.
    if let Some(gp) = target {
        let to = gp - motion.pos;
        if to.length() < hold {
            let np = graph.patch(motion.patch);
            let h = project_tangent(to, np.normal).normalize_or(motion.heading);
            motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
            // Keep applying the Reynolds separation push while holding — this branch returns before
            // `steer_surface` (where separation now lives), so without it a converging crew clumps onto
            // one point and z-fights instead of ringing the chunk (Reynolds 1999, GDC).
            motion.pos += project_tangent(sep, np.normal) * bc.sep_strength * dt;
            return CrabState::Attack;
        }
    }

    // Desired travel direction. Near a committed chunk → straight at it (`steer_surface`'s `home` walks
    // it in once it's on the same patch). Far, or uncommitted → climb the MEAT gradient (wall-aware:
    // the field only lives on floor and routes around walls), falling back toward the coarse hotspot.
    let grad = {
        let g = stig.gradient(crate::ai::field::FieldId::MEAT, dungeon, motion.pos);
        Vec3::new(g.x, 0.0, g.y)
    };
    let desired = match target {
        Some(gp) if (gp - motion.pos).length() < bc.los_range => gp - motion.pos,
        Some(gp) => {
            if grad.length_squared() > 1.0e-6 {
                grad
            } else {
                gp - motion.pos
            }
        }
        None => {
            if grad.length_squared() > 1.0e-6 {
                grad
            } else {
                coarse.map(|c| c - motion.pos).unwrap_or(Vec3::ZERO)
            }
        }
    };
    if steer_surface(motion, graph, dungeon, desired, target, bc.speed, sep, dt, t, bc) {
        CrabState::Walk
    } else {
        CrabState::Idle
    }
}

/// Commit foraging crabs to specific gibs (the "recruitment" step of cooperative transport). For each
/// `SeekMeat` crab without a live target, pick the nearest chunk that still needs crew (Σ committed
/// carrier capacity < weight, not yet being hauled) and enlist it: push the crab into the gib's
/// `carriers`, point `CrabCarry.target` at the gib, and move the gib to `Crewing`. This and `carry_gibs`
/// are the ONLY mutators of `Carryable.carriers` (one-path ownership). Holland & Melhuish 1999
/// (stigmergic clustering); Dorigo ACO (recruitment by trail).
pub(crate) fn assign_meat_targets(
    mut crabs: Query<
        (
            Entity,
            &CrabMotion,
            &mut CrabCarry,
            &crate::ai::brain::ActiveBehavior,
            &CrabSeed,
        ),
        With<Crab>,
    >,
    mut gibs: Query<(Entity, &Transform, &mut crate::gore::Carryable, &crate::gore::GibKey)>,
    beh: Res<BehaviorTuning>,
) {
    let bc = &beh.crab;
    // Drop targets whose gib no longer exists (e.g. capped out of the ring mid-haul) so the crab
    // re-forages. Clearing `hauling` alongside `target` is essential: a lone `target = None` strands a
    // carrier that was mid-haul, because `hauling` keeps `Fact::CarryingMeat` — and thus the `Carry`
    // mode — latched with no chunk to carry. The brain then never leaves Carry, so it steers nowhere
    // (`target` is None), `release_uncommitted_carriers` can't recover it (Carry counts as
    // "committed"), and `carry_gibs` never touches it (the gib is gone) — the crab freezes forever.
    for (_, _, mut cc, _, _) in &mut crabs {
        if let Some(g) = cc.target {
            if gibs.get(g).is_err() {
                cc.target = None;
                cc.hauling = false;
            }
        }
    }

    // No meat on the floor → nothing to enlist crews for. Skip the caps/committed/snapshot/seeker
    // allocations entirely (the common case once a pile is cleared); the stale-target release above has
    // already run, so carriers are freed regardless.
    if gibs.is_empty() {
        return;
    }

    // Snapshot per-crab capacity — summing a gib's committed crew capacity needs every carrier's value.
    let caps: HashMap<Entity, f32> = crabs.iter().map(|(e, _, c, _, _)| (e, c.capacity)).collect();

    // Snapshot each gib: position, weight, whether it's already being hauled, and its current committed
    // capacity. `committed` is mutated as we enlist crabs this tick so several seekers don't over-crew.
    let mut committed: HashMap<Entity, f32> = HashMap::new();
    let mut gib_snap: Vec<(Entity, Vec3, f32, bool, u64)> = gibs
        .iter()
        .map(|(e, tf, c, key)| {
            // Sum the crew's capacities in a canonical (ascending) order, NOT `carriers` Vec order. The
            // sum feeds the `committed >= weight` lift/commit gate below, and float addition is
            // non-associative, so summing in enumeration order lets a carrier-order difference flip that
            // gate at the boundary — diverging which crab commits, and the physics-free replay hash.
            let mut caps_v: Vec<u32> = c.carriers.iter().filter_map(|x| caps.get(x)).map(|v| v.to_bits()).collect();
            // SORT-OK: bare f32 bits about to be summed — a tie is the same term twice. Interchangeable.
            caps_v.sort_unstable();
            let sum: f32 = caps_v.into_iter().map(f32::from_bits).sum();
            committed.insert(e, sum);
            (e, tf.translation, c.weight, c.phase == crate::gore::CarryPhase::Hauling, key.0)
        })
        .collect();
    // Determinism: gib enumeration follows entity spawn / ID-reuse order, which is NOT a stable
    // semantic ordering — two same-seed runs can produce the *same* set of chunks in a different query
    // order. The nearest-chunk pick below keeps the FIRST gib at the minimum distance (`d < bd`), so on
    // an exact distance tie it would commit a crab to a different chunk per run, diverging crab targets
    // and cascading into the physics-free replay hash (`deterministic_core_is_bit_identical`). Sort by
    // world position — a stable key independent of entity order — so the choice depends only on geometry.
    crate::sort_total!(&mut gib_snap, |&(_, p, _, _, key)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), key));

    // Seeking crabs that still need a chunk. The enlist loop below is greedy and stateful (each commit
    // bumps `committed`, so who picks first decides who gets the last slot on a contested chunk), and it
    // pushes carriers whose per-crab (±20%) capacities are then summed non-associatively — so the
    // processing order must be a STABLE TOTAL order. Crab QUERY order isn't reproducible across same-seed
    // runs (see `util::nearest_planar`); world position alone still ties when two crabs sit on the exact
    // same point (early, pre-dispersal), and `sort_unstable` would then break that tie by unstable entity
    // order. Fall back to the stable, unique `CrabSeed` so the whole assignment is deterministic.
    let mut seekers: Vec<(Entity, Vec3, u32)> = crabs
        .iter()
        .filter(|(_, _, c, ab, _)| {
            matches!(ab.mode, crate::ai::utility::Mode::SeekMeat) && c.target.is_none()
        })
        .map(|(e, m, _, _, seed)| (e, m.pos, seed.0))
        .collect();
    crate::sort_total!(&mut seekers, |(_, p, seed)| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), *seed));

    for (crab_e, cpos, _) in seekers {
        let mut best: Option<(Entity, f32)> = None;
        for &(ge, gpos, weight, hauling, _) in &gib_snap {
            if hauling {
                continue;
            }
            if committed.get(&ge).copied().unwrap_or(0.0) >= weight {
                continue; // already has enough crew to lift
            }
            let d = gpos.distance(cpos);
            if d > bc.max_commit_dist {
                continue; // too far to reach by straight-line steering — gradient-forage toward it first
            }
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((ge, d));
            }
        }
        let Some((ge, _)) = best else { continue };

        // Commit: enlist on the gib and record the target on the crab.
        if let Ok((_, _, mut carry, _)) = gibs.get_mut(ge) {
            if !carry.carriers.contains(&crab_e) {
                carry.carriers.push(crab_e);
            }
            if carry.phase == crate::gore::CarryPhase::Resting {
                carry.phase = crate::gore::CarryPhase::Crewing;
            }
        }
        if let Ok((_, _, mut cc, _, _)) = crabs.get_mut(crab_e) {
            cc.target = Some(ge);
        }
        // Count this crab's capacity so later seekers this tick see the fuller crew.
        if let Some(c) = committed.get_mut(&ge) {
            *c += caps.get(&crab_e).copied().unwrap_or(0.0);
        }
    }
}

/// A crab that stops participating in transport drops its load: clearing its target makes `carry_gibs`
/// prune it from the gib's crew next frame, so its capacity leaves and a stalled crew can re-evaluate.
/// A carrier is committed only while its mode is `SeekMeat` (crewing) or `Carry` (hauling); the moment
/// the brain flips it to anything else — Flee (scatter), but also Latch or Forage when a unit wanders
/// into range or the MEAT trace fades — it must release, or a seeker peeling off leaves a phantom
/// carrier that never reaches the pile and stalls the lift until `bc.crew_timeout`. Touches only
/// `CrabCarry` — the sole system besides the two carrier-mutators, and it never edits `carriers`.
pub(crate) fn release_uncommitted_carriers(
    mut crabs: Query<(&crate::ai::brain::ActiveBehavior, &mut CrabCarry), With<Crab>>,
) {
    for (active, mut cc) in &mut crabs {
        let committed = matches!(
            active.mode,
            crate::ai::utility::Mode::SeekMeat | crate::ai::utility::Mode::Carry
        );
        if !committed && cc.target.is_some() {
            cc.target = None;
            cc.hauling = false;
        }
    }
}

/// The cooperative-transport state machine — the SOLE authority over a lifted chunk's transform and
/// rigid-body mode. Each frame, per gib: prune dead/reassigned carriers, sum the live crew's capacity,
/// then take exactly one transition:
///   Resting/Crewing → Hauling   when the capacity *gathered at the chunk* ≥ weight
///                                (switch Dynamic→Kinematic, zero velocities, pick the nearest nest);
///   Crewing → Resting            after `bc.crew_timeout` (disband a crew that can't lift);
///   Hauling → delivered          within `bc.deliver_range` of the nest (hoard += weight, consume the gib);
///   Hauling → Crewing (drop)     if the crew's total capacity falls below weight (Kinematic→Dynamic).
/// The three rigid-body switches are mutually exclusive (never two in one frame) — the "kinematic
/// hand-off guard". During Hauling the gib LEADS along the nest's prebuilt `FlowField` (so it routes
/// around walls), riding at mouth height; the crew just chases and grips it via the `Carry` locomotion
/// branch, so there's no circular follow. Haul speed scales with crew size and inversely with weight.
/// Holland & Melhuish 1999 (stigmergic cooperative transport); avian3d kinematic bodies are moved by
/// transform, so zeroing Lin/Ang velocity on the switch prevents a residual-impulse launch.
#[allow(clippy::type_complexity)]
pub(crate) fn carry_gibs(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    mut commands: Commands,
    mut gib_ring: ResMut<crate::gore::GibRing>,
    // `RigidBody` is an immutable component in avian3d — switch a body's type by re-inserting it via
    // `Commands`, not by mutating it in place. Velocities are mutable and zeroed on the switch.
    mut gibs: Query<
        (
            Entity,
            &mut crate::gore::Carryable,
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
            // Carried solely to give the DELIVER pass below a stable total order — see there.
            &crate::gore::GibKey,
        ),
        With<crate::gore::GibChunk>,
    >,
    mut crabs: Query<
        (&Transform, &mut CrabCarry),
        (With<Crab>, Without<crate::gore::GibChunk>, Without<crate::nest::Nest>),
    >,
    mut nests: Query<
        (Entity, &Transform, &mut crate::nest::Nest),
        (Without<crate::gore::GibChunk>, Without<Crab>),
    >,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let bc = &beh.crab;

    // Deliveries are collected here and applied AFTER the loop, in `GibKey` order.
    //
    // The per-gib motion below is order-independent (each gib touches only its own Transform/velocities),
    // but a DELIVER is not: it accumulates `nest.hoard += weight` and `nest.spawn_boost += …` into a SHARED
    // nest with a non-associative `f32 +=`, and it despawns (feeding entity-id reuse). Gib query order is
    // not stable across `App` instances, so two chunks delivering to one nest on one tick summed to
    // different bits per run. That is not cosmetic: `nest_reproduce` gates on `hoard < meat_per_crab`, and
    // crossing that gate consumes a slot from the shared `CrabSpawnSeq`, which sets every subsequent crab's
    // caste, capacity and RNG. The sibling `assign_meat_targets` sorts both its lists for exactly this
    // reason; this loop was left raw (its INNER capacity sums were canonicalised, so only the outer pass
    // was missed). Mutant-armed: `deliver_range` ships 1.2 but the genome bounds reach 4.0 (~11× the
    // delivery area, so deliveries coincide instead of arriving one at a time).
    let mut delivered: Vec<(u64, Entity, Entity, f32)> = Vec::new(); // (GibKey, gib, nest, weight)
    for (ge, mut carry, mut gtf, mut lv, mut av, gib_key) in &mut gibs {
        // 1. Prune carriers down to crabs that still exist AND still point at this gib.
        carry
            .carriers
            .retain(|&c| crabs.get(c).map(|(_, cc)| cc.target == Some(ge)).unwrap_or(false));

        // 2. Sum crew capacity two ways: `cap_here` counts only carriers that have actually gathered at
        // the chunk (within `bc.grab_range`) — that's what can lift it; `cap_total` counts every committed
        // carrier — that's what sustains an in-progress haul (a chaser lagging a little mustn't drop it).
        // Requiring the gathered capacity (not the whole roster) to lift avoids a deadlock where one
        // straggler that can't path to the chunk keeps a full-strength crew from ever lifting.
        // Sum crew capacities in a canonical (ascending) order, NOT `carriers` Vec order: these feed the
        // `cap_here >= weight` lift gate, and float addition is non-associative, so an enumeration-order
        // difference would flip the lift at the boundary and diverge the replay hash (see
        // `assign_meat_targets`).
        let mut here_caps: Vec<u32> = Vec::new();
        let mut total_caps: Vec<u32> = Vec::new();
        for &c in &carry.carriers {
            if let Ok((ctf, cc)) = crabs.get(c) {
                total_caps.push(cc.capacity.to_bits());
                if ctf.translation.xz().distance(gtf.translation.xz()) <= bc.grab_range {
                    here_caps.push(cc.capacity.to_bits());
                }
            }
        }
        // SORT-OK: bare f32 bits about to be summed — ties are identical terms.
        total_caps.sort_unstable();
        here_caps.sort_unstable();
        let cap_total: f32 = total_caps.into_iter().map(f32::from_bits).sum();
        let cap_here: f32 = here_caps.into_iter().map(f32::from_bits).sum();
        let has_crew = !carry.carriers.is_empty();

        match carry.phase {
            crate::gore::CarryPhase::Resting | crate::gore::CarryPhase::Crewing => {
                // A gathered crew lifts only if it has enough capacity at the chunk AND a nest still
                // exists to receive it. Selecting the destination BEFORE committing means a chunk with
                // every nest razed never lifts into an undeliverable Kinematic haul (which used to
                // oscillate Crewing<->Hauling every fixed tick, resetting `crew_timer` each frame so the
                // crew never disbanded). With no nest it stays a Crewing crew and disbands at
                // bc.crew_timeout, cleanly re-foraging.
                let mut dest: Option<Entity> = None;
                if has_crew && cap_here >= carry.weight {
                    // Nearest nest, ranked by the floor delivery cell (`nest.pos`), NOT the wall-mounted
                    // dome `Transform`. Every other consumer (haul nav, deliver check, breeding, scout
                    // home) uses `nest.pos`; ranking by the dome could commit the haul to a nest whose
                    // delivery cell is across a wall, so the flow field returns nothing and the chunk
                    // gets dragged straight through it. Match the deliver check's horizontal distance.
                    // Determinism: break an exact distance tie by the nest's delivery position, not the
                    // nest query order (unstable across same-seed runs — see `util::nearest_planar`); the
                    // chosen nest sets the haul destination and steers the whole crew.
                    let mut best: Option<(f32, Vec3)> = None;
                    for (ne, _ntf, nest) in nests.iter() {
                        let d = (nest.pos - gtf.translation).with_y(0.0).length();
                        let better = match best {
                            None => true,
                            Some((bd, bp)) => {
                                (d.to_bits(), nest.pos.x.to_bits(), nest.pos.z.to_bits())
                                    < (bd.to_bits(), bp.x.to_bits(), bp.z.to_bits())
                            }
                        };
                        if better {
                            best = Some((d, nest.pos));
                            dest = Some(ne);
                        }
                    }
                }

                if let Some(nest_e) = dest {
                    // --- LIFT (Dynamic → Kinematic) ---
                    if crate::ai::diag::AI_DIAG {
                        let crew = carry
                            .carriers
                            .iter()
                            .filter(|&&c| {
                                crabs
                                    .get(c)
                                    .map(|(ctf, _)| {
                                        ctf.translation.xz().distance(gtf.translation.xz())
                                            <= bc.grab_range
                                    })
                                    .unwrap_or(false)
                            })
                            .count();
                        info!(
                            "carry: LIFT weight={:.2} crew={crew} cap_here={cap_here:.2}",
                            carry.weight
                        );
                    }
                    carry.phase = crate::gore::CarryPhase::Hauling;
                    carry.crew_timer = 0.0;
                    commands.entity(ge).insert(RigidBody::Kinematic);
                    lv.0 = Vec3::ZERO;
                    av.0 = Vec3::ZERO;
                    carry.nest = Some(nest_e);
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.hauling = true;
                        }
                    }
                } else if has_crew {
                    // Crewing: not enough gathered capacity yet, OR nowhere to deliver. Keep gathering;
                    // disband a crew that waits past bc.crew_timeout without lifting.
                    carry.phase = crate::gore::CarryPhase::Crewing;
                    carry.crew_timer += dt;
                    if carry.crew_timer >= bc.crew_timeout {
                        // Disband a crew that has waited too long without lifting.
                        for &c in &carry.carriers {
                            if let Ok((_, mut cc)) = crabs.get_mut(c) {
                                cc.target = None;
                                cc.hauling = false;
                            }
                        }
                        carry.carriers.clear();
                        carry.phase = crate::gore::CarryPhase::Resting;
                        carry.crew_timer = 0.0;
                    }
                } else {
                    carry.phase = crate::gore::CarryPhase::Resting;
                    carry.crew_timer = 0.0;
                }
            }
            crate::gore::CarryPhase::Hauling => {
                // Destination = the nest's floor delivery point + its walkway flow field (if it still
                // exists; a razed nest yields None → the haul aborts and the chunk drops).
                let nest_nav: Option<(Vec3, Arc<crate::flowfield::FlowField>)> = carry
                    .nest
                    .and_then(|n| nests.get(n).ok())
                    .map(|(_, _, nest)| (nest.pos, nest.flow.clone()));

                if nest_nav.is_none() {
                    // --- NEST RAZED MID-HAUL: full release (Kinematic → Dynamic) ---
                    // The destination nest no longer resolves, so there's nowhere to deliver. Drop the
                    // load and fully release the crew (clear carriers, back to Resting) — mirroring the
                    // bc.crew_timeout disband — instead of dropping to a Crewing limbo that can never
                    // re-lift (the LIFT gate now refuses a nestless chunk) and would only sit emitting
                    // MEAT scent until it timed out.
                    commands.entity(ge).insert(RigidBody::Dynamic);
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.target = None;
                            cc.hauling = false;
                        }
                    }
                    carry.carriers.clear();
                    carry.phase = crate::gore::CarryPhase::Resting;
                    carry.crew_timer = 0.0;
                    carry.nest = None;
                } else if cap_total < carry.weight {
                    // --- ABORT / DROP (Kinematic → Dynamic): crew shrank below liftable capacity ---
                    commands.entity(ge).insert(RigidBody::Dynamic);
                    carry.phase = crate::gore::CarryPhase::Crewing;
                    carry.crew_timer = 0.0;
                    for &c in &carry.carriers {
                        if let Ok((_, mut cc)) = crabs.get_mut(c) {
                            cc.hauling = false;
                        }
                    }
                } else if let Some((npos, flow)) = nest_nav {
                    let horiz = (npos - gtf.translation).with_y(0.0);
                    if horiz.length() <= bc.deliver_range {
                        // --- DELIVER (Kinematic → despawn) --- deferred to the sorted pass below; the
                        // hoard/boost accumulate and the despawn are the order-sensitive parts.
                        if let Some(n) = carry.nest {
                            delivered.push((gib_key.0, ge, n, carry.weight));
                        }
                        // Releasing this chunk's own carriers is order-independent (a crab hauls exactly
                        // one chunk), so it stays here.
                        for &c in &carry.carriers {
                            if let Ok((_, mut cc)) = crabs.get_mut(c) {
                                cc.target = None;
                                cc.hauling = false;
                            }
                        }
                    } else {
                        // Haul along the nest's flow field so the chunk threads walkways instead of
                        // beelining through walls (Item #3). Speed scales up with crew and down with
                        // weight (Item #6): a heavy chunk on a bare crew crawls; more carriers speed it.
                        let steer = flow.steer(&dungeon, gtf.translation);
                        let mut dir = Vec3::new(steer.x, 0.0, steer.y);
                        if dir.length_squared() <= 1.0e-6 {
                            dir = horiz; // at/near the goal cell but still outside bc.deliver_range: close in
                        }
                        let crew = carry.carriers.len() as f32;
                        let speed = (bc.carry_speed * crew / (carry.weight * bc.weight_drag))
                            .clamp(bc.carry_speed * 0.35, bc.carry_speed);
                        // Wall-confine the haul step. The flow field already threads walkways, but the
                        // `dir = horiz` straight-line fallback above (taken when the steer is ~0 right
                        // next to the wall-mounted nest) has no wall backstop, so a hauled chunk could
                        // be dragged through the wall/corner onto the void floor. Sweep the horizontal
                        // move against room walls with the same `resolve_move` the Dynamic path uses in
                        // `gore::confine_gibs`, so the chunk stops at the room-side wall face instead.
                        let step = dir.normalize_or_zero() * (speed * dt);
                        let resolved = dungeon.resolve_move(
                            gtf.translation.with_y(0.0),
                            Vec3::new(step.x, 0.0, step.z),
                            Vec2::splat(crate::gore::GIB_CONFINE_HALF),
                        );
                        gtf.translation.x = resolved.x;
                        gtf.translation.z = resolved.z;
                        gtf.translation.y = CARRY_HEIGHT; // ride at the crew's mouth height (Item #2)
                        lv.0 = Vec3::ZERO;
                        av.0 = Vec3::ZERO;
                    }
                }
            }
        }
    }

    // Apply the deliveries in `GibKey` order — the shared-nest accumulate and the despawn, i.e. exactly the
    // parts the raw gib query order was deciding. `GibKey` is unique by construction (it mixes a monotonic
    // `GibSeq`; before that fix it was derived from the death origin and COLLIDED for two creatures dying on
    // one coordinate — that was G0c), so this is a genuine total order and `sort_total!` proves it.
    crate::sort_total!(&mut delivered, |&(key, ..): &(u64, Entity, Entity, f32)| key);
    for (_, ge, n, weight) in delivered {
        if let Ok((_, _, mut nest)) = nests.get_mut(n) {
            nest.hoard += weight;
            // Feeding surge: heavier chunks accelerate births more, up to ~10×.
            nest.spawn_boost =
                (nest.spawn_boost + weight * sim.breeding.feed_gain).min(sim.breeding.spawn_boost_max);
        }
        // The ONE early-removal path (drops the id from the ring, then despawns).
        gib_ring.consume(&mut commands, ge);
    }
}

/// Each crab lays into two channels, both at a per-second rate ≈ the channel's evaporation, so each
/// cell's value tracks the local crab count:
///
/// - CRAB_DENSITY — the swarm's own crowding/recruitment substrate (read by `nest_reproduce`).
/// - THREAT_CRAB — the menace the swarm radiates, read as FEAR by the *squad* (never by crabs; see
///   `ai::faction`). Separate from density because dread wants a wider radius and slower decay than
///   crowding, and because the two must be tunable apart.
///
/// Determinism: overlapping discs accumulate into shared cells and float `+=` is non-associative, so a
/// deposit emitted in entity-iteration order would make the summed field depend on that order (which can
/// shift between same-seed runs). Emit in a stable *position* order, exactly as `deposit_meat_scent` does.
pub(crate) fn deposit_crab_fields(
    time: Res<Time>,
    crabs: Query<&Transform, With<Crab>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    let dt = time.delta_secs();
    let density = sim.deposit.crab_density_rate * dt;
    let menace = sim.deposit.crab_menace_rate * dt;
    let mut positions: Vec<Vec3> = crabs.iter().map(|tf| tf.translation).collect();
    // SORT-OK: bare positions — the position IS the whole value, so a tie means two identical deposits,
    // which contribute identical terms to the same sum. Interchangeable.
    positions.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in positions {
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::CRAB_DENSITY,
            amount: density,
        });
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::THREAT_CRAB,
            amount: menace,
        });
    }
}

/// Each *resting/crewing* meat gib lays into the MEAT field, so foraging crabs sense a pile from a
/// distance and climb its gradient (ACO-style trail-following; Dorigo). A gib that is already lifted and
/// being hauled is skipped — otherwise it drags the moving MEAT hotspot (the SeekMeat target) toward the
/// nest, pulling uncommitted foragers onto an already-crewed chunk instead of dispersing to fresh piles.
pub(crate) fn deposit_meat_scent(
    time: Res<Time>,
    gibs: Query<(&Transform, &crate::gore::Carryable)>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    let amount = sim.deposit.meat_rate * time.delta_secs();
    // Determinism: gibs enumerate in unstable entity order (see `util::nearest_planar`). Each MEAT
    // deposit spreads over a disc, so overlapping chunks accumulate into shared field cells; pushing
    // them in enumeration order makes the summed gradient depend on that order (float `+=` is
    // non-associative), drifting swarm steering enough to break the replay hash. Emit in a stable
    // position order so the MEAT field depends only on WHERE the chunks are.
    let mut positions: Vec<Vec3> = gibs
        .iter()
        .filter(|(_, carry)| carry.phase != crate::gore::CarryPhase::Hauling)
        .map(|(tf, _)| tf.translation)
        .collect();
    // SORT-OK: bare positions — the position IS the whole value, so a tie means two identical deposits,
    // which contribute identical terms to the same sum. Interchangeable.
    positions.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in positions {
        deposits.0.push(crate::ai::field::Deposit {
            pos,
            field: crate::ai::field::FieldId::MEAT,
            amount,
        });
    }
}

/// Scout recon + recruitment. Scouts roam to find prey (minimalist-agent foraging; Talamali, Bose, Haire,
/// Xu, Marshall & Reina, "Sophisticated collective foraging with minimalist agents", Swarm Intelligence
/// 2019, DOI 10.1007/s11721-019-00176-9) and
/// mark it with the **vectorial rally pheromone** (Tang, Xu, Yu, Zhang & Zhang, "Dynamic target searching
/// and tracking with swarm robots based on stigmergy", Robotics & Autonomous Systems 2019): while a scout
/// senses prey it deposits a direction vector (an "intermediate-vector") pointing at the prey's live
/// position, so the map continuously encodes the bearing to the moving target rather than a stale scalar
/// at where the prey once was. Runs before the rally deposits drain so the beacon is live this frame and
/// `think` reads a fresh Scout state:
/// - **Roaming → Tracking**: on sensing prey within `bc.scout_sight` (planar), lock onto the nearest.
/// - **Tracking**: refresh the tracked prey and, throttled by `bc.rally_deposit_cooldown`, deposit a vector
///   toward it (strength eases with proximity). Losing sight drops back to Roaming; the pheromone then
///   evaporates on its own — the automatic "call off the attack".
pub(crate) fn scout_mark_prey(
    time: Res<Time>,
    mut scouts: Query<(&Transform, &mut Scout)>,
    prey: Query<&Transform, With<Prey>>,
    mut deposits: ResMut<crate::ai::field::RallyDeposits>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let bc = &beh.crab;
    // Rally marks are collected and sorted before queueing — the same idiom every SCALAR deposit producer
    // uses (`nest_alarm`, `crab_alarm_on_damage`, `deposit_crab_fields`, …), which this site never got
    // because `RallyDeposits` had no `sort_*` helper to call: `sort_deposits` is typed `&mut [Deposit]`.
    // The scout query order is not stable across `App` instances, and `RallyField::deposit` accumulates with
    // a non-associative `Vec2 +=` over a `deposit_radius`-wide disc, so two scouts within ~2·radius write
    // the same cells on one tick and raw push order set the low bits of that cell's vector. That is not
    // cosmetic: `re_role_crabs` gates a caste flip on `rally.sample(..).length() > bc.rally_live`, and while
    // the authored 0.15 sits clear of the field's noise floor, the genome's lower bound is **0.02** — right
    // on it. See `field::sort_rally_deposits`.
    let mut batch: Vec<crate::ai::field::RallyDeposit> = Vec::new();
    for (tf, mut scout) in &mut scouts {
        let pos = tf.translation;
        scout.report_cooldown = (scout.report_cooldown - dt).max(0.0);

        // Nearest prey on the ground plane, within sight (the shared ranking; payload is unit `()`).
        let hit = crate::util::nearest_planar(pos, prey.iter().map(|pt| ((), pt.translation)));
        match hit.filter(|(_, _, d)| *d <= bc.scout_sight) {
            Some(((), prey_pos, best)) => {
                scout.state = ScoutState::Tracking { prey_pos };
                // Deposit an intermediate-vector pointing at the prey (Tang's `s`), throttled so a cell
                // isn't saturated frame-by-frame. Strength eases with proximity so nearer marks weigh more.
                if scout.report_cooldown <= 0.0
                    && let Some(dir) = (prey_pos.xz() - pos.xz()).try_normalize()
                {
                    let strength =
                        sim.deposit.rally_mark * ((bc.scout_sight - best) / bc.scout_sight).clamp(0.0, 1.0);
                    batch.push(crate::ai::field::RallyDeposit { pos, vec: dir * strength });
                    scout.report_cooldown = bc.rally_deposit_cooldown;
                    if crate::ai::diag::AI_DIAG {
                        info!("scout: MARK prey@{:?} from@{:?}", prey_pos.xz(), pos.xz());
                    }
                }
            }
            None => {
                // Lost the prey — resume roaming; the pheromone evaporates on its own (call-off).
                scout.state = ScoutState::Roaming;
            }
        }
    }
    crate::ai::field::sort_rally_deposits(&mut batch);
    deposits.0.extend(batch);
}

/// Meat-fuelled breeding — the ONE reproduction path. A nest births a crab only when it has hoarded at
/// least `MEAT_PER_CRAB` of delivered meat AND its `NEST_RESPAWN_INTERVAL` rate-limiter has elapsed; each
/// birth *spends* that meat, so the forage→haul→deliver economy is the sole source of reinforcements —
/// starve the swarm (destroy its gibs) and the hoard drains and births stop. Also capped by
/// `CRAB_COUNT_MAX` and suppressed while the nest cell is crowded (local CRAB_DENSITY high) so births
/// don't pile onto births. Newborns seat on the nest's floor delivery cell; `spawn_boost` (from heavier
/// deliveries) shortens the interval for a well-fed nest.
pub(crate) fn nest_reproduce(
    time: Res<Time>,
    mut commands: Commands,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    crab_assets: Option<Res<CrabAssets>>,
    mut nests: Query<(Entity, &mut crate::nest::Nest)>,
    crabs: Query<(), With<Crab>>,
    mut seq: ResMut<CrabSpawnSeq>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let (Some(graph), Some(crab_assets)) = (graph, crab_assets) else {
        return;
    };
    let dt = time.delta_secs();
    let mut total = crabs.iter().count();

    // CANONICAL ORDER — load-bearing, and the same class of bug as `laser::fire_laser`'s shared aim draw.
    // This loop is greedy and stateful over a SHARED counter, so the order the nests are visited in is
    // part of the result: `seq` is a monotonic counter, and `CrabSpawnSeq`'s own doc spells out what rides
    // on it — "scout/assault role, think-stagger, jump cadence, carry capacity, climb/angle biases, RNG".
    // When two nests breed on the same tick, raw query order decided WHICH nest's newborn got seed N and
    // which got N+1, so a crab's caste and capacity flipped between two same-seed runs.
    // Nest query order is NOT stable across `App` instances (`sim_harness::nest_cells` was canonicalised
    // for exactly this reason; the breeding loop itself never was). `nest.pos` is assigned at spawn and
    // immortal, so its bits are a stable, total key — the `world_to_cell` quantisation is deliberately NOT
    // used here: two nests can share a cell.
    let mut order: Vec<(u32, u32, u32, Entity)> = nests
        .iter()
        .map(|(e, n)| (n.pos.x.to_bits(), n.pos.y.to_bits(), n.pos.z.to_bits(), e))
        .collect();
    crate::sort_total!(&mut order, |k: &(u32, u32, u32, Entity)| (k.0, k.1, k.2));

    for (.., nest_e) in order {
        let Ok((_, mut nest)) = nests.get_mut(nest_e) else {
            continue;
        };
        // Fade the feeding surge, then re-arm the next spawn at the boosted rate (up to 10× faster).
        nest.spawn_boost = (nest.spawn_boost - sim.breeding.spawn_boost_decay * dt).max(0.0);
        nest.respawn_timer -= dt;
        if nest.respawn_timer > 0.0 {
            continue;
        }
        // Effective rate = 1 + spawn_boost (SPAWN_BOOST_MAX ⇒ ~10× faster). Re-arm even if this tick
        // can't spawn (no hoard yet, or the delivery cell isn't floor), so a fed nest keeps its fast
        // cadence. No population cap or local-crowding gate: the meat economy below is the swarm's only
        // size lever — a nest that keeps feeding keeps breeding.
        nest.respawn_timer = sim.breeding.respawn_interval / (1.0 + nest.spawn_boost);

        // Meat gate: breeding both requires and consumes hoarded meat. No hoard → no birth, so cutting
        // off the swarm's food halts reinforcements (the economy's one lever).
        if nest.hoard < sim.breeding.meat_per_crab {
            continue;
        }
        let Some(patch) = graph.floor_patch_cell(dungeon.world_to_cell(nest.pos)) else {
            continue; // nest's delivery cell isn't floor — can't seat a newborn here
        };
        nest.hoard -= sim.breeding.meat_per_crab; // spend the meat this birth cost
        let s = seq.0 as u32;
        seq.0 += 1;
        spawn_crab_on_patch(
            &mut commands,
            &graph,
            patch,
            &crab_assets.collider,
            &crab_assets.scene,
            s,
            &sim,
            beh.crab,
        );
        total += 1;
        if crate::ai::diag::AI_DIAG {
            info!("nest: RESPAWN total={total}");
        }
    }
}

