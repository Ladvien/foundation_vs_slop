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

/// The highest mask group a slot may name — `AnimationMask` in Bevy 0.19 is a `u64`, so the bits are
/// 0..63 and `1 << 64` is undefined behaviour the compiler turns into a panic.
pub const MAX_MASK_GROUP: u32 = 64;

/// Bumped when the shape below changes in a way an older file cannot satisfy.
///
/// v2 added the required per-rig `scale`. Required and versioned rather than serde-defaulted on
/// purpose: a default of 1.0 would silently mis-scale the 0.07 manca by 14×, and a wrong-but-running
/// game is the failure mode this manifest exists to prevent. An old file fails loudly at load.
pub const RIGS_VERSION: u32 = 2;

/// The node the FK checks anchor on when a rig does not name its own — see [`Rig::root_node`].
pub const DEFAULT_ROOT_NODE: &str = "Root";

/// The contact joint the measurements anchor on when a rig does not name its own.
pub const DEFAULT_CONTACT_JOINT: &str = "foot_l";

/// Bumped when the bench's measurement method changes meaning — a provenance stamped by an older
/// tool is a measurement made by different rules.
pub const BENCH_TOOL_VERSION: u32 = 1;

/// **Who measured a rig's numbers, off which bytes** — the stamp the bench writes beside the
/// values it adopts. Tool-owned; never hand-edit.
///
/// The stamp is what turns the bench's check from a value comparison into a provenance one:
/// *"this GLB changed since these numbers were measured"* is strictly stronger than *"1.417
/// declared vs 1.402 measured"* — it catches a re-export that changed something the value checks
/// never look at, and it cannot false-alarm on float drift.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Provenance {
    /// FNV-1a of the GLB's file bytes, spelled `"0x"` + 16 lowercase hex digits. A string so the
    /// on-disk spelling is stable and readable — RON re-spells a bare u64 in decimal on a write.
    pub glb_fnv1a: String,
    /// How many animations the asset carried when measured.
    pub clips: usize,
    /// The clip names in index order (`""` for an unnamed clip) — what lets a re-export diff say
    /// "strafe_l added at index 6; indices after shifted" instead of "index out of range".
    pub clip_names: Vec<String>,
    /// [`BENCH_TOOL_VERSION`] at measure time.
    pub tool: u32,
    /// `YYYY-MM-DD`, UTC, at measure time.
    pub date: String,
}

/// The one spelling of a fingerprint on disk.
pub fn fingerprint_string(hash: u64) -> String {
    format!("0x{hash:016x}")
}

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
    /// **"Leave my numbers alone", with the reason** — the declared-override marker the bench's
    /// adopt action honors by skipping this slot. A reason string rather than a flag on the same
    /// rule as policy patches' `because`: a kept number is somebody's decision, and a decision
    /// with no recorded why becomes folklore. Only meaningful on a gait (adopt writes nothing
    /// else), and `validate` refuses it elsewhere.
    #[serde(default)]
    pub keep: Option<String>,
    /// An explicit cycle-distance tolerance for this slot, as a fraction of the declared value —
    /// overriding `rig_check`'s policy (loose by default, tight once measured-and-adopted). Set it
    /// when a slot has a documented reason to disagree with its asset by a known margin. Only
    /// meaningful on a gait; `validate` refuses it elsewhere.
    #[serde(default)]
    pub tolerance: Option<f32>,
}

/// One animated character.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Rig {
    /// Asset-relative path to the GLB, so the manifest can be checked against the file it describes.
    pub mesh: String,
    /// **The render scale the game applies at spawn**, and therefore the file-units → world-units
    /// factor every measured `cycle_distance` is multiplied by. One number, owned here: it used to
    /// live as five separate `1.13`/`0.15`/`0.07` literals across the spawners and the checks, and
    /// five copies of a measurement is four copies that go stale.
    pub scale: f32,
    /// The node whose translation must be bit-zero in a gait clip — the in-place anchor. `None`
    /// means the conventional name, [`DEFAULT_ROOT_NODE`]; set it only for a rig whose exporter
    /// names its root something else.
    #[serde(default)]
    pub root_node: Option<String>,
    /// The joint names whose FK tracks define ground contact, best foot first — the first entry
    /// anchors cycle-distance and phase measurement. Empty means the conventional
    /// [`DEFAULT_CONTACT_JOINT`]. Only meaningful on a rig with gaits, and `validate` refuses it
    /// elsewhere — configuration nothing reads is configuration that lies.
    #[serde(default)]
    pub contact_joints: Vec<String>,
    /// The measurement stamp, written by the bench's adopt action. `None` = never measured.
    #[serde(default)]
    pub provenance: Option<Provenance>,
    /// **Ordered.** See the module note.
    pub slots: Vec<SlotDef>,
}

impl Rig {
    /// The in-place anchor's node name.
    pub fn root_node(&self) -> &str {
        self.root_node.as_deref().unwrap_or(DEFAULT_ROOT_NODE)
    }

    /// The measurement contact joint's node name.
    pub fn contact_joint(&self) -> &str {
        self.contact_joints
            .first()
            .map(String::as_str)
            .unwrap_or(DEFAULT_CONTACT_JOINT)
    }

