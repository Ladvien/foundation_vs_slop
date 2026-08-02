//! **The Foundation's personnel axes** — what someone's *job* is, and what they are *allowed to know*.
//!
//! Reference: `docs/lore/2026-07-12-scp-role-taxonomy.md` §3–§5, whose quoted definitions come from the
//! SCP Wiki's Security Clearance Levels page (CC BY-SA 3.0). The Site-67 cast that uses these is in
//! `docs/lore/2026-08-02-site-67-recommissioned.md` §3.
//!
//! # Why this is not `RoleId`
//!
//! `squad_ai::role::RoleId` looks like a job title and is not one. Its own header says so: *"A role is
//! nothing but a repertoire of dual-utility behaviours."* It keys [`RoleBrains`](crate::squad_ai::role::RoleBrains)
//! and the `roles.ron` override map, so its variant names are a serialized format and a search-space
//! alphabet, not prose.
//!
//! The taxonomy doc models the Foundation as **five orthogonal axes** — clearance, personnel class,
//! staff title, department, MTF assignment — and the game had a term for exactly one of them. That gap
//! is why three different files disagreed about who the five operatives were: `RoleId::ALL` said
//! `Researcher` where `config.ron`'s speaker note said "xenobiologist", and neither was wrong, because
//! they were answering different questions. This module supplies the missing axis so both can be right.
//!
//! **So: behaviour is [`RoleId`](crate::squad_ai::role::RoleId), job is [`StaffTitle`], and permission
//! to know is [`Clearance`].** A gunman by behaviour can be an MTF Operative by title at Level 2; a
//! researcher by behaviour can be a Xenobiologist at Level 3.
//!
//! Both are enums rather than strings for the reason `site::layout::AreaId` gives: a typo should be a
//! compile error, and every variant should be provably handled.

use serde::{Deserialize, Serialize};

/// What someone's job is.
///
/// The first eight are the **canonical list** — the only staff titles the SCP Wiki's clearance page
/// names, and the taxonomy doc's instruction is to use them verbatim (§5.1). The rest are canon
/// specialist tracks (§5.2, §5.5) that this Site actually staffs.
///
/// ⚠️ **The three-tier combat distinction is load-bearing.** [`SecurityOfficer`](Self::SecurityOfficer),
/// [`TacticalResponseOfficer`](Self::TacticalResponseOfficer) and [`MtfOperative`](Self::MtfOperative)
/// are three different jobs, and canon states the difference as an analogy: guards are military police,
/// response teams are combat infantry, MTFs are special operations. Collapsing them into one "soldier"
/// is listed in §14 as a recognisable amateur tell. Site-67 posts one of each so the difference is
/// visible in play, which is the only reason it is worth modelling at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum StaffTitle {
    // ── the canonical eight ───────────────────────────────────────────────────────────────────────
    /// The scientific branch. Specialties span chemistry and botany to theoretical physics.
    Researcher,
    /// Leads teams. Canon describes them as rare, which is why Site-67 has exactly one.
    SeniorResearcher,
    /// Engineers and technicians who design and maintain cells, and field teams who establish *initial*
    /// containment. **Not combat personnel** — canon jokes that they carry a pipe wrench.
    ContainmentSpecialist,
    /// Physical and information security. In a breach their job is to call for backup and evacuate,
    /// **not to fight**.
    SecurityOfficer,
    /// The SWAT tier: heavy weapons, real armour, escorts containment teams.
    TacticalResponseOfficer,
    /// The eyes and ears — embedded or investigating. Not equipped to fight an anomaly.
    FieldAgent,
    /// Special forces, drawn from across the Foundation. **The player's five are these.**
    MtfOperative,

    // ── specialist tracks this Site staffs (§5.2, §5.5) ───────────────────────────────────────────
    /// Named explicitly in the canonical clearance document. The party's anomaly-biology voice.
    Xenobiologist,
    /// Canon — exemplar Specialist Samara Maclear, a former field agent employed for her clairvoyance.
    /// **Not a thaumaturge**; §10 of the taxonomy doc keeps psionics, thaumaturgy and reality-bending
    /// separate and §14 lists merging them as a tell.
    PsionicsSpecialist,
    /// RAISA — records and information security. *"Those `[REDACTED]` blocks have an author."*
    /// At Site-67 this is the person standing where SCP-9191's unattributed reports appear.
    Archivist,
    /// Canon term: therapy for people who have seen things. The Site models FEAR on operatives who
    /// carry it between expeditions, so this is the job that addresses a mechanic rather than a room.
    Paratherapist,
    /// Medical staff — canonically *the people who declare you Class E*.
    MedicalOfficer,
    /// Support: logistics, clerical, the galley. §5.8 is explicit that skipping these is what makes a
    /// site read as a dungeon rather than a workplace.
    Logistics,

    // ── not staff ─────────────────────────────────────────────────────────────────────────────────
    /// Expendable personnel drawn from prison populations. A *class*, not really a title, but it is
    /// what a placard by their block would say. Always [`Clearance::Level0`].
    ClassD,
}

