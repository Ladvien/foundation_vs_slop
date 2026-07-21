//! `crab::movement` — per-crab locomotion, jump, camera-facing anim, and caste role logic (split out of the former `crab.rs`, 2026-07-19 review Finding: large files).

use super::*;

/// Move every crab one step along the surface toward the nearest unit, transferring between patches and
/// re-orienting flat to each new surface.
pub(crate) fn crab_locomotion(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    crab_field: Res<CrabField>,
    dungeon: Res<Dungeon>,
    stig: Res<crate::ai::field::Stig>,
    rally: Res<crate::ai::field::RallyField>,
    // The gameplay light field (baked in `light::LightFieldWritten`, ordered before this system) and the
    // config gains — for the photophobic/-philic light nudge below.
    light_field: Res<crate::light::LightField>,
    // The Almond Water field (written in `AlmondWaterWritten`, ordered before this system) — for the
    // wounded-forage nudge below (a wounded crab climbs the water gradient toward a seep).
    almond_water: Res<crate::almond_water::AlmondWater>,
    config: Res<crate::config::GameConfig>,
    units: Query<(Entity, &Transform), (With<Prey>, Without<Crab>)>,
    // Gib transforms, for a `SeekMeat`/`Carry` crab to steer to the specific chunk it's committed to.
    gibs: Query<&Transform, (With<crate::gore::GibChunk>, Without<Crab>, Without<Unit>)>,
    mut crabs: Query<
        (
            &mut CrabMotion,
            &mut CrabState,
            &mut CrabAttached,
            &mut Transform,
            &crate::ai::brain::ActiveBehavior,
            Option<&CrabCarry>,
            Option<&CrabJump>,
            Option<&mut Scout>,
            // Light response (`light::Photophobic`/`Photophilic`) — added at spawn; drives the light nudge.
            Option<&crate::light::Photophobic>,
            Option<&crate::light::Photophilic>,
            // Health gates the Almond Water forage nudge — only a wounded crab seeks the water.
            &Health,
            // Whether this crab can smell the cyanide warning — an anosmic crab can't tell a poison pool from
            // a heal pool, so it forages toward any water (and walks into cyanide).
            &crate::health::CyanideSmell,
        ),
        With<Crab>,
    >,
    // Reused across frames: a fresh HashMap + a Vec per occupied cell every frame (40-90 crabs on the
    // hottest per-crab path) churned dozens of small allocations. Held in a Local and cleared in place
    // (keys + Vec capacities retained, bounded by the fixed dungeon), so steady state is allocation-free.
    mut hash: Local<HashMap<IVec2, Vec<Vec3>>>,
) {
    let bc = config.behavior.crab;
    let Some(graph) = graph else { return };
    let Some(field) = crab_field.field.as_ref() else {
        return;
    };
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let now = time.elapsed_secs(); // for per-crab path jitter (see CRAB_JITTER_*)

    // Per-unit: entity, foot position, and forward (local -Z) — its gun only reaches the front.
    let unit_data: Vec<(Entity, Vec3, Vec3)> = units
        .iter()
        .map(|(e, t)| {
            let fwd = (t.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
            (e, t.translation, fwd)
        })
        .collect();

    // Spatial hash of crab positions (3D) keyed by floor cell, for O(n·k) separation. Clear in place
    // (retain the buckets + each cell's Vec capacity) rather than reallocating the map every frame.
    for v in hash.values_mut() {
        v.clear();
    }
    for (motion, _, _, _, _, _, _, _, _, _, _, _) in &crabs {
        hash.entry(dungeon.world_to_cell(motion.pos))
            .or_default()
            .push(motion.pos);
    }
    // Sort each bucket by position bits so the separation SUM below (`sep += …`) is canonical. Float
    // addition is non-associative and the bucket is filled in crab QUERY order, which is NOT reproducible
    // across same-seed runs (documented at the carry logistics below, and see `util::nearest_planar`): an
    // unsorted bucket lets a query-order difference flip a rounding bit in `sep`, diverging a crab's
    // position and cascading into the physics-off replay hash (~1–3% of runs, pinned tick 549). Mirrors the
    // identical fix on the parasite swarm hash (`parasite.rs`, `manca_swarm`).
    for v in hash.values_mut() {
        // VALUE-CANONICAL: the bucket holds bare positions, so two coincident crabs contribute the
        // identical term to `sep` and their order cannot matter. (Contrast the drink/cull sorts, whose tied
        // elements carry identity.)
        crate::util::sort_value_canonical(v, |p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    }

    for (
        mut motion,
        mut state,
        mut attached,
        mut transform,
        active,
        carry,
        jump,
        mut scout,
        photophobic,
        photophilic,
        health,
        smell,
    ) in &mut crabs
    {
        // Mid-pounce crabs are owned by `crab_jump` (it drives their arc + transform) — skip them here.
        if jump.is_some_and(|j| j.phase != JumpPhase::Ready) {
            continue;
        }
        // Reynolds separation: raw 3D push away from nearby crabs (bounding-box spacing). Self is
        // skipped by the `d > eps` test; the per-cell hash keeps this O(n·k).
        let cell = dungeon.world_to_cell(motion.pos);
        let mut sep = Vec3::ZERO;
        for gy in -1..=1 {
            for gx in -1..=1 {
                if let Some(others) = hash.get(&(cell + IVec2::new(gx, gy))) {
                    for &o in others {
                        let away = motion.pos - o;
                        let d = away.length();
                        if d > 1.0e-4 && d < bc.sep_radius {
                            sep += away / d * (bc.sep_radius - d);
                        }
                    }
                }
            }
        }

        // Nearest unit on the ground plane (the brain decides *whether* to latch; this is *which* unit).
        // Payload carries the entity + precomputed forward vector; the shared ranking returns the winner.
        let nunit = crate::util::nearest_planar(
            motion.pos,
            unit_data.iter().map(|&(e, up, fwd)| ((e, fwd), up)),
        )
        .map(|((e, fwd), up, _d)| (e, up, fwd));
        let t = (NORMAL_EASE * dt).min(1.0);

        // The brain (see `crate::ai`) chose the mode; the surface/piranha *mechanics* below are
        // unchanged — Latch runs the piranha block, Flee is the new panic, everything else forages.
        let latching = matches!(active.mode, crate::ai::utility::Mode::Latch);
        let fleeing = matches!(active.mode, crate::ai::utility::Mode::Flee);
        // SeekMeat and Carry both steer to the crab's committed gib (Carry keeps formation while
        // `carry_gibs` drives the actual haul). The specific chunk lives in `CrabCarry.target`.
        let seeking = matches!(
            active.mode,
            crate::ai::utility::Mode::SeekMeat | crate::ai::utility::Mode::Carry
        );
        // Scout recon modes + the swarm's recruited rally (see `crate::ai::brain::scout_brain`).
        let scouting = matches!(active.mode, crate::ai::utility::Mode::Scout);
        let marking = matches!(active.mode, crate::ai::utility::Mode::Mark);
        let rallying = matches!(active.mode, crate::ai::utility::Mode::Rally);
        // Investigate: drawn toward the squad's audible din (`NOISE_SQUAD`) — steer to the noise hotspot
        // the brain aimed at (dormant unless the audio search turned it on; see `ai::brain::crab_brain`).
        let investigating = matches!(active.mode, crate::ai::utility::Mode::Investigate);
        // Muster: alarmed by a wounded neighbour — pursue the squad (same surface flow-field path as a
        // forage) but at a faster surge speed, so the retaliation reads as an aggressive charge.
        let mustering = matches!(active.mode, crate::ai::utility::Mode::Muster);
        let gib_pos = carry
            .and_then(|c| c.target)
            .and_then(|e| gibs.get(e).ok())
            .map(|t| t.translation);
        let want = if latching && let Some((host, u, fwd)) = nunit {
            // --- PIRANHA MODE: climb onto the unit and cover its body, biting from a free slot. ---
            {
                // On first latching (no host yet), claim a body-relative slot: fanned across the unit's
                // REAR (where the host's own forward-firing gun can't reach), spread by this crab's
                // `angle_bias`. Held thereafter, so the crab clings to that spot and rides along as the
                // host walks. `attached.host.is_none()` IS the "not yet latched" gate (host is the single
                // source of truth for latched-ness).
                if attached.host.is_none() {
                    motion.latch_rel = (motion.angle_bias - 0.5) * bc.back_spread;
                }
                attached.host = Some(host);

                // World cling direction = the unit's back rotated by the crab's body-relative slot.
                let back_angle = (-fwd.z).atan2(-fwd.x);
                let ang = back_angle + motion.latch_rel;
                let radial = Vec3::new(ang.cos(), 0.0, ang.sin());
                let slot_y = 0.1 + motion.climb_bias * (UNIT_BODY_HEIGHT - 0.1);
                let target = u + radial * UNIT_BODY_RADIUS + Vec3::Y * slot_y;

                let to = target - motion.pos;
                let move_vec = to.normalize_or_zero() * bc.climb_speed + sep * bc.sep_strength;
                motion.pos += move_vec * dt;
                motion.pos.y = motion.pos.y.max(0.0);

                // Keep the floor patch under the crab current, so it drops back into surface pathing
                // cleanly when this unit dies.
                if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(motion.pos)) {
                    motion.patch = fp;
                }

                // Cling flat to the body side (up = outward radial).
                motion.normal = motion.normal.lerp(radial, t).normalize_or(radial);
                let h = to.normalize_or(motion.heading);
                motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);

                if to.length() < bc.eat_range {
                    CrabState::Attack
                } else {
                    CrabState::Walk
                }
            }
        } else if fleeing {
            // --- FLEE: panic away from the THREAT gradient (the frenzy→scatter payoff). ---
            attached.host = None;
            crab_flee(&mut motion, &stig, &dungeon, &graph, sep, dt, t, bc)
        } else if rallying {
            // --- RALLY: mass on the scout's marked sighting by following the local rally-pheromone
            // vector (Tang et al. 2019) — it already points at the (moving) prey, no gradient needed. ---
            attached.host = None;
            crab_rally(&mut motion, &rally, &dungeon, &graph, sep, dt, t, bc)
        } else if scouting && let Some(scout) = scout.as_deref_mut() {
            // --- SCOUT ROAM: aggressive wander across floor + walls hunting for prey to mark. ---
            attached.host = None;
            crab_scout_roam(&mut motion, scout, &graph, &dungeon, sep, dt, t, bc)
        } else if marking {
            // --- MARK: track the spotted prey — approach its position so the scout stays in sensing
            // range and `scout_mark_prey` keeps laying the rally pheromone toward its live cell. No
            // final-approach snap (home = None); falls back to holding heading if the sighting is gone. ---
            attached.host = None;
            let prey_pos = active.target;
            let desired = prey_pos.map(|p| p - motion.pos).unwrap_or(motion.heading);
            if steer_surface(&mut motion, &graph, &dungeon, desired, None, bc.speed * bc.scout_speed_mul, sep, dt, t, bc) {
                CrabState::Walk
            } else {
                CrabState::Idle
            }
        } else if investigating {
            // --- INVESTIGATE: steer toward the NOISE_SQUAD hotspot the brain aimed at — the swarm
            // converging on the sound of the guns. Same point-steering as Mark (no final-approach snap;
            // hold heading if the din is gone). Self-limiting: as the din evaporates the brain drops
            // Investigate and the crab reverts to foraging/fear. ---
            attached.host = None;
            let din_pos = active.target;
            let desired = din_pos.map(|p| p - motion.pos).unwrap_or(motion.heading);
            if steer_surface(&mut motion, &graph, &dungeon, desired, None, bc.speed, sep, dt, t, bc) {
                CrabState::Walk
            } else {
                CrabState::Idle
            }
        } else if seeking {
            // --- SEEK MEAT: steer to the committed gib, or climb the MEAT gradient toward a pile. ---
            attached.host = None;
            // Coarse fallback = the MEAT hotspot the brain aimed at; a hauling carrier hugs its chunk.
            let coarse = active.target;
            let hauling = carry.is_some_and(|c| c.hauling);
            crab_seek_meat(
                &mut motion, &stig, &dungeon, &graph, gib_pos, coarse, hauling, sep, dt, t, bc,
            )
        } else {
            // --- SURFACE MODE: shared flow-field pursuit across floor + walls. ---
            {
                attached.host = None;
                let p = graph.patch(motion.patch);
                let flow = field.flow(motion.patch);
                let tangent = match flow {
                    Some((_, gate)) => {
                        project_tangent(gate - motion.pos, p.normal).normalize_or_zero()
                    }
                    None => Vec3::ZERO,
                };
                // Weave side-to-side across the flow direction (on the surface plane) so crabs fan out
                // over the path instead of stacking; each crab's phase is offset by its bias.
                let side = tangent.cross(p.normal).normalize_or_zero();
                let phase = now * bc.jitter_freq + motion.angle_bias * std::f32::consts::TAU;
                let jitter = side * (phase.sin() * bc.jitter_strength);
                // Mustered (alarmed) crabs surge faster than a calm forage — the scary charge.
                let pursue_speed = if mustering { bc.speed * bc.muster_speed_mul } else { bc.speed };
                // Blind-side stalk: if the nearest unit is close and looking at this crab, arc around
                // toward its rear (tangential to the bearing, on the side that heads for its back) rather
                // than charging head-on — until the crab clears the facing cone and the pounce gate opens.
                let stalk = match nunit {
                    Some((_, upos, ufwd))
                        if {
                            let d = (upos - motion.pos).with_y(0.0).length();
                            d > bc.jump_min
                                && d < bc.stalk_band
                                && unit_is_facing(upos, ufwd, motion.pos, bc.pounce_blind_cos)
                        } =>
                    {
                        let bearing = (upos - motion.pos).with_y(0.0).normalize_or_zero();
                        let tang = Vec3::new(-bearing.z, 0.0, bearing.x); // perpendicular, ground plane
                        let sign = if tang.dot(ufwd) >= 0.0 { 1.0 } else { -1.0 }; // toward the unit's rear
                        project_tangent(tang * sign, p.normal).normalize_or_zero()
                            * (pursue_speed * bc.stalk_strength)
                    }
                    _ => Vec3::ZERO,
                };
                let move_vec = tangent * pursue_speed
                    + stalk
                    + jitter
                    + project_tangent(sep, p.normal) * bc.sep_strength;
                let moving = move_vec.length_squared() > 1.0e-6;
                motion.pos += move_vec * dt;
                motion.pos = clamp_to_patch(motion.pos, p);

                if let Some((next, gate)) = flow {
                    if motion.pos.distance(gate) < TRANSFER_RADIUS {
                        motion.patch = next;
                        motion.pos = clamp_to_patch(gate, graph.patch(next));
                    }
                }

                let target_n = graph.patch(motion.patch).normal;
                motion.normal = motion.normal.lerp(target_n, t).normalize_or(target_n);
                if moving {
                    let h = project_tangent(move_vec, motion.normal).normalize_or(motion.heading);
                    motion.heading = motion.heading.lerp(h, t).normalize_or(motion.heading);
                }
                if moving {
                    CrabState::Walk
                } else {
                    CrabState::Idle
                }
            }
        };

        // Light response: a photophobic (or photophilic) crab drifts down (or up) the LightField gradient
        // on top of whatever its mode is doing — light as a constant environmental force. This is local
        // photophobic/photophilic taxis (down/up the illuminance gradient); the avoidance direction is
        // consistent with Nakagaki et al. 2007's Physarum result, not their minimum-risk routing (a global
        // path integral between fixed endpoints, which this local step has none of). Skipped while latching:
        // a piranha crab rides a unit's body, off the floor light field. Deterministic (field + gradient +
        // config gain) on the pinned FixedUpdate path, so it folds into the replay hash like any crab
        // motion. `clamp_to_patch` keeps the nudge on the current surface patch (gate crossings stay with
        // the mode's flow-field).
        if !latching {
            // Aggression overrides light. A *committed* crab — one the swarm has recruited via the ALARM
            // (Muster) or rally (Rally) pheromone, one already climbing/feeding (Latch), or one hauling a
            // gib home (Carry) — drives THROUGH the light instead of being repelled by it. So the moment the
            // squad opens fire, the ALARM bloom flips nearby crabs to Muster and the swarm floods the lit
            // room; an *idle* forager still shies from the light, so lit ground stays tactical cover ("dark =
            // danger" holds). This is a per-mode gain scale on the existing photophobic taxis, NOT a second
            // path — one light-push, its strength gated by the crab's current decision. `ActiveBehavior.mode`
            // is written by `think` on the pinned FixedUpdate path, so this stays deterministic / replay-safe.
            use crate::ai::utility::Mode;
            let commit = matches!(
                active.mode,
                Mode::Muster | Mode::Rally | Mode::Latch | Mode::Carry
            );
            let light_scale = if commit { 0.0 } else { 1.0 };
            let signed_gain = if photophobic.is_some() {
                -config.lighting.photophobic_gain * light_scale
            } else if photophilic.is_some() {
                config.lighting.photophilic_gain * light_scale
            } else {
                0.0
            };
            let push = crate::light::light_push(&light_field, &dungeon, motion.pos, signed_gain);
            if push.length_squared() > 1.0e-9 {
                let p = graph.patch(motion.patch);
                motion.pos += project_tangent(push, p.normal) * dt;
                motion.pos = clamp_to_patch(motion.pos, p);
            }
        }

        // Almond Water foraging: a WOUNDED crab climbs the water gradient toward a richer seep, on top of
        // whatever its mode is doing — stigmergic foraging over a regenerating resource (Heylighen,
        // *Cognitive Systems Research* 2015). A healthy crab (`fraction` above the wounded threshold) ignores
        // the field — no cost when full — and the push is zero on flat water, so a crab nowhere near a
        // gradient is unbiased. Skipped while latching (a crab riding a unit's body is off the floor water).
        // Deterministic (field + gradient + belief + config gain) on the pinned FixedUpdate path, folding into
        // the replay hash like the light nudge above. This is what makes the seeps contested territory: the
        // same pools heal the squad, so a wounded crab and a wounded unit are drawn to the same water.
        //
        // Belief-modulated (the inversion mechanic): a crab that can smell seeks water it reads as heal and
        // FLEES water it reads as cyanide; an anosmic crab can't tell, so it seeks any water — and forages
        // straight into poison (emergent selection pressure against anosmia). The reading is gated by the
        // deadband so an unsettled pool draws no forage either way.
        if !latching && health.fraction() <= config.almond_water.forage_wounded_frac {
            let aw = &config.almond_water;
            let belief = almond_water.belief_at(dungeon.world_to_cell(motion.pos));
            let seek = if smell.anosmic || belief >= aw.belief_flip_hi {
                1.0 // seek water (heal pool, or can't smell the danger)
            } else if belief <= aw.belief_flip_lo {
                -1.0 // flee (a smelling crab avoids cyanide water)
            } else {
                0.0 // unsettled deadband — no forage
            };
            if seek != 0.0 {
                // The water gradient is on the scale of `capacity` (~100), not light's ~1, so normalise by
                // capacity to keep `forage_gain` on the same footing as the light gains — otherwise the push
                // is ~capacity× too strong and a wounded crab lurches across the map toward the nearest seep.
                let forage_gain = seek * aw.forage_gain / aw.capacity.max(1.0e-6);
                let push =
                    crate::almond_water::almond_push(&almond_water, &dungeon, motion.pos, forage_gain);
                if push.length_squared() > 1.0e-9 {
                    let p = graph.patch(motion.patch);
                    motion.pos += project_tangent(push, p.normal) * dt;
                    motion.pos = clamp_to_patch(motion.pos, p);
                }
            }
        }

        // Seat & orient flat to the current surface (floor, wall, or a unit's body).
        transform.translation = motion.pos + motion.normal * CRAB_BODY_CENTER;
        transform.rotation = surface_orientation(motion.heading, motion.normal);

        if *state != want {
            *state = want;
        }
    }
}


