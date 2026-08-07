//! Shared test-side helpers. The bulk of this file is the hand-rolled reader for binary glTF
//! (`.glb`) containers — how the asset-contract tests (`tests/valkyrie_asset.rs`,
//! `tests/creature_clip_contract.rs`) look at what a rig actually ships; [`source_scan`] carries the
//! line-scanner the source lints share.
//!
//! The glTF reader is deliberately hand-rolled: pulling a glTF crate in as a dev-dependency to read
//! a header would be a second, differently-behaved reader of an asset the engine already parses its
//! own way.
//!
//! Each integration-test crate compiles its own copy of this module and uses the subset it needs,
//! hence the file-wide `dead_code` allowance.
#![allow(dead_code)]

pub mod source_roots;
pub mod source_scan;

pub struct Glb {
    pub path: String,
    pub json: serde_json::Value,
    pub bin: Vec<u8>,
}

impl Glb {
    /// Parse the JSON chunk and keep the BIN chunk of the binary glTF container. Anything malformed
    /// is refused rather than silently misread.
    pub fn load(path: &str) -> Glb {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        assert!(bytes.len() > 20, "{path} is too short to be a glb");
        assert_eq!(&bytes[0..4], b"glTF", "{path} is not a binary glTF container");
        let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        assert_eq!(&bytes[16..20], b"JSON", "{path}'s first chunk is not JSON");
        let json_end = 20 + json_len;
        assert!(json_end <= bytes.len(), "{path}'s JSON chunk runs past the end of the file");
        let json = serde_json::from_slice(&bytes[20..json_end])
            .unwrap_or_else(|e| panic!("{path}'s JSON chunk does not parse: {e}"));

        assert!(json_end + 8 <= bytes.len(), "{path} has no BIN chunk");
        let bin_len = u32::from_le_bytes([
            bytes[json_end],
            bytes[json_end + 1],
            bytes[json_end + 2],
            bytes[json_end + 3],
        ]) as usize;
        assert_eq!(&bytes[json_end + 4..json_end + 8], b"BIN\0", "{path}'s second chunk is not BIN");
        let bin_start = json_end + 8;
        let bin_end = bin_start + bin_len;
        assert!(bin_end <= bytes.len(), "{path}'s BIN chunk runs past the end of the file");
        Glb { path: path.to_string(), json, bin: bytes[bin_start..bin_end].to_vec() }
    }

    /// Decode a `VEC3`/`FLOAT` accessor. Enough of the accessor model for what these tests ask —
    /// anything else is refused rather than silently misread.
    pub fn read_vec3(&self, index: usize) -> Vec<[f32; 3]> {
        let acc = &self.json["accessors"][index];
        assert_eq!(acc["type"].as_str(), Some("VEC3"), "accessor {index} is not VEC3");
        assert_eq!(acc["componentType"].as_u64(), Some(5126), "accessor {index} is not FLOAT");
        let count = acc["count"].as_u64().expect("accessor count") as usize;
        let view = &self.json["bufferViews"][acc["bufferView"].as_u64().expect("bufferView") as usize];
        let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = view["byteStride"].as_u64().unwrap_or(12) as usize;
        (0..count)
            .map(|i| {
                let mut v = [0.0f32; 3];
                for (c, out) in v.iter_mut().enumerate() {
                    let at = base + i * stride + c * 4;
                    let raw: [u8; 4] = self.bin[at..at + 4].try_into().expect("4 bytes");
                    *out = f32::from_le_bytes(raw);
                }
                v
            })
            .collect()
    }

    pub fn animations(&self) -> Vec<&serde_json::Value> {
        self.json["animations"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no animations array", self.path))
            .iter()
            .collect()
    }

    pub fn node_names(&self) -> Vec<&str> {
        self.json["nodes"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no nodes array", self.path))
            .iter()
            .filter_map(|n| n["name"].as_str())
            .collect()
    }

    /// The index of `name` in the **full** `nodes` array — i.e. the id that `channels[].target.node`
    /// uses. Do NOT derive this from [`node_names`]: that list drops unnamed nodes, so a `position()`
    /// within it is a *filtered* index that silently diverges from the real node id the moment the rig
    /// gains an unnamed node before the one you want. A channel lookup keyed on the wrong id matches
    /// nothing, and an assertion that only fires inside `if channel.node == id` then passes vacuously.
    pub fn node_index(&self, name: &str) -> u64 {
        self.json["nodes"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no nodes array", self.path))
            .iter()
            .position(|n| n["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{} has no `{name}` node", self.path)) as u64
    }

    /// The longest keyframe time in an animation, i.e. what Bevy will report as its `duration`.
    pub fn duration(&self, anim: &serde_json::Value) -> f32 {
        let accessors = self.json["accessors"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no accessors array", self.path));
        let samplers = anim["samplers"]
            .as_array()
            .unwrap_or_else(|| panic!("an animation in {} has no samplers", self.path));
        let mut max = 0.0f32;
        for sampler in samplers {
            let input = sampler["input"].as_u64().expect("sampler input index") as usize;
            if let Some(m) = accessors[input]["max"][0].as_f64() {
                max = max.max(m as f32);
            }
        }
        max
    }
}
