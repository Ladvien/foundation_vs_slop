//! `crab::combat` — alarm, feeding, contact damage, and death/despawn (split out of the former `crab.rs`, 2026-07-19 review Finding: large files).

use super::*;

/// Per-crab damage watermark for [`crab_alarm_on_damage`]: `current < last_hp` this tick means it was
/// just hit. A stored delta comparison, not `Health::is_changed()` — `Changed` also fires on an *upward*
/// mutation (the Almond Water heal, `almond_water::almond_water_effect`), which would flood ALARM every
/// heal tick a wounded crab spends standing in a pool. Seeded to spawn HP.
#[derive(Component)]
pub(crate) struct CrabDamageWatch {
    pub(crate) last_hp: f32,
}

/// Alarm-pheromone recruitment to defense: a wounded crab floods the local ALARM channel so every crab
/// within ~one room reads `Fact::AlarmHere`, musters (converges on the squad), and stops fleeing — the
/// fix for "shoot the crabs and they just scatter". This is the retaliatory, *local* twin of the nest's
/// own alarm (`nest::nest_alarm`): nest hit → a stronger, wider bloom, crab hit → a one-room alarm bloom
/// that self-limits as the field evaporates. Detection is a [`CrabDamageWatch`] delta, not
/// `Health::is_changed()` — see its doc for why. A stigmergic warning cry (Heylighen, "Stigmergy as a
/// universal coordination mechanism", CSR 2016).
pub(crate) fn crab_alarm_on_damage(
    mut crabs: Query<(&Health, &mut CrabDamageWatch, &Transform), With<Crab>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<SimTuning>,
) {
    // Collect this tick's wounded-crab alarm deposits into a local batch, then sort it into the canonical
    // deposit order before appending to the shared queue. `drain_deposits` applies each with a
    // non-associative `f32 +=` in queue order, and the crab query order is NOT reproducible across runs
    // (async GLB load + entity-id reuse — see the carry logistics), so two wounded crabs whose ALARM
    // blooms overlap would otherwise sum to a query-order-dependent value, diverging the ALARM channel and
    // the physics-off replay hash (~1-3% of runs). Sorting the batch makes the drained field a pure
    // function of the deposits, exactly as `ai::field::sort_deposits` documents (the same class of fix as
    // the crab-separation bucket sort above).
    let mut batch: Vec<crate::ai::field::Deposit> = Vec::new();
    for (hp, mut watch, tf) in &mut crabs {
        if hp.current < watch.last_hp - 1.0e-3 {
            batch.push(crate::ai::field::Deposit {
                pos: tf.translation,
                field: crate::ai::field::FieldId::ALARM,
                amount: sim.deposit.alarm_crab,
            });
        }
        watch.last_hp = hp.current;
    }
    crate::ai::field::sort_deposits(&mut batch);
    deposits.0.extend(batch);
}

/// Feeding sates hunger: an actively-biting crab (`CrabState::Attack`) drains its HUNGER drive, so a fed
/// crab's forage/latch/seek weighting falls and it peels off while hungrier crabs press. Without this,
/// HUNGER only ever rises (nothing consumed it), saturating every crab at 1.0 within ~33 s — a uniform
/// constant that cancels out of the utility maths and gives zero per-agent differentiation. Pairs with
/// the per-crab HUNGER seed at spawn.
pub(crate) fn crab_feeding_sates_hunger(
    time: Res<Time>,
    mut crabs: Query<(&CrabState, &mut crate::ai::drives::Drives), With<Crab>>,
    sim: Res<SimTuning>,
) {
    let dt = time.delta_secs();
    for (state, mut drives) in &mut crabs {
        if *state == CrabState::Attack {
            let h = drives.get(crate::ai::drives::DriveId::HUNGER);
            drives.set(crate::ai::drives::DriveId::HUNGER, h - sim.breeding.hunger_sate_rate * dt);
        }
    }
}

