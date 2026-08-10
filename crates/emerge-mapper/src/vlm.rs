//! **The VLM labeler's pure core** — config, prompt, transport, and the early gate.
//!
//! The Tiles tab's judgement fields (`kind`/`effects`/`look` tags, `offers.surfaces`, `mount`, the
//! `note` prose, `placement.rooms`/`group`) are hand-authored, entry by entry, across the whole
//! library. This module asks a vision model to propose them from two booth renders —
//! under `docs/llm_rule_authoring.md`'s guardrails, which are not negotiable here:
//!
//! - **The LLM authors metadata dev-time only; it never runs in the sim.** This crate is the
//!   editor, never shipped — the playbook's "stripped from release" holds by construction.
//! - **Closed vocabulary only.** A suggestion carrying a token the vocabulary does not implement
//!   is rejected WHOLE at [`validate`], naming the axis — the early gate. `library.resolve`
//!   inside the commit door stays the final gate on the exact bytes written.
//! - **Human review is mandatory.** Nothing here writes a descriptor; the output is a
//!   [`Suggestion`] the review UI pre-stages and a human applies.
//! - **One path.** One configured endpoint per run (no fallback chain); an unconfigured endpoint
//!   is a loud refusal at the verb; a rejected suggestion is rejected whole, never salvaged into
//!   a degraded guess.
//!
//! No Bevy types in this module, so the whole thing is unit-testable in the GPU-free gate; the
//! only I/O is [`request_labels`]'s blocking POST, which callers run on a task-pool thread.
//!
//! # Measured decisions (change these only against the papers)
//!
//! - **Prompt-only JSON, no `response_format`/grammar.** Closed-set classification is the case
//!   where JSON output helps rather than hurts (Tam et al., "Let Me Speak Freely?",
//!   EMNLP-Industry 2024), and grammar-constrained decoding's edge shrinks for ≥14B models given
//!   examples (Raspanti et al., ACL 2025, vs Geng et al., EMNLP 2023). Revisit with llama-server
//!   GBNF only if the observed reject rate is high — it would delete the malformed-JSON failure
//!   class [`parse_reply`]'s fence tolerance papers over.
//! - **`what` is the FIRST schema key, deliberately.** Reasoning-first schemas avoid the
//!   accuracy drop answer-first ones suffer (Tam et al. 2024). Do not reorder the schema example.
//! - **One automatic reprompt on rejection** ([`label_with_retry`]): feeding the failure back and
//!   asking again is the difference between unusable and competitive affordance judgment
//!   (OVAL-Prompt, Tong et al. 2024 — F 0.39 without retry, 0.71 with). Bounded at one retry:
//!   same endpoint, same schema, same gate, and the gate's second verdict is final.

use emerge_core::descriptor::{mount_label, mount_options, Face, Mount, DecalHost};
use emerge_core::vocab::{nearest, Vocabularies, Vocabulary};

/// Where and what to ask. Endpoint, model and key are environment config so the bmb tunnel and
/// Ollama Cloud are the same code path with different values.
pub struct VlmConfig {
    pub url: String,
    pub model: String,
    key: String,
    pub timeout_secs: u64,
}

impl std::fmt::Debug for VlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key never appears in logs, errors, or panics.
        f.debug_struct("VlmConfig")
            .field("url", &self.url)
            .field("model", &self.model)
            .field("key", &"<redacted>")
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

/// The `.env` file at the project root, parsed — `KEY=VALUE` lines, `#` comments, an optional
/// `export ` prefix and single/double quotes tolerated. Hand-rolled: twenty lines of parsing is
/// not worth a dependency, and this is a dev tool's config, not a spec implementation.
fn dotenv(root: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(root.join(".env")) else {
        return out;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        out.insert(key.trim().to_owned(), value.to_owned());
    }
    out
}

impl VlmConfig {
    /// Read the endpoint config: the process environment first, then the project root's `.env`
    /// file — so the file is what a session loads on start, and a one-off
    /// `EMERGE_VLM_MODEL=... cargo run` still overrides it. Says exactly how to configure it
    /// when it cannot.
    ///
    /// The default URL is the SSH-tunnel form — the bmb service is bound to `127.0.0.1` on bmb,
    /// and the human brings the tunnel up (never this program):
    /// `ssh -fN -L 9292:127.0.0.1:9292 bmb`. Ollama Cloud is a pure config flip:
    /// `EMERGE_VLM_URL=https://ollama.com/v1/chat/completions EMERGE_VLM_MODEL=qwen3-vl:235b
    /// EMERGE_VLM_KEY=$OLLAMA_API_KEY`.
    pub fn load(root: &std::path::Path) -> Result<VlmConfig, String> {
        let file = dotenv(root);
        Self::from_lookup(|name| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| file.get(name).cloned())
        })
    }

    /// The same rules over any lookup — what the tests drive, since mutating the process
    /// environment is both racy under parallel tests and `unsafe` in edition 2024.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<VlmConfig, String> {
        let key = get("EMERGE_VLM_KEY").filter(|k| !k.is_empty());
        let Some(key) = key else {
            return Err(
                "VLM not configured: put EMERGE_VLM_KEY in the project root's .env (gitignored) \
                 or the environment. For the local bmb model: \
                 `ssh -fN -L 9292:127.0.0.1:9292 bmb` then \
                 `echo EMERGE_VLM_KEY=$(ssh -n bmb 'cat ~/llm/.api-key') >> .env`. \
                 For Ollama Cloud also set EMERGE_VLM_URL and EMERGE_VLM_MODEL."
                    .to_owned(),
            );
        };
        let url = get("EMERGE_VLM_URL")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:9292/v1/chat/completions".to_owned());
        let model = get("EMERGE_VLM_MODEL")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "qwen3-vl-30b".to_owned());
        let timeout_secs = get("EMERGE_VLM_TIMEOUT_SECS")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120);
        Ok(VlmConfig { url, model, key, timeout_secs })
    }

    /// A config pointed at a test stub — visible to tests only.
    #[cfg(test)]
    fn for_stub(url: String, timeout_secs: u64) -> VlmConfig {
        VlmConfig {
            url,
            model: "stub-model".to_owned(),
            key: "stub-key".to_owned(),
            timeout_secs,
        }
    }
}

/// How sure the model said it was — display only, never a branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// A vocabulary addition the model wanted and could not have — the only legal exit for an
/// out-of-vocab idea. Flagged for a human; never enters the axes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TokenProposal {
    pub axis: String,
    pub token: String,
    pub why: String,
}

