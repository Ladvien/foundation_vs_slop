//! Squad **personas** — the speaker identity a unit carries so its dialogue reads as a specific
//! character, not an anonymous colored figure. The dialogue system today keys only on `SquadMember`
//! index + team colour; a persona attaches a name, a role, and a temperament that shapes generated
//! lines. It is the "persona-via-preamble" surface for an LLM provider (Shanahan et al., "Role-Play
//! with Large Language Models", 2023) and the flavour key for the deterministic template provider.

use bevy::prelude::*;
use serde::Deserialize;

use super::role::RoleId;

/// A squad member's character. `Deserialize` so the roster lives in `assets/config/personas.ron`.
#[derive(Component, Clone, Debug, Deserialize)]
pub struct Persona {
    /// Callsign / name shown or spoken (e.g. "Vasquez", "Dr. Okafor").
    pub name: String,
    /// The role this persona plays (also carried as a [`RoleId`] component for the AI).
    ///
    /// **This is a behaviour repertoire, not a job.** See [`Self::title`] — the two used to be
    /// conflated, and that conflation is why three files disagreed about who these five people are.
    pub role: RoleId,
    /// What this operative's **job** is, on the Foundation's own org chart.
    ///
    /// Added 2026-08-02 to reconcile a three-way disagreement, and the fix is to stop asking one field
    /// two questions. `RoleId` answers *how does this unit behave* — it keys `RoleBrains` and the
    /// `roles.ron` override map, so its variants are a serialized format. This answers *what is this
    /// person's job*, which is what the player reads and what `docs/lore/` specifies.
    ///
    /// So slot 1 is `RoleId::Researcher` **and** [`StaffTitle::Xenobiologist`], and both the AI and
    /// `config.ron`'s speaker note were right about it all along.
    pub title: crate::personnel::StaffTitle,
    /// What this operative is permitted to know.
    ///
    /// Not a rank (see [`Clearance`](crate::personnel::Clearance)). It is here rather than derived from
    /// [`Self::title`] because canon is explicit that the two axes are independent — a Level 1 guard
    /// and a Level 3 researcher can hold the same post, and a title does not imply a ceiling.
    pub clearance: crate::personnel::Clearance,
    /// A one-word temperament tag steering tone (e.g. "clipped", "clinical", "haunted"). Consumed by
    /// the dialogue templates and injected into the LLM preamble.
    pub temperament: String,
    /// How readily this member speaks, `[0,1]` — throttles bark frequency (a taciturn gunman vs. a
    /// chatty researcher).
    pub verbosity: f32,
}

/// The compile-safe default roster (index-matched to [`RoleId::ALL`] / spawn order), used when no
/// `personas.ron` is present. SCP Mobile-Task-Force flavour.
///
/// **The cast, reconciled 2026-08-02** (`docs/lore/2026-08-02-site-67-recommissioned.md` §2). Each row
/// now carries both axes, so the AI's repertoire and the player-facing job can differ without either
/// being wrong:
///
/// | # | `RoleId` (behaviour) | Name | [`StaffTitle`](crate::personnel::StaffTitle) (job) | Clearance |
/// |---|---|---|---|---|
/// | 0 | `Gunman` | Vasquez | MTF Operative — team lead | 2 |
/// | 1 | `Researcher` | Dr. Okafor | Xenobiologist | 3 |
/// | 2 | `Psionic` | Sable | Psionics Specialist | 2 |
/// | 3 | `Medic` | Reyes | Medical Officer | 2 |
/// | 4 | `Engineer` | Kowalski | Containment Specialist | 2 |
///
/// Titles are the taxonomy doc's §5 canon list. Okafor's is the one that resolves the old conflict:
/// `config.ron`'s speaker note called voice 1 "xenobiologist" while `RoleId` called slot 1
/// `Researcher`, and both were describing the same person from different axes.
///
/// ⚠️ **This list is append-only in spirit.** Reordering it renumbers every `SquadMember`, and
/// `SquadKnowledge` is an array indexed by that number — so a swap here silently reassigns who believes
/// what, in every existing save.
pub fn default_personas() -> [Persona; 5] {
    use crate::personnel::{Clearance, StaffTitle};
    [
        Persona {
            name: "Vasquez".into(),
            role: RoleId::Gunman,
            // The team lead, and the one job on this roster that canon reserves for special forces.
            title: StaffTitle::MtfOperative,
            clearance: Clearance::Level2,
            temperament: "clipped".into(),
            verbosity: 0.4,
        },
        Persona {
            name: "Dr. Okafor".into(),
            role: RoleId::Researcher,
            // Level 3 alone on this roster: the taxonomy doc reserves Secret for staff who need
            // recovery circumstances and long-term planning, which is what a research lead reads.
            title: StaffTitle::Xenobiologist,
            clearance: Clearance::Level3,
            temperament: "clinical".into(),
            verbosity: 0.9,
        },
        Persona {
            name: "Sable".into(),
            role: RoleId::Psionic,
            title: StaffTitle::PsionicsSpecialist,
            clearance: Clearance::Level2,
            temperament: "haunted".into(),
            verbosity: 0.6,
        },
        Persona {
            name: "Reyes".into(),
            role: RoleId::Medic,
            title: StaffTitle::MedicalOfficer,
            clearance: Clearance::Level2,
            temperament: "steady".into(),
            verbosity: 0.6,
        },
        Persona {
            name: "Kowalski".into(),
            role: RoleId::Engineer,
            // Canon's containment specialists are engineers and technicians, explicitly *not* combat
            // personnel — which is exactly what this slot's behaviour repertoire already was.
            title: StaffTitle::ContainmentSpecialist,
            clearance: Clearance::Level2,
            temperament: "dry".into(),
            verbosity: 0.5,
        },
    ]
}