    /// Whether any slot is a gait.
    pub fn has_gaits(&self) -> bool {
        self.slots
            .iter()
            .any(|s| matches!(s.playback, Playback::Gait { .. }))
    }
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
            if !(rig.scale > 0.0) {
                return Err(format!(
                    "rig `{name}` has scale {}; the manifest's cycle distances are world units = \
                     file units × scale, so a non-positive scale makes every measurement \
                     meaningless",
                    rig.scale
                ));
            }
            if rig.root_node.as_deref() == Some("") {
                return Err(format!("rig `{name}`'s root_node names no node"));
            }
            if rig.contact_joints.iter().any(String::is_empty) {
                return Err(format!("rig `{name}`'s contact_joints include an empty name"));
            }
            for (i, j) in rig.contact_joints.iter().enumerate() {
                if rig.contact_joints[..i].contains(j) {
                    return Err(format!("rig `{name}` lists contact joint `{j}` twice"));
                }
            }
            if !rig.has_gaits() && (rig.root_node.is_some() || !rig.contact_joints.is_empty()) {
                return Err(format!(
                    "rig `{name}` configures measurement anchors but declares no gait — nothing \
                     reads them, and configuration nothing reads is configuration that lies"
                ));
            }
            if let Some(p) = &rig.provenance {
                let hex_ok = p.glb_fnv1a.len() == 18
                    && p.glb_fnv1a.starts_with("0x")
                    && p.glb_fnv1a.as_bytes()[2..]
                        .iter()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
                if !hex_ok {
                    return Err(format!(
                        "rig `{name}`'s provenance fingerprint `{}` is not 0x + 16 lowercase hex \
                         digits — the stamp is tool-owned; do not hand-edit it",
                        p.glb_fnv1a
                    ));
                }
                if p.clip_names.len() != p.clips {
                    return Err(format!(
                        "rig `{name}`'s provenance records {} clips but {} names — the stamp is \
                         tool-owned; do not hand-edit it",
                        p.clips,
                        p.clip_names.len()
                    ));
                }
            }
            if rig.slots.is_empty() {
                return Err(format!("rig `{name}` has no slots, so nothing could animate it"));
            }
            for (i, s) in rig.slots.iter().enumerate() {
                match &s.keep {
                    Some(reason) if reason.is_empty() => {
                        return Err(format!(
                            "rig `{name}` slot {i} is kept with no reason — a kept number is a \
                             decision, and a decision needs its why recorded"
                        ));
                    }
                    Some(_) if !matches!(s.playback, Playback::Gait { .. }) => {
                        return Err(format!(
                            "rig `{name}` slot {i} is kept but is not a gait — adopt writes only \
                             gait numbers, so this keep is configuration nothing reads"
                        ));
                    }
                    _ => {}
                }
                if let Some(t) = s.tolerance {
                    if !matches!(s.playback, Playback::Gait { .. }) {
                        return Err(format!(
                            "rig `{name}` slot {i} carries a tolerance but is not a gait — only \
                             the cycle-distance check reads it, so this is configuration nothing \
                             reads"
                        ));
                    }
                    if !(t > 0.0 && t <= 1.0) {
                        return Err(format!(
                            "rig `{name}` slot {i} has tolerance {t}; it is a fraction of the \
                             declared cycle distance and must be within (0, 1]"
                        ));
                    }
                }
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
            keep: None,
            tolerance: None,
        }
    }

    fn one(slot: SlotDef) -> Rigs {
        let mut rigs = BTreeMap::new();
        rigs.insert(
            "valkyrie".to_owned(),
            Rig {
                mesh: "characters/valkyrie.glb".to_owned(),
                scale: 1.13,
                root_node: None,
                contact_joints: Vec::new(),
                provenance: None,
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

    #[test]
    fn a_rig_with_no_positive_scale_is_refused() {
        let mut m = one(gait());
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            r.scale = 0.0;
        }
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("scale"), "{e}");
    }

    #[test]
    fn anchors_on_a_gaitless_rig_are_refused_as_config_nothing_reads() {
        let mut m = one(SlotDef {
            clip: 0,
            playback: Playback::Free { speed: 1.0 },
            mask: None,
            note: None,
            state: None,
            keep: None,
            tolerance: None,
        });
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            r.contact_joints = vec!["foot_l".to_owned()];
        }
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("no gait"), "{e}");
    }

    #[test]
    fn a_tolerance_outside_the_unit_interval_or_off_a_gait_is_refused() {
        let mut m = one(gait());
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            if let Some(s) = r.slots.get_mut(0) {
                s.tolerance = Some(1.5);
            }
        }
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("tolerance"), "{e}");

        let mut m = one(SlotDef {
            clip: 0,
            playback: Playback::Free { speed: 1.0 },
            mask: None,
            note: None,
            state: None,
            keep: None,
            tolerance: Some(0.1),
        });
        // A free slot has no cycle distance for a tolerance to govern.
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("nothing"), "{e}");
        let _ = &mut m;
    }

    fn stamp() -> Provenance {
        Provenance {
            glb_fnv1a: fingerprint_string(0xdead_beef_0000_0001),
            clips: 2,
            clip_names: vec!["idle".to_owned(), "walk".to_owned()],
            tool: BENCH_TOOL_VERSION,
            date: "2026-08-06".to_owned(),
        }
    }

    #[test]
    fn a_provenance_stamp_survives_a_round_trip() {
        let mut m = one(gait());
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            r.provenance = Some(stamp());
        }
        let text = m.to_ron().unwrap_or_else(|e| panic!("{e}"));
        let back = Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m, back);
    }

    #[test]
    fn a_hand_mangled_provenance_is_refused() {
        let mut m = one(gait());
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            let mut p = stamp();
            p.glb_fnv1a = "0xNOTHEX".to_owned();
            r.provenance = Some(p);
        }
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("tool-owned"), "{e}");

        let mut m = one(gait());
        if let Some(r) = m.rigs.get_mut("valkyrie") {
            let mut p = stamp();
            p.clip_names.pop();
            r.provenance = Some(p);
        }
        let e = m.validate().err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("2 clips but 1 names"), "{e}");
    }
}
