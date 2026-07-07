//! The asset manifest — extensibility axis #2 (vetting §3.2). A RON file maps opaque asset keys to
//! GLB paths plus the metadata the grammar needs: a placement `Role` (the dispatch key), a footprint,
//! and **affordances** ("sit", "sleep", "support", "emit"…) so rules target what an object *affords*
//! rather than its kit-specific name (Fisher 2012; Qi 2018). Porting to a new kit is a matter of
//! authoring one manifest — no code changes — which is what the Stage-5 asset-swap test exercises.
//!
//! The manifest reuses the engine-free IR `Role`/`Host` directly, so an entry declares e.g.
//! `role: Anchor(host: Ceiling)` or `role: Freestanding` in RON with no translation layer.

use serde::Deserialize;

use super::ir::Role;

/// One catalogued asset. `glb` is a path under `assets/`; `footprint` is (width, depth) in metres
/// (= tiles, since `TILE_SIZE` is 1 m). Fields default so a terse manifest stays valid.
#[derive(Debug, Clone, Deserialize)]
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
    // Optional grouping token: items sharing a `group` are drawn together by a soft `Near` relation
    // (e.g. a bathroom's toilet + sink). Opaque like `tags`/`affordances` — matched, never interpreted.
    #[serde(default)]
    pub group: Option<String>,
}

/// A parsed furniture manifest.
#[derive(Debug, Clone, Deserialize)]
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
    let manifest =
        ron::from_str::<FurnitureManifest>(text).map_err(|e| format!("manifest parse error: {e}"))?;
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
    Ok(manifest)
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
        assert!(matches!(m.items[0].role, Role::Anchor { host: Host::Ceiling }));
        assert!(matches!(m.items[1].role, Role::Freestanding));
        assert_eq!(m.by_role(|r| matches!(r, Role::Freestanding)).len(), 1);
        assert_eq!(m.items[1].affordances, vec!["sit".to_string()]);
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
        let err = parse_manifest(&text).expect_err("more than the cap of Tiled items must be rejected");
        assert!(err.contains("Tiled"), "error should name the Tiled cap: {err}");
    }
}
