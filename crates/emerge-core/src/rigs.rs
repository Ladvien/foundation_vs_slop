//! **What each rig's clips are and how they play** — the manifest that replaces six hand-written
//! tables of magic indices.
//!
//! Today every creature's clip table lives in Rust: `GAIT_*` triples and `CLIP_*` indices in
//! `src/squad.rs`, `from_clips([7, 6, 10, 3, 1, 2])` in `src/parasite.rs`, a `ClipSpec` table per
//! SCP-1048 variant, and so on. Those numbers were measured by hand off the GLB — and nothing
//! re-checks them when an artist re-exports. `docs/animation.md` calls the measuring step *"a manual
//! offline step, not a repo tool"*; the cost of it being in code is that re-measuring is a code edit,
//! so in practice it does not happen.
//!
//! This is the data half. [`crate::clips`] can now measure a GLB, so the manifest can be **checked
//! against the asset it describes** — which is the whole point, and is what `rigs.rs`'s own test does.
//!
//! # Order is the contract
//!
//! [`Rig::slots`] is a **list, and its order is load-bearing**: the index of a slot is the handle the
//! game's drivers use (`anim::blend`'s `SLOT_IDLE`, `SLOT_WALK`, …). A slot's `note` is documentation
//! and nothing resolves by it — the Valkyrie's `strafe_l`/`strafe_r` are *backwards in the asset*, and
//! the code has always wired them by measured direction, which only works because names decide
//! nothing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bumped when the shape below changes in a way an older file cannot satisfy.
/// The highest mask group a slot may name — `AnimationMask` in Bevy 0.19 is a `u64`, so the bits are
/// 0..63 and `1 << 64` is undefined behaviour the compiler turns into a panic.
pub const MAX_MASK_GROUP: u32 = 64;

pub const RIGS_VERSION: u32 = 1;

/// How a clip advances. Mirrors `anim::Playback` — the game converts one to the other and nothing
/// else here knows what an `AnimationPlayer` is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Playback {
    /// Loops at its own speed. Idles, and anything with no relationship to the ground.
    Free { speed: f32 },
    /// A gait: seek time is derived from the shared phase, so this needs the three measured numbers.
    /// `cycle_distance` is what ties cadence to ground speed — an inaccurate one is foot-skate.
    Gait {
        duration: f32,
        phase_offset: f32,
        cycle_distance: f32,
    },
    /// Plays once when triggered. Fires, deaths.
    OneShot { speed: f32 },
}

/// One entry in a rig's slot table.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SlotDef {
    /// The glTF animation index. **The only identifier the runtime uses.**
    pub clip: usize,
    pub playback: Playback,
    /// The mask group this clip is confined to, if any — the Valkyrie's aim and fire ride the upper
    /// body over whatever the legs are doing. `None` means the whole skeleton.
    #[serde(default)]
    pub mask: Option<u32>,
    /// What this slot is, for a reader. Never a lookup key; see the module note.
    #[serde(default)]
    pub note: Option<String>,
    /// **The one slot label that IS a lookup key**, and the exception is deliberate.
    ///
    /// Eleven of the twelve rigs address slots positionally — slot N is what `SLOT_N` names — and for
    /// them this stays `None`. SCP-1048 cannot: its four variants ship *different* clip sets (only the
    /// original can draw; SCP-1048-A cannot dance), so a driver asks for a **state** and the variant
    /// answers with a slot or with nothing. Position cannot express "this variant does not have one".
    ///
    /// A string here, resolved to the game's own enum at load and refused loudly if unknown — see
    /// `scp1048::anim`. That refusal is the whole reason it is a key rather than a note.
    #[serde(default)]
    pub state: Option<String>,
}

/// One animated character.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Rig {
    /// Asset-relative path to the GLB, so the manifest can be checked against the file it describes.
    pub mesh: String,
    /// **Ordered.** See the module note.
    pub slots: Vec<SlotDef>,
}

/// The whole manifest.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Rigs {
    pub version: u32,
    #[serde(default)]
    pub note: Option<String>,
    /// Keyed by rig name. A `BTreeMap`, so writing the file back out is byte-stable — a manifest that
    /// reordered itself on every save would make every diff unreadable.
    pub rigs: BTreeMap<String, Rig>,
}

