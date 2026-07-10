//! **Faction** — who an agent belongs to, and therefore whose danger it reads.
//!
//! Fear in this game is stigmergic: an agent's FEAR drive eases toward the local value of a *threat*
//! field (see [`crate::ai::field`]). That only works if the threat channels are separated **by emitter**,
//! and each faction reads only its enemies' channels. A single undifferentiated `THREAT` channel — the
//! original design — meant a firing squad member deposited danger at its own muzzle and then read it back
//! one system later, pinning its FEAR to ~1.0 and making `Mode::Flee` (the top rank for every role)
//! preempt its own combat behaviour. The squad fled from its own gunfire.
//!
//! So: one threat field per creature type, and a `Faction` on every agent that carries [`Drives`] to say
//! which of those fields are *someone else's*.
//!
//! [`Drives`]: crate::ai::drives::Drives

use bevy::prelude::*;

use super::drives::Drives;

/// Which side an agent fights for. Every entity carrying [`Drives`] must have exactly one — enforced at
/// startup by [`validate_factions`].
///
/// Deliberately **not** `Default`: there is no sensible default faction. Defaulting to one would silently
/// give a new creature the wrong enemies (and, worse, make it fear its own kind's emissions) — precisely
/// the "magic result that takes hours to trace" the one-path rule exists to prevent. That is also why this
/// is not a Bevy required-component: `#[require]` needs a `Default`. Tag it explicitly at every spawn.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Faction {
    /// The player's SCP Mobile Task Force squad.
    Foundation,
    /// The crab swarm (assault crabs and scouts alike).
    Crab,
    /// The watcher.
    Anomaly,
}

impl Faction {
    /// Every variant, in registry slot order. Extend alongside the enum.
    pub const ALL: [Faction; 3] = [Faction::Foundation, Faction::Crab, Faction::Anomaly];

    /// Dense index into the per-faction drive registry.
    pub const fn index(self) -> usize {
        match self {
            Faction::Foundation => 0,
            Faction::Crab => 1,
            Faction::Anomaly => 2,
        }
    }
}

/// Number of factions — the width of the per-faction drive registry.
pub const FACTION_COUNT: usize = Faction::ALL.len();

/// Startup guard: every agent that carries [`Drives`] must also carry a [`Faction`].
///
/// `update_drives` selects its rule set by faction. If we instead let the query filter drop factionless
/// agents, a creature spawned without the tag would simply never feel fear — a silent, invisible-in-play
/// bug. So we fail loudly at startup instead, the same way `RoleBrains::get` guards its own invariant.
///
/// This runs once, after `Startup`, covering the squad, the boss, and the initial crab wave. Crabs bred at
/// runtime go through the same `crab::spawn_crab_on_patch` funnel, so their tag is structural rather than
/// checked here; the harness test `every_drives_carrier_has_a_faction_throughout_a_live_run` holds that
/// line over a long unattended run.
pub fn validate_factions(agents: Query<Entity, (With<Drives>, Without<Faction>)>) {
    let missing: Vec<Entity> = agents.iter().collect();
    if !missing.is_empty() {
        panic!(
            "{} agent(s) carry `Drives` without a `Faction` (first: {:?}). Every `Drives` carrier must be \
             tagged at spawn — an untagged agent would silently never feel fear.",
            missing.len(),
            missing[0],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faction_indices_are_dense_and_unique() {
        // The registry is an array indexed by `Faction::index()`, so the indices must be exactly
        // `0..FACTION_COUNT` with no gaps or collisions — otherwise a faction silently reads another's
        // drive rules, or indexes out of bounds.
        let mut seen: Vec<usize> = Faction::ALL.iter().map(|f| f.index()).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..FACTION_COUNT).collect::<Vec<_>>());
    }
}
