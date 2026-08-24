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

use emerge_core::descriptor::{DecalHost, Face, Mount, mount_label, mount_options};
use emerge_core::vocab::{Vocabularies, Vocabulary, nearest};

/// Where and what to ask. Endpoint, model and key are environment config so the bmb tunnel and
/// Ollama Cloud are the same code path with different values.
pub struct VlmConfig {
    pub url: String,
    pub model: String,
    key: String,
    pub timeout_secs: u64,
    /// **The generation budget, and it belongs beside the model rather than in the request.**
    ///
    /// It was a literal `800` at the POST. That number was chosen against `qwen3-vl-30b`, which
    /// answers directly — the whole budget was the JSON. `qwen3.8-27b` is a **reasoning** model
    /// (`--reasoning-format deepseek` on bmb), and the budget covers everything it generates, so
    /// the thinking is spent out of the same 800 the answer needs. Measured 2026-08-17: a
    /// one-word colour question at `max_tokens: 20` came back with **empty content and a full
    /// paragraph of `reasoning_content`** — the exact shape a truncated label would take, except
    /// across a 700-mesh batch it would look like an intermittent parse failure rather than a
    /// budget.
    ///
    /// Config rather than a constant because that is the fault this whole incident was: the model
    /// changed under a number compiled into the binary. Swapping the model again is a `.env` edit,
    /// and the budget it needs travels with it.
    pub max_tokens: u32,
    /// **Whether to let a reasoning model deliberate.** Off by default — see the measurement at
    /// the `chat_template_kwargs` site in [`request_labels`]. `EMERGE_VLM_THINK=1` turns it on.
    pub think: bool,
}

impl std::fmt::Debug for VlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key never appears in logs, errors, or panics.
        f.debug_struct("VlmConfig")
            .field("url", &self.url)
            .field("model", &self.model)
            .field("key", &"<redacted>")
            .field("timeout_secs", &self.timeout_secs)
            .field("max_tokens", &self.max_tokens)
            .field("think", &self.think)
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
    /// # Three setups, and only one of them needs a tunnel
    ///
    /// **The default URL is the SSH-tunnel form**, and it is a workaround rather than a design:
    /// `llama-server` on bmb binds to `127.0.0.1:9292`, so nothing on the LAN can reach it and the
    /// forward is what borrows it — `ssh -fN -L 9292:127.0.0.1:9292 bmb`, brought up by the human
    /// and never by this program. It is also the least portable thing here. A new machine has to
    /// have a key on bmb, and the forward cannot be raised at all while bmb is locked, because
    /// macOS declines key auth until somebody unlocks it at its own keyboard.
    ///
    /// **The portable setup is a LAN bind, and this program already speaks it.** Bind the service
    /// to the LAN on bmb, then every machine needs one line and no tunnel:
    /// `EMERGE_VLM_URL=http://192.168.1.113:9292/v1/chat/completions`. The API key is already the
    /// authentication, so this trades an SSH hop for a host firewall — worth stating out loud,
    /// since it puts the endpoint in front of everything on the subnet. [`probe`] treats a LAN
    /// address exactly like loopback, so the preflight survives the move.
    ///
    /// **Ollama Cloud is a pure config flip**, for a machine that is not on this network at all:
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
        // **The model bmb serves, as of 2026-08-17.** This said `qwen3-vl-30b` until that day, when
        // the endpoint stopped carrying it — `could not find suitable inference handler` — and the
        // batch failed with a message about the SSH forward, which was down too and hid it. A
        // default naming a model nothing serves is a second thing to keep in step with bmb; this
        // one is at least the same name the `.env` beside it uses.
        let model = get("EMERGE_VLM_MODEL")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "qwen3.8-27b".to_owned());
        // **600, because the first request of a batch pays for loading the model.** 120 was chosen
        // when the doc below said llama-swap warms "a cold model in tens of seconds". Qwen3.8-27B
        // is 31 GB at Q8 plus a 927 MB projector: measured 2026-08-17, the batch died at mesh 1
        // of 778 with `timeout: global` while `llama-server` sat at 0.1% CPU and 35 GB resident —
        // not computing, still loading. Warm, the same endpoint answers in 3 s.
        //
        // It matches llama-swap's own `ttl: 600`, which is the other half of this: ten idle
        // minutes unloads the model, so a batch that is paused long enough pays the load again
        // mid-run, and a timeout shorter than the load turns that into a failure rather than a
        // wait.
        let timeout_secs = get("EMERGE_VLM_TIMEOUT_SECS")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600);
        // Deliberation off by default; `EMERGE_VLM_THINK=1` restores it.
        let think = get("EMERGE_VLM_THINK").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
        // 2000 rather than the old 800: see [`VlmConfig::max_tokens`]. A direct-answering model
        // spends a few hundred of these and stops, so the higher ceiling costs it nothing — the
        // endpoint streams until the JSON closes, not until the budget is used.
        let max_tokens = get("EMERGE_VLM_MAX_TOKENS")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(2000);
        Ok(VlmConfig {
            url,
            model,
            key,
            timeout_secs,
            max_tokens,
            think,
        })
    }

    /// A config pointed at a test stub — visible to tests only.
    #[cfg(test)]
    fn for_stub(url: String, timeout_secs: u64) -> VlmConfig {
        VlmConfig {
            url,
            model: "stub-model".to_owned(),
            key: "stub-key".to_owned(),
            timeout_secs,
            max_tokens: 2000,
            think: false,
        }
    }
}

/// How sure the model said it was — display only, never a branch.
///
/// **Defined in `emerge-core` now**, because a descriptor carries one: since 2026-08-20 the record
/// of who wrote a label survives the keypress that applies it
/// (`emerge_core::descriptor::LabelOrigin`), and the confidence is part of that record. A type two
/// crates share belongs in the one they both depend on; re-exported here so every `vlm::Confidence`
/// in this crate still resolves, and so the JSON the model answers with parses against the same
/// three variants it always did.
pub use emerge_core::descriptor::Confidence;

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
    /// `"x"` or `"z"` — the horizontal axis of the righting turn.
    pub axis: String,
    /// **How many 90-degree quarter turns about that axis**, 1 to 3.
    ///
    /// It was one, implied, and that is only enough for a piece lying on its side. An asset
    /// authored upside down needs two, and the only way to say so was to right it once, re-shoot
    /// it, and let the model ask again — a second photograph and a second inference, about
    /// twenty-five seconds, to express a number the model already knew. Asked for at the keyboard,
    /// 2026-08-18: *"if the mesh is upside down, it can detect that, and send back a command to
    /// rotate it so many times (snapped to grid)."*
    ///
    /// **Quarter turns, because that is what the descriptor can hold**: `align.rotate` is three
    /// integer degrees stepped by `RotateAxis::bumped`, so 90 is the smallest expressible turn and
    /// 4 is where it started. 0 is spelled `needs_turn: null`.
    ///
    /// **The direction is not asked for**, deliberately. A turn is always in the `+` sense, so a
    /// piece needing a quarter turn the other way is `3` — and telling a model which visual result
    /// `+90` produces would mean stating a handedness convention this prompt cannot verify. `2` is
    /// the case that has no such ambiguity, and it is the one that was asked for. An odd turn taken
    /// the wrong way is corrected by the re-photograph the righting already performs.
    pub turns: u8,
    pub why: String,
}

/// What the model proposed, already validated against the live vocabulary — every token in these
/// lists exists, in vocabulary order, deduplicated.
///
/// Serde is derived and currently unread: the reason for it was the suggestion cache under
/// `target/` that persisted these between sessions, and that cache is gone — see `labels`' module
/// note for why a proposal no longer outlives the frame it lands in.
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
    ///
    /// **A proposal, and it loses to a measurement.** `Glb::derive_front` reads the vertex buffer,
    /// and symmetry is a property of the buffer that two three-quarter renders cannot settle, so
    /// `labels::apply_fields` fills an unmeasured front from this and refuses to overwrite a
    /// measured one.
    pub front: Option<Face>,
    pub needs_turn: Option<NeedsTurn>,
    pub note: Option<String>,
    pub rooms: Vec<String>,
    pub group: Option<String>,
    pub confidence: Confidence,
    pub token_proposals: Vec<TokenProposal>,
}