/// The model's judgement that an asset is authored lying down, and which quarter turn would
/// stand it up. **Flagged, never auto-applied**: the turn itself (`tiles::rotate_mesh`) is a
/// measurement operation — it re-measures extents and rewrites the lattice off the GLB — and
/// measurements are the importer's, triggered deliberately by a human. The VLM supplies only the
/// judgement that one is needed.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NeedsTurn {
    /// `"x"` or `"z"` — the horizontal axis of the righting quarter turn.
    pub axis: String,
    pub why: String,
}

/// What the model proposed, already validated against the live vocabulary — every token in these
/// lists exists, in vocabulary order, deduplicated. Serde because the suggestions cache persists
/// these between sessions.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Suggestion {
    /// The model's one-sentence identification — the reasoning the axis answers hang off, and the
    /// human-review headline.
    pub what: String,
    pub kind: Vec<String>,
    pub effects: Vec<String>,
    pub look: Vec<String>,
    pub offers_surfaces: Vec<String>,
    pub mount: Option<Mount>,
    /// The item's visual front — which face it should present to the room. A judgement only
    /// appearance can answer, which is why the importer defaults it and the model proposes it.
    pub front: Option<Face>,
    pub needs_turn: Option<NeedsTurn>,
    pub note: Option<String>,
    pub rooms: Vec<String>,
    pub group: Option<String>,
    pub confidence: Confidence,
    pub token_proposals: Vec<TokenProposal>,
}

/// Who said so, when, in how many attempts — the review header's facts. Never the key, never the
/// endpoint host.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    pub model: String,
    /// `YYYY-MM-DD`, supplied by the caller (this module owns no clock).
    pub date: String,
    /// 1 = accepted first try; 2 = accepted after the reprompt.
    pub attempts: u8,
}

/// Everything the prompt needs about the subject, gathered by the caller — no `&Project` in here.
pub struct PromptCtx {
    pub id: String,
    pub mesh: String,
    /// Measured footprint (w, d) and height, metres — given to the model so size disambiguates a
    /// pan from a pot; never the model's to infer.
    pub footprint: Option<(f32, f32)>,
    pub height: Option<f32>,
    pub mount_now: String,
    pub kind_now: Vec<String>,
    pub effects_now: Vec<String>,
    pub look_now: Vec<String>,
    pub offers_now: Vec<String>,
    pub note_now: Option<String>,
    /// Room / group names already in use across the library, so the model converges on the
    /// existing vocabulary of free text instead of coining synonyms.
    pub rooms_in_use: Vec<String>,
    pub groups_in_use: Vec<String>,
    /// **What the MESH says about its own front**, measured off the vertices by
    /// `emerge_core::glb::Glb::derive_front` — not something the model is asked to see.
    ///
    /// `Some(Some(face))` measured and asymmetric; `Some(None)` measured **symmetric**, so there is
    /// no front to claim; `None` could not be measured at all (no mesh, unreadable file).
    ///
    /// This exists because the model got it wrong three times in one pass, on facts the code already
    /// held: a front for a 1x1 floor tile, and a front for two seats that
    /// `the_seat_meshes_declare_the_front_that_was_measured_off_them` asserts are symmetric and of
    /// which *"none may be asserted"*. Two renders cannot settle symmetry; a vertex buffer can. The
    /// prompt already takes this line about size — *"Sizes are measured and given to you"* — and
    /// this is the same rule applied to the same class of question.
    pub front_measured: Option<Option<Face>>,
}

/// One axis section of the system prompt: every token with its authored note. Generated LIVE from
/// the vocabulary, so a vocab edit changes the prompt with no code change.
fn axis_lines(title: &str, hint: &str, v: &Vocabulary) -> String {
    let mut out = format!("{title} - {hint} (use any that apply):\n");
    for t in &v.tokens {
        out.push_str(&format!("- {}: {}\n", t.name, t.note));
    }
    out
}

/// **The one place a `Mount` gets its wire name.**
///
/// This name was written out by hand in three places that all had to agree: the prompt's JSON
/// example, `parse_mount`'s match arms, and the refusal listing the legal options. Renaming
/// `Overlay` to `Decal` meant editing all three, and nothing would have failed if one had been
/// missed — the model would simply have been offered a token the parser rejects, and the reprompt
/// would have argued with it about a word neither side could win on.
///
/// The parser still needs literal arms, because parsing is by literal. What this removes is the
/// possibility of the *offered* set and the *accepted* set disagreeing — `every_offered_mount_
/// round_trips_through_its_own_token` walks `mount_options` and proves it.
pub fn mount_token(m: &Mount) -> &'static str {
    match m {
        Mount::OnFloor => "floor",
        Mount::OnSurface { .. } => "surface",
        Mount::OnWall { .. } => "wall",
        Mount::OnCeiling => "ceiling",
        Mount::Tiled => "tiled",
        Mount::InOpening { .. } => "opening",
        Mount::Decal { on: DecalHost::Floor } => "decal_floor",
        Mount::Decal { on: DecalHost::Wall { .. } } => "decal_wall",
        Mount::Decal { on: DecalHost::Ceiling } => "decal_ceiling",
    }
}

