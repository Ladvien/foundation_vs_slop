//! Squad **personas** — the speaker identity a unit carries so its dialogue reads as a specific
//! character, not an anonymous colored figure. The dialogue system today keys only on `SquadMember`
//! index + team colour; a persona attaches a name, a role, and a temperament that shapes generated
//! lines. It is the "persona-via-preamble" surface for an LLM provider (Shanahan et al., "Role-Play
//! with Large Language Models", 2023) and the flavour key for the deterministic template provider.

use bevy::prelude::*;
use serde::Deserialize;

use super::role::RoleId;

/// A squad member's character. `Deserialize` so the roster lives in `assets/config/personas.ron`.
#[derive(Component, Clone, Deserialize)]
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
}