/// Wire the crab's asynchronously-spawned `AnimationPlayer` to the shared graph. Skips players that
/// don't belong to a crab (e.g. squad figurines) and tolerates the player not existing yet.
pub(crate) fn attach_crab_animation(
    mut commands: Commands,
    anim: Res<CrabAnim>,
    added: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    crabs: Query<(), With<Crab>>,
) {
    for player in &added {
        // Walk up the hierarchy to find the owning crab, if any.
        let mut cur = player;
        let owner = loop {
            if crabs.get(cur).is_ok() {
                break Some(cur);
            }
            match parents.get(cur) {
                Ok(child_of) => cur = child_of.parent(),
                Err(_) => break None,
            }
        };
        let Some(owner) = owner else { continue };

        commands
            .entity(player)
            .insert((AnimationGraphHandle(anim.graph.clone()), AnimationTransitions::new()));
        commands.entity(owner).insert(CrabAnimPlayer {
            player,
            playing: None,
        });
    }
}

/// Cross-fade each crab's clip to match its state; only acts on a real change (or first wiring). The
/// walk/attack clips play faster than authored so the leg cycle keeps pace with the scuttle rather than
/// foot-sliding.
pub(crate) fn drive_crab_animation(
    anim: Res<CrabAnim>,
    mut crabs: Query<(&CrabState, &mut CrabAnimPlayer)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (state, mut link) in &mut crabs {
        if link.playing == Some(*state) {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue; // transitions component not applied yet — retry next frame
        };
        let (node, speed) = match state {
            CrabState::Idle => (anim.idle, 1.0),
            CrabState::Walk => (anim.walk, WALK_ANIM_SPEED),
            CrabState::Attack => (anim.attack, ATTACK_ANIM_SPEED),
        };
        let active = transitions.play(&mut player, node, CROSSFADE);
        active.repeat().set_speed(speed);
        link.playing = Some(*state);
    }
}