/// Feeding frenzy: damage to a unit grows **super-linearly** with how many crabs are on it
/// (`CRAB_CONTACT_DPS * count^DAMAGE_EXPONENT`), so one crab is a real nuisance and a pile shreds it —
/// a smooth ramp with NO critical-mass cliff (1 crab ≈ 3 DPS, 3 ≈ 15, 5 ≈ 33, 10 ≈ 95). The old hard
/// `MASS_MIN` gate made 1–4 crabs deal literally zero damage, so a thinned/split swarm played harmless;
/// the super-linear curve already makes a lone crab weak and a pile terrifying without a dead zone.
/// Counts by PLANAR distance so a crab clinging high on the body still feeds.
pub(crate) fn crab_contact_damage(
    time: Res<Time>,
    crabs: Query<(Entity, &Transform, &CrabSeed), (With<Crab>, Without<Prey>)>,
    // `Option<&mut LastAttacker>` is present only on the smiley watcher: a crab biting it records itself as
    // the attacker so the watcher retaliates against the swarm (not a bystander unit) — see `enemy::smiley_zap`.
    mut prey: Query<(&Transform, &mut Health, Option<&mut crate::enemy::LastAttacker>), (With<Prey>, Without<Crab>)>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let dt = time.delta_secs();
    let bc = &beh.crab;
    // Reach = body radius + a little, so anything latched onto the cylinder counts (units and the boss).
    let reach_sq = (UNIT_BODY_RADIUS + bc.contact_radius).powi(2);
    for (prey_tf, mut hp, last_attacker) in &mut prey {
        // Count biters and attribute the bite to ONE of them for the boss's retaliation. WHICH biter is
        // recorded (`LastAttacker`, which `enemy::smiley_zap` INSTAKILLS) must not depend on query order —
        // it is not reproducible across same-seed runs (see `util::nearest_planar`).
        //
        // `CrabSeed` is the tiebreak and it is load-bearing: world position ALONE is not a total order, and
        // this key was position-only under a comment calling it "a stable geometric key". Crabs pile onto
        // the boss to bite it — that is the entire point of this system — and `clamp_to_patch` pins a
        // pressed crab onto the same float, so the biters routinely sit at BIT-IDENTICAL coordinates. The
        // `<` then compared false, the loop kept whichever the ECS yielded first, and the boss executed a
        // different crab run to run. Measured (mutant #4, world `0x5C09191`, tick 94): two crabs at
        // `(14.9412, 13.9406)`, seeds 2 and 8 — seed 8 zapped in one run, seed 2 in the other.
        let mut count = 0usize;
        let mut biter: Option<Entity> = None;
        let mut biter_key: Option<(u32, u32, u32, u32)> = None;
        for (ce, ctf, seed) in &crabs {
            if (ctf.translation.xz() - prey_tf.translation.xz()).length_squared() <= reach_sq {
                count += 1;
                let key = (
                    ctf.translation.x.to_bits(),
                    ctf.translation.y.to_bits(),
                    ctf.translation.z.to_bits(),
                    seed.0,
                );
                if biter_key.is_none_or(|bk| key < bk) {
                    biter = Some(ce);
                    biter_key = Some(key);
                }
            }
        }
        if count > 0 {
            hp.apply_damage(sim.combat.crab_contact_dps * (count as f32).powf(sim.combat.crab_damage_exponent) * dt);
            if let Some(mut la) = last_attacker {
                la.entity = biter;
                la.age = 0.0;
            }
        }
    }
}

/// Despawn dead crabs with a small blood burst + squelch (reuses the enemy-death VFX/SFX path).
/// Tag set by `enemy::smiley_defense` on a crab it culls. Read by `crab_despawn_dead` (the single crab
/// despawn owner) to emit the boss-swat gore variant and — crucially — suppress the blood SCENT bloom a
/// normal death emits (a scent here would magnet more crabs into a feeding feedback loop).
#[derive(Component)]
pub struct Culled;