impl Suggestion {
    /// **The words this proposal offers as the piece's description**, and the only place that
    /// decides which they are.
    ///
    /// The schema asks for two things that are nearly one question — [`Suggestion::what`], *"one
    /// sentence: what real-world thing this is"*, and [`Suggestion::note`], *"one or two sentences a
    /// human author would keep"* — and models answer the first and skip the second. Measured on the
    /// running editor: `what` read *"A low-poly three-drawer dresser with a wooden finish"* while
    /// `note` was `null`, so the detail pane showed its `describe it…` placeholder with a full
    /// identification sitting underneath it in another colour. Reported from the keyboard: *"there's
    /// no intuition to be drawn from the description. And do we even need all of that?"*
    ///
    /// `what` is **not** dropped from the schema, because it is first on purpose — reasoning-first
    /// ordering is measured (Tam et al. 2024), and deleting it would be trading answer quality for a
    /// tidier panel. It simply also serves as the description when nothing better was offered.
    ///
    /// Stated once so [`crate::labels::apply_fields`] writes exactly the words the panel showed. Two
    /// readers, one fact: a panel proposing one sentence and an apply that wrote another is the
    /// class of defect the whole "one value, never two" pass exists to end.
    pub fn description(&self) -> Option<&str> {
        [self.note.as_deref(), Some(self.what.as_str())]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|s| !s.is_empty())
    }
}

/// Who said so, when, in how many attempts — the review header's facts. Never the key, never the
/// endpoint host.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    /// **The model that ANSWERED**, off the reply envelope — see [`Reply::model`]. Never
    /// [`VlmConfig::model`], which is only the name the request asked for: `llama-swap` serves what
    /// it serves, and this is the field a person reads to know whose judgement is in the library.
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
    // **"Use any that apply" invites a guess; most axes apply to almost nothing.**
    //
    // A barrel came back tagged `uses-electricity`. The token's own note already said "stops
    // working when the power does", so the model was not missing the definition — it was filling a
    // field because a field was offered. Reported from the keyboard, 2026-08-15.
    //
    // `effects` is the axis this bites hardest: it is a *functional consequence in the game*, and
    // the shipped vocabulary is four narrow behaviours. Nearly every prop has none. The general
    // "prefer an empty list over a guess" line is already in the system prompt and was not enough,
    // because it reads as advice about uncertainty rather than about the common case.
    let expectation = if title == "effects" {
        "MOST OBJECTS HAVE NONE - a barrel, a crate, a chair, a table do nothing to the world. \
         Leave this EMPTY unless the object plainly has the described behaviour. Do not infer it \
         from what the object is made of, what it might contain, or where it might be plugged in"
    } else {
        "use any that apply, and none is a normal answer"
    };
    let mut out = format!("{title} - {hint} ({expectation}):\n");
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
        Mount::OnFace { .. } => "face",
        Mount::OnWall { .. } => "wall",
        Mount::OnCeiling => "ceiling",
        Mount::Tiled => "tiled",
        Mount::InOpening { .. } => "opening",
        Mount::Decal {
            on: DecalHost::Floor,
        } => "decal_floor",
        Mount::Decal {
            on: DecalHost::Wall { .. },
        } => "decal_wall",
        Mount::Decal {
            on: DecalHost::Ceiling,
        } => "decal_ceiling",
    }
}

/// The mount options as JSON discriminants, generated from the same table the editor's `M` key
/// cycles — every offered mount is one the schema can express, named by [`mount_token`].
/// **What each mount token MEANS**, in the model's terms rather than the editor's.
///
/// The tokens stay short — they are parsed by literal, and a small model copies a short word more
/// reliably than a phrase — but short words carry no meaning on their own, and the wrong one was
/// being chosen for a reason that is obvious in hindsight: **every asset is photographed standing
/// on a neutral floor**, so "on floor" describes the picture of nearly everything. Reported from the
/// keyboard, 2026-08-15: *"we have the labels on floor and on top of the floor, but that seems to
/// confuse the VLM because most things are on top of the floor."*
///
/// So each line says what the mount is FOR — whether the piece needs a host, and which — rather
/// than where it happens to be resting in the render. The question the model has to answer is about
/// the object's nature in a real room, not about the photograph.
fn mount_meaning(m: &Mount) -> &'static str {
    match m {
        Mount::OnFloor => {
            "stands on the ground by itself in a real room - furniture, crates, \
                           machines, appliances. Do NOT choose this merely because the photo shows \
                           it on a floor; everything is photographed that way"
        }
        Mount::OnSurface { .. } => {
            "too small or unstable to stand on the ground - it belongs ON \
                                    TOP OF another piece of furniture (a mug, a lamp, a keyboard, \
                                    a book, a tray)"
        }
        Mount::OnFace { .. } => {
            "fixed flat against the vertical face of another piece - not \
                                 resting on anything"
        }
        Mount::OnWall { .. } => {
            "fixed to a room wall at a height, carrying no weight below it - a \
                                 sconce, a sign, a switch, a screen"
        }
        Mount::OnCeiling => "hangs from above - a pendant lamp, a duct, a fan",
        Mount::Tiled => {
            "a repeating surface piece that covers ground or wall in a grid - floor \
                         panels, wall panels"
        }
        Mount::InOpening { .. } => "fills a hole cut in a wall - a door leaf, a window, a grille",
        Mount::Decal {
            on: DecalHost::Floor,
        } => {
            "a flat marking painted ON the floor with no \
                                                  thickness - a line, an arrow, a stain"
        }
        Mount::Decal {
            on: DecalHost::Wall { .. },
        } => {
            "a flat marking painted ON a wall with no \
                                                       thickness - a sign, a stencil, a poster"
        }
        Mount::Decal {
            on: DecalHost::Ceiling,
        } => {
            "a flat marking painted ON the ceiling with no \
                                                    thickness"
        }
    }
}