/// Nearest prey: its position, its planar **forward** (`rotation * −Z`, for the blind-side pounce gate),
/// and the planar distance to `pos`. Read-only over the pounce system's prey query; a thin wrapper over
/// [`crate::util::nearest_planar`] (the shared ranking) carrying the forward vector as the payload.
pub(crate) fn nearest_prey(
    prey: &Query<(&Transform, &mut Health), (With<Prey>, Without<Crab>)>,
    pos: Vec3,
) -> Option<(Vec3, Vec3, f32)> {
    crate::util::nearest_planar(
        pos,
        prey.iter()
            .map(|(ptf, _)| (ptf.rotation * Vec3::NEG_Z, ptf.translation)),
    )
    .map(|(fwd, p, d)| (p, fwd, d))
}

/// Per-crab caste-swap cooldown (hysteresis) so a crab can't re-role again until it counts down —
/// stops castes chattering tick-to-tick. Not itself in `snapshot_hash` (crabs hash by Transform+Health);
/// see the determinism note on [`re_role_crabs`].
#[derive(Component)]
pub(crate) struct Caste {
    pub(crate) cooldown: f32,
}

/// The crab's immortal spawn seed, kept so a promotion re-seeds [`Scout::new`] deterministically and so
/// re-role's per-tick flip budget selects the SAME crabs regardless of ECS iteration order (sort key).
///
/// `pub` because it is the swarm's only stable identity, and the boss's cull
/// ([`crate::enemy::smiley_defense`]) needs it too: that swat picks WHICH crabs die by sorted order, and a
/// position-only key is not a total order — crabs piled on the boss sit at bit-identical coordinates
/// (measured: 6 fully-tied pairs on held-in world `0xA11CE`), so the tie decided a LETHAL pick by ECS query
/// order. A raw `Entity` cannot serve: ids are recycled and their order is not reproducible across
/// same-seed runs — that is the instability being guarded against, not a guard.
#[derive(Component, Clone, Copy)]
pub struct CrabSeed(pub u32);