/// The resolved cast, loaded once at plugin build.
///
/// **A resource rather than a per-run [`load_personas`] call, because the Site needs it.** Squad
/// `Unit`s only exist while `RunState::Active` (`spawn_unit` carries `session::run_scoped()`), so
/// between expeditions the persona components are gone — which is why the roster screen at Site-67
/// printed `OPERATIVE 0`..`OPERATIVE 4` for its entire existence: both callers of
/// [`roster_rows_all`](crate::knowledge::roster::roster_rows_all) had no names to pass and passed
/// `&[]`. Identity outlives the bodies, so it lives in a resource rather than on them.
///
/// Loaded in `SquadPlugin::build` exactly the way `SitePlugin` loads `SiteKitRes` — one path, and a
/// malformed override is a loud startup panic rather than a silent fall back to the defaults the
/// author was trying to replace.
#[derive(Resource, Debug, Clone)]
pub struct PersonaRoster(pub [Persona; 5]);

impl PersonaRoster {
    /// The five names, in `SquadMember` order — what the roster screen prints.
    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|p| p.name.clone()).collect()
    }

    /// `Vasquez — MTF OPERATIVE · LEVEL 2`, in `SquadMember` order.
    ///
    /// The title is the axis added on 2026-08-02 (see [`Persona::title`]); printing it here is what
    /// gives that field a caller, and what lets a player see that the xenobiologist and the psionics
    /// specialist are different jobs rather than two identically-labelled researchers.
    pub fn name_plates(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|p| format!("{} — {} · {}", p.name, p.title.label(), p.clearance.label()))
            .collect()
    }
}

/// Parse a `personas.ron` roster: `[ (name: "...", role: Gunman, temperament: "...", verbosity: 0.4), ... ]`.
pub fn parse_personas_ron(src: &str) -> Result<Vec<Persona>, ron::error::SpannedError> {
    ron::from_str(src)
}

/// Resolve the squad roster used by `spawn_squad`: the validated `assets/config/personas.ron` when
/// present, else the code-literal [`default_personas`]. A missing file is the normal case; a
/// present-but-malformed-or-invalid file is an **error, never a silent fallback to defaults** — the
/// author asked for a re-voiced squad and must see if it failed (symmetric with `roles.ron`, the exact
/// asymmetry the review flagged: previously `parse_personas_ron` had no non-test caller, so the file
/// was inert). Validity = exactly five personas whose roles match the spawn order (`RoleId::ALL`,
/// member *i* plays role *i*) with in-range verbosity. Returns the roster or a human-readable error.
pub fn load_personas() -> Result<[Persona; 5], String> {
    let src = match std::fs::read_to_string("assets/config/personas.ron") {
        Ok(src) => src,
        // *Absent* is the expected common case → the complete, playable default roster. Any OTHER io
        // error (permission denied, path-is-a-directory, bad encoding) means the author put a file
        // there and we could not read it: fail loudly rather than silently voicing the squad with the
        // defaults they meant to replace. Only `NotFound` is "no override".
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(default_personas()),
        Err(e) => return Err(format!("unreadable: {e}")),
    };
    let list = parse_personas_ron(&src).map_err(|e| format!("malformed: {e}"))?;
    validate_personas(list)
}

