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
    pub role: RoleId,
    /// A one-word temperament tag steering tone (e.g. "clipped", "clinical", "haunted"). Consumed by
    /// the dialogue templates and injected into the LLM preamble.
    pub temperament: String,
    /// How readily this member speaks, `[0,1]` — throttles bark frequency (a taciturn gunman vs. a
    /// chatty researcher).
    pub verbosity: f32,
}

/// The compile-safe default roster (index-matched to [`RoleId::ALL`] / spawn order), used when no
/// `personas.ron` is present. SCP Mobile-Task-Force flavour.
pub fn default_personas() -> [Persona; 5] {
    [
        Persona {
            name: "Vasquez".into(),
            role: RoleId::Gunman,
            temperament: "clipped".into(),
            verbosity: 0.4,
        },
        Persona {
            name: "Dr. Okafor".into(),
            role: RoleId::Researcher,
            temperament: "clinical".into(),
            verbosity: 0.9,
        },
        Persona {
            name: "Sable".into(),
            role: RoleId::Psionic,
            temperament: "haunted".into(),
            verbosity: 0.6,
        },
        Persona {
            name: "Reyes".into(),
            role: RoleId::Medic,
            temperament: "steady".into(),
            verbosity: 0.6,
        },
        Persona {
            name: "Kowalski".into(),
            role: RoleId::Engineer,
            temperament: "dry".into(),
            verbosity: 0.5,
        },
    ]
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
        // No override file → the complete, playable default roster (the expected common case).
        Err(_) => return Ok(default_personas()),
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
            (name: "Test", role: Researcher, temperament: "clinical", verbosity: 0.8),
        ]"#;
        let v = parse_personas_ron(src).expect("valid personas.ron");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].role, RoleId::Researcher);
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
