//! The asset manifest — extensibility axis #2 (vetting §3.2). A RON file maps opaque asset keys to
//! GLB paths plus the metadata the grammar needs: a placement `Role` (the dispatch key), a footprint,
//! **affordances** ("sit", "sleep", "emit"…) so rules target what an object *affords* rather than its
//! kit-specific name (Fisher 2012; Qi 2018), and **surfaces** — the separate feature axis of tops a
//! piece *offers* to scatter props ("support", "worktop"; Tutenel et al. 2010). Porting to a new kit is
//! a matter of authoring one manifest — no code changes — which is what the Stage-5 asset-swap test
//! exercises.
//!
//! The manifest reuses the engine-free IR `Role`/`Host` directly, so an entry declares e.g.
//! `role: Anchor(host: Ceiling)` or `role: Freestanding` in RON with no translation layer.

use serde::Deserialize;

use super::ir::Role;

/// One catalogued asset. `glb` is a path under `assets/`; `footprint` is (width, depth) in metres
/// (= tiles, since `TILE_SIZE` is 1 m). Fields default so a terse manifest stays valid.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManifestItem {
    pub key: String,
    pub glb: String,
    // `category` is an opaque grouping token parsed now so the schema is complete; not yet consumed.
    #[allow(dead_code)]
    pub category: String,
    // `tags` are opaque room-type tokens the furnish pass matches to pick a room's freestanding set
    // (see `furnish::room_profile`) — kit-agnostic, never interpreted.
    #[serde(default)]
    pub tags: Vec<String>,
    pub role: Role,
    pub footprint: (f32, f32),
    #[serde(default)]
    pub affordances: Vec<String>,
    // Surface classes this piece OFFERS to scatter props ("support", "worktop") — the *feature* axis
    // (Tutenel et al. 2010), kept deliberately SEPARATE from `affordances` (the *service* axis: what the
    // piece is *for*). A desk offers `["support", "worktop"]`; a bed offers nothing, so nothing rests on
    // it. Folding a surface token into `affordances` is exactly the "prop rests on a bed" bug — a bed
    // affords sleep but is not a shelf. Opaque, matched via `furnish::surface_bits`, never interpreted.
    #[serde(default)]
    pub surfaces: Vec<String>,
    // Optional grouping token: items sharing a `group` are drawn together by a soft `Near` relation
    // (e.g. a bathroom's toilet + sink). Opaque like `tags`/`affordances` — matched, never interpreted.
    #[serde(default)]
    pub group: Option<String>,
    // Overall height in metres (top of the piece's bounding box). For a piece that offers a surface
    // (`surfaces` non-empty) this is the top `Scatter` props rest on — vertical placement falls out of
    // it (Tutenel et al. 2010). Defaults to 0 for pieces whose height no rule needs (floor props, anchors).
    #[serde(default)]
    pub height: f32,
    // Local XZ offset (metres, from the glb bbox) of the mesh's bounding-box centre relative to its
    // glTF origin. Kit meshes are often authored off-centre — e.g. Drawer A's body spans z ∈ [−0.44,
    // +0.16], so its bbox centre sits at −0.14, not the origin. Placement reasons about a *symmetric*
    // footprint about the origin, so an off-centre mesh seated against a wall pokes its far side
    // through it (the "furniture halfway through a wall" report). The furnish spawn shifts the model by
    // −(yaw · pivot) so its bbox centre lands on the placement point, making the symmetric `footprint`
    // an accurate reservation. Defaults to (0,0) — a centred mesh needs no correction.
    #[serde(default)]
    pub pivot: (f32, f32),
    // Vertical seat, metres — the Y twin of `pivot`, which is XZ-only.
    //
    // Every mesh in this kit is authored base-at-0 (`docs/artist_guide.md` §3), so a prop placed at a
    // floor cell rests ON the floor and needs nothing. This exists for the pieces that are meant to be
    // **recessed INTO** it: `ozea/floor_grate.glb` and `ozea/floor_light.glb` are 0.06 m hazard-station
    // plates that should read as set into the deck, not as a step standing on it.
    //
    // They used to get that by accident — the meshes were centre-origined, so half of each sank below
    // y = 0 on its own. Re-origining the Ozea kit to one convention (2026-08-01) removed the accident
    // and left the intent with nowhere to live, which is what this field is. A negative value sinks;
    // the default 0.0 means "sits on the floor", which is right for every other row.
    #[serde(default)]
    pub y_offset: f32,
}

