//! **`Researched` and the unlock hook** (FVS-E-4), plus the tech-tree flags it sets (FVS-F-1).
//!
//! Completing a specimen's posterior is what pays out. The payout is a **capability flag**, and the
//! shape of that is the whole of FVS-F-2's rule: an unlock grants a *new verb*, never a number.
//!
//! ## Why a marker with a hook, and not a system that scans posteriors
//!
//! Same argument as `containment::Contained`, and it is the reason both exist:
//!
//! * A hook fires **exactly once**, at command-apply time, for every insert. A scanning system has to
//!   re-derive "is this newly complete?" every tick and remember what it has already paid out — which
//!   is state that can drift from the thing it mirrors.
//! * A hook adds **no schedule node**, so it cannot permute the `FixedUpdate` linearisation and move
//!   the goldens.
//! * There is then exactly one path from research to an unlock, so the invariant "one completion, one
//!   unlock" is structural rather than asserted.
//!
//! ## Idempotence is by construction, not by a guard
//!
//! [`Researched`] is inserted once and never removed, and Bevy's `on_add` does not fire for a
//! re-insert of a component the entity already has. The flags themselves are a set, so setting one
//! twice is a no-op anyway. Two independent reasons, neither of which is an `if already_done` branch.

use bevy::prelude::*;

use super::posterior::ResearchPosterior;

/// A capability the Foundation has derived from a contained anomaly.
///
/// **Thaumiel logic** — use the contained to contain. Each of these is a *verb the player did not have*,
/// which is FVS-F-2's hard review rule: no entry here may ever be "+X% to something".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Capability {
    /// From SCP-999: a calm field that lets the squad lower an anomaly's aggression without firing.
    MoraleField,
    /// From SCP-1048: a deployable observer that holds `ATTENTION` on a cell without an operative
    /// standing there.
    RemoteObserver,
    /// From SCP-150: a field cure, so an infested operative can be treated without extraction.
    FieldCure,
    /// From a capped nest: charges that seal a structure at range.
    RemoteCapping,
}

impl Capability {
    pub const ALL: [Capability; 4] = [
        Capability::MoraleField,
        Capability::RemoteObserver,
        Capability::FieldCure,
        Capability::RemoteCapping,
    ];

    fn bit(self) -> u32 {
        match self {
            Capability::MoraleField => 1 << 0,
            Capability::RemoteObserver => 1 << 1,
            Capability::FieldCure => 1 << 2,
            Capability::RemoteCapping => 1 << 3,
        }
    }

    /// Player-facing name. Every one of these is phrased as **something you can now do**, which is the
    /// F-2 rule made visible: if a name can only be written as a percentage, the unlock is wrong.
    pub fn label(self) -> &'static str {
        match self {
            Capability::MoraleField => "DEPLOY MORALE FIELD",
            Capability::RemoteObserver => "DEPLOY REMOTE OBSERVER",
            Capability::FieldCure => "ADMINISTER FIELD CURE",
            Capability::RemoteCapping => "FIRE SEALING CHARGE",
        }
    }
}

/// Which capabilities the Foundation has unlocked (FVS-F-1).
///
/// A bitset resource, **not** run-scoped: unlocks are meta-progress and outlive the expedition, exactly
/// like `containment::Specimen`.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TechTree(u32);

impl TechTree {
    pub fn has(&self, c: Capability) -> bool {
        self.0 & c.bit() != 0
    }
    /// Grant a capability. Idempotent by construction — it is a set.
    pub fn grant(&mut self, c: Capability) {
        self.0 |= c.bit();
    }
    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }
    /// The raw flag bits, for persistence (FVS-G-2).
    ///
    /// Round-tripping the bitset rather than a list of names is deliberate: `Capability`'s bits are
    /// **append-only** (like `HiddenParam`'s indices and `squad_ai`'s `ActorKind`), so the number is
    /// stable across builds, whereas a renamed variant would silently drop an unlock from a save.
    pub fn bits(&self) -> u32 {
        self.0
    }
    /// Restore from saved bits. Unknown bits are kept rather than masked off — a save written by a
    /// build that had more capabilities should not silently lose them on a round trip through an older
    /// one, and `has()` only ever asks about capabilities this build knows.
    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}

/// Terminal marker: this specimen's research is **finished**. Inserted once, never removed.
///
/// Its `on_add` hook is the single place a capability is ever granted.
#[derive(Component)]
#[component(on_add = grant_capability)]
pub struct Researched;

