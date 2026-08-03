//! **Turning the two shipped schemas into descriptors**, without losing anything that matters or
//! carrying anything that does not.
//!
//! `docs/2026-08-03-asset-schema-audit.md` §4 puts `ManifestItem` and `KitPiece` side by side. They
//! describe the same kind of thing in two vocabularies that grew apart: one says `Role::Scatter {
//! surface }`, the other says `rests_on: Some("worktop")`, and those are the *same relation*. A third
//! schema is only worth having if both collapse into it exactly, so this module is written to be
//! checked rather than trusted — see [`manifest_from_descriptor`], the inverse.
//!
//! # The gate is a diff, not a judgement
//!
//! Every field either round-trips or is on the list below. `tests/descriptor_migration.rs` converts
//! the shipped manifest, converts it back, and asserts equality item by item; a field quietly dropped
//! shows up as a failing comparison rather than as an empty room three weeks later.
//!
//! # What is deliberately dropped
//!
//! From the audit's "dead surface — drop it in any migration, do not inherit it":
//!
//! * **`category`** has no reader and carries `#[allow(dead_code)]`. It is not *discarded* — its
//!   fourteen values are the seed of the `kind` axis in `assets/emerge/vocab.ron`, which is a reader.
//!   So it stops being an unvalidated string and becomes a validated token.
//! * **`Role::Custom`** has never been constructed in the life of the codebase. An open variant that
//!   nothing produces is surface every reader must consider and no writer uses.
//! * **`Host::Floor`** has zero references; **`Host::Opening`** is implemented in the pure layer and
//!   has no spawn branch.
//! * **`sleep`, `store`, `decor`, `hygiene`** — affordance tokens nothing reads outside `#[cfg(test)]`.
//! * **`sit`, `back_to_wall`** — read, but as *placement constraints*, not as effects. Under this
//!   schema `sit` is a socket with a role and `back_to_wall` is a mount plus clearance. Carrying them
//!   on the `effects` axis would be two spellings of one idea.
//!
//! Dropping is stated per field rather than done silently, because [`manifest_from_descriptor`] has to
//! know what it cannot reconstruct, and a reader has to be able to tell "deliberately gone" from
//! "forgotten".
//!
//! # What is NOT dropped, and where it went
//!
//! `tags` and `group` look like the dead ones and are not: `furnish::room_profile` matches `tags`
//! against room types to pick a room's freestanding set, and items sharing a `group` are drawn
//! together by a soft `Near` relation. Both are facts about the asset — *a toilet suits a bathroom* —
//! so they live on [`crate::descriptor::Placement`] rather than on a semantic axis. They are
//! generation hints, and keeping them apart from `kind`/`effects`/`look` is what stops the axes from
//! becoming the same free-text soup the audit found.

use crate::descriptor::{Align, Descriptor, Extent, Mount, Offers, Placement};
use crate::placement::ir::{Host, Role};
use crate::placement::manifest::ManifestItem;

/// Project policy the conversion needs and the asset cannot supply.
///
/// Same argument as `stretch_y`: how high this game hangs a wall light is not a fact about the mesh,
/// and baking it into a shared library is how one game's wall height ends up in another's. The game
/// passes its own (`placement::furnish::WALL_LIGHT_HEIGHT`, 1.8 m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Policy {
    /// Height at which a wall-anchored piece hangs, metres.
    pub wall_mount_height: f32,
}

/// Affordance tokens that are placement constraints rather than effects, and are therefore expressed
/// by the descriptor's shape instead of by a tag. Listed so the inverse can put them back.
const CONSTRAINT_AFFORDANCES: &[&str] = &["sit", "back_to_wall"];

/// Affordance tokens with no reader anywhere in `src/` outside `#[cfg(test)]`.
///
/// `support` is here for a different reason and it is worth stating: it *was* an affordance, and the
/// split that gave surfaces their own axis left `furniture_kenney.ron:15,18` still authoring
/// `affordances: ["support"]`. The audit calls that "dead config post-split" — the same word in the
/// `surfaces` field is the live one. Migrating it onto `effects` would resurrect a spelling that was
/// deliberately retired.
const UNREAD_AFFORDANCES: &[&str] = &["sleep", "store", "decor", "hygiene", "support"];