// Dynamic-caste policy + bounds (README "let crabs re-role between scout and assault as swarm needs
// shift") moved to `behavior.crab` (caste_cooldown, caste_flips_per_tick, rally_live, alarm_high,
// promote_density, scout_min_frac, scout_max_frac). Live scouts are held in
// `[scout_min_frac, scout_max_frac]`; promote/demote signals + a per-crab cooldown give hysteresis.

/// One crab's re-role verdict this tick (pure; unit-tested).
#[derive(PartialEq, Debug)]
pub(crate) enum Rerole {
    Promote,
    Demote,
    Hold,
}

/// Pure caste decision from the local stigmergic picture (cooldown gating handled by the caller):
/// a scout demotes when the swarm is already converging (live beacon) or pressed (alarm); an assault
/// crab promotes when it's crowded with no beacon (recon is the marginal need). Everything else holds.
pub(crate) fn caste_decision(
    is_scout: bool,
    beacon: bool,
    density: f32,
    alarm: f32,
    alarm_high: f32,
    promote_density: f32,
) -> Rerole {
    if is_scout {
        if beacon || alarm > alarm_high {
            Rerole::Demote
        } else {
            Rerole::Hold
        }
    } else if !beacon && density > promote_density {
        Rerole::Promote
    } else {
        Rerole::Hold
    }
}