/// The mount options as JSON discriminants, generated from the same table the editor's `M` key
/// cycles — every offered mount is one the schema can express, named by [`mount_token`].
fn mount_lines(surfaces: &[String]) -> String {
    let mut out = String::from(
        "mount - where THIS asset is placed (exactly one object, or null when unclear):\n",
    );
    for m in mount_options(surfaces) {
        let token = mount_token(&m);
        // Only the shape of the EXTRA fields is per-variant now; the name comes from one table.
        let json = match &m {
            Mount::OnSurface { class } => format!(r#"{{"on": "{token}", "class": "{class}"}}"#),
            Mount::OnWall { .. } | Mount::Decal { on: DecalHost::Wall { .. } } => {
                format!(r#"{{"on": "{token}", "height_m": 1.8}}"#)
            }
            _ => format!(r#"{{"on": "{token}"}}"#),
        };
        out.push_str(&format!("- {json} - {}\n", mount_label(Some(&m))));
    }
    out
}

/// **The one place a `Face` gets its wire name** — [`mount_token`]'s twin, and added for the same
/// reason: the parser knew these four words and nothing could write them, so stating a measured
/// front in the prompt meant inventing a fifth spelling of "south".
pub fn face_token(f: Face) -> &'static str {
    match f {
        Face::North => "north",
        Face::East => "east",
        Face::South => "south",
        Face::West => "west",
    }
}

/// **The measured front, stated as a fact rather than asked as a question.**
///
/// The mesh is the authority here and the renders are not: symmetry is a property of the vertex
/// buffer, and two three-quarter views cannot settle it. An unmeasurable mesh says so and leaves the
/// judgement where it was, which is the honest third state — not a default.
fn front_measured_line(measured: Option<Option<Face>>) -> String {
    match measured {
        Some(Some(face)) => format!(
            "  The mesh MEASURES a front at \"{}\" (its upper mass sits to that side). Answer that \
             unless the images plainly contradict it, and lower `confidence` if they do.\n",
            face_token(face)
        ),
        Some(None) => "  The mesh MEASURES SYMMETRIC: it has no front. Answer null. Do not infer one \
             from the images — a symmetric prop presenting a \"front\" is a claim about the art that \
             the geometry does not support.\n"
            .to_owned(),
        None => "  This mesh could not be measured, so judge the front from the images alone.\n"
            .to_owned(),
    }
}

/// Build the `(system, user)` prompt pair.
pub fn build_prompt(vocab: &Vocabularies, ctx: &PromptCtx) -> (String, String) {
    let surface_names: Vec<String> =
        vocab.surfaces.tokens.iter().map(|t| t.name.clone()).collect();
    // The schema example. `what` FIRST — reasoning-first ordering is measured, not style
    // (Tam et al. 2024); a future edit must not move it below the axis fields.
    let schema = r#"{
  "what": "one sentence: what real-world thing this is",
  "kind": [], "effects": [], "look": [],
  "offers_surfaces": [],
  "mount": {"on": "floor"},
  "front": "south",
  "needs_turn": null,
  "note": "one or two sentences a human author would keep",
  "rooms": [], "group": null,
  "confidence": "high",
  "token_proposals": []
}"#;
    let system = format!(
        "You label ONE 3D game asset for a level-placement library. You are given two renders of \
         the same asset: image 1 is a three-quarter front view, image 2 is a three-quarter rear \
         view.\n\
         Answer with ONE JSON object matching the schema at the end. No prose outside the JSON. \
         No code fences.\n\n\
         Rules:\n\
         - Write `what` first: one sentence naming the real-world thing you see. Reason there, \
         then answer in the fields.\n\
         - Use ONLY the tokens listed below, spelled exactly. If a token you need does not exist, \
         add it to `token_proposals` with a one-line reason and leave the axes honest without it.\n\
         - When unsure about an axis, prefer an empty list over a guess.\n\
         - The asset's id and mesh filename are the designer's own naming — read them as intent \
         (a plain box named `soap_bar` is soap, not a crate). When the name and the images \
         disagree, describe what you SEE and lower `confidence`.\n\
         - Sizes are measured and given to you; never infer size from the image when the \
         measurement disagrees.\n\
         - `offers_surfaces` lists surface classes OTHER items may rest ON THIS asset. What the \
         asset is FOR never goes there: a bed affords sleep and offers no surface.\n\n\
         {kind}\n{effects}\n{look}\n{offers}\n\
         {mounts}\n\
         Orientation. The camera geometry: image 1 shows the asset's east (+X) and south (+Z) \
         faces; image 2 shows its west (-X) and north (-Z) faces.\n\
         front - which face is the item's visual FRONT, the side it should present to the room \
         (a sofa fronts where you sit, a screen where it is watched): \"north\", \"east\", \
         \"south\", \"west\", or null for items with no front (symmetric props).\n\
         {front_measured}\
         needs_turn - null when the asset stands upright as authored. ONLY when it clearly lies \
         on its side or back, {{\"axis\": \"x\"|\"z\", \"why\": \"...\"}} names the horizontal \
         quarter turn that would stand it up. Never guess; unsure means null.\n\
         rooms - snake_case room names where this belongs. Prefer names already in use: \
         [{rooms_in_use}].\n\
         group - an optional snake_case set name for items placed together. Prefer names already \
         in use: [{groups_in_use}].\n\
         confidence - \"high\", \"medium\" or \"low\" for the labels overall.\n\
         token_proposals - entries like {{\"axis\": \"surfaces\", \"token\": \"hob\", \"why\": \
         \"...\"}}. Axes: kind, effects, look, surfaces.\n\n\
         Schema (keep this key order):\n{schema}",
        kind = axis_lines("kind", "what the thing IS", &vocab.kind),
        effects = axis_lines("effects", "what it DOES to the world", &vocab.effects),
        look = axis_lines("look", "how it LOOKS", &vocab.look),
        offers = axis_lines(
            "offers_surfaces",
            "surface classes THIS asset provides to others",
            &vocab.surfaces
        ),
        mounts = mount_lines(&surface_names),
        rooms_in_use = ctx.rooms_in_use.join(", "),
        groups_in_use = ctx.groups_in_use.join(", "),
        front_measured = front_measured_line(ctx.front_measured),
        schema = schema,
    );

    let size = match (ctx.footprint, ctx.height) {
        (Some((w, d)), Some(h)) => {
            format!("Measured footprint: {w:.2} x {d:.2} m; height: {h:.2} m.")
        }
        (Some((w, d)), None) => format!("Measured footprint: {w:.2} x {d:.2} m."),
        (None, Some(h)) => format!("Measured height: {h:.2} m."),
        (None, None) => "No measurements available.".to_owned(),
    };
    let user = format!(
        "Asset id (the designer's name for it): {id}\nMesh file: {mesh}\n{size}\nCurrent mount: \
         {mount}. Current kind: [{kind}]; \
         effects: [{effects}]; look: [{look}]; offers_surfaces: [{offers}].\nCurrent note: \
         {note}\nLabel this asset.",
        id = ctx.id,
        mesh = ctx.mesh,
        mount = ctx.mount_now,
        kind = ctx.kind_now.join(", "),
        effects = ctx.effects_now.join(", "),
        look = ctx.look_now.join(", "),
        offers = ctx.offers_now.join(", "),
        note = ctx.note_now.as_deref().unwrap_or("(none)"),
    );
    (system, user)
}

/// The shape the model answers in, before the gate. Field order mirrors the schema example.
#[derive(Debug, serde::Deserialize)]
pub struct RawSuggestion {
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub kind: Vec<String>,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub look: Vec<String>,
    #[serde(default)]
    pub offers_surfaces: Vec<String>,
    #[serde(default)]
    pub mount: Option<RawMount>,
    #[serde(default)]
    pub front: Option<String>,
    #[serde(default)]
    pub needs_turn: Option<RawTurn>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub rooms: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub token_proposals: Vec<RawProposal>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RawMount {
    pub on: String,
    #[serde(default)]
    pub height_m: Option<f32>,
    #[serde(default)]
    pub class: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RawTurn {
    #[serde(default)]
    pub axis: String,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct RawProposal {
    #[serde(default)]
    pub axis: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub why: String,
}

/// The model's reply text out of the OpenAI response envelope.
pub fn extract_content(http_body: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(http_body)
        .map_err(|e| format!("the endpoint's response is not JSON: {e}"))?;
    if let Some(err) = v.get("error") {
        // llama-swap and Ollama both put their complaint here; surface it verbatim.
        return Err(format!("the endpoint refused: {err}"));
    }
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "the response carries no choices[0].message.content".to_owned())
}

/// The model's JSON out of its reply text — fence-tolerant (a model that wraps its answer in
/// ```` ```json ```` fences answered correctly and sloppily; text outside the outermost braces is
/// discarded). Tolerance stops there: no braces is a rejection, not a guess.
pub fn parse_reply(content: &str) -> Result<RawSuggestion, String> {
    let start = content.find('{');
    let end = content.rfind('}');
    let (Some(start), Some(end)) = (start, end) else {
        return Err("the reply contains no JSON object".to_owned());
    };
    if end < start {
        return Err("the reply contains no JSON object".to_owned());
    }
    serde_json::from_str::<RawSuggestion>(&content[start..=end])
        .map_err(|e| format!("the reply's JSON does not match the schema: {e}"))
}

/// One validated axis: every token exists, deduplicated, re-sorted into VOCABULARY order (the
/// `on_tag_chip` rule — diffs show real changes only). Rejection names the axis and carries the
/// did-you-mean plus the full token list, which is exactly what the reprompt turn needs.
fn valid_axis(axis: &str, proposed: &[String], v: &Vocabulary) -> Result<Vec<String>, String> {
    for token in proposed {
        if !v.contains(token) {
            let hint = nearest(v, token)
                .map(|n| format!(" (did you mean `{n}`?)"))
                .unwrap_or_default();
            let listed: Vec<&str> = v.tokens.iter().map(|t| t.name.as_str()).collect();
            return Err(format!(
                "`{token}` is not a `{axis}` token{hint}; the `{axis}` axis implements: {}",
                listed.join(", ")
            ));
        }
    }
    Ok(v.tokens
        .iter()
        .map(|t| t.name.as_str())
        .filter(|name| proposed.iter().any(|p| p == name))
        .map(str::to_owned)
        .collect())
}

/// A wall height a human would believe — the model answering `18` for `1.8` must be a rejection,
/// not a sconce on a chimney.
const WALL_HEIGHT_RANGE: std::ops::RangeInclusive<f32> = 0.1..=4.0;

fn valid_mount(raw: &RawMount, surfaces: &Vocabulary) -> Result<Mount, String> {
    let wall_height = |h: Option<f32>| -> Result<f32, String> {
        let h = h.ok_or("mount on `wall` needs `height_m`")?;
        if !WALL_HEIGHT_RANGE.contains(&h) {
            return Err(format!("wall height {h} m is outside {WALL_HEIGHT_RANGE:?}"));
        }
        Ok(h)
    };
    match raw.on.as_str() {
        "floor" => Ok(Mount::OnFloor),
        "surface" => {
            let class = raw.class.clone().ok_or("mount on `surface` needs `class`")?;
            if !surfaces.contains(&class) {
                let hint = nearest(surfaces, &class)
                    .map(|n| format!(" (did you mean `{n}`?)"))
                    .unwrap_or_default();
                return Err(format!(
                    "mount class `{class}` is not a `surfaces` token{hint}"
                ));
            }
            Ok(Mount::OnSurface { class })
        }
        "wall" => Ok(Mount::OnWall { height: wall_height(raw.height_m)? }),
        "ceiling" => Ok(Mount::OnCeiling),
        "tiled" => Ok(Mount::Tiled),
        "opening" => Ok(Mount::InOpening { clear: None }),
        "decal_floor" => Ok(Mount::Decal { on: DecalHost::Floor }),
        "decal_wall" => Ok(Mount::Decal {
            on: DecalHost::Wall { height: wall_height(raw.height_m)? },
        }),
        "decal_ceiling" => Ok(Mount::Decal { on: DecalHost::Ceiling }),
        // **Listed from the same table the prompt offers**, so a refusal can never name a set the
        // model was not shown — which is how a reprompt turns into an argument about a word.
        other => Err(format!(
            "`{other}` is not a mount; the options are {}",
            mount_options(
                &surfaces.tokens.iter().map(|t| t.name.clone()).collect::<Vec<_>>()
            )
            .iter()
            .map(mount_token)
            .fold(Vec::new(), |mut acc: Vec<&str>, t| {
                // One `OnSurface` per surface class collapses to one `surface` here: the class is a
                // separate field, and listing it twice reads as two different mounts.
                if !acc.contains(&t) {
                    acc.push(t);
                }
                acc
            })
            .join(", ")
        )),
    }
}

/// **The early gate.** A suggestion with any invalid part is rejected WHOLE — no partial salvage,
/// no degraded guess (the playbook's one-path rule). The `Err` text names the axis and the legal
/// tokens, so it doubles as the reprompt correction.
pub fn validate(raw: RawSuggestion, vocab: &Vocabularies) -> Result<Suggestion, String> {
    if raw.what.trim().is_empty() {
        return Err("`what` is empty — identify the thing before labelling it".to_owned());
    }
    let kind = valid_axis("kind", &raw.kind, &vocab.kind)?;
    let effects = valid_axis("effects", &raw.effects, &vocab.effects)?;
    let look = valid_axis("look", &raw.look, &vocab.look)?;
    let offers_surfaces = valid_axis("surfaces", &raw.offers_surfaces, &vocab.surfaces)?;
    let mount = match &raw.mount {
        Some(m) => Some(valid_mount(m, &vocab.surfaces)?),
        None => None,
    };
    let front = match raw.front.as_deref() {
        None | Some("") => None,
        Some("north") => Some(Face::North),
        Some("east") => Some(Face::East),
        Some("south") => Some(Face::South),
        Some("west") => Some(Face::West),
        Some(other) => {
            return Err(format!(
                "`{other}` is not a face; front is north, east, south, west, or null"
            ));
        }
    };
    let needs_turn = match &raw.needs_turn {
        None => None,
        Some(t) => match t.axis.as_str() {
            "x" | "z" => Some(NeedsTurn { axis: t.axis.clone(), why: t.why.clone() }),
            other => {
                return Err(format!(
                    "`{other}` is not a righting axis; needs_turn.axis is \"x\" or \"z\" — a \
                     y turn changes the facing, which is `front`'s to say"
                ));
            }
        },
    };
    let confidence = match raw.confidence.as_deref() {
        Some("high") => Confidence::High,
        Some("low") => Confidence::Low,
        // An unstated confidence is a medium one; this is display-only and never branches.
        _ => Confidence::Medium,
    };
    let rooms: Vec<String> = raw
        .rooms
        .iter()
        .map(|r| emerge_core::naming::to_snake_case(r))
        .filter(|r| !r.is_empty())
        .collect();
    let group = raw
        .group
        .as_deref()
        .map(emerge_core::naming::to_snake_case)
        .filter(|g| !g.is_empty());
    let axis_of = |name: &str| -> Option<&Vocabulary> {
        match name {
            "kind" => Some(&vocab.kind),
            "effects" => Some(&vocab.effects),
            "look" => Some(&vocab.look),
            "surfaces" => Some(&vocab.surfaces),
            _ => None,
        }
    };
    let mut token_proposals = Vec::new();
    for p in &raw.token_proposals {
        let Some(axis) = axis_of(&p.axis) else {
            return Err(format!(
                "token proposal names axis `{}`; the axes are kind, effects, look, surfaces",
                p.axis
            ));
        };
        // A "proposal" for a token that already exists is noise, not an error — drop it.
        if axis.contains(&p.token) || p.token.trim().is_empty() {
            continue;
        }
        token_proposals.push(TokenProposal {
            axis: p.axis.clone(),
            token: emerge_core::naming::to_snake_case(&p.token),
            why: p.why.clone(),
        });
    }
    Ok(Suggestion {
        what: raw.what.trim().to_owned(),
        kind,
        effects,
        look,
        offers_surfaces,
        mount,
        front,
        needs_turn,
        note: raw.note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()),
        rooms,
        group,
        confidence,
        token_proposals,
    })
}

/// The reprompt turn: the model's own reply plus the gate's verdict, appended as conversation.
pub struct RetryTurn<'a> {
    pub prior_reply: &'a str,
    pub rejection: &'a str,
}

/// One blocking OpenAI-style chat POST. Called only from a task-pool thread — never the UI
/// thread. Returns the raw response body; envelope and JSON handling are the parsers' business.
pub fn request_labels(
    cfg: &VlmConfig,
    pngs: &[Vec<u8>; 2],
    system: &str,
    user: &str,
    retry: Option<RetryTurn<'_>>,
) -> Result<String, String> {
    use base64::Engine as _;
    let image_part = |png: &[u8]| {
        serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                )
            }
        })
    };
    let mut messages = vec![
        serde_json::json!({ "role": "system", "content": system }),
        serde_json::json!({
            "role": "user",
            "content": [
                { "type": "text", "text": user },
                image_part(&pngs[0]),
                image_part(&pngs[1]),
            ]
        }),
    ];
    if let Some(turn) = &retry {
        messages.push(serde_json::json!({ "role": "assistant", "content": turn.prior_reply }));
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!(
                "Your previous answer was rejected: {}. Answer again, corrected, with the same \
                 JSON schema. No prose outside the JSON.",
                turn.rejection
            )
        }));
    }
    let body = serde_json::json!({
        "model": cfg.model,
        "temperature": 0.1,
        "max_tokens": 800,
        "messages": messages,
    });

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(cfg.timeout_secs)))
        // Non-2xx carries the endpoint's own complaint in its body; read it rather than mapping
        // every status to a bare number.
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(&cfg.url)
        .header("Authorization", &format!("Bearer {}", cfg.key))
        .send_json(&body)
        .map_err(|e| format!("the VLM endpoint is unreachable: {e}"))?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("reading the VLM response failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("the VLM endpoint answered {status}: {text}"));
    }
    Ok(text)
}