/// The set holding [`crab_despawn_dead`], the single owner of crab removal.
///
/// Anything that *tags* a crab for death rather than despawning it — `enemy::smiley_defense`, which writes
/// [`Culled`] — must be ordered `.before` this set. A sole despawn owner prevents a double-despawn, but it
/// does nothing about an `insert` command queued **after** the despawn command: applying it panics with
/// "Entity despawned". The two systems live in different plugins, so nothing but this set can express the
/// ordering they have always relied on.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CrabDespawn;

/// The ONE system that removes a crab at ≤ 0 HP, whatever zeroed it (laser, `smiley_zap`, or a boss cull
/// via the `Culled` tag). Being the sole despawn+gore owner is what prevents the double-despawn /
/// double-gore race with `smiley_defense`, which now only zeroes HP + tags instead of despawning itself.
pub(crate) fn crab_despawn_dead(
    mut commands: Commands,
    mut gore: ResMut<GoreQueue>,
    mut sfx: MessageWriter<Sfx>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    crabs: Query<(Entity, &Health, &Transform, Option<&Culled>, &CrabSeed), With<Crab>>,
    sim: Res<SimTuning>,
    audio: Res<crate::audio_tuning::AudioTuning>,
) {
    // Emit gore + despawn deaths in a STABLE order (by `CrabSeed`, unique+deterministic), NOT crab query
    // order — which is not reproducible across same-seed runs (see `util::nearest_planar`). The gore
    // drain stamps each meat chunk with a per-event seed counter, and the ECS entity free-list reuses
    // these just-freed ids; both depend on THIS order, so an unsorted pass gives meat chunks
    // nondeterministic spawn params AND a nondeterministic gib table/query order, which then makes the
    // crab foraging assignment (`assign_meat_targets`) diverge and breaks the physics-free replay hash.
    let mut dead: Vec<(u32, Entity, Vec3, bool)> = crabs
        .iter()
        .filter(|(_, hp, _, _, _)| hp.current <= 0.0)
        .map(|(e, _, tf, culled, seed)| (seed.0, e, tf.translation, culled.is_some()))
        .collect();
    crate::sort_total!(&mut dead, |(seed, _, _, _)| *seed);
    for (_, entity, pos, culled) in dead {
        if culled {
            // Boss cull (`smiley_defense`): green-ichor swat, and deliberately NO SCENT deposit — a
            // scent bloom here would magnet more crabs into a feeding feedback loop. No per-crab death
            // sfx either (the boss already played one batched swat for the whole cull).
            gore.0.push(GoreEvent {
                pos,
                kind: GoreKind::EnemySplat,
                tint: crate::palette::CRAB_ICHOR, // Type-Gray reanimated ichor (green)
                gib: None,
                intensity: 0.2,
            });
        } else {
            gore.0.push(GoreEvent {
                pos,
                kind: GoreKind::EnemySplat,
                tint: crate::palette::CRAB_ICHOR_DULL, // sickly green crab ichor
                gib: None,
                // Chaff: a crab death barely nudges the camera, so a whole swarm dying doesn't read as
                // one giant explosion (the gib chunks still pop — only the feel layer is scaled down).
                intensity: crate::gore::death_intensity(sim.combat.crab_hp, sim.combat.crab_contact_dps),
            });
            // Blood → SCENT: a fresh kill draws the swarm and the boss to the feeding site.
            deposits.0.push(crate::ai::field::Deposit {
                pos,
                field: crate::ai::field::FieldId::SCENT,
                amount: sim.deposit.blood_scent,
            });
            sfx.write(Sfx::EnemyDeath(pos));
            // The wet crunch of a crab death carries as swarm din (`NOISE_SWARM`); the *units* read it.
            deposits.0.push(crate::ai::field::Deposit {
                pos,
                field: crate::ai::field::FieldId::NOISE_SWARM,
                amount: audio.stimulus.enemy_death_loudness,
            });
        }
        commands.entity(entity).despawn();
    }
}

// Flee speed multiplier + the carry-crew tuning (carry_capacity, grab_range, los_range, carry_hold,
// carry_speed, weight_drag, deliver_range, crew_timeout, max_commit_dist) moved to `behavior.crab`. Only
// the render-height seat stays in code.