impl Rigs {
    pub fn parse(text: &str) -> Result<Rigs, String> {
        let parsed: Rigs = ron::from_str(text).map_err(|e| format!("rigs.ron is not valid: {e}"))?;
        parsed.validate()?;
        Ok(parsed)
    }

    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("cannot write rigs.ron: {e}"))
    }

    pub fn get(&self, name: &str) -> Option<&Rig> {
        self.rigs.get(name)
    }

    /// **Refuse a manifest that cannot be true**, rather than letting it become a rig that animates
    /// wrongly. Every rule here is one whose violation is silent at runtime: a zero `cycle_distance`
    /// divides into the cadence, a phase offset outside one cycle wraps to a different clip alignment,
    /// and an empty slot table is a creature that stands still for reasons nobody can find.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != RIGS_VERSION {
            return Err(format!(
                "rigs.ron is version {}, this build reads {RIGS_VERSION}",
                self.version
            ));
        }
        for (name, rig) in &self.rigs {
            // **A mask names a bit, and a bit has to exist.** `rigs::build` evaluates `1 << group`
            // against Bevy's 64-bit `AnimationMask`; nothing checked the group, so `mask: Some(64)`
            // in the manifest panicked the whole game at Startup with "attempt to shift left with
            // overflow" in debug, or wrapped to bit 0 in release and masked the wrong bone group —
            // which is worse, because it looks like the animation is merely wrong.
            for slot in &rig.slots {
                if let Some(group) = slot.mask {
                    if group >= MAX_MASK_GROUP {
                        return Err(format!(
                            "`{name}`'s slot masks to group {group}; a mask group is a bit index into \
                             a 64-bit animation mask, so it must be below {MAX_MASK_GROUP}"
                        ));
                    }
                }
            }
            if rig.mesh.is_empty() {
                return Err(format!("rig `{name}` names no mesh"));
            }
            if rig.slots.is_empty() {
                return Err(format!("rig `{name}` has no slots, so nothing could animate it"));
            }
            for (i, s) in rig.slots.iter().enumerate() {
                match s.playback {
                    Playback::Gait {
                        duration,
                        phase_offset,
                        cycle_distance,
                    } => {
                        if !(duration > 0.0) {
                            return Err(format!(
                                "rig `{name}` slot {i} is a gait with duration {duration}; a gait \
                                 with no length has no phase to seek"
                            ));
                        }
                        if !(cycle_distance > 0.0) {
                            return Err(format!(
                                "rig `{name}` slot {i} is a gait covering {cycle_distance} m per \
                                 cycle; cadence is speed / cycle_distance, so this would divide by \
                                 nothing. Measure it with `emerge_core::clips::cycle_distance`."
                            ));
                        }
                        // **Signed, and that is not sloppiness.** An offset is a shift along a cycle
                        // that wraps, so -0.141 and 0.859 are the same alignment; the game's tables
                        // are written with the small signed value because "a seventh of a cycle
                        // early" is what an author means. Anything outside one whole cycle either
                        // way is not a fraction and is a typo.
                        if !(-1.0..=1.0).contains(&phase_offset) {
                            return Err(format!(
                                "rig `{name}` slot {i} has phase offset {phase_offset}; it is a \
                                 fraction of one cycle and must be within -1.0..=1.0"
                            ));
                        }
                    }
                    Playback::Free { speed } | Playback::OneShot { speed } => {
                        if !(speed > 0.0) {
                            return Err(format!(
                                "rig `{name}` slot {i} plays at speed {speed}"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gait() -> SlotDef {
        SlotDef {
            clip: 5,
            playback: Playback::Gait {
                duration: 1.417,
                phase_offset: 0.0,
                cycle_distance: 1.388,
            },
            mask: None,
            note: Some("walk".into()),
            state: None,
        }
    }

    fn one(slot: SlotDef) -> Rigs {
        let mut rigs = BTreeMap::new();
        rigs.insert(
            "valkyrie".to_owned(),
            Rig {
                mesh: "characters/valkyrie.glb".to_owned(),
                slots: vec![slot],
            },
        );
        Rigs {
            version: RIGS_VERSION,
            note: None,
            rigs,
        }
    }

    #[test]
    fn a_manifest_survives_a_round_trip() {
        let before = one(gait());
        let text = before.to_ron().unwrap_or_else(|e| panic!("{e}"));
        let after = Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, after);
    }

    #[test]
    fn a_gait_that_covers_no_ground_is_refused() {
        let mut slot = gait();
        slot.playback = Playback::Gait {
            duration: 1.0,
            phase_offset: 0.0,
            // Cadence is speed / cycle_distance. This is the division by nothing.
            cycle_distance: 0.0,
        };
        let e = one(slot).validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("cycle"), "{e}");
    }

    #[test]
    fn a_phase_offset_outside_one_cycle_is_refused() {
        let mut slot = gait();
        slot.playback = Playback::Gait {
            duration: 1.0,
            phase_offset: 1.5,
            cycle_distance: 1.0,
        };
        let e = one(slot).validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("phase offset"), "{e}");
    }

    #[test]
    fn a_rig_with_no_slots_is_refused() {
        let mut m = one(gait());
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            r.slots.clear();
        }
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("no slots"), "{e}");
    }

    #[test]
    fn an_older_version_is_refused_rather_than_guessed_at() {
        let mut m = one(gait());
        m.version = RIGS_VERSION + 1;
        assert!(m.validate().is_err());
    }
}