/// **The whole exchange**: prompt, request, parse, gate — with ONE automatic reprompt when the
/// gate rejects. OVAL-Prompt (Tong et al. 2024) measured reprompt-on-failure as the difference
/// between unusable (F 0.39) and competitive (0.71) affordance judgment; one retry, fixed, is a
/// loop inside the one path, not a fallback — same endpoint, same schema, same gate, and the
/// gate's second verdict is final.
pub fn label_with_retry(
    cfg: &VlmConfig,
    pngs: &[Vec<u8>; 2],
    vocab: &Vocabularies,
    ctx: &PromptCtx,
    date: String,
) -> Result<(Suggestion, Provenance), String> {
    let (system, user) = build_prompt(vocab, ctx);
    let body = request_labels(cfg, pngs, &system, &user, None)?;
    let reply = extract_content(&body)?;
    match parse_reply(&reply).and_then(|raw| validate(raw, vocab)) {
        Ok(s) => Ok((
            s,
            Provenance { model: cfg.model.clone(), date, attempts: 1 },
        )),
        Err(rejection) => {
            let body = request_labels(
                cfg,
                pngs,
                &system,
                &user,
                Some(RetryTurn { prior_reply: &reply, rejection: &rejection }),
            )?;
            let reply = extract_content(&body)?;
            let s = parse_reply(&reply).and_then(|raw| validate(raw, vocab))?;
            Ok((
                s,
                Provenance { model: cfg.model.clone(), date, attempts: 2 },
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocabularies {
        Vocabularies {
            kind: Vocabulary::of(&[
                ("light", "casts light"),
                ("table", "a flat-topped piece"),
                ("appliance", "a powered machine"),
            ]),
            effects: Vocabulary::of(&[("emit", "lights the room")]),
            look: Vocabulary::of(&[("metal", "bare metal"), ("worn", "scuffed")]),
            surfaces: Vocabulary::of(&[
                ("support", "any prop-bearing top"),
                ("worktop", "a desk or table top"),
            ]),
            capabilities: Vocabulary::of(&[("cook", "prepares food")]),
            // The lattice axes. A labeller proposes nothing on either, so both are empty here — and
            // empty is not permissive: an invented token is refused, naming the axis.
            edge: Vocabulary::default(),
            anchor: Vocabulary::default(),
        }
    }

    /// [`ctx`] with a stated measurement, so the front cases read as one line each.
    fn ctx_with_front(front_measured: Option<Option<Face>>) -> PromptCtx {
        PromptCtx { front_measured, ..ctx() }
    }

    fn ctx() -> PromptCtx {
        PromptCtx {
            id: "wall_light".to_owned(),
            mesh: "kit/Wall Light.glb".to_owned(),
            footprint: Some((0.38, 0.2)),
            height: Some(0.31),
            mount_now: "unset".to_owned(),
            kind_now: vec![],
            effects_now: vec![],
            look_now: vec![],
            offers_now: vec![],
            note_now: None,
            rooms_in_use: vec!["kitchen".to_owned(), "office".to_owned()],
            groups_in_use: vec!["desk_set".to_owned()],
                    front_measured: None,
        }
    }

    #[test]
    fn the_prompt_carries_every_token_note_and_mount_option() {
        let v = vocab();
        let (system, user) = build_prompt(&v, &ctx());
        for axis in [&v.kind, &v.effects, &v.look, &v.surfaces] {
            for t in &axis.tokens {
                assert!(system.contains(&t.name), "prompt lost token `{}`", t.name);
                assert!(system.contains(&t.note), "prompt lost note for `{}`", t.name);
            }
        }
        let surfaces: Vec<String> = v.surfaces.tokens.iter().map(|t| t.name.clone()).collect();
        for m in mount_options(&surfaces) {
            assert!(
                system.contains(&mount_label(Some(&m))),
                "prompt lost mount `{}`",
                mount_label(Some(&m))
            );
        }
        // The reasoning-first ordering is measured, not style (Tam et al. 2024): `what` leads.
        let schema_at = system.find("\"what\"").unwrap_or(usize::MAX);
        let kind_at = system.rfind("\"kind\"").unwrap_or(0);
        assert!(schema_at < kind_at, "`what` must stay the first schema key");
        // The measurements and the in-use names reach the model.
        assert!(user.contains("0.38 x 0.20 m"), "{user}");
        assert!(system.contains("kitchen, office"), "{system}");
        assert!(system.contains("desk_set"), "{system}");
    }

    fn raw(json: &str) -> RawSuggestion {
        parse_reply(json).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn an_unknown_token_rejects_naming_the_axis_on_every_axis() {
        let v = vocab();
        for (field, axis) in [
            ("kind", "kind"),
            ("effects", "effects"),
            ("look", "look"),
            ("offers_surfaces", "surfaces"),
        ] {
            let json = format!(r#"{{"what": "a thing", "{field}": ["zzgobbledygook"]}}"#);
            let e = validate(raw(&json), &v).err().unwrap_or_else(|| panic!("accepted {field}"));
            assert!(e.contains(&format!("`{axis}` token")), "{field}: {e}");
            assert!(e.contains("implements:"), "{field} rejection must list the axis: {e}");
        }
        // The did-you-mean rides along when the misspelling is close (nearest() is bounded at
        // len/3 edits, so `ligt` — one deletion — qualifies where a transposition would not).
        let e = validate(raw(r#"{"what": "a lamp", "kind": ["ligt"]}"#), &v)
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("did you mean `light`"), "{e}");
    }

    /// **A measured front reaches the prompt, and a measured-symmetric mesh forbids one.**
    ///
    /// The labelling pass that prompted this asserted a front for a 1x1 floor tile and for two seats
    /// the kit measures symmetric — three claims about the art that the geometry does not support.
    /// The model was never told what the mesh said, so it answered from two renders, which cannot
    /// settle symmetry. This pins that the measurement is in the prompt and that it is stated as an
    /// instruction rather than as a hint.
    ///
    /// Also pins the spelling: `face_token` is the one place a `Face` becomes a word, and the
    /// parser's four literals are the other side of it — a fifth spelling would be a question the
    /// model cannot answer correctly.
    #[test]
    fn the_measured_front_is_told_to_the_model_and_symmetric_forbids_one() {
        let v = vocab();

        let (measured, _) = build_prompt(&v, &ctx_with_front(Some(Some(Face::East))));
        assert!(
            measured.contains("MEASURES a front at \"east\""),
            "a measured front must reach the prompt, spelled the way the parser reads it"
        );

        let (symmetric, _) = build_prompt(&v, &ctx_with_front(Some(None)));
        assert!(
            symmetric.contains("MEASURES SYMMETRIC") && symmetric.contains("Answer null"),
            "a symmetric mesh must be told to answer null, not left to the images"
        );

        let (unknown, _) = build_prompt(&v, &ctx_with_front(None));
        assert!(
            unknown.contains("could not be measured"),
            "an unmeasurable mesh says so rather than asserting a front nobody measured"
        );
        assert!(
            !unknown.contains("MEASURES"),
            "and it must not claim a measurement it does not have"
        );

        // Every face the parser accepts is one `face_token` can write, and back again.
        for f in [Face::North, Face::East, Face::South, Face::West] {
            let json = format!(r#"{{"what": "a thing", "front": "{}"}}"#, face_token(f));
            let got = validate(raw(&json), &v).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(got.front, Some(f), "{json} did not round trip");
        }
    }

    /// **Every mount the prompt OFFERS is one the parser ACCEPTS**, walked from `mount_options`
    /// rather than from a list written beside it.
    ///
    /// The hand-written cases below are worth keeping — they pin the exact wire shapes, including
    /// the `height_m` and `class` payloads. What they cannot do is notice a *new* `Mount` variant:
    /// they would go on passing while the model was offered a token nothing parses, and the reprompt
    /// would argue with it about a word neither side could win on. This walks the offered set, so a
    /// variant added tomorrow fails here until its token exists on both sides.
    ///
    /// The assertion is on the **token**, not the value: `mount_options` supplies representative
    /// heights and this must not be coupled to which ones.
    #[test]
    fn every_offered_mount_round_trips_through_its_own_token() {
        let v = vocab();
        let surfaces: Vec<String> = v.surfaces.tokens.iter().map(|t| t.name.clone()).collect();
        for m in mount_options(&surfaces) {
            let token = mount_token(&m);
            let json = match &m {
                Mount::OnSurface { class } => {
                    format!(r#"{{"on": "{token}", "class": "{class}"}}"#)
                }
                Mount::OnWall { .. } | Mount::Decal { on: DecalHost::Wall { .. } } => {
                    format!(r#"{{"on": "{token}", "height_m": 1.8}}"#)
                }
                _ => format!(r#"{{"on": "{token}"}}"#),
            };
            let full = format!(r#"{{"what": "a thing", "mount": {json}}}"#);
            let got = validate(raw(&full), &v)
                .unwrap_or_else(|e| panic!("the prompt offers `{json}` and the parser refuses it: {e}"))
                .mount
                .unwrap_or_else(|| panic!("`{json}` parsed to no mount at all"));
            assert_eq!(
                mount_token(&got),
                token,
                "`{json}` came back as a different mount than the one it names"
            );
        }
    }

    #[test]
    fn every_mount_discriminant_round_trips_and_bad_ones_reject() {
        let v = vocab();
        let cases = [
            (r#"{"on": "floor"}"#, Mount::OnFloor),
            (
                r#"{"on": "surface", "class": "worktop"}"#,
                Mount::OnSurface { class: "worktop".to_owned() },
            ),
            (r#"{"on": "wall", "height_m": 2.2}"#, Mount::OnWall { height: 2.2 }),
            (r#"{"on": "ceiling"}"#, Mount::OnCeiling),
            (r#"{"on": "tiled"}"#, Mount::Tiled),
            (r#"{"on": "opening"}"#, Mount::InOpening { clear: None }),
            (r#"{"on": "decal_floor"}"#, Mount::Decal { on: DecalHost::Floor }),
            (
                r#"{"on": "decal_wall", "height_m": 1.5}"#,
                Mount::Decal { on: DecalHost::Wall { height: 1.5 } },
            ),
            (r#"{"on": "decal_ceiling"}"#, Mount::Decal { on: DecalHost::Ceiling }),
        ];
        for (json, want) in cases {
            let full = format!(r#"{{"what": "a thing", "mount": {json}}}"#);
            let got = validate(raw(&full), &v).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(got.mount, Some(want), "{json}");
        }
        for bad in [
            r#"{"on": "roof"}"#,                          // not a mount
            r#"{"on": "surface", "class": "shelf"}"#,     // unknown class
            r#"{"on": "wall"}"#,                          // missing height
            r#"{"on": "wall", "height_m": 18.0}"#,        // a sconce on a chimney
        ] {
            let full = format!(r#"{{"what": "a thing", "mount": {bad}}}"#);
            assert!(validate(raw(&full), &v).is_err(), "accepted {bad}");
        }
    }

    /// Orientation judgements: every face round-trips, junk faces reject, and `needs_turn` takes
    /// only the two righting axes — a y turn is `front`'s business, and the rejection says so.
    #[test]
    fn front_faces_round_trip_and_turns_take_only_righting_axes() {
        let v = vocab();
        for (json, want) in [
            ("north", Face::North),
            ("east", Face::East),
            ("south", Face::South),
            ("west", Face::West),
        ] {
            let full = format!(r#"{{"what": "a sofa", "front": "{json}"}}"#);
            let got = validate(raw(&full), &v).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(got.front, Some(want), "{json}");
        }
        assert!(validate(raw(r#"{"what": "x", "front": "up"}"#), &v).is_err());

        let got = validate(
            raw(r#"{"what": "a barrel on its side", "needs_turn": {"axis": "x", "why": "authored lying down"}}"#),
            &v,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let turn = got.needs_turn.unwrap_or_else(|| panic!("no turn"));
        assert_eq!(turn.axis, "x");
        let e = validate(
            raw(r#"{"what": "x", "needs_turn": {"axis": "y", "why": ""}}"#),
            &v,
        )
        .err()
        .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("front"), "the y rejection points at front: {e}");
        // The prompt explains the camera geometry the face answer depends on.
        let (system, _) = build_prompt(&v, &ctx());
        assert!(system.contains("east (+X) and south (+Z)"), "camera geometry stated");
    }

    #[test]
    fn fenced_json_is_tolerated_and_braceless_prose_is_not() {
        let fenced = "```json\n{\"what\": \"a lamp\", \"kind\": [\"light\"]}\n```";
        let got = parse_reply(fenced).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.what, "a lamp");
        assert!(parse_reply("it is a lamp, hope that helps").is_err());
    }

    #[test]
    fn validated_axes_come_back_in_vocabulary_order_deduplicated() {
        let v = vocab();
        let got = validate(
            raw(r#"{"what": "x", "look": ["worn", "metal", "worn"]}"#),
            &v,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.look, vec!["metal".to_owned(), "worn".to_owned()]);
    }

    #[test]
    fn proposals_for_existing_tokens_are_dropped_and_unknown_axes_reject() {
        let v = vocab();
        let got = validate(
            raw(
                r#"{"what": "a stove", "token_proposals": [
                    {"axis": "surfaces", "token": "worktop", "why": "already there"},
                    {"axis": "surfaces", "token": "hob", "why": "cooktops are not worktops"}
                ]}"#,
            ),
            &v,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.token_proposals.len(), 1);
        assert_eq!(got.token_proposals[0].token, "hob");
        let e = validate(
            raw(r#"{"what": "x", "token_proposals": [{"axis": "vibes", "token": "cool", "why": ""}]}"#),
            &v,
        )
        .err()
        .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("vibes"), "{e}");
    }

    #[test]
    fn rooms_and_group_are_forced_snake_case_and_empty_what_rejects() {
        let v = vocab();
        let got = validate(
            raw(r#"{"what": "x", "rooms": ["Living Room"], "group": "Desk Set"}"#),
            &v,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.rooms, vec!["living_room".to_owned()]);
        assert_eq!(got.group.as_deref(), Some("desk_set"));
        assert!(validate(raw(r#"{"what": "  "}"#), &v).is_err());
    }

    /// The `.env` loader: KEY=VALUE with comments, `export ` prefixes and quotes; and the
    /// precedence contract — a process-env value overrides the file's, so a one-off
    /// `EMERGE_VLM_MODEL=... cargo run` still wins over the session default.
    #[test]
    fn the_dotenv_file_loads_and_the_process_env_overrides_it() {
        let dir = std::env::temp_dir().join(format!("vlm_dotenv_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            dir.join(".env"),
            "# the labeler's endpoint\nexport EMERGE_VLM_KEY=\"file-key\"\n\
             EMERGE_VLM_MODEL='file-model'\n\nnot a pair\nEMERGE_VLM_TIMEOUT_SECS=7\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let file = dotenv(&dir);
        assert_eq!(file.get("EMERGE_VLM_KEY").map(String::as_str), Some("file-key"));
        assert_eq!(file.get("EMERGE_VLM_MODEL").map(String::as_str), Some("file-model"));
        assert_eq!(file.get("EMERGE_VLM_TIMEOUT_SECS").map(String::as_str), Some("7"));
        assert!(!file.contains_key("not a pair"));
        // Precedence, composed the way `load` composes it — without touching the real process
        // env (unsafe to mutate in edition 2024, racy under parallel tests).
        let process: std::collections::BTreeMap<&str, &str> =
            [("EMERGE_VLM_MODEL", "process-model")].into_iter().collect();
        let cfg = VlmConfig::from_lookup(|name| {
            process
                .get(name)
                .map(|v| (*v).to_owned())
                .or_else(|| file.get(name).cloned())
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg.model, "process-model", "the process env wins");
        assert_eq!(cfg.timeout_secs, 7, "the file fills what the env leaves unset");
        // A missing file is simply empty config, not an error.
        assert!(dotenv(std::path::Path::new("/nonexistent-vlm-dotenv")).is_empty());
    }

    #[test]
    fn a_missing_key_errs_with_the_remedy() {
        let e = VlmConfig::from_lookup(|_| None).err().unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("EMERGE_VLM_KEY") && e.contains(".env"), "{e}");
        let cfg = VlmConfig::from_lookup(|name| match name {
            "EMERGE_VLM_KEY" => Some("k".to_owned()),
            _ => None,
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg.url, "http://127.0.0.1:9292/v1/chat/completions");
        assert_eq!(cfg.model, "qwen3-vl-30b");
        // The redaction: the key never appears in Debug output.
        assert!(!format!("{cfg:?}").contains('k') || !format!("{cfg:?}").contains("\"k\""));
        assert!(format!("{cfg:?}").contains("<redacted>"));
    }

    // ── the loopback stub: transport + the retry loop, no external network ───────────────────────

    /// Serve `responses` in order on one listener, one HTTP/1.1 exchange each, then stop.
    fn stub(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("bind: {e}"));
        let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in responses {
                let Ok((mut sock, _)) = listener.accept() else { break };
                // Read headers + declared body; enough HTTP for a test double.
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let request = loop {
                    let Ok(n) = sock.read(&mut tmp) else { break String::new() };
                    if n == 0 {
                        break String::from_utf8_lossy(&buf).into_owned();
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    let text = String::from_utf8_lossy(&buf);
                    if let Some(head_end) = text.find("\r\n\r\n") {
                        let content_length = text
                            .lines()
                            .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                            .and_then(|l| l.split(':').nth(1))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buf.len() >= head_end + 4 + content_length {
                            break text.into_owned();
                        }
                    }
                };
                seen.push(request);
                let _ = write!(
                    sock,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            }
            seen
        });
        (format!("http://{addr}/v1/chat/completions"), handle)
    }

    fn envelope(content: &str) -> String {
        serde_json::json!({ "choices": [{ "message": { "content": content } }] }).to_string()
    }

    #[test]
    fn the_retry_loop_feeds_the_rejection_back_and_accepts_the_correction() {
        let v = vocab();
        // First answer smuggles an unknown token; the corrected second answer passes the gate.
        let (url, handle) = stub(vec![
            envelope(r#"{"what": "a wall lamp", "kind": ["sconce"]}"#),
            envelope(r#"{"what": "a wall lamp", "kind": ["light"], "effects": ["emit"]}"#),
        ]);
        let cfg = VlmConfig::for_stub(url, 5);
        let pngs = [vec![1u8, 2, 3], vec![4u8, 5, 6]];
        let (s, prov) = label_with_retry(&cfg, &pngs, &v, &ctx(), "2026-08-06".to_owned())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.kind, vec!["light".to_owned()]);
        assert_eq!(prov.attempts, 2);
        let seen = handle.join().unwrap_or_else(|_| panic!("stub died"));
        assert_eq!(seen.len(), 2);
        // The reprompt carries the gate's verdict and the model's own prior reply.
        assert!(seen[1].contains("rejected"), "no rejection fed back");
        assert!(seen[1].contains("sconce"), "the reprompt lost the prior reply");
        // The auth header and both images went out; the key is in the header ONLY.
        assert!(seen[0].contains("Bearer stub-key"));
        assert!(seen[0].matches("data:image/png;base64,").count() == 2);
    }

    #[test]
    fn a_clean_first_answer_is_one_attempt_and_a_second_rejection_is_final() {
        let v = vocab();
        let (url, handle) = stub(vec![envelope(
            r#"{"what": "a lamp", "kind": ["light"], "confidence": "high"}"#,
        )]);
        let cfg = VlmConfig::for_stub(url, 5);
        let pngs = [vec![0u8], vec![0u8]];
        let (s, prov) = label_with_retry(&cfg, &pngs, &v, &ctx(), "2026-08-06".to_owned())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(prov.attempts, 1);
        assert_eq!(s.confidence, Confidence::High);
        drop(handle);

        // Two bad answers: the gate's second verdict surfaces unchanged.
        let (url, _handle) = stub(vec![
            envelope(r#"{"what": "x", "kind": ["zzz"]}"#),
            envelope(r#"{"what": "x", "kind": ["zzz"]}"#),
        ]);
        let cfg = VlmConfig::for_stub(url, 5);
        let e = label_with_retry(&cfg, &pngs, &v, &ctx(), "2026-08-06".to_owned())
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("`kind` token"), "{e}");
    }

    #[test]
    fn endpoint_errors_surface_verbatim() {
        // An error envelope (llama-swap's model-load failure, Ollama's auth complaint) is shown,
        // not swallowed.
        let (url, _h) = stub(vec![r#"{"error": {"message": "model not found"}}"#.to_owned()]);
        let cfg = VlmConfig::for_stub(url, 5);
        let e = label_with_retry(
            &cfg,
            &[vec![0u8], vec![0u8]],
            &vocab(),
            &ctx(),
            "2026-08-06".to_owned(),
        )
        .err()
        .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("model not found"), "{e}");
    }
}