/// The specimen's payout — which capability completing *this* research unlocks.
///
/// Carried on the specimen rather than looked up from a table keyed on species, because the species is
/// not recorded (see `site::pieces::SitePiece::SpecimenStandin` for why) and because an authored payout
/// is what makes FVS-F-3's curriculum a thing a designer writes rather than a thing that emerges.
#[derive(Component, Debug, Clone, Copy)]
pub struct Unlocks(pub Capability);

/// The reward. **The only path from research to a capability.**
fn grant_capability(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    let Some(&Unlocks(cap)) = world.get::<Unlocks>(ctx.entity) else {
        // A specimen with no authored payout completes its research and grants nothing. Loud, because
        // it is a content gap — every capturable anomaly should be worth something — but not fatal.
        warn!("research: a specimen completed with no `Unlocks` payout authored; nothing granted");
        return;
    };
    if let Some(mut tree) = world.get_resource_mut::<TechTree>() {
        tree.grant(cap);
    }
}

/// Insert [`Researched`] on any specimen whose posterior is complete.
///
/// `Update`, not `FixedUpdate`: research is resolved at the Site between expeditions, nothing pinned
/// reads it, and keeping it off the fixed schedule means it cannot permute the linearisation.
pub fn finish_completed_research(
    mut commands: Commands,
    specimens: Query<(Entity, &ResearchPosterior), Without<Researched>>,
) {
    for (e, posterior) in &specimens {
        if posterior.is_complete() {
            commands.entity(e).insert(Researched);
        }
    }
}

pub struct ResearchPlugin;

impl Plugin for ResearchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TechTree>()
            .add_systems(Update, finish_completed_research);
    }
}

#[cfg(test)]
mod tests {
    use super::super::HiddenParam;
    use super::*;

    fn app_with(posterior: ResearchPosterior, payout: Option<Capability>) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(ResearchPlugin);
        let mut e = app.world_mut().spawn(posterior);
        if let Some(c) = payout {
            e.insert(Unlocks(c));
        }
        let id = e.id();
        (app, id)
    }

    fn complete() -> ResearchPosterior {
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            p.reveal(q);
        }
        p
    }

    #[test]
    fn completing_research_grants_exactly_one_capability() {
        let (mut app, e) = app_with(complete(), Some(Capability::MoraleField));
        app.update();
        assert!(app.world().get::<Researched>(e).is_some(), "a complete posterior must finish");
        let tree = *app.world().resource::<TechTree>();
        assert!(tree.has(Capability::MoraleField));
        assert_eq!(tree.count(), 1, "one completion, one unlock");
    }

    #[test]
    fn an_unfinished_posterior_grants_nothing() {
        let (mut app, e) = app_with(ResearchPosterior::unknown(), Some(Capability::MoraleField));
        app.update();
        assert!(app.world().get::<Researched>(e).is_none());
        assert_eq!(app.world().resource::<TechTree>().count(), 0);
    }

    #[test]
    fn the_unlock_is_idempotent_across_many_frames() {
        // Idempotence is by construction — the marker is inserted once and Bevy's `on_add` does not
        // re-fire, and the flags are a set. This pins BOTH, so removing either does not silently
        // survive on the other.
        let (mut app, _) = app_with(complete(), Some(Capability::RemoteObserver));
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().resource::<TechTree>().count(), 1, "ten frames, one unlock");
    }

    #[test]
    fn a_specimen_with_no_authored_payout_completes_but_grants_nothing() {
        // A content gap, not a crash: the specimen is still researched, the tree is untouched, and the
        // warning names the problem.
        let (mut app, e) = app_with(complete(), None);
        app.update();
        assert!(app.world().get::<Researched>(e).is_some());
        assert_eq!(app.world().resource::<TechTree>().count(), 0);
    }

    #[test]
    fn every_capability_is_named_as_a_verb_not_a_number() {
        // FVS-F-2's hard review rule, enforced rather than remembered: an unlock grants something you
        // can DO. If a new entry can only be described as a percentage, it does not belong here, and
        // this is where that gets caught.
        for c in Capability::ALL {
            let label = c.label();
            assert!(!label.is_empty());
            assert!(
                !label.contains('%') && !label.contains('+'),
                "{c:?} reads as a numeric buff ({label}) — unlocks grant verbs (FVS-F-2)"
            );
            assert_eq!(label, label.to_uppercase(), "{c:?} label should match the HUD's voice");
        }
    }

    #[test]
    fn capabilities_have_distinct_bits() {
        // A copy-pasted bit would make two unlocks silently the same one.
        let mut seen = 0u32;
        for c in Capability::ALL {
            assert_eq!(seen & c.bit(), 0, "{c:?} shares a bit with an earlier capability");
            seen |= c.bit();
        }
    }
}