/// Dynamic castes: re-role crabs between scout and assault as the swarm's needs shift, instead of the
/// birth-fixed split. Runs on `FixedUpdate` after the stigmergic fields refresh and before the brains
/// think, so a flipped crab runs its new brain next tick.
///
/// **Determinism (per `TESTING.md`).** No RNG entropy: the decision is a pure function of deterministic
/// field samples + the fixed [`CrabSeed`]; the per-tick flip budget picks crabs in `CrabSeed` order
/// (order-independent of ECS iteration); a promotion re-seeds `Scout::new` from the stored seed. So
/// two same-seed runs make identical flips and `deterministic_core_is_bit_identical` holds by
/// construction (no committed crab hash to re-pin). `BrainId` and the `Scout` component are always
/// changed **together** so the brain (keys off `BrainId`) and the scout systems (key off `Scout`) never
/// desync.
pub(crate) fn re_role_crabs(
    time: Res<Time>,
    stig: Res<crate::ai::field::Stig>,
    rally: Res<crate::ai::field::RallyField>,
    dungeon: Res<Dungeon>,
    mut commands: Commands,
    mut crabs: Query<(Entity, &CrabMotion, &mut Caste, Option<&Scout>, &CrabSeed)>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let bc = &beh.crab;
    let total = crabs.iter().count();
    if total == 0 {
        return;
    }

    let mut scouts = 0usize;
    let mut promotes: Vec<(Entity, u32)> = Vec::new();
    let mut demotes: Vec<(Entity, u32)> = Vec::new();
    for (e, motion, mut caste, scout, seed) in &mut crabs {
        caste.cooldown = (caste.cooldown - dt).max(0.0);
        let is_scout = scout.is_some();
        if is_scout {
            scouts += 1;
        }
        if caste.cooldown > 0.0 {
            continue; // hysteresis: recently flipped, leave it be
        }
        let density = stig.sample(crate::ai::field::FieldId::CRAB_DENSITY, &dungeon, motion.pos);
        let beacon = rally.sample(&dungeon, motion.pos).length() > bc.rally_live;
        let alarm = stig.sample(crate::ai::field::FieldId::ALARM, &dungeon, motion.pos);
        match caste_decision(is_scout, beacon, density, alarm, bc.alarm_high, bc.promote_density) {
            Rerole::Promote => promotes.push((e, seed.0)),
            Rerole::Demote => demotes.push((e, seed.0)),
            Rerole::Hold => {}
        }
    }

    let min_scouts = (total as f32 * bc.scout_min_frac).round() as usize;
    let max_scouts = (total as f32 * bc.scout_max_frac).round() as usize;
    // Deterministic tiebreak: same crabs flip regardless of iteration order.
    // `CrabSeed` is unique per crab, so these ARE total — and the check now proves it rather than trusting
    // the comment. Both feed a `take(flips_per_tick)` budget, so a tie would silently pick different crabs.
    crate::sort_total!(&mut promotes, |&(_, s): &(Entity, u32)| s);
    crate::sort_total!(&mut demotes, |&(_, s): &(Entity, u32)| s);

    let mut budget = bc.caste_flips_per_tick;
    for &(e, seed) in &promotes {
        if budget == 0 || scouts >= max_scouts {
            break;
        }
        // Promote in lockstep: assault brain → scout brain + insert the Scout component + arm cooldown.
        // `try_insert`: a targeted crab can be shot dead this same tick before the command applies —
        // skip it silently rather than panic on a despawned entity (the count stays deterministic since
        // the death is deterministic).
        commands.entity(e).try_insert((
            crate::ai::brain::BrainId::Scout,
            Scout::new(seed),
            Caste { cooldown: bc.caste_cooldown },
        ));
        scouts += 1;
        budget -= 1;
    }
    for &(e, _) in &demotes {
        if budget == 0 || scouts <= min_scouts {
            break;
        }
        // Demote in lockstep: drop the Scout component AND switch the brain back to assault. `try_*` for
        // the same same-tick-death reason as the promote path above.
        commands
            .entity(e)
            .try_remove::<Scout>()
            .try_insert((crate::ai::brain::BrainId::Crab, Caste { cooldown: bc.caste_cooldown }));
        scouts -= 1;
        budget -= 1;
    }
}