/// **The wire shape the prompt offers for one mount** — the one place a mount's JSON example is
/// written.
///
/// It was written twice: here, for the prompt, and again inside
/// `every_offered_mount_round_trips_through_its_own_token`, which is the test that exists to prove
/// the offered set and the accepted set agree. The copies disagreed — the test's had an `OnFace`
/// arm and the prompt's did not — so the prompt advertised `{"on": "face"}`, a shape
/// [`valid_mount`] refuses for want of both its payloads, and the test went on passing. One
/// function, read by both, is the only arrangement in which that cannot recur.
fn mount_shape(m: &Mount) -> String {
    let token = mount_token(m);
    match m {
        Mount::OnSurface { class } => format!(r#"{{"on": "{token}", "class": "{class}"}}"#),
        // A face carries both halves — the class saying which face, the height saying how far up
        // it — and `valid_mount` demands both rather than inventing either.
        Mount::OnFace { class, height } => {
            format!(r#"{{"on": "{token}", "class": "{class}", "height_m": {height}}}"#)
        }
        Mount::OnWall { height }
        | Mount::Decal {
            on: DecalHost::Wall { height },
        } => format!(r#"{{"on": "{token}", "height_m": {height}}}"#),
        _ => format!(r#"{{"on": "{token}"}}"#),
    }
}

fn mount_lines(surfaces: &[String]) -> String {
    let mut out = String::from(
        "mount - what kind of support THIS asset needs (exactly one object, or null when unclear).\n\
         Every asset is photographed standing on a plain floor, so the photo cannot tell you this: \
         judge it by what the object IS.\n",
    );
    for m in mount_options(surfaces) {
        let json = mount_shape(&m);
        // The editor's own terse label AND what it means — the first is what the author sees in
        // the panel, the second is what the model needs to choose between two words that describe
        // the same photograph.
        out.push_str(&format!(
            "- {json} - {}: {}\n",
            mount_label(Some(&m)),
            mount_meaning(&m)
        ));
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
    let surface_names: Vec<String> = vocab
        .surfaces
        .tokens
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let schema = SCHEMA_EXAMPLE;
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
         needs_turn - null when the asset stands upright as authored. ONLY when it clearly does \
         not - lying on its side or back, or standing on its head - \
         {{\"axis\": \"x\"|\"z\", \"turns\": 1|2|3, \"why\": \"...\"}} names the turn that \
         would stand it up. `axis` is the horizontal axis it turns about; `turns` is HOW MANY \
         90-degree quarter turns about that axis: 2 for upside down, 1 for lying on its side, 3 \
         for a side turn the other way. If it lies on its side and you cannot tell which way it \
         should tip, say 1 - the asset is re-photographed after the turn and you will be asked \
         again. Never guess that a piece is wrong; unsure means null.\n\
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

/// **The key order the model is told to keep**, and the only place it is written.
///
/// `what` FIRST — reasoning-first ordering is measured, not style (Tam et al. 2024); a future edit
/// must not move it below the axis fields.
///
/// Hoisted out of `build_prompt` so a test can hold it against [`RawSuggestion`]'s own fields. It
/// was a local literal, which is the third way a prompt drifts from the code: add a field to the
/// parser and the example does not mention it, the model is never asked for it, and nothing fails —
/// the field simply arrives absent forever. `the_schema_example_names_every_field_the_parser_reads`
/// closes that.
pub const SCHEMA_EXAMPLE: &str = r#"{
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

/// The shape the model answers in, before the gate. Field order mirrors the schema example.
///
/// `Serialize` is derived for one reason: it makes the field list **readable by a test**, so
/// [`SCHEMA_EXAMPLE`] cannot quietly stop naming a field the parser reads. Every field carries
/// `#[serde(default)]`, so `{}` parses — which is what lets that test build a complete value
/// without a hand-written one to drift alongside.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RawMount {
    pub on: String,
    #[serde(default)]
    pub height_m: Option<f32>,
    #[serde(default)]
    pub class: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RawTurn {
    #[serde(default)]
    pub axis: String,
    /// **Required when `needs_turn` is present**, and `Option` only so its absence is a gate
    /// rejection with a sentence rather than a silent `0`. See [`NeedsTurn::turns`].
    #[serde(default)]
    pub turns: Option<u8>,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RawProposal {
    #[serde(default)]
    pub axis: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub why: String,
}

/// **What the endpoint answered** — the reply text, and the name of the model that produced it.
///
/// There was no response type at all: the envelope was walked for `choices[0].message.content` and
/// everything else was thrown away, so [`label_with_retry`] stamped its [`Provenance`] with
/// `cfg.model` — the name of the model that was **asked**. Those are two different facts the moment
/// `llama-swap` serves something other than the name it was handed (a stale `EMERGE_VLM_MODEL`, a
/// proxy that routes elsewhere), and saying whose judgement is in the library is the whole point of
/// the field.
pub struct Reply {
    /// The model's own words, still unparsed — [`parse_reply`]'s input.
    pub content: String,
    /// **The model that answered**, off the envelope's own top-level `model`.
    pub model: String,
}

/// The model's reply, and which model gave it, out of the OpenAI response envelope.
pub fn extract_reply(http_body: &str) -> Result<Reply, String> {
    let v: serde_json::Value = serde_json::from_str(http_body)
        .map_err(|e| format!("the endpoint's response is not JSON: {e}"))?;
    if let Some(err) = v.get("error") {
        // llama-swap and Ollama both put their complaint here; surface it verbatim.
        return Err(format!("the endpoint refused: {err}"));
    }
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "the response carries no choices[0].message.content".to_owned())?
        .to_owned();
    // **Refused, rather than filled in from the request.** `model` is a required member of the
    // chat-completion object every endpoint this speaks to serves, and the alternative to reading
    // it is what stood here: `cfg.model` written into a provenance record, so `library.ron` named
    // the model that was asked as though it were the one that replied. There is no honest empty
    // either — `LabelOrigin.model` of `None` is drawn as *"by hand"* by the Meshes pane, which
    // would trade one false claim for a worse one — so an envelope that names nobody is a refusal.
    let model = v["model"]
        .as_str()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| {
            "the response names no `model`, so nothing can say whose judgement this is".to_owned()
        })?
        .to_owned();
    Ok(Reply { content, model })
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
    let mount_height = |h: Option<f32>| -> Result<f32, String> {
        let h = h.ok_or_else(|| {
            format!(
                "mount on `{}` needs `height_m` — a height in {WALL_HEIGHT_RANGE:?} m",
                raw.on
            )
        })?;
        if !WALL_HEIGHT_RANGE.contains(&h) {
            return Err(format!(
                "mount on `{}` at {h} m is outside {WALL_HEIGHT_RANGE:?}",
                raw.on
            ));
        }
        Ok(h)
    };
    match raw.on.as_str() {
        "floor" => Ok(Mount::OnFloor),
        "surface" => {
            let class = raw
                .class
                .clone()
                .ok_or_else(|| {
                    format!(
                        "mount on `surface` needs `class` — the class is one of: {}",
                        surfaces.names().collect::<Vec<_>>().join(", ")
                    )
                })?;
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
        // **The face mount needs both halves, and neither is inventable.** A class the project has
        // not declared, or a height nobody stated, would be a guess written into a library entry —
        // so both are demanded rather than defaulted, exactly as `surface` demands its class.
        "face" => {
            let class = raw.class.clone().ok_or_else(|| {
                format!(
                    "mount on `face` needs `class` — the class is one of: {}",
                    surfaces.names().collect::<Vec<_>>().join(", ")
                )
            })?;
            if !surfaces.contains(&class) {
                let hint = nearest(surfaces, &class)
                    .map(|n| format!(" (did you mean `{n}`?)"))
                    .unwrap_or_default();
                return Err(format!(
                    "mount class `{class}` is not a `surfaces` token{hint}"
                ));
            }
            Ok(Mount::OnFace {
                class,
                height: mount_height(raw.height_m)?,
            })
        }
        "wall" => Ok(Mount::OnWall {
            height: mount_height(raw.height_m)?,
        }),
        "ceiling" => Ok(Mount::OnCeiling),
        "tiled" => Ok(Mount::Tiled),
        "opening" => Ok(Mount::InOpening { clear: None }),
        "decal_floor" => Ok(Mount::Decal {
            on: DecalHost::Floor,
        }),
        "decal_wall" => Ok(Mount::Decal {
            on: DecalHost::Wall {
                height: mount_height(raw.height_m)?,
            },
        }),
        "decal_ceiling" => Ok(Mount::Decal {
            on: DecalHost::Ceiling,
        }),
        // **Listed from the same table the prompt offers**, so a refusal can never name a set the
        // model was not shown — which is how a reprompt turns into an argument about a word.
        other => Err(format!(
            "`{other}` is not a mount; the options are {}",
            mount_options(
                &surfaces
                    .tokens
                    .iter()
                    .map(|t| t.name.clone())
                    .collect::<Vec<_>>()
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
        Some(t) => {
            if !matches!(t.axis.as_str(), "x" | "z") {
                return Err(format!(
                    "`{}` is not a righting axis; needs_turn.axis is \"x\" or \"z\" — a \
                     y turn changes the facing, which is `front`'s to say",
                    t.axis
                ));
            }
            // **A missing count is refused rather than assumed to be 1.** The old shape meant one
            // quarter turn by omission; carrying that forward as a default would make the two
            // answers "turn it once" and "you forgot to say" the same wire message, which is the
            // one thing a gate exists to keep apart. The reprompt costs a round trip and says
            // exactly what to add.
            let turns = match t.turns {
                Some(n @ 1..=3) => n,
                Some(other) => {
                    return Err(format!(
                        "`{other}` is not a turn count; needs_turn.turns is 1, 2 or 3 quarter \
                         turns — 4 is where it started, and 0 is spelled `needs_turn: null`"
                    ));
                }
                None => {
                    return Err(
                        "needs_turn needs a `turns` count: 1, 2 or 3 quarter turns about that \
                         axis (2 for an asset standing on its head)"
                            .to_owned(),
                    );
                }
            };
            Some(NeedsTurn {
                axis: t.axis.clone(),
                turns,
                why: t.why.clone(),
            })
        }
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
        note: raw
            .note
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty()),
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

/// **Why a labelling request produced no suggestion** — a type, because the one thing a caller
/// branches on is a fact about the failure and it was being recovered by searching the sentence.
///
/// `labels::poll_tasks` asked `e.contains("endpoint is unreachable")`, twice, to decide whether to
/// mark the link down and stop a 778-mesh walk rather than burn it. Every word of that sentence was
/// written in two places at once — [`request_labels`] and [`warm`], byte-identical — so the walk
/// only stopped while three copies of one string agreed, and nothing said so: reword one and the
/// batch quietly goes back to discovering a dead endpoint one mesh at a time, which is a failure
/// this module has already paid for once. The sentence is written exactly once now, in the
/// [`std::fmt::Display`] impl, and the question is asked of the value.
#[derive(Debug, Clone, PartialEq)]
pub enum LabelFailure {
    /// Nothing is listening. Carries the transport's own words plus the remedy for this address,
    /// when it is one whose refusal means something — see [`refusal_remedy`].
    Unreachable {
        transport: String,
        remedy: Option<String>,
    },
    /// **Everything else, in the words the author has to read**: a timeout with its advice, a
    /// non-2xx body, an envelope carrying no reply, the gate's verdict on a rejected suggestion.
    /// One variant and not five, because nothing branches on the difference — the day something
    /// does is the day to split it.
    Refused(String),
}

impl LabelFailure {
    /// **The endpoint could not be reached at all**, with this URL's remedy travelling along.
    fn unreachable(url: &str, transport: impl std::fmt::Display) -> LabelFailure {
        LabelFailure::Unreachable {
            transport: transport.to_string(),
            remedy: remedy_for_url(url),
        }
    }

    /// **Is this the failure whose remedy is "bring the endpoint up"?**
    ///
    /// The one question a caller asks of a failure, and the reason this is a type rather than a
    /// sentence: it decides whether the status band says the link is down and whether a running
    /// batch stops instead of reporting the same fault once per queued mesh.
    pub fn is_unreachable(&self) -> bool {
        matches!(self, LabelFailure::Unreachable { .. })
    }
}

impl std::fmt::Display for LabelFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LabelFailure::Unreachable {
                transport,
                remedy: Some(remedy),
            } => write!(f, "the VLM endpoint is unreachable ({transport}) — {remedy}"),
            LabelFailure::Unreachable {
                transport,
                remedy: None,
            } => write!(f, "the VLM endpoint is unreachable: {transport}"),
            LabelFailure::Refused(text) => f.write_str(text),
        }
    }
}

/// **A plain-string complaint from outside the transport is a refusal, and never the link being
/// down.** `labels::spawn_request` mixes `VlmConfig::load`'s and `encode_png`'s `String` errors in
/// with this type at one boundary; converting them here is what keeps any of them from being read
/// as an unreachable endpoint, which is precisely what a substring match over the joined text
/// allowed.
impl From<String> for LabelFailure {
    fn from(text: String) -> LabelFailure {
        LabelFailure::Refused(text)
    }
}

/// One blocking OpenAI-style chat POST. Called only from a task-pool thread — never the UI
/// thread. Returns the raw response body; envelope and JSON handling are the parsers' business.
pub fn request_labels(
    cfg: &VlmConfig,
    pngs: &[Vec<u8>; 2],
    system: &str,
    user: &str,
    retry: Option<RetryTurn<'_>>,
) -> Result<String, LabelFailure> {
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
    let mut body = serde_json::json!({
        "model": cfg.model,
        "temperature": 0.1,
        "max_tokens": cfg.max_tokens,
        "messages": messages,
    });
    // **Ask the chat template to skip the thinking, and the batch stops being an overnight job.**
    //
    // Measured 2026-08-17 against bmb, same prompt and two images, model warm:
    //
    //   thinking on             88 s   826 completion tokens
    //   `/no_think` in the text 63 s   650 tokens, 2,193 characters of reasoning — this template
    //                                  does not honour the suffix, so it is not an option
    //   enable_thinking: false   6 s    48 tokens, no reasoning at all
    //
    // Fifteen times, and over 778 meshes it is the difference between ~78 minutes and ~19 hours.
    // A closed-set classification off two pictures is recall, not deduction, so the deliberation
    // was buying very little of what it cost.
    //
    // Sent as `chat_template_kwargs`, which is a **hint to the Jinja template** rather than an API
    // parameter: a template that does not read `enable_thinking` ignores it, so this stays inert
    // against an endpoint that is not Qwen — which is why it can be on by default without making
    // the Ollama Cloud path a second configuration. `EMERGE_VLM_THINK=1` turns deliberation back
    // on for a run where the labels matter more than the wall clock.
    if !cfg.think {
        body["chat_template_kwargs"] = serde_json::json!({ "enable_thinking": false });
    }

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
        // **A timeout is not an unreachable endpoint, and saying so sent us the wrong way.** Every
        // transport error used to read "the VLM endpoint is unreachable", and `labels.rs` turns
        // that into *"bring the forward up and press Shift+L again"*. On 2026-08-17 the batch
        // stopped at 1/778 with `timeout: global` **while the forward was up** — the advice was
        // confidently wrong, and the actual cause (a 31 GB model still loading) was not something
        // the message let you reach.
        .map_err(|e| match e {
            ureq::Error::Timeout(_) => LabelFailure::Refused(format!(
                "the VLM endpoint did not answer within {}s. The forward may be fine: a cold model \
                 load on bmb costs minutes and is spent before the first mesh. Check with \
                 `curl -sS http://127.0.0.1:9292/health`, and if that answers OK, raise \
                 EMERGE_VLM_TIMEOUT_SECS rather than restarting the tunnel.",
                cfg.timeout_secs
            )),
            // **The remedy travels with the fault, on every path.** `probe` only guards the batch,
            // so without this the single `L` and the sentinel reported a bare
            // "io: Connection refused" for the exact condition `Shift+L` explains in full. The
            // sentence itself lives in `LabelFailure`'s `Display`, written once for both callers.
            other => LabelFailure::unreachable(&cfg.url, other),
        })?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("reading the VLM response failed: {e}"))?;
    if !status.is_success() {
        return Err(LabelFailure::Refused(format!(
            "the VLM endpoint answered {status}: {text}"
        )));
    }
    Ok(text)
}

/// **What to do about a refused connection — written once, for every path that can meet one.**
///
/// This text used to live inside [`probe`] alone, and `probe` is run by the **batch** and nothing
/// else. So `Shift+L` explained itself while the single `L`, the sentinel and any mid-run failure
/// fell through to `request_labels`'s generic mapping and said *"the VLM endpoint is unreachable:
/// io: Connection refused"* — the same fault, one message actionable and the other not. Reported at
/// the keyboard 2026-08-18 as *"why isn't it showing the error message?"*, which is the right
/// question to ask of an editor that had just shown it a minute earlier.
///
/// The two cases want opposite advice, which is why the remedy is a function of the address rather
/// than one sentence:
///
/// - **Loopback** means the SSH forward. The service on bmb binds to `127.0.0.1`, so nothing on the
///   LAN can reach it and the tunnel is the workaround — which is also why this is the failure a
///   machine that has never been set up hits first. A refusal here is not always the port either:
///   macOS declines key auth entirely while the host is locked ("This system is locked. To unlock
///   it, use a local account name and password"), so the forward can be impossible to raise for a
///   reason that has nothing to do with 9292.
/// - **A LAN address** means the host answered and the service did not, so the network is fine and
///   the advice is about the process.
pub fn refusal_remedy(host: &str, port: u16, loopback: bool) -> String {
    if loopback {
        format!(
            "nothing is listening on {host}:{port} — this URL is the SSH-forward setup, so bring \
             it up with `ssh -fN -L {port}:127.0.0.1:{port} bmb` (if that is refused, bmb is \
             locked — unlock it at its own keyboard first). To stop needing a tunnel at all, bind \
             the model to the LAN on bmb and set EMERGE_VLM_URL to its address."
        )
    } else {
        format!(
            "{host} is up but nothing is serving {port} — the model host is reachable, so this is \
             the service rather than the network. Start it on {host}, or check it is bound to the \
             LAN and not to 127.0.0.1."
        )
    }
}

/// **The remedy for this URL, if a refusal against it would mean anything.**
///
/// `None` for an address [`is_near`] rejects — out there a refusal folds DNS, TLS and proxies
/// together and the transport's own words are the better message.
fn remedy_for_url(url: &str) -> Option<String> {
    let (host, port) = host_port(url)?;
    if url.starts_with("https://") {
        return None;
    }
    use std::net::ToSocketAddrs;
    let ip = (host.as_str(), port).to_socket_addrs().ok()?.next()?.ip();
    is_near(ip).then(|| refusal_remedy(&host, port, ip.is_loopback()))
}

/// **Is this address one a refused connection tells the truth about?**
///
/// Loopback and this LAN, yes: nothing in between can turn a running service into a refusal, so
/// "refused" means "not serving" and the preflight is worth its 400 ms. Anything further away, no —
/// a refusal out there folds DNS, TLS, proxies and the remote's own health into one verdict, and
/// the real request's error says more than a guess would.
fn is_near(ip: std::net::IpAddr) -> bool {
    ip.is_loopback()
        || match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_unique_local() || v6.is_unicast_link_local(),
        }
}

/// **Load the model, so the batch does not pay for it inside its first real request.**
///
/// [`probe`] answers "is the socket open"; this answers "is the model resident". They are different
/// questions with very different costs: a TCP connect is instant, and a cold `qwen3.8-27b` is 31 GB
/// at Q8 plus a 927 MB projector, which is why [`VlmConfig::timeout_secs`] defaults to 600. Measured
/// 2026-08-17, that load was being paid *inside* mesh 1 of 778 — `llama-server` at 0.1 % CPU and
/// 35 GB resident, not computing, still loading — and it read as a hang and then as a timeout.
///
/// So the wait is spent here, once, where the caller can say what it is waiting for. The request is
/// the smallest one the endpoint will accept: one token, no images, no deliberation. `llama-swap`
/// hot-swaps on the model name, so naming [`VlmConfig::model`] is what makes the swap happen — the
/// content is irrelevant and deliberately so.
///
/// **Text-only, and that is a judgement rather than an oversight.** `llama-server` loads the vision
/// projector with the model it belongs to, so one text token brings both in. A warm-up carrying a
/// 1×1 PNG would exercise the projector's own first call as well; if the first *real* mesh of a
/// batch is ever measured to be much slower than the second, that is the thing to try next.
pub fn warm(cfg: &VlmConfig) -> Result<(), LabelFailure> {
    let mut body = serde_json::json!({
        "model": cfg.model,
        "temperature": 0.0,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "ready" }],
    });
    // Inert against a template that does not read it — see `request_labels` for the measurement.
    if !cfg.think {
        body["chat_template_kwargs"] = serde_json::json!({ "enable_thinking": false });
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(cfg.timeout_secs)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(&cfg.url)
        .header("Authorization", &format!("Bearer {}", cfg.key))
        .send_json(&body)
        .map_err(|e| match e {
            ureq::Error::Timeout(_) => LabelFailure::Refused(format!(
                "the model did not finish loading within {}s. Raise EMERGE_VLM_TIMEOUT_SECS — a \
                 cold 31 GB load can outlast it on a busy GPU.",
                cfg.timeout_secs
            )),
            // Same fault, same words as `request_labels` — because they are now the same words,
            // spelled in one place. They were two copies a substring match had to keep in step.
            other => LabelFailure::unreachable(&cfg.url, other),
        })?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("reading the VLM response failed: {e}"))?;
    if !status.is_success() {
        // A model name the endpoint does not serve lands here, and it is worth the whole body:
        // `could not find suitable inference handler` is what a stale EMERGE_VLM_MODEL looks like,
        // and it cost a batch once already (see `VlmConfig::from_lookup`).
        return Err(LabelFailure::Refused(format!(
            "the VLM endpoint answered {status}: {text}"
        )));
    }
    Ok(())
}

/// **Is anybody home, and are they ready?**
///
/// Three answers, because two of the failures want opposite responses from the caller and lumping
/// them together is what made a batch of 778 meshes fail 778 times in a row:
///
/// - [`Reach::Ready`] — the socket answered.
/// - [`Reach::Warming`] — something is listening but not serving yet. `llama-swap` hot-swaps models
///   on one GPU and a cold model takes tens of seconds to load, so this is an ordinary state on the
///   first request of a session, not a fault. The caller waits; it does not cancel.
/// - [`Reach::Unreachable`] — nothing is listening. Almost always the SSH tunnel, and the remedy is
///   one line, so the remedy travels with the verdict rather than being left for the reader to
///   remember.
///
/// **A TCP connect, not a chat request.** The question is whether the endpoint exists, and asking it
/// with a real inference costs a model load and up to `timeout_secs` to learn something a refused
/// socket says instantly.
#[derive(Debug, Clone, PartialEq)]
pub enum Reach {
    Ready,
    Warming(String),
    Unreachable(String),
}

/// The host and port a URL dials, for the connect probe. `None` when the URL is not one this can
/// take apart — in which case the caller should just try the request rather than refuse on a guess.
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?']).next().unwrap_or(rest);
    let https = url.starts_with("https://");
    // Strip any userinfo, then split host from port. IPv6 literals are not dialled by this tool.
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    match authority.rsplit_once(':') {
        Some((host, port)) => port.parse().ok().map(|p| (host.to_owned(), p)),
        None => Some((authority.to_owned(), if https { 443 } else { 80 })),
    }
}

/// **The preflight.** Cheap, synchronous, and run before a batch commits to a queue.
pub fn probe(cfg: &VlmConfig) -> Reach {
    let Some((host, port)) = host_port(&cfg.url) else {
        // An address this cannot parse is not evidence of anything; let the real request judge it.
        return Reach::Ready;
    };
    // A remote endpoint (Ollama Cloud) is not worth a connect probe — DNS and TLS make a refusal
    // mean several different things, and the request's own error is the better message.
    if !(host == "localhost" || host.starts_with("127.") || host == "::1") {
        return Reach::Ready;
    }
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = (host.as_str(), port).to_socket_addrs() else {
        return Reach::Unreachable(format!("`{host}` does not resolve"));
    };
    let Some(addr) = addrs.next() else {
        return Reach::Unreachable(format!("`{host}` resolves to nothing"));
    };
    // **A machine on this LAN is worth probing; a machine on the internet is not.** The old rule
    // was "loopback only", which quietly deleted the preflight for the portable setup — point
    // `EMERGE_VLM_URL` at `http://192.168.1.113:9292/...` and `probe` answered `Ready` without
    // asking anything, so a batch went back to learning the endpoint was down one mesh at a time.
    // Decided on the resolved address rather than the spelling of the host, so a name that maps to
    // the LAN is treated the same as its literal.
    let ip = addr.ip();
    if !is_near(ip) {
        return Reach::Ready;
    }
    match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(400)) {
        Ok(_) => Reach::Ready,
        // **Refused is the one worth spelling out**, and what to say depends on which setup this is.
        //
        // A loopback URL means the SSH forward: the service on bmb is bound to `127.0.0.1`, so
        // nothing on the LAN can reach it and the tunnel is the workaround. That workaround is why
        // this fails on a machine that has never been set up, and why the message names the other
        // way out — a LAN bind on bmb plus `EMERGE_VLM_URL` is one line of config per machine and
        // no tunnel at all.
        //
        // **And a refusal is not always the port.** macOS declines key auth entirely while the host
        // is locked ("This system is locked. To unlock it, use a local account name and password"),
        // so a forward can be impossible to raise for a reason that has nothing to do with 9292.
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            Reach::Unreachable(refusal_remedy(&host, port, ip.is_loopback()))
        }
        // Reachable-but-not-answering. A host that is up with a model still loading times out here,
        // and that is a wait rather than a fault.
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Reach::Warming(format!(
            "{host}:{port} is slow to answer — the model may still be loading"
        )),
        Err(e) => Reach::Unreachable(format!("{host}:{port} cannot be reached: {e}")),
    }
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
) -> Result<(Suggestion, Provenance), LabelFailure> {
    let (system, user) = build_prompt(vocab, ctx);
    let body = request_labels(cfg, pngs, &system, &user, None)?;
    let reply = extract_reply(&body)?;
    match parse_reply(&reply.content).and_then(|raw| validate(raw, vocab)) {
        Ok(s) => Ok((
            s,
            Provenance {
                // **The model that ANSWERED, not the one that was asked.** This was
                // `cfg.model.clone()`, so every record named whatever `EMERGE_VLM_MODEL` happened
                // to say — and `llama-swap` serves what it serves. `extract_reply` refuses an
                // envelope that names nobody, so this can never be a guess.
                model: reply.model,
                date,
                attempts: 1,
            },
        )),
        Err(rejection) => {
            let body = request_labels(
                cfg,
                pngs,
                &system,
                &user,
                Some(RetryTurn {
                    prior_reply: &reply.content,
                    rejection: &rejection,
                }),
            )?;
            let reply = extract_reply(&body)?;
            let s = parse_reply(&reply.content).and_then(|raw| validate(raw, vocab))?;
            Ok((
                s,
                Provenance {
                    // The SECOND answer's model, for the same reason: a swap can happen between the
                    // two turns and the record is about the answer that landed.
                    model: reply.model,
                    date,
                    attempts: 2,
                },
            ))
        }
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    /// **The address the probe dials**, including the shapes that made a naive split wrong.
    #[test]
    fn a_url_yields_the_host_and_port_it_dials() {
        assert_eq!(
            host_port("http://127.0.0.1:9292/v1/chat/completions"),
            Some(("127.0.0.1".to_owned(), 9292))
        );
        // No port: the scheme decides, which is the case Ollama Cloud takes.
        assert_eq!(
            host_port("https://ollama.com/v1/chat/completions"),
            Some(("ollama.com".to_owned(), 443))
        );
        assert_eq!(
            host_port("http://example.test/v1"),
            Some(("example.test".to_owned(), 80))
        );
        // A path containing a colon must not be read as a port.
        assert_eq!(
            host_port("http://localhost:9292/a:b"),
            Some(("localhost".to_owned(), 9292))
        );
    }

    /// **Nothing listening on a local port is a REFUSAL with a remedy**, not a mystery.
    ///
    /// The batch reported the same transport failure once per queued mesh — 778 of them — because
    /// it only ever asked whether the VLM was *configured*. Port 1 is reserved and never listening,
    /// which is a refusal every platform agrees on.
    #[test]
    fn a_dead_local_port_is_unreachable_and_says_how_to_fix_it() {
        let cfg = VlmConfig::for_stub("http://127.0.0.1:1/v1/chat/completions".to_owned(), 5);
        match probe(&cfg) {
            Reach::Unreachable(why) => {
                assert!(
                    why.contains("ssh -fN -L"),
                    "the remedy travels with the verdict: {why}"
                );
                assert!(
                    why.contains("bmb"),
                    "and names the machine the model is on: {why}"
                );
            }
            other => panic!("a dead port must refuse, got {other:?}"),
        }
    }

    /// **A remote endpoint is not probed by connecting.** DNS and TLS make a refusal mean several
    /// different things there, and the request's own error is the better message — so the batch is
    /// never blocked by a probe that cannot be trusted.
    #[test]
    fn a_remote_endpoint_is_left_to_the_request_itself() {
        let cfg = VlmConfig::for_stub("https://ollama.com/v1/chat/completions".to_owned(), 5);
        assert_eq!(
            probe(&cfg),
            Reach::Ready,
            "no connect probe for a remote host"
        );
    }

    /// **A listening socket is ready**, whatever it later says about the model.
    #[test]
    fn something_listening_reads_as_ready() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind a local test socket: {e}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|e| panic!("no local addr: {e}"))
            .port();
        let cfg = VlmConfig::for_stub(format!("http://127.0.0.1:{port}/v1/chat/completions"), 5);
        assert_eq!(probe(&cfg), Reach::Ready);
    }
}

#[cfg(test)]
mod tests {
    /// **Every path to the model explains a refusal the same way.**
    ///
    /// The remedy used to live inside `probe`, and `probe` guards the **batch** and nothing else. So
    /// `Shift+L` said what to do while the single `L` and the sentinel — the same fault, the same
    /// socket — reported "the VLM endpoint is unreachable: io: Connection refused" and stopped
    /// there. Reported at the keyboard as *"why isn't it showing the error message?"*, asked of an
    /// editor that had shown it a minute before on the other key.
    ///
    /// Pinned on the text rather than the call sites, because what matters is that an author reading
    /// either one is told the same thing.
    #[test]
    fn a_refusal_names_its_remedy_wherever_it_is_met() {
        let loopback = super::refusal_remedy("127.0.0.1", 9292, true);
        assert!(
            loopback.contains("ssh -fN -L 9292:127.0.0.1:9292 bmb") && loopback.contains("locked"),
            "the loopback remedy must name the forward AND the locked-host case, which is the one \
             that makes the forward impossible to raise: {loopback}"
        );
        assert!(
            loopback.contains("EMERGE_VLM_URL"),
            "and the way to stop needing a tunnel at all: {loopback}"
        );

        let lan = super::refusal_remedy("192.168.1.205", 9292, false);
        assert!(
            !lan.contains("ssh"),
            "a LAN host that answered is not a tunnel problem — advising one sends the reader at \
             the wrong layer: {lan}"
        );
        assert!(
            lan.contains("192.168.1.205"),
            "and it has to name the host, since the point is that THAT machine answered: {lan}"
        );

        // The far case: no remedy, because a refusal out there means too many things at once.
        assert!(
            super::remedy_for_url("https://ollama.com/v1/chat/completions").is_none(),
            "a cloud endpoint must not be handed local advice"
        );
    }

    /// **The preflight reaches the LAN, which is what makes the tunnel optional.**
    ///
    /// `probe` used to connect only for loopback and answer `Ready` for everything else, so the
    /// portable setup — the model bound to the LAN on bmb, `EMERGE_VLM_URL` pointed at its address,
    /// no SSH forward anywhere — silently lost its preflight and went back to discovering a dead
    /// endpoint one mesh at a time. Pinned on the resolved address rather than the URL's spelling,
    /// because a hostname that maps onto the LAN has to be treated as the LAN.
    #[test]
    fn the_preflight_covers_loopback_and_this_lan_but_not_the_internet() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        let v4 = |a, b, c, d| IpAddr::V4(Ipv4Addr::new(a, b, c, d));
        for near in [
            v4(127, 0, 0, 1),
            v4(192, 168, 1, 113), // bmb
            v4(10, 0, 0, 5),
            v4(172, 16, 4, 1),
            v4(169, 254, 3, 2), // link-local, which is what a .local name can land on
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            assert!(super::is_near(near), "{near} is reachable without leaving the house");
        }
        for far in [v4(1, 1, 1, 1), v4(104, 18, 0, 1), v4(172, 32, 0, 1)] {
            assert!(
                !super::is_near(far),
                "{far} is on the internet — its refusal means too many things to preflight on"
            );
        }
    }

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
            // The edge axis and the slot axis. A labeller proposes nothing on either, so both are
            // empty here — and empty is not permissive: an invented token is refused, naming the axis.
            edge: Vocabulary::default(),
            slot: Vocabulary::default(),
        }
    }

    /// [`ctx`] with a stated measurement, so the front cases read as one line each.
    fn ctx_with_front(front_measured: Option<Option<Face>>) -> PromptCtx {
        PromptCtx {
            front_measured,
            ..ctx()
        }
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
                assert!(
                    system.contains(&t.note),
                    "prompt lost note for `{}`",
                    t.name
                );
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
            let e = validate(raw(&json), &v)
                .err()
                .unwrap_or_else(|| panic!("accepted {field}"));
            assert!(e.contains(&format!("`{axis}` token")), "{field}: {e}");
            assert!(
                e.contains("implements:"),
                "{field} rejection must list the axis: {e}"
            );
        }
        // The did-you-mean rides along when the misspelling is close (nearest() is bounded at
        // len/3 edits, so `ligt` — one deletion — qualifies where a transposition would not).
        let e = validate(raw(r#"{"what": "a lamp", "kind": ["ligt"]}"#), &v)
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("did you mean `light`"), "{e}");
    }

    /// **The schema example names every field the parser reads.**
    ///
    /// The example is what the model is told to answer in. Add a field to [`RawSuggestion`] and,
    /// before this, nothing connected the two: the example would not mention it, the model would
    /// never send it, and the field would arrive absent forever with no test going red.
    ///
    /// Derived from the type rather than from a list beside it — every field carries
    /// `#[serde(default)]`, so `{}` parses into a complete value whose serialization *is* the field
    /// set. A list written here would be the same drift one level up.
    #[test]
    fn the_schema_example_names_every_field_the_parser_reads() {
        let complete: RawSuggestion =
            serde_json::from_str("{}").expect("every field defaults, so an empty object parses");
        let as_json = serde_json::to_value(&complete).expect("serializes");
        let fields = as_json.as_object().expect("an object");
        assert!(
            fields.len() >= 12,
            "sanity: the parser reads more than a couple of fields"
        );
        for key in fields.keys() {
            assert!(
                SCHEMA_EXAMPLE.contains(&format!("\"{key}\"")),
                "the prompt's schema example never mentions `{key}`, so the model is not asked for \
                 it and it will arrive absent forever. Example:\n{SCHEMA_EXAMPLE}"
            );
        }
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

    /// **Every mount the prompt OFFERS is one the parser ACCEPTS**, walked from `mount_options` and
    /// read from the prompt's own [`mount_shape`] rather than from a copy written beside it.
    ///
    /// The copy is what let this pass while the prompt advertised `{"on": "face"}` — no `class`, no
    /// `height_m`, refused by [`valid_mount`] every time, so every face-mounted mesh in a batch was
    /// lost to a reprompt arguing about a shape the model had been handed. Asserting `contains` on
    /// the assembled prompt is what makes "offered" mean offered.
    ///
    /// The identity assertion is on the **token**, not the value: `mount_options` supplies
    /// representative heights and this must not be coupled to which ones.
    #[test]
    fn every_offered_mount_round_trips_through_its_own_token() {
        let v = vocab();
        let (system, _) = build_prompt(&v, &ctx());
        let surfaces: Vec<String> = v.surfaces.tokens.iter().map(|t| t.name.clone()).collect();
        for m in mount_options(&surfaces) {
            let json = mount_shape(&m);
            assert!(
                system.contains(&json),
                "the prompt does not offer `{json}`, so the shape this test proves is not the \
                 shape the model is shown"
            );
            let full = format!(r#"{{"what": "a thing", "mount": {json}}}"#);
            let got = validate(raw(&full), &v)
                .unwrap_or_else(|e| {
                    panic!("the prompt offers `{json}` and the parser refuses it: {e}")
                })
                .mount
                .unwrap_or_else(|| panic!("`{json}` parsed to no mount at all"));
            assert_eq!(
                mount_token(&got),
                mount_token(&m),
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
                Mount::OnSurface {
                    class: "worktop".to_owned(),
                },
            ),
            (
                r#"{"on": "wall", "height_m": 2.2}"#,
                Mount::OnWall { height: 2.2 },
            ),
            (r#"{"on": "ceiling"}"#, Mount::OnCeiling),
            (r#"{"on": "tiled"}"#, Mount::Tiled),
            (r#"{"on": "opening"}"#, Mount::InOpening { clear: None }),
            (
                r#"{"on": "decal_floor"}"#,
                Mount::Decal {
                    on: DecalHost::Floor,
                },
            ),
            (
                r#"{"on": "decal_wall", "height_m": 1.5}"#,
                Mount::Decal {
                    on: DecalHost::Wall { height: 1.5 },
                },
            ),
            (
                r#"{"on": "decal_ceiling"}"#,
                Mount::Decal {
                    on: DecalHost::Ceiling,
                },
            ),
        ];
        for (json, want) in cases {
            let full = format!(r#"{{"what": "a thing", "mount": {json}}}"#);
            let got = validate(raw(&full), &v).unwrap_or_else(|e| panic!("{json}: {e}"));
            assert_eq!(got.mount, Some(want), "{json}");
        }
        for bad in [
            r#"{"on": "roof"}"#,                      // not a mount
            r#"{"on": "surface", "class": "shelf"}"#, // unknown class
            r#"{"on": "wall"}"#,                      // missing height
            r#"{"on": "wall", "height_m": 18.0}"#,    // a sconce on a chimney
        ] {
            let full = format!(r#"{{"what": "a thing", "mount": {bad}}}"#);
            assert!(validate(raw(&full), &v).is_err(), "accepted {bad}");
        }
    }

    /// **A missing class names the classes that exist** — the reprompt's only chance to correct.
    ///
    /// The gate's rejection is fed back to the model verbatim, so a message that says what is
    /// missing without saying what to put there ("mount on `surface` needs `class`") leaves a small
    /// model nothing to correct with, and the second answer fails identically. The `valid_axis`
    /// rejections already list the legal tokens; the mount arms must do the same.
    #[test]
    fn a_missing_mount_class_names_the_legal_classes() {
        let v = vocab();
        let e = validate(
            raw(r#"{"what": "a plant", "mount": {"on": "surface"}}"#),
            &v,
        )
        .err()
        .unwrap_or_else(|| panic!("accepted a classless surface mount"));
        assert!(
            e.contains("support") && e.contains("worktop"),
            "the rejection must name the legal classes: {e}"
        );
        assert!(
            e.contains("needs `class`"),
            "the rejection must still say what is missing: {e}"
        );
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
            raw(r#"{"what": "a barrel on its side", "needs_turn": {"axis": "x", "turns": 1, "why": "authored lying down"}}"#),
            &v,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let turn = got.needs_turn.unwrap_or_else(|| panic!("no turn"));
        assert_eq!(turn.axis, "x");
        assert_eq!(turn.turns, 1);

        // **Two quarter turns is the answer for an asset standing on its head** — the case the count
        // was added for. Asked for at the keyboard, 2026-08-18.
        let got = validate(
            raw(r#"{"what": "a lamp upside down", "needs_turn": {"axis": "z", "turns": 2, "why": "on its head"}}"#),
            &v,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.needs_turn.map(|t| (t.axis, t.turns)), Some(("z".to_owned(), 2)));

        // **A count outside 1..=3 is refused rather than clamped.** 4 quarter turns is where it
        // started and 0 is `needs_turn: null`, so both are answers the gate can only guess at.
        for bad in ["0", "4", "255"] {
            let full = format!(r#"{{"what": "x", "needs_turn": {{"axis": "x", "turns": {bad}, "why": ""}}}}"#);
            let e = validate(raw(&full), &v)
                .err()
                .unwrap_or_else(|| panic!("accepted turns: {bad}"));
            assert!(e.contains("turn count"), "{bad} rejected by count: {e}");
        }

        // **A missing count is refused, not read as 1.** The old wire shape meant one turn by
        // omission; keeping that as a default would make "turn it once" and "you forgot to say"
        // the same message.
        let e = validate(
            raw(r#"{"what": "x", "needs_turn": {"axis": "x", "why": "lying down"}}"#),
            &v,
        )
        .err()
        .unwrap_or_else(|| panic!("accepted a turn with no count"));
        assert!(e.contains("turns"), "the refusal names the field: {e}");

        // And the prompt asks for what the gate demands.
        let (system, _) = build_prompt(&v, &ctx());
        assert!(
            system.contains("\"turns\": 1|2|3") && system.contains("2 for upside down"),
            "the prompt states the count and what 2 means"
        );
        let e = validate(
            raw(r#"{"what": "x", "needs_turn": {"axis": "y", "why": ""}}"#),
            &v,
        )
        .err()
        .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("front"), "the y rejection points at front: {e}");
        // The prompt explains the camera geometry the face answer depends on.
        let (system, _) = build_prompt(&v, &ctx());
        assert!(
            system.contains("east (+X) and south (+Z)"),
            "camera geometry stated"
        );
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
            raw(r#"{"what": "a stove", "token_proposals": [
                    {"axis": "surfaces", "token": "worktop", "why": "already there"},
                    {"axis": "surfaces", "token": "hob", "why": "cooktops are not worktops"}
                ]}"#),
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
        assert_eq!(
            file.get("EMERGE_VLM_KEY").map(String::as_str),
            Some("file-key")
        );
        assert_eq!(
            file.get("EMERGE_VLM_MODEL").map(String::as_str),
            Some("file-model")
        );
        assert_eq!(
            file.get("EMERGE_VLM_TIMEOUT_SECS").map(String::as_str),
            Some("7")
        );
        assert!(!file.contains_key("not a pair"));
        // Precedence, composed the way `load` composes it — without touching the real process
        // env (unsafe to mutate in edition 2024, racy under parallel tests).
        let process: std::collections::BTreeMap<&str, &str> =
            [("EMERGE_VLM_MODEL", "process-model")]
                .into_iter()
                .collect();
        let cfg = VlmConfig::from_lookup(|name| {
            process
                .get(name)
                .map(|v| (*v).to_owned())
                .or_else(|| file.get(name).cloned())
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg.model, "process-model", "the process env wins");
        assert_eq!(
            cfg.timeout_secs, 7,
            "the file fills what the env leaves unset"
        );
        // A missing file is simply empty config, not an error.
        assert!(dotenv(std::path::Path::new("/nonexistent-vlm-dotenv")).is_empty());
    }

    #[test]
    fn a_missing_key_errs_with_the_remedy() {
        let e = VlmConfig::from_lookup(|_| None)
            .err()
            .unwrap_or_else(|| panic!("accepted"));
        assert!(e.contains("EMERGE_VLM_KEY") && e.contains(".env"), "{e}");
        let cfg = VlmConfig::from_lookup(|name| match name {
            "EMERGE_VLM_KEY" => Some("k".to_owned()),
            _ => None,
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg.url, "http://127.0.0.1:9292/v1/chat/completions");
        // **The name bmb answers to**, and it moved once already — `qwen3-vl-30b` until
        // 2026-08-17, when the endpoint stopped carrying it and the batch failed with a message
        // about the SSH forward. Pinned here so the default cannot drift from bmb silently: if
        // this fails, `llama-swap.yaml` moved and the `.env` beside it needs the same edit.
        assert_eq!(cfg.model, "qwen3.8-27b");
        // Big enough for a reasoning model, because that is what the default now names — the
        // budget covers the thinking as well as the JSON. See [`VlmConfig::max_tokens`].
        assert_eq!(cfg.max_tokens, 2000);
        // Long enough to cover a cold 31 GB load, not just a warm answer — the failure this
        // replaced looked like an unreachable endpoint and was a model still loading.
        assert_eq!(cfg.timeout_secs, 600);
        // **Deliberation off by default.** 88 s per mesh with it, 6 s without: over 778 meshes
        // that is 19 hours against 78 minutes.
        assert!(!cfg.think, "thinking must be opt-IN, or a batch is an overnight job");
        assert!(
            VlmConfig::from_lookup(|name| match name {
                "EMERGE_VLM_KEY" => Some("k".to_owned()),
                "EMERGE_VLM_THINK" => Some("1".to_owned()),
                _ => None,
            })
            .unwrap_or_else(|e| panic!("{e}"))
            .think,
            "a run that wants the deliberation back must be able to ask for it"
        );
        assert_eq!(
            VlmConfig::from_lookup(|name| match name {
                "EMERGE_VLM_KEY" => Some("k".to_owned()),
                "EMERGE_VLM_MAX_TOKENS" => Some("512".to_owned()),
                _ => None,
            })
            .unwrap_or_else(|e| panic!("{e}"))
            .max_tokens,
            512,
            "the budget has to be overridable, or swapping the model needs a recompile"
        );
        // The redaction: the key never appears in Debug output.
        assert!(!format!("{cfg:?}").contains('k') || !format!("{cfg:?}").contains("\"k\""));
        assert!(format!("{cfg:?}").contains("<redacted>"));
    }

    // ── the loopback stub: transport + the retry loop, no external network ───────────────────────

    /// Serve `responses` in order on one listener, one HTTP/1.1 exchange each, then stop.
    fn stub(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("bind: {e}"));
        let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in responses {
                let Ok((mut sock, _)) = listener.accept() else {
                    break;
                };
                // Read headers + declared body; enough HTTP for a test double.
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let request = loop {
                    let Ok(n) = sock.read(&mut tmp) else {
                        break String::new();
                    };
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

    /// **The model a test envelope says answered** — deliberately not [`VlmConfig::for_stub`]'s
    /// `stub-model`, so a `Provenance` built out of the REQUEST rather than the reply is a failing
    /// assertion instead of a coincidence nobody can see.
    const ANSWERED: &str = "the-model-that-answered";

    fn envelope(content: &str) -> String {
        // `model` is not decoration here: `extract_reply` refuses an envelope without one, because
        // the only other way to fill `Provenance::model` is to copy the name the request asked for.
        serde_json::json!({
            "model": ANSWERED,
            "choices": [{ "message": { "content": content } }]
        })
        .to_string()
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
        assert_eq!(
            prov.model, ANSWERED,
            "the record names the model that ANSWERED, never the one the request asked for — which \
             is `stub-model` here for exactly this reason"
        );
        let seen = handle.join().unwrap_or_else(|_| panic!("stub died"));
        assert_eq!(seen.len(), 2);
        // The reprompt carries the gate's verdict and the model's own prior reply.
        assert!(seen[1].contains("rejected"), "no rejection fed back");
        assert!(
            seen[1].contains("sconce"),
            "the reprompt lost the prior reply"
        );
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
        assert_eq!(prov.model, ANSWERED, "and on the first-answer path too");
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
        assert!(e.to_string().contains("`kind` token"), "{e}");
    }

    #[test]
    fn endpoint_errors_surface_verbatim() {
        // An error envelope (llama-swap's model-load failure, Ollama's auth complaint) is shown,
        // not swallowed.
        let (url, _h) = stub(vec![
            r#"{"error": {"message": "model not found"}}"#.to_owned(),
        ]);
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
        assert!(e.to_string().contains("model not found"), "{e}");
    }

    /// **An envelope that names no model is refused, not credited to the model we asked for.**
    ///
    /// `Provenance::model` exists to say whose judgement ends up in `library.ron`, and the request's
    /// own `model` is not evidence of who replied — it is only what was asked, which is what
    /// `cfg.model.clone()` used to write. There is no honest empty to fall back to either: the
    /// Meshes pane draws a `LabelOrigin` with no model as *"by hand"*, so a blank would claim a
    /// person did work a machine did. Refusing out loud, naming the field, is what is left.
    #[test]
    fn a_reply_that_names_no_model_is_refused_rather_than_credited_to_the_request() {
        let bare = serde_json::json!({
            "choices": [{ "message": { "content": r#"{"what": "a lamp", "kind": ["light"]}"# } }]
        })
        .to_string();
        let (url, _h) = stub(vec![bare]);
        let cfg = VlmConfig::for_stub(url, 5);
        let e = label_with_retry(
            &cfg,
            &[vec![0u8], vec![0u8]],
            &vocab(),
            &ctx(),
            "2026-08-06".to_owned(),
        )
        .err()
        .unwrap_or_else(|| panic!("a reply naming no model was accepted"));
        let said = e.to_string();
        assert!(said.contains("names no `model`"), "{said}");
        assert!(
            !said.contains("stub-model"),
            "and it must not have reached for the request's own model name: {said}"
        );
    }
}