impl StaffTitle {
    /// Player-facing copy, for placards and the roster screen. Upper case because every other readable
    /// surface in this game is (`REQUISITION`, `NO SPECIMEN ON THE SLAB`, `MTF LOST`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Researcher => "RESEARCHER",
            Self::SeniorResearcher => "SENIOR RESEARCHER",
            Self::ContainmentSpecialist => "CONTAINMENT SPECIALIST",
            Self::SecurityOfficer => "SECURITY OFFICER",
            Self::TacticalResponseOfficer => "TACTICAL RESPONSE OFFICER",
            Self::FieldAgent => "FIELD AGENT",
            Self::MtfOperative => "MTF OPERATIVE",
            Self::Xenobiologist => "XENOBIOLOGIST",
            Self::PsionicsSpecialist => "PSIONICS SPECIALIST",
            Self::Archivist => "ARCHIVIST",
            Self::Paratherapist => "PARATHERAPIST",
            Self::MedicalOfficer => "MEDICAL OFFICER",
            Self::Logistics => "LOGISTICS",
            Self::ClassD => "CLASS-D",
        }
    }

    /// Every title, for exhaustiveness tests. Kept in declaration order.
    pub const ALL: [StaffTitle; 14] = [
        Self::Researcher,
        Self::SeniorResearcher,
        Self::ContainmentSpecialist,
        Self::SecurityOfficer,
        Self::TacticalResponseOfficer,
        Self::FieldAgent,
        Self::MtfOperative,
        Self::Xenobiologist,
        Self::PsionicsSpecialist,
        Self::Archivist,
        Self::Paratherapist,
        Self::MedicalOfficer,
        Self::Logistics,
        Self::ClassD,
    ];
}

/// What someone is allowed to know.
///
/// ⚠️ **Clearance is a ceiling on information, not a rank and not an XP ladder** — both confusions are
/// named amateur tells (§14). Someone still approves each individual read; the taxonomy doc calls the
/// Disclosure Officer *"the most under-exploited role in the entire mythos"* for exactly that reason.
///
/// Descriptions are the canonical ones, condensed. Site-67 has no Level 5 personnel and should never
/// grow one: **Thaumiel** in this game is the research tree (`src/research/`), and the collision of
/// names is deliberate — the doctrine of using the contained to contain is what Level 5 is named for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum Clearance {
    /// For Official Use Only — no need to access anomaly information at all.
    Level0,
    /// Confidential — works in proximity to contained anomalies, with no access to them.
    Level1,
    /// Restricted — security and research staff who need direct access to anomaly *information*.
    Level2,
    /// Secret — senior staff who need source, recovery circumstances and long-term planning.
    Level3,
    /// Top Secret — senior administration; site-wide and regional intelligence. **The Director.**
    Level4,
    /// Thaumiel — effectively unlimited. Not held by anyone at this Site.
    Level5,
}

impl Clearance {
    /// Player-facing copy for a placard: `LEVEL 2`.
    /// The level as a number, `0..=5`.
    ///
    /// **A ceiling on information, not a rank** — `Clearance`'s own doc calls the rank reading one of
    /// the amateur tells, and this does not change that. It exists because a door plaque has to show
    /// *how many* levels a door demands: `site::visuals` hangs one plaque per level, so the count on
    /// the wall is the clearance, read without hue (`docs/lore/2026-07-12-scp-color-language.md`).
    pub fn rank(self) -> u8 {
        match self {
            Self::Level0 => 0,
            Self::Level1 => 1,
            Self::Level2 => 2,
            Self::Level3 => 3,
            Self::Level4 => 4,
            Self::Level5 => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Level0 => "LEVEL 0",
            Self::Level1 => "LEVEL 1",
            Self::Level2 => "LEVEL 2",
            Self::Level3 => "LEVEL 3",
            Self::Level4 => "LEVEL 4",
            Self::Level5 => "LEVEL 5",
        }
    }

    /// The canonical name of the level — the word the Foundation uses when it is not using the number.
    pub fn code_word(self) -> &'static str {
        match self {
            Self::Level0 => "FOR OFFICIAL USE ONLY",
            Self::Level1 => "CONFIDENTIAL",
            Self::Level2 => "RESTRICTED",
            Self::Level3 => "SECRET",
            Self::Level4 => "TOP SECRET",
            Self::Level5 => "THAUMIEL",
        }
    }

    pub const ALL: [Clearance; 6] = [
        Self::Level0,
        Self::Level1,
        Self::Level2,
        Self::Level3,
        Self::Level4,
        Self::Level5,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_title_and_clearance_has_player_facing_copy() {
        // The labels are what a placard and the roster screen print. A blank one would render as an
        // empty sign, which reads as a missing asset rather than as missing text.
        for t in StaffTitle::ALL {
            assert!(!t.label().is_empty(), "{t:?} has no label");
            assert_eq!(t.label(), t.label().to_uppercase(), "{t:?}'s label is not upper case");
        }
        for c in Clearance::ALL {
            assert!(!c.label().is_empty(), "{c:?} has no label");
            assert!(!c.code_word().is_empty(), "{c:?} has no code word");
        }
    }

    #[test]
    fn the_three_combat_tiers_stay_three_distinct_titles() {
        // Pinned because collapsing them is the specific amateur tell the taxonomy doc names, and
        // because a future tidy-up ("these are all just guards") would look like a simplification.
        let tiers = [
            StaffTitle::SecurityOfficer,
            StaffTitle::TacticalResponseOfficer,
            StaffTitle::MtfOperative,
        ];
        for (i, a) in tiers.iter().enumerate() {
            for b in tiers.iter().skip(i + 1) {
                assert_ne!(a, b);
                assert_ne!(a.label(), b.label());
            }
        }
    }

    #[test]
    fn clearance_orders_from_least_to_most_permitted() {
        // `Ord` is derived from declaration order, and a placard-vs-holder comparison will lean on it.
        assert!(Clearance::Level0 < Clearance::Level2);
        assert!(Clearance::Level4 < Clearance::Level5);
        let mut sorted = Clearance::ALL;
        sorted.sort();
        assert_eq!(sorted, Clearance::ALL, "declaration order must already be ascending");
    }
}