/// Affordance tokens that survive as `effects`, because something acts on them.
fn is_effect(token: &str) -> bool {
    !CONSTRAINT_AFFORDANCES.contains(&token) && !UNREAD_AFFORDANCES.contains(&token)
}

/// One manifest row → one descriptor.
pub fn descriptor_from_manifest(item: &ManifestItem, policy: Policy) -> Result<Descriptor, String> {
    let mount = match &item.role {
        Role::Freestanding => Some(Mount::OnFloor),
        Role::Tiled => Some(Mount::Tiled),
        // The same relation the Site kit spells `rests_on`. Collapsing the two spellings is most of
        // the reason a third schema is worth having.
        Role::Scatter { surface } => Some(Mount::OnSurface {
            class: surface.clone(),
        }),
        Role::Anchor { host: Host::Wall } => Some(Mount::OnWall {
            height: policy.wall_mount_height,
        }),
        Role::Anchor {
            host: Host::Ceiling,
        } => Some(Mount::OnCeiling),
        // **`Host::Opening` is not dead after all.** The audit lists it as having no spawn branch in
        // `furnish_region`, which is true — but `furniture_kenney.ron` ships a `door` row using it, so
        // there IS data to migrate even though nothing places it. Refusing here would have made kit B
        // unmigratable to protect an assumption about kit B.
        //
        // The opening's clear size is `None` because the manifest schema has no field for one. That is
        // a fact about the row, not a gap in the conversion.
        Role::Anchor {
            host: Host::Opening,
        } => Some(Mount::InOpening { clear: None }),
        // `Host::Floor` genuinely has zero references anywhere, so there is no shipped row and no
        // behaviour to reproduce — a mapping would be a guess dressed as a migration.
        Role::Anchor { host } => {
            return Err(format!(
                "`{}`: anchor host {host:?} has zero references anywhere in the tree — no shipped \
                 row, no spawn branch. If one is being added, decide what it means and give it a \
                 mount here; do not let the converter invent one.",
                item.key
            ))
        }
        Role::Custom(token) => {
            return Err(format!(
                "`{}`: `Role::Custom({token:?})` has never been constructed in this codebase, so \
                 there is no behaviour to preserve. Add a real mount variant instead of reviving the \
                 escape hatch.",
                item.key
            ))
        }
    };

    Ok(Descriptor {
        id: item.key.clone(),
        mesh: Some(item.glb.clone()),
        align: Align {
            pivot: Some(item.pivot),
            y_offset: Some(item.y_offset),
            ..Align::default()
        },
        extent: Extent {
            footprint: Some(item.footprint),
            height: Some(item.height),
        },
        mount,
        clearance: Vec::new(),
        offers: Offers {
            surfaces: item.surfaces.clone(),
            sockets: Vec::new(),
        },
        placement: Placement {
            rooms: item.tags.clone(),
            group: item.group.clone(),
        },
        // The audit's fourteen `category` values become the seeded `kind` axis. One token, because
        // that is what `category` was: a single grouping word.
        kind: if item.category.is_empty() {
            Vec::new()
        } else {
            vec![item.category.clone()]
        },
        effects: item
            .affordances
            .iter()
            .filter(|a| is_effect(a))
            .cloned()
            .collect(),
        // No shipped row records appearance. An empty list is the honest statement of that.
        look: Vec::new(),
        note: None,
    })
}