/// Pounce attack: a grounded, hunting crab hunkers down, then leaps a ballistic arc (~10 body lengths)
/// onto a nearby unit and bites on landing. While hunkering/airborne this owns the crab's transform
/// (`crab_locomotion` skips it); on landing it re-homes onto the surface and starts a cooldown. A short
/// wind-up + high peak reads as a real pounce, not a glide.
pub(crate) fn crab_jump(
    time: Res<Time>,
    graph: Option<Res<SurfaceGraph>>,
    dungeon: Res<Dungeon>,
    mut crabs: Query<
        (
            &mut CrabMotion,
            &mut CrabState,
            &mut CrabJump,
            &mut Transform,
            &crate::ai::brain::ActiveBehavior,
        ),
        With<Crab>,
    >,
    mut prey: Query<(&Transform, &mut Health), (With<Prey>, Without<Crab>)>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let Some(graph) = graph else { return };
    let bc = &beh.crab;
    let dt = time.delta_secs().min(MAX_FRAME_DT);

    for (mut motion, mut state, mut jump, mut tf, active) in &mut crabs {
        match jump.phase {
            JumpPhase::Ready => {
                jump.cooldown = (jump.cooldown - dt).max(0.0);
                if jump.cooldown > 0.0 {
                    continue;
                }
                // Only pounce while hunting units (approaching prey), and only at a unit in the band.
                // Muster (alarm surge) and Rally (scout-recruited surge) are aggressive presses too — a
                // charging crab must be able to leap, or the surge reads as a plain walk-up. Without them
                // a mustering crab crosses the whole pounce band (bc.jump_min..bc.jump_len) before flipping to
                // Latch at dist<1.2 (already inside bc.jump_min), so it would never lunge.
                let aggressive = matches!(
                    active.mode,
                    crate::ai::utility::Mode::Latch
                        | crate::ai::utility::Mode::Forage
                        | crate::ai::utility::Mode::Muster
                        | crate::ai::utility::Mode::Rally
                );
                if !aggressive {
                    continue;
                }
                if let Some((tpos, tfwd, d)) = nearest_prey(&prey, motion.pos) {
                    // Blind-side gate: only commit the leap from outside the prey's facing cone, so a
                    // crab pounces when the unit isn't looking rather than lunging head-on into its guns.
                    let in_blind_spot = !unit_is_facing(tpos, tfwd, motion.pos, bc.pounce_blind_cos);
                    if d > bc.jump_min && d < bc.jump_len && in_blind_spot {
                        jump.phase = JumpPhase::Hunker;
                        jump.timer = bc.jump_hunker;
                        jump.from = motion.pos;
                        jump.to = tpos;
                    }
                }
            }
            JumpPhase::Hunker => {
                jump.timer -= dt;
                *state = CrabState::Attack;
                // Crouch: dip toward the surface during the wind-up.
                tf.translation = motion.pos + motion.normal * (CRAB_BODY_CENTER * 0.4);
                if jump.timer <= 0.0 {
                    // Launch toward the prey's CURRENT position.
                    if let Some((tpos, _, _)) = nearest_prey(&prey, motion.pos) {
                        jump.to = tpos;
                    }
                    jump.from = motion.pos;
                    jump.phase = JumpPhase::Air;
                    jump.timer = bc.jump_air;
                    if crate::ai::diag::AI_DIAG {
                        info!("crab: POUNCE dist={:.1}", (jump.to.xz() - jump.from.xz()).length());
                    }
                }
            }
            JumpPhase::Air => {
                jump.timer -= dt;
                let s = (1.0 - (jump.timer / bc.jump_air)).clamp(0.0, 1.0);
                let ground = jump.from.lerp(jump.to, s);
                let height = bc.jump_arc * (std::f32::consts::PI * s).sin();
                motion.pos = ground;
                // Re-home onto the surface beneath the arc so it lands on a real patch.
                if let Some(fp) = graph.floor_patch_cell(dungeon.world_to_cell(ground)) {
                    motion.patch = fp;
                    motion.normal = graph.patch(fp).normal;
                }
                let dir = jump.to.xz() - jump.from.xz();
                if dir.length_squared() > 1.0e-6 {
                    motion.heading = Vec3::new(dir.x, 0.0, dir.y).normalize_or(motion.heading);
                }
                tf.translation = ground + motion.normal * CRAB_BODY_CENTER + Vec3::Y * height;
                tf.rotation = surface_orientation(motion.heading, motion.normal);
                *state = CrabState::Attack;
                if jump.timer <= 0.0 {
                    // Land: clamp onto the patch and bite the nearest prey in reach. A pounce is a
                    // committed lunge, so it always bites on landing — a flat, reliable JUMP_DAMAGE hit
                    // (the super-linear pile bonus lives in `crab_contact_damage`). No critical-mass gate:
                    // the old MASS_MIN check made a lone leap deal zero, so a pouncing crab read as a
                    // harmless hop; a lunge that connects should hurt.
                    motion.pos = clamp_to_patch(motion.pos, graph.patch(motion.patch));
                    let reach_sq = (UNIT_BODY_RADIUS + bc.contact_radius + 0.2).powi(2);
                    // WHICH unit the lunge bites must not depend on prey query order. This used to take
                    // the first in-reach prey the ECS happened to yield and `break` — a
                    // keep-the-first-on-a-tie pick straight into `Health`, and query order is not stable
                    // across `App` instances. A crab landing between two units bit a different one run to
                    // run. `nearest_planar` ranks by `(distance bits, position bits)`, so the victim is a
                    // pure function of geometry — and biting the NEAREST is what the lunge meant anyway.
                    if let Some((_, tpos, _)) =
                        crate::util::nearest_planar(motion.pos, prey.iter().map(|(ptf, _)| ((), ptf.translation)))
                        && (tpos.xz() - motion.pos.xz()).length_squared() <= reach_sq
                    {
                        for (ptf, mut hp) in &mut prey {
                            if ptf.translation == tpos {
                                hp.apply_damage(sim.combat.crab_jump_damage);
                                break;
                            }
                        }
                    }
                    jump.phase = JumpPhase::Ready;
                    jump.cooldown = bc.jump_cooldown;
                }
            }
        }
    }
}