/// Validate a parsed persona list into the index-matched spawn roster (pure, so it is unit-testable
/// without touching the filesystem). Invariants: exactly five personas, roles matching `RoleId::ALL`
/// spawn order (member *i* plays role *i*), verbosity in `[0,1]`. Returns the roster or a loud error.
fn validate_personas(list: Vec<Persona>) -> Result<[Persona; 5], String> {
    let roster: [Persona; 5] = list
        .try_into()
        .map_err(|v: Vec<Persona>| format!("must define exactly 5 personas, got {}", v.len()))?;
    for (i, (p, role)) in roster.iter().zip(RoleId::ALL).enumerate() {
        if p.role != role {
            return Err(format!(
                "persona #{i} '{}' has role {:?} but spawn slot {i} plays {role:?} \
                 (roster order must match RoleId::ALL)",
                p.name, p.role
            ));
        }
        if !(0.0..=1.0).contains(&p.verbosity) {
            return Err(format!(
                "persona #{i} '{}' has verbosity {} outside [0,1]",
                p.name, p.verbosity
            ));
        }
    }
    Ok(roster)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roster_matches_role_order() {
        let personas = default_personas();
        for (p, role) in personas.iter().zip(RoleId::ALL) {
            assert_eq!(p.role, role);
            assert!(!p.name.is_empty());
            assert!((0.0..=1.0).contains(&p.verbosity));
        }
    }

    #[test]
    fn personas_ron_parses() {
        let src = r#"[
            (name: "Test", role: Researcher, title: Xenobiologist, clearance: Level3,
             temperament: "clinical", verbosity: 0.8),
        ]"#;
        let v = parse_personas_ron(src).expect("valid personas.ron");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].role, RoleId::Researcher);
        // The two axes are independent, and this row is the case that proves it: behaviour
        // `Researcher`, job `Xenobiologist`. Conflating them is what `personnel.rs` exists to stop.
        assert_eq!(v[0].title, crate::personnel::StaffTitle::Xenobiologist);
        assert_eq!(v[0].clearance, crate::personnel::Clearance::Level3);
    }

    #[test]
    fn an_authored_persona_must_state_its_title_and_clearance() {
        // No `#[serde(default)]` on either field, deliberately. A persona file that omits the job is
        // half-authored, and defaulting it would silently hand every operative the same title — the
        // exact "one field answering two questions" state the reconciliation was undoing.
        let src = r#"[
            (name: "Test", role: Researcher, temperament: "clinical", verbosity: 0.8),
        ]"#;
        assert!(
            parse_personas_ron(src).is_err(),
            "a persona with no title parsed — the field must be required, not defaulted"
        );
    }

    #[test]
    fn the_name_plate_prints_the_job_not_the_behaviour_repertoire() {
        // The player-facing string. `RoleId::Gunman` is a dual-utility behaviour set and must never
        // reach a screen; `MTF OPERATIVE` is the job, and it is what canon calls this person.
        let plates = PersonaRoster(default_personas()).name_plates();
        assert_eq!(plates.len(), 5);
        assert_eq!(plates[0], "Vasquez — MTF OPERATIVE · LEVEL 2");
        assert_eq!(plates[1], "Dr. Okafor — XENOBIOLOGIST · LEVEL 3");
        for plate in &plates {
            assert!(!plate.contains("Gunman"), "a behaviour repertoire reached the screen: {plate}");
            assert!(!plate.contains("Psionic,"), "a behaviour repertoire reached the screen: {plate}");
        }
    }

    #[test]
    fn validate_accepts_the_default_roster() {
        // The default roster is index-matched to RoleId::ALL, so it must validate cleanly (the loader
        // returns it verbatim when no file is present).
        let roster = validate_personas(default_personas().to_vec()).expect("defaults are valid");
        assert_eq!(roster[0].role, RoleId::Gunman);
    }

    #[test]
    fn validate_rejects_wrong_count() {
        let one = vec![default_personas()[0].clone()];
        let err = validate_personas(one).expect_err("a 1-persona roster must be rejected");
        assert!(err.contains("exactly 5"), "unhelpful error: {err}");
    }

    #[test]
    fn validate_rejects_role_order_mismatch() {
        // Roles must match spawn order; a roster whose slot 0 isn't the Gunman is rejected loudly
        // rather than silently mis-voicing every unit.
        let mut roster = default_personas().to_vec();
        roster.swap(0, 1); // now slot 0 is the Researcher
        let err = validate_personas(roster).expect_err("role-order mismatch must be rejected");
        assert!(err.contains("spawn slot 0"), "unhelpful error: {err}");
    }
}