/// A descriptor → the manifest row it came from. **The gate, not a feature.**
///
/// Nothing in the game calls this; `tests/descriptor_migration.rs` does, to turn "did I port 41 items
/// correctly" from a reading exercise into a diff. It reconstructs only what the descriptor models —
/// the constraint affordances and the unread ones are gone by design, so the test compares against a
/// manifest with those stripped rather than against the raw file, and the stripping is spelled out in
/// [`without_dropped_affordances`] where a reader can check it.
pub fn manifest_from_descriptor(d: &Descriptor) -> Result<ManifestItem, String> {
    let role = match &d.mount {
        Some(Mount::OnFloor) => Role::Freestanding,
        Some(Mount::Tiled) => Role::Tiled,
        Some(Mount::OnSurface { class }) => Role::Scatter {
            surface: class.clone(),
        },
        Some(Mount::OnWall { .. }) => Role::Anchor { host: Host::Wall },
        Some(Mount::OnCeiling) => Role::Anchor {
            host: Host::Ceiling,
        },
        Some(Mount::InOpening { .. }) => Role::Anchor {
            host: Host::Opening,
        },
        Some(other) => {
            return Err(format!(
                "`{}`: mount {other:?} has no manifest equivalent — it is one of the layering \
                 relations the old schema could not express, which is why the new one exists.",
                d.id
            ))
        }
        None => {
            return Err(format!(
                "`{}`: no mount. A manifest row must say how it is placed.",
                d.id
            ))
        }
    };

    Ok(ManifestItem {
        key: d.id.clone(),
        glb: d
            .mesh
            .clone()
            .ok_or_else(|| format!("`{}`: no mesh", d.id))?,
        category: d.kind.first().cloned().unwrap_or_default(),
        tags: d.placement.rooms.clone(),
        role,
        footprint: d
            .extent
            .footprint
            .ok_or_else(|| format!("`{}`: no footprint", d.id))?,
        affordances: d.effects.clone(),
        surfaces: d.offers.surfaces.clone(),
        group: d.placement.group.clone(),
        height: d.extent.height.unwrap_or_default(),
        // Absence in a patch means "inherit the default", and the manifest's defaults for these two
        // are `#[serde(default)]` zeros — so `None` reconstructs the row a manifest without the field
        // would have produced. That is the round trip, not a fallback.
        pivot: d.align.pivot.unwrap_or((0.0, 0.0)),
        y_offset: d.align.y_offset.unwrap_or(0.0),
    })
}