/// A parsed furniture manifest.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FurnitureManifest {
    pub items: Vec<ManifestItem>,
}

impl FurnitureManifest {
    /// Items whose role matches a predicate — the furnish pass partitions the catalogue by role this way.
    pub fn by_role(&self, pred: impl Fn(&Role) -> bool) -> Vec<&ManifestItem> {
        self.items.iter().filter(|i| pred(&i.role)).collect()
    }
}

/// The WFC scatter solver packs `tiled.len() + 1` prototypes (the extra slot is the empty cell) into
/// a single `u32` compatibility mask, so a manifest may declare at most this many `Role::Tiled` items.
/// Enforced at parse time so an oversized kit fails loudly at the door rather than shift-overflowing
/// the solver (`collapse_grid`'s `assert!(n <= 32)`) at furnish time.
pub const MAX_TILED_PROTOTYPES: usize = 31;

/// Parse a manifest from RON text. Returns a descriptive error rather than panicking — the caller
/// (plugin build) decides how loudly to surface a malformed manifest. Also enforces the WFC
/// [`MAX_TILED_PROTOTYPES`] cap so a data-only kit swap can never crash the solver later.
pub fn parse_manifest(text: &str) -> Result<FurnitureManifest, String> {
    let manifest = ron::from_str::<FurnitureManifest>(text)
        .map_err(|e| format!("manifest parse error: {e}"))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Enforce the WFC [`MAX_TILED_PROTOTYPES`] cap and the surface-class contract on an already-deserialized
/// manifest. Split from [`parse_manifest`] so the game's unified config loader
/// can validate the `placement.furniture` slice it deserializes as part of the master `GameConfig` — one
/// path, no fallback.
pub fn validate_manifest(manifest: &FurnitureManifest) -> Result<(), String> {
    let tiled = manifest
        .items
        .iter()
        .filter(|i| matches!(i.role, Role::Tiled))
        .count();
    if tiled > MAX_TILED_PROTOTYPES {
        return Err(format!(
            "manifest declares {tiled} `role: Tiled` items; the WFC scatter solver supports at most \
             {MAX_TILED_PROTOTYPES} (its u32 prototype mask). Reduce the Tiled set or retag items."
        ));
    }

    // Surface-class contract — reject silent-drop authoring at the door (the project's one-path rule:
    // a kit that can never place a prop is misauthored input, not a runtime condition to degrade on).
    // Post the surface/affordance split, `furnish` Pass 4 places a `Scatter` prop only where some item's
    // `surfaces` provides its class; without these checks a typo'd token or a provider-less kit would
    // simply never spawn the prop, with nothing anywhere saying why.
    let known = || {
        super::surfaces::SURFACE_CLASSES
            .iter()
            .map(|(t, _)| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut provided = 0u32;
    for item in &manifest.items {
        for token in &item.surfaces {
            let bits = super::surfaces::surface_bits(token);
            if bits == 0 {
                return Err(format!(
                    "item `{}` offers unknown surface class `{token}` in `surfaces` — known classes: \
                     {} (surfaces::SURFACE_CLASSES). An unknown class provides nothing, so every prop \
                     targeting it would be silently dropped; add the class there or fix the token.",
                    item.key,
                    known()
                ));
            }
            provided |= bits;
        }
    }
    for item in &manifest.items {
        if let Role::Scatter { surface } = &item.role {
            let need = super::surfaces::surface_bits(surface);
            if need == 0 {
                return Err(format!(
                    "scatter prop `{}` targets unknown surface class `{surface}` — known classes: {} \
                     (surfaces::SURFACE_CLASSES). It could never be placed.",
                    item.key,
                    known()
                ));
            }
            if provided & need == 0 {
                return Err(format!(
                    "scatter prop `{}` targets surface class `{surface}`, but no item in this manifest \
                     offers it (no `surfaces` list contains it) — the prop could never be placed. Give \
                     some support piece `surfaces: [\"{surface}\"]` or drop the prop.",
                    item.key
                ));
            }
        }
    }
    Ok(())
}

/// Read + parse a manifest file. One path: a missing or malformed manifest is a hard, loud error
/// (the placement grammar has no default catalogue to fall back to).
pub fn load_manifest(path: &str) -> Result<FurnitureManifest, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    parse_manifest(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::ir::{Host, Role};

    #[test]
    fn parses_roles_and_affordances() {
        let text = r#"(
            items: [
                ( key: "ceiling_light", glb: "x/Ceiling Light.glb", category: "light",
                  tags: ["ceiling"], role: Anchor(host: Ceiling), footprint: (0.6, 0.6),
                  affordances: ["emit"] ),
                ( key: "sofa", glb: "x/Sofa A.glb", category: "seating",
                  role: Freestanding, footprint: (1.9, 0.9), affordances: ["sit"] ),
            ],
        )"#;
        let m = parse_manifest(text).expect("valid manifest");
        assert_eq!(m.items.len(), 2);
        assert!(matches!(
            m.items[0].role,
            Role::Anchor {
                host: Host::Ceiling
            }
        ));
        assert!(matches!(m.items[1].role, Role::Freestanding));
        assert_eq!(m.by_role(|r| matches!(r, Role::Freestanding)).len(), 1);
        assert_eq!(m.items[1].affordances, vec!["sit".to_string()]);
    }

    /// The surface-class contract, both sides. A `Scatter` prop whose class no item offers — or any
    /// unknown token on either the requiring or the offering side — is a load-time error naming the item
    /// and the vocabulary, never a prop that silently fails to spawn at furnish time. This is the
    /// manifest-level half of the surface/affordance split's "fail loudly at the door" guarantee.
    #[test]
    fn rejects_silently_droppable_scatter_authoring() {
        // A provided class parses: the lamp targets "worktop" and the desk offers it.
        let ok = r#"(
            items: [
                ( key: "desk", glb: "x/Desk.glb", category: "table", role: Freestanding,
                  footprint: (1.9, 0.9), surfaces: ["support", "worktop"] ),
                ( key: "lamp", glb: "x/Lamp.glb", category: "light",
                  role: Scatter(surface: "worktop"), footprint: (0.3, 0.3) ),
            ],
        )"#;
        parse_manifest(ok).expect("a provided scatter class is valid");

        // (1) A scatter class nothing offers → rejected, naming the prop and the class.
        let unprovided = r#"(
            items: [
                ( key: "bed", glb: "x/Bed.glb", category: "bed", role: Freestanding,
                  footprint: (2.0, 1.6), affordances: ["sleep"] ),
                ( key: "lamp", glb: "x/Lamp.glb", category: "light",
                  role: Scatter(surface: "worktop"), footprint: (0.3, 0.3) ),
            ],
        )"#;
        let err =
            parse_manifest(unprovided).expect_err("a provider-less scatter class must be rejected");
        assert!(
            err.contains("lamp") && err.contains("worktop"),
            "error names prop + class: {err}"
        );

        // (2) An unknown token in `surfaces` (the typo'd-provider case) → rejected at the door.
        let bad_provider = r#"(
            items: [
                ( key: "desk", glb: "x/Desk.glb", category: "table", role: Freestanding,
                  footprint: (1.9, 0.9), surfaces: ["workop"] ),
            ],
        )"#;
        let err =
            parse_manifest(bad_provider).expect_err("an unknown surfaces token must be rejected");
        assert!(
            err.contains("desk") && err.contains("workop"),
            "error names item + token: {err}"
        );

        // (3) An unknown token in a `Scatter` role (e.g. the playbook's old "media") → rejected.
        let bad_target = r#"(
            items: [
                ( key: "tv", glb: "x/TV.glb", category: "appliance",
                  role: Scatter(surface: "media"), footprint: (0.9, 0.3) ),
            ],
        )"#;
        let err =
            parse_manifest(bad_target).expect_err("an unknown scatter class must be rejected");
        assert!(
            err.contains("tv") && err.contains("media"),
            "error names prop + token: {err}"
        );
    }

    #[test]
    fn rejects_too_many_tiled() {
        // One past the cap: the WFC u32 mask can't fit `n = tiled.len() + 1` prototypes, so the
        // manifest must be rejected at the door rather than panicking the solver at furnish time.
        let mut body = String::new();
        for i in 0..=MAX_TILED_PROTOTYPES {
            body.push_str(&format!(
                "( key: \"t{i}\", glb: \"x/t{i}.glb\", category: \"decor\", role: Tiled, footprint: (0.5, 0.5) ),\n"
            ));
        }
        let text = format!("( items: [ {body} ] )");
        let err =
            parse_manifest(&text).expect_err("more than the cap of Tiled items must be rejected");
        assert!(
            err.contains("Tiled"),
            "error should name the Tiled cap: {err}"
        );
    }

}