/// A manifest row with the affordance tokens this migration deliberately drops removed, so a
/// round-trip comparison measures the conversion rather than re-litigating the drop list.
pub fn without_dropped_affordances(item: &ManifestItem) -> ManifestItem {
    let mut out = item.clone();
    out.affordances.retain(|a| is_effect(a));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: Policy = Policy {
        wall_mount_height: 1.8,
    };

    fn desk() -> ManifestItem {
        ManifestItem {
            key: "kenney/desk".into(),
            glb: "kenney/desk.glb".into(),
            category: "table".into(),
            tags: vec!["office".into(), "living".into()],
            role: Role::Freestanding,
            footprint: (1.2, 0.6),
            affordances: vec!["store".into(), "back_to_wall".into()],
            surfaces: vec!["support".into(), "worktop".into()],
            group: Some("office".into()),
            height: 0.75,
            pivot: (0.0, -0.14),
            y_offset: 0.0,
        }
    }

    #[test]
    fn a_freestanding_item_round_trips() {
        let item = desk();
        let d = descriptor_from_manifest(&item, POLICY).unwrap_or_else(|e| panic!("{e}"));
        let back = manifest_from_descriptor(&d).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back, without_dropped_affordances(&item));
    }

    /// The two spellings of one relation. `Role::Scatter { surface }` and the Site kit's
    /// `rests_on: Some(class)` mean the same thing, and collapsing them is most of the point.
    #[test]
    fn scatter_becomes_a_surface_mount() {
        let mut mug = desk();
        mug.key = "kenney/mug".into();
        mug.role = Role::Scatter {
            surface: "worktop".into(),
        };
        let d = descriptor_from_manifest(&mug, POLICY).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            d.mount,
            Some(Mount::OnSurface {
                class: "worktop".into()
            })
        );
        assert_eq!(
            manifest_from_descriptor(&d).unwrap_or_else(|e| panic!("{e}")).role,
            mug.role
        );
    }

    /// Wall height is policy, so it comes from the caller and not from the mesh — and a different
    /// project gets a different number without touching this crate.
    #[test]
    fn a_wall_anchor_takes_its_height_from_project_policy() {
        let mut sconce = desk();
        sconce.role = Role::Anchor { host: Host::Wall };
        let d = descriptor_from_manifest(&sconce, POLICY).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.mount, Some(Mount::OnWall { height: 1.8 }));

        let taller = descriptor_from_manifest(&sconce, Policy { wall_mount_height: 2.2 })
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(taller.mount, Some(Mount::OnWall { height: 2.2 }));
    }

    /// `sit` and `back_to_wall` are placement constraints wearing an affordance's clothes; `store`
    /// and friends have no reader at all. Neither becomes an effect.
    #[test]
    fn only_affordances_something_reads_become_effects() {
        let mut lamp = desk();
        lamp.affordances = vec![
            "emit".into(),
            "sit".into(),
            "store".into(),
            "decor".into(),
            "screen".into(),
        ];
        let d = descriptor_from_manifest(&lamp, POLICY).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.effects, vec!["emit".to_string(), "screen".to_string()]);
    }

    /// Room tags and the grouping token are NOT dead surface — they have readers — so they survive,
    /// on the generation-hint block rather than on a semantic axis.
    #[test]
    fn room_tags_and_grouping_survive_on_the_placement_block() {
        let d = descriptor_from_manifest(&desk(), POLICY).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.placement.rooms, vec!["office".to_string(), "living".to_string()]);
        assert_eq!(d.placement.group.as_deref(), Some("office"));
        // And they stay off the axes, which is what keeps the axes meaningful.
        assert!(!d.kind.contains(&"office".to_string()));
        assert!(!d.effects.contains(&"office".to_string()));
    }

    /// `category` had no reader and becomes a validated `kind` token rather than vanishing.
    #[test]
    fn category_becomes_the_kind_axis() {
        let d = descriptor_from_manifest(&desk(), POLICY).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.kind, vec!["table".to_string()]);
    }

    /// The escape hatch nothing ever used does not get carried across, and the error says why rather
    /// than just refusing.
    #[test]
    fn a_custom_role_is_refused_with_its_reason() {
        let mut odd = desk();
        odd.role = Role::Custom("hovering".into());
        let err = descriptor_from_manifest(&odd, POLICY).err().unwrap_or_default();
        assert!(err.contains("never been constructed"), "{err}");
        assert!(err.contains("kenney/desk"), "must name the item: {err}");
    }

    /// `Host::Floor` is the one anchor host with genuinely zero references anywhere — no shipped row
    /// in either manifest, no spawn branch. There is nothing to preserve, so a mapping would be an
    /// invention, and the error says which.
    #[test]
    fn an_anchor_host_with_no_shipped_row_is_refused_rather_than_guessed() {
        let mut odd = desk();
        odd.role = Role::Anchor { host: Host::Floor };
        let err = descriptor_from_manifest(&odd, POLICY).err().unwrap_or_default();
        assert!(err.contains("zero references"), "{err}");
        assert!(err.contains("kenney/desk"), "must name the item: {err}");
    }

    /// **`Host::Opening` is not in that category, and finding out cost a failing gate.** The audit
    /// lists it as having no spawn branch, which is true — but `furniture_kenney.ron` ships a `door`
    /// row using it, so there is data to migrate. The opening size is `None` because the manifest
    /// schema has no field for one.
    #[test]
    fn an_opening_anchor_migrates_even_though_nothing_spawns_it() {
        let mut door = desk();
        door.key = "kenney/door".into();
        door.role = Role::Anchor {
            host: Host::Opening,
        };
        let d = descriptor_from_manifest(&door, POLICY).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.mount, Some(Mount::InOpening { clear: None }));
        assert_eq!(
            manifest_from_descriptor(&d).unwrap_or_else(|e| panic!("{e}")).role,
            door.role
        );
    }
}
