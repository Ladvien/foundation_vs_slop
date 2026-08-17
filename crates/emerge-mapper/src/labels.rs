//! **VLM label suggestions** — the state between "photographed" and "a human applied it".
//!
//! `label_booth` photographs a subject; this module ships the shots to the model (`vlm`), holds
//! the validated [`vlm::Suggestion`]s the review UI pre-stages, and persists them across sessions
//! so a 450-item batch survives a restart. Nothing here writes a descriptor: applying a
//! suggestion is the review UI's verb, through the Tiles tab's own mutator idiom and commit door.
//!
//! Async shape: the booth's `ShotsReady` message → one `AsyncComputeTaskPool` task per item
//! (PNG-encode → fingerprint → request with one reprompt-on-rejection → validate) → a frame
//! system polls the task and lands the result — with the [`crate::tiles::EditTarget`] stale
//! guards, because the selection can move, a rescan can drop a candidate, and a re-import can
//! reuse an id while a request is in flight. A result whose target no longer matches is dropped
//! with a status note, never written to whatever is focused instead.
//!
//! The cache follows `anim_cache`'s recipe: one RON file under the project's `target/`, entries
//! kept on load only while every fact they depend on still holds — and additionally re-validated
//! against the LIVE vocabulary, so a vocab edit invalidates exactly the suggestions that used a
//! retired token.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};

use crate::label_booth::{ShotRig, ShotsReady};
use crate::project::Project;
use crate::tiles::EditTarget;
use crate::vlm::{self, Provenance, Suggestion, VlmConfig};

/// Bumped when the cache's own shape changes.
pub const CACHE_VERSION: u32 = 1;

/// The cache file, relative to the project root — gitignored with the rest of `target/`.
pub const CACHE_PATH: &str = "target/vlm_suggestions.ron";

/// The suggestion key for a target — id for library entries, mesh path for candidates, NEVER an
/// index (the thumbs lesson: indices shift under removal and the wrong item inherits the data).
fn key_of(target: &EditTarget) -> String {
    match target {
        EditTarget::Library(id) => format!("library:{id}"),
        EditTarget::Candidate(mesh) => format!("candidate:{mesh}"),
    }
}

/// The short name a status line calls a target.
pub(crate) fn name_of(target: &EditTarget) -> &str {
    match target {
        EditTarget::Library(id) => id,
        EditTarget::Candidate(mesh) => mesh,
    }
}

/// One landed suggestion and the facts that keep it honest.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub suggestion: Suggestion,
    pub provenance: Provenance,
    /// The mesh the shots were of — a re-imported id must not inherit another mesh's labels.
    pub mesh: String,
    /// FNV-1a of the GLB's bytes at request time — a re-export invalidates.
    pub fingerprint: u64,
}

/// Every pending suggestion, keyed by target.
#[derive(Resource, Default)]
pub struct Suggestions {
    entries: BTreeMap<String, Entry>,
}

impl Suggestions {
    pub fn get(&self, target: &EditTarget) -> Option<&Entry> {
        self.entries.get(&key_of(target))
    }

    pub fn remove(&mut self, target: &EditTarget) -> Option<Entry> {
        self.entries.remove(&key_of(target))
    }

    pub fn insert(&mut self, target: &EditTarget, entry: Entry) {
        self.entries.insert(key_of(target), entry);
    }

    /// How many proposals await review — the tab badge's number.
    pub fn pending(&self) -> usize {
        self.entries.len()
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.entries.iter()
    }
}

/// **Bumped only when the suggestion set actually changes** (arrival, apply, discard, warm) — the
/// `ThumbGeneration` pattern, because the pollers take [`Suggestions`] as `ResMut` every frame and
/// `resource_changed` on it would repaint the pane continuously.
#[derive(Resource, Default)]
pub struct LabelGeneration(pub u32);

struct InFlight {
    target: EditTarget,
    mesh: String,
    task: Task<Result<(Suggestion, Provenance, u64), String>>,
}

/// The requests currently at the model.
#[derive(Resource, Default)]
pub struct LabelTasks(Vec<InFlight>);

impl LabelTasks {
    pub fn in_flight(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn holds(&self, target: &EditTarget) -> bool {
        self.0.iter().any(|f| &f.target == target)
    }
}

pub struct LabelsPlugin;

impl Plugin for LabelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Suggestions>()
            .init_resource::<LabelGeneration>()
            .init_resource::<LabelTasks>()
            .init_resource::<LabelQueue>()
            .add_systems(Startup, warm_cache)
            .add_systems(
                Update,
                (
                    suggest_labels.in_set(crate::keys::Phase::Act),
                    suggest_all.in_set(crate::keys::Phase::Act),
                    drive_batch,
                    spawn_request,
                    poll_tasks,
                    watch_sentinel,
                    save_cache.run_if(resource_changed::<LabelGeneration>),
                    paint_labels_badge.run_if(resource_changed::<LabelGeneration>),
                ),
            );
    }
}

/// Queue one target for its photo shoot — the shared tail of the `L` key, the sentinel, and the
/// righting-turn relabel, so there is exactly one way a labeling request begins.
pub(crate) fn request_photos(
    target: EditTarget,
    d: &emerge_core::descriptor::Descriptor,
    tasks: &LabelTasks,
    rig: &mut ShotRig,
) -> String {
    if tasks.holds(&target) {
        return format!("`{}` is already at the model", name_of(&target));
    }
    let Some(mesh) = d.mesh.clone() else {
        return "this piece has no mesh — nothing to photograph".to_owned();
    };
    let scale = d.align.scale.unwrap_or(1.0);
    let said = format!("photographing `{}` for labels...", name_of(&target));
    rig.push_unique(crate::label_booth::ShotJob { target, mesh, scale });
    said
}

/// **Everything the labeler holds, dropped at once** — pending suggestions, the batch queue, the
/// booth queue, and in-flight requests (dropping a Bevy task cancels it). The generation bump
/// makes `save_cache` write the now-empty set, so the on-disk cache clears with the state: one
/// key genuinely clears everything the model has tagged and everything it was about to.
pub(crate) fn clear_all_labels(
    suggestions: &mut Suggestions,
    generation: &mut LabelGeneration,
    queue: &mut LabelQueue,
    tasks: &mut LabelTasks,
    rig: &mut ShotRig,
) -> String {
    let proposals = suggestions.entries.len();
    let queued = queue.queue.len() + rig.clear_queue();
    let in_flight = tasks.0.len();
    suggestions.entries.clear();
    queue.queue.clear();
    queue.total = 0;
    tasks.0.clear();
    if proposals + queued + in_flight > 0 {
        generation.0 = generation.0.wrapping_add(1);
    }
    format!(
        "cleared {proposals} proposal(s), {queued} queued, {in_flight} in flight — applied \
         labels are undone per piece with Cmd+Z"
    )
}

/// `L`: photograph the focused piece and ask the model for labels. The refusals are loud and name
/// their remedies — an unconfigured endpoint or an empty focus is a status line, never a silent
/// no-op.
pub(crate) fn suggest_labels(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Option<Res<Project>>,
    mut state: ResMut<crate::tiles::ImportState>,
    tasks: Res<LabelTasks>,
    mut rig: ResMut<ShotRig>,
) {
    if !crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::SuggestLabels) {
        return;
    }
    let Some(project) = project else { return };
    // The config check is the verb's gate: fail here, loudly, not after a photo shoot.
    if let Err(remedy) = VlmConfig::load(&project.root) {
        state.status.note(remedy);
        return;
    }
    let Some(target) = state.target() else {
        state.status.note("nothing focused — select a piece to label".to_owned());
        return;
    };
    let Some(d) = state.placed_at_target(&target, &project) else {
        return;
    };
    let asked = request_photos(target, &d.clone(), &tasks, &mut rig);
    state.status.note(asked);
}

/// **Script driving, the devshot way**: `echo wall_light > labels.request` queues that library
/// entry through the exact production path (booth → model → gate → suggestion → cache);
/// `echo clear > labels.request` runs the clear-all. The mapper is checked by scripts and
/// measured frames (see `devshot.rs`'s module note) and the labeler is no exception — this is
/// the handle a test harness holds, since the capture path cannot run headless.
pub(crate) fn watch_sentinel(
    project: Option<Res<Project>>,
    mut state: ResMut<crate::tiles::ImportState>,
    mut suggestions: ResMut<Suggestions>,
    mut generation: ResMut<LabelGeneration>,
    mut queue: ResMut<LabelQueue>,
    mut tasks: ResMut<LabelTasks>,
    mut rig: ResMut<ShotRig>,
) {
    const REQUEST: &str = "labels.request";
    if !std::path::Path::new(REQUEST).exists() {
        return;
    }
    let content = std::fs::read_to_string(REQUEST).unwrap_or_default().trim().to_owned();
    let _ = std::fs::remove_file(REQUEST);
    if content == "clear" {
        state.status.note(clear_all_labels(
            &mut suggestions,
            &mut generation,
            &mut queue,
            &mut tasks,
            &mut rig,
        ));
        info!("labels sentinel: {}", state.status.line());
        return;
    }
    let Some(project) = project else { return };
    if let Err(remedy) = VlmConfig::load(&project.root) {
        state.status.note(remedy);
        warn!("labels sentinel: {}", state.status.line());
        return;
    }
    let Some(d) = project.library.get(&content).cloned() else {
        state.status.problem(format!("labels sentinel: no library entry named `{content}`"));
        warn!("{}", state.status.line());
        return;
    };
    state.status.note(request_photos(EditTarget::Library(content), &d, &tasks, &mut rig));
    info!("labels sentinel: {}", state.status.line());
}

/// A booth job finished: build the prompt from the CURRENT descriptor and send the whole exchange
/// to a task-pool thread. Everything the task needs moves into it — nothing async ever touches
/// ECS state.
pub(crate) fn spawn_request(
    mut ready: MessageReader<ShotsReady>,
    state: Res<crate::tiles::ImportState>,
    project: Option<Res<Project>>,
    mut tasks: ResMut<LabelTasks>,
) {
    let Some(project) = project else { return };
    for shot in ready.read() {
        // First stale guard: the target must still resolve to the mesh that was photographed.
        let Some(d) = state.placed_at_target(&shot.target, &project) else {
            continue;
        };
        if d.mesh.as_deref() != Some(shot.mesh.as_str()) {
            continue;
        }
        let ctx = prompt_ctx(d, &shot.mesh, &project);
        let vocab = project.vocab.clone();
        let images = shot.images.clone();
        let mesh_abs = project.root.join("assets").join(&shot.mesh);
        let root = project.root.clone();
        let date = date_today();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let cfg = VlmConfig::load(&root)?;
            let bytes = std::fs::read(&mesh_abs)
                .map_err(|e| format!("cannot read {}: {e}", mesh_abs.display()))?;
            let fingerprint = emerge_core::glb::fnv1a(&bytes);
            let pngs = [encode_png(&images[0])?, encode_png(&images[1])?];
            let (suggestion, provenance) =
                vlm::label_with_retry(&cfg, &pngs, &vocab, &ctx, date)?;
            Ok((suggestion, provenance, fingerprint))
        });
        tasks.0.push(InFlight {
            target: shot.target.clone(),
            mesh: shot.mesh.clone(),
            task,
        });
    }
}

/// Land finished requests — behind the stale guards, because minutes may have passed.
pub(crate) fn poll_tasks(
    mut tasks: ResMut<LabelTasks>,
    mut suggestions: ResMut<Suggestions>,
    mut generation: ResMut<LabelGeneration>,
    mut state: ResMut<crate::tiles::ImportState>,
    project: Option<Res<Project>>,
) {
    let Some(project) = project else { return };
    let mut finished = Vec::new();
    for (i, inflight) in tasks.0.iter_mut().enumerate() {
        let Some(result) = bevy::tasks::futures::check_ready(&mut inflight.task) else {
            continue;
        };
        finished.push(i);
        let name = name_of(&inflight.target).to_owned();
        match result {
            Ok((suggestion, provenance, fingerprint)) => {
                // Second stale guard: the target must STILL resolve to the same mesh.
                let still = state
                    .placed_at_target(&inflight.target, &project)
                    .is_some_and(|d| d.mesh.as_deref() == Some(inflight.mesh.as_str()));
                if !still {
                    state.status.problem(format!("labels for `{name}` arrived after it changed — dropped"));
                    continue;
                }
                let entry = Entry {
                    suggestion,
                    provenance,
                    mesh: inflight.mesh.clone(),
                    fingerprint,
                };
                // Flagged vocabulary ideas go on record at arrival — the idea outlives the
                // instance even if this suggestion is later discarded.
                record_proposals(&project.root, &entry, &name);
                suggestions.insert(&inflight.target, entry);
                generation.0 = generation.0.wrapping_add(1);
                state.status.note(format!("labels proposed for `{name}` — U applies, Y discards"));
            }
            // The gate's rejection text (axis + legal tokens) or the transport's complaint,
            // verbatim — the author decides what to do with it.
            Err(e) => {
                state.status.problem(format!("labeling `{name}` failed: {e}"));
            }
        }
    }
    for i in finished.into_iter().rev() {
        tasks.0.remove(i);
    }
}

/// `YYYY-MM-DD` (UTC) now — the adopt stamp's own date recipe.
fn date_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    emerge_core::rig_check::civil_date_utc(secs)
}

/// The booth's raw RGBA readback as PNG bytes — runs on the task thread, not in a frame.
fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let dynamic = image
        .clone()
        .try_into_dynamic()
        .map_err(|e| format!("the capture cannot become an image: {e:?}"))?;
    let mut out = std::io::Cursor::new(Vec::new());
    dynamic
        .to_rgb8()
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encoding failed: {e}"))?;
    Ok(out.into_inner())
}

/// Everything the prompt says about the subject, off the CURRENT descriptor plus the library's
/// in-use room/group names (so the model converges on existing free text instead of coining
/// synonyms).
fn prompt_ctx(
    d: &emerge_core::descriptor::Descriptor,
    mesh: &str,
    project: &Project,
) -> vlm::PromptCtx {
    let mut rooms_in_use: Vec<String> = Vec::new();
    let mut groups_in_use: Vec<String> = Vec::new();
    for other in &project.measured.descriptors {
        for r in &other.placement.rooms {
            if !rooms_in_use.contains(r) {
                rooms_in_use.push(r.clone());
            }
        }
        if let Some(g) = &other.placement.group {
            if !groups_in_use.contains(g) {
                groups_in_use.push(g.clone());
            }
        }
    }
    rooms_in_use.sort();
    groups_in_use.sort();
    vlm::PromptCtx {
        id: d.id.clone(),
        mesh: mesh.to_owned(),
        footprint: emerge_core::descriptor::placed_footprint(d),
        height: d.extent.height,
        mount_now: emerge_core::descriptor::mount_label(d.mount.as_ref()),
        kind_now: d.kind.clone(),
        effects_now: d.effects.clone(),
        look_now: d.look.clone(),
        offers_now: d.offers.surfaces.clone(),
        note_now: d.note.clone(),
        rooms_in_use,
        groups_in_use,
        // **Measured, not seen.** `derive_front` reads the vertex buffer, which is the only thing
        // that can settle symmetry; two three-quarter renders cannot. An unreadable mesh is honestly
        // `None` — the prompt then says so and leaves the judgement to the images, rather than
        // asserting a front nobody measured.
        front_measured: project
            .root
            .join("assets")
            .join(mesh)
            .pipe_open()
            .and_then(|g| g.derive_front().ok()),
    }
}

/// A tiny read-and-measure step, named so the expression above reads as one thought.
trait OpenGlb {
    fn pipe_open(&self) -> Option<emerge_core::glb::Glb>;
}

impl OpenGlb for std::path::PathBuf {
    fn pipe_open(&self) -> Option<emerge_core::glb::Glb> {
        emerge_core::glb::Glb::open(self).ok()
    }
}

// ── the token-proposals review file ──────────────────────────────────────────────────────────────

/// Where flagged vocabulary ideas wait for a human. Under `slop/` because that is dev-time prose
/// territory — **nothing loads this file**, which is the whole point: the LLM never touches
/// `vocab.ron`, and adopting a token is a person's two-file edit.
pub const PROPOSALS_PATH: &str = "slop/llm/vocab_proposals.ron";

/// A `readme` FIELD rather than a comment, because comments do not survive re-serialization.
const PROPOSALS_README: &str = "PROPOSALS ONLY - nothing loads this file. Adopting a token is a \
    human edit: append a row to assets/emerge/vocab.ron (append-only; bits come from position), \
    and for a `surfaces` token ALSO add the row to \
    crates/emerge-core/src/placement/surfaces.rs SURFACE_CLASSES. See docs/llm_rule_authoring.md.";

#[derive(serde::Serialize, serde::Deserialize)]
struct ProposalsFile {
    readme: String,
    proposals: Vec<ProposalRow>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ProposalRow {
    axis: String,
    token: String,
    why: String,
    first_seen: String,
    model: String,
    /// Which assets wanted it — grows on repeat sightings, which is itself the signal a human
    /// reads: a token five assets asked for is a different proposal than a one-off.
    assets: Vec<String>,
}

/// Merge new proposals into the file's text, deduplicated by `(axis, token)`. Pure over strings so
/// the whole policy is testable; `existing` is `None` for a first write. An existing file that no
/// longer parses is refused — overwriting a hand-annotated review file would destroy exactly the
/// notes it exists to collect.
fn merge_proposals(
    existing: Option<&str>,
    incoming: &[crate::vlm::TokenProposal],
    asset: &str,
    model: &str,
    date: &str,
) -> Result<Option<String>, String> {
    if incoming.is_empty() {
        return Ok(None);
    }
    let mut file = match existing {
        Some(text) => ron::from_str::<ProposalsFile>(text)
            .map_err(|e| format!("{PROPOSALS_PATH} no longer parses ({e}); not overwriting it"))?,
        None => ProposalsFile {
            readme: PROPOSALS_README.to_owned(),
            proposals: Vec::new(),
        },
    };
    let mut changed = false;
    for p in incoming {
        match file
            .proposals
            .iter_mut()
            .find(|row| row.axis == p.axis && row.token == p.token)
        {
            Some(row) => {
                if !row.assets.iter().any(|a| a == asset) {
                    row.assets.push(asset.to_owned());
                    changed = true;
                }
            }
            None => {
                file.proposals.push(ProposalRow {
                    axis: p.axis.clone(),
                    token: p.token.clone(),
                    why: p.why.clone(),
                    first_seen: date.to_owned(),
                    model: model.to_owned(),
                    assets: vec![asset.to_owned()],
                });
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("cannot serialize {PROPOSALS_PATH}: {e}"))?;
    Ok(Some(text))
}

/// Write a suggestion's flagged tokens into the review file — called at arrival, so a proposal is
/// on record even if the suggestion itself is later discarded (the idea outlives the instance).
fn record_proposals(root: &std::path::Path, entry: &Entry, asset: &str) {
    let path = root.join(PROPOSALS_PATH);
    let existing = std::fs::read_to_string(&path).ok();
    match merge_proposals(
        existing.as_deref(),
        &entry.suggestion.token_proposals,
        asset,
        &entry.provenance.model,
        &entry.provenance.date,
    ) {
        Ok(None) => {}
        Ok(Some(text)) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = emerge_core::ron_surgery::save_atomic(&path, &text) {
                warn!("could not write {PROPOSALS_PATH}: {e}");
            }
        }
        Err(e) => warn!("{e}"),
    }
}

// ── the batch ────────────────────────────────────────────────────────────────────────────────────

/// The walk `Shift+L` runs: targets still to photograph, and how big the walk was when it
/// started — the progress line's denominator.
#[derive(Resource, Default)]
pub struct LabelQueue {
    queue: std::collections::VecDeque<EditTarget>,
    total: usize,
}

impl LabelQueue {
    pub fn running(&self) -> bool {
        !self.queue.is_empty()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.total - self.queue.len(), self.total)
    }
}

/// Does this descriptor still need judgement? The batch's membership test: any judgement axis
/// empty, or no description. (Mount is deliberately not in the test — `unset` is common on
/// candidates and the per-item `L` covers it.)
pub(crate) fn needs_labels(d: &emerge_core::descriptor::Descriptor) -> bool {
    d.kind.is_empty() || d.effects.is_empty() || d.look.is_empty() || d.note.is_none()
}

/// `Shift+L`: walk everything missing judgement fields through the labeler — or, when a walk is
/// already running, cancel it. One key, both directions, stated in the status line.
pub(crate) fn suggest_all(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    project: Option<Res<Project>>,
    mut state: ResMut<crate::tiles::ImportState>,
    suggestions: Res<Suggestions>,
    tasks: Res<LabelTasks>,
    mut queue: ResMut<LabelQueue>,
    mut rig: ResMut<ShotRig>,
) {
    if !crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::SuggestAll) {
        return;
    }
    if queue.running() {
        let (done, total) = queue.progress();
        queue.queue.clear();
        queue.total = 0;
        let dropped = rig.clear_queue();
        state.status.problem(format!(
            "batch cancelled at {done}/{total} ({dropped} unphotographed); pending proposals keep"
        ));
        return;
    }
    let Some(project) = project else { return };
    if let Err(remedy) = VlmConfig::load(&project.root) {
        state.status.note(remedy);
        return;
    }
    let mut targets: Vec<EditTarget> = Vec::new();
    for d in &project.measured.descriptors {
        if needs_labels(d) && d.mesh.is_some() {
            targets.push(EditTarget::Library(d.id.clone()));
        }
    }
    for c in &state.candidates {
        if !c.blocked() && needs_labels(&c.proposed) {
            targets.push(EditTarget::Candidate(c.mesh.clone()));
        }
    }
    targets.retain(|t| suggestions.get(t).is_none() && !tasks.holds(t));
    if targets.is_empty() {
        state.status.note("nothing is missing labels — L re-asks for a single piece".to_owned());
        return;
    }
    queue.total = targets.len();
    queue.queue = targets.into_iter().collect();
    state.status.note(format!("labeling {} piece(s)... Shift+L cancels", queue.total));
}

/// Feed the booth one subject at a time — fully serial, matching the one-subject booth and the
/// local endpoint's single slot. NOT gated on the Tiles tab: a 450-item walk survives the author
/// switching tabs to do other work.
pub(crate) fn drive_batch(
    project: Option<Res<Project>>,
    mut state: ResMut<crate::tiles::ImportState>,
    mut queue: ResMut<LabelQueue>,
    tasks: Res<LabelTasks>,
    mut rig: ResMut<ShotRig>,
) {
    let Some(project) = project else { return };
    if queue.queue.is_empty() || !rig.is_idle() || tasks.in_flight() > 0 {
        return;
    }
    let Some(target) = queue.queue.pop_front() else { return };
    // The target may have vanished since the walk was built (a rescan, a removal) — skip, the
    // next frame takes the next one.
    let Some(d) = state.placed_at_target(&target, &project) else {
        return;
    };
    let Some(mesh) = d.mesh.clone() else { return };
    let scale = d.align.scale.unwrap_or(1.0);
    let (done, total) = queue.progress();
    state.status.note(format!("labeling {done}/{total} - `{}`", name_of(&target)));
    rig.push_unique(crate::label_booth::ShotJob { target, mesh, scale });
}

/// The Tiles tab strip carries the pending-proposal count — `anim_watch::paint_stale_badge`'s
/// shape, so the fact survives tab switches.
pub(crate) fn paint_labels_badge(
    suggestions: Option<Res<Suggestions>>,
    tabs: Query<(&crate::tiles::Tab, &Children)>,
    mut labels: Query<&mut Text, With<crate::tiles::TabLabel>>,
) {
    let Some(suggestions) = suggestions else { return };
    let pending = suggestions.pending();
    let want = if pending == 0 {
        crate::tiles::Mode::Meshes.label().to_owned()
    } else {
        format!("{} ({pending} PROPOSED)", crate::tiles::Mode::Meshes.label())
    };
    for (tab, children) in &tabs {
        if tab.0 != crate::tiles::Mode::Meshes {
            continue;
        }
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child) {
                if text.0 != want {
                    text.0 = want.clone();
                }
            }
        }
    }
}

/// **Apply a suggestion to a descriptor** — pure, so the review verb stays three lines of idiom.
///
/// Replacement semantics on the axis lists, not union: the model saw the current values in its
/// prompt and its answer is the complete proposed state — a union would make every review a merge
/// puzzle. The lists arrive already deduplicated and in vocabulary order ([`vlm::validate`]'s
/// contract, the `on_tag_chip` sort rule), so diffs show real changes only. Fields the suggestion
/// does not carry stay untouched; undo covers regret like any other edit.
pub fn apply_fields(d: &mut emerge_core::descriptor::Descriptor, s: &Suggestion) {
    d.kind = s.kind.clone();
    d.effects = s.effects.clone();
    d.look = s.look.clone();
    d.offers.surfaces = s.offers_surfaces.clone();
    if let Some(mount) = &s.mount {
        d.mount = Some(mount.clone());
    }
    // The front face is a judgement from appearance; `needs_turn` deliberately is NOT applied —
    // the righting turn is a re-measure (`tiles::rotate_mesh`), a human's key to press.
    if let Some(front) = s.front {
        d.align.front = Some(front);
    }
    if let Some(note) = &s.note {
        d.note = Some(note.clone());
    }
    if !s.rooms.is_empty() {
        d.placement.rooms = s.rooms.clone();
    }
    if let Some(group) = &s.group {
        d.placement.group = Some(group.clone());
    }
}

// ── the persistent cache ─────────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheFile {
    version: u32,
    entries: BTreeMap<String, Entry>,
}

/// Does a cached suggestion still speak the live vocabulary? A retired token invalidates exactly
/// the suggestions that used it; an appended token invalidates nothing.
fn still_valid(s: &Suggestion, vocab: &emerge_core::vocab::Vocabularies) -> bool {
    let ok = |list: &[String], v: &emerge_core::vocab::Vocabulary| {
        list.iter().all(|t| v.contains(t))
    };
    let mount_ok = match &s.mount {
        Some(emerge_core::descriptor::Mount::OnSurface { class }) => vocab.surfaces.contains(class),
        _ => true,
    };
    ok(&s.kind, &vocab.kind)
        && ok(&s.effects, &vocab.effects)
        && ok(&s.look, &vocab.look)
        && ok(&s.offers_surfaces, &vocab.surfaces)
        && mount_ok
}

/// **The load logic, pure over the filesystem root** — `anim_cache`'s shape. An entry survives
/// only while: its target still exists (library id present with the same mesh, or the candidate's
/// mesh still on disk), the GLB's bytes still hash to its fingerprint, and its tokens still exist
/// in the live vocabulary. Anything else is dropped without a complaint line.
fn warm_entries(
    root: &std::path::Path,
    measured: &emerge_core::library::Library,
    vocab: &emerge_core::vocab::Vocabularies,
) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(root.join(CACHE_PATH)) else {
        return out;
    };
    let Ok(file) = ron::from_str::<CacheFile>(&text) else {
        return out;
    };
    if file.version != CACHE_VERSION {
        return out;
    }
    for (key, entry) in file.entries {
        let target_alive = match key.split_once(':') {
            Some(("library", id)) => measured
                .descriptors
                .iter()
                .any(|d| d.id == id && d.mesh.as_deref() == Some(entry.mesh.as_str())),
            Some(("candidate", mesh)) => {
                mesh == entry.mesh && root.join("assets").join(mesh).is_file()
            }
            _ => false,
        };
        if !target_alive {
            continue;
        }
        let Ok(bytes) = std::fs::read(root.join("assets").join(&entry.mesh)) else {
            continue;
        };
        if emerge_core::glb::fnv1a(&bytes) != entry.fingerprint {
            continue;
        }
        if !still_valid(&entry.suggestion, vocab) {
            continue;
        }
        out.insert(key, entry);
    }
    out
}

/// Startup: warm the suggestion set from disk, so a batch survives a restart and the badge is
/// truthful before the tab is opened.
pub(crate) fn warm_cache(
    project: Option<Res<Project>>,
    mut suggestions: ResMut<Suggestions>,
    mut generation: ResMut<LabelGeneration>,
) {
    let Some(project) = project else { return };
    let warmed = warm_entries(&project.root, &project.measured, &project.vocab);
    if warmed.is_empty() {
        return;
    }
    suggestions.entries.extend(warmed);
    generation.0 = generation.0.wrapping_add(1);
}

/// Write-through on every real change. Entries are self-contained, so the warm bump's one
/// redundant rewrite of identical content is a no-op in effect — stated so nobody "fixes" it into
/// a gate that can miss a real change. Atomic via a pid-suffixed temp + rename (two stepped test
/// apps sharing a root must not share a temp name).
pub(crate) fn save_cache(project: Option<Res<Project>>, suggestions: Res<Suggestions>) {
    let Some(project) = project else { return };
    let file = CacheFile {
        version: CACHE_VERSION,
        entries: suggestions.iter().map(|(k, e)| (k.clone(), e.clone())).collect(),
    };
    let Ok(text) = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()) else {
        return;
    };
    let path = project.root.join(CACHE_PATH);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, &text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vlm::{Confidence, Suggestion};

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// A disposable project root with the real manifest-adjacent files this cache depends on: the
    /// library, the vocab, and one real GLB.
    fn temp_project() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vlm_labels_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let ws = workspace_root();
        std::fs::create_dir_all(dir.join("assets/characters")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::copy(
            ws.join("assets/characters/valkyrie.glb"),
            dir.join("assets/characters/valkyrie.glb"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        dir
    }

    fn vocab() -> emerge_core::vocab::Vocabularies {
        emerge_core::vocab::Vocabularies {
            kind: emerge_core::vocab::Vocabulary::of(&[("light", "casts light")]),
            effects: emerge_core::vocab::Vocabulary::of(&[("emit", "lights the room")]),
            look: emerge_core::vocab::Vocabulary::of(&[("metal", "bare metal")]),
            surfaces: emerge_core::vocab::Vocabulary::of(&[("support", "a top")]),
            capabilities: emerge_core::vocab::Vocabulary::of(&[]),
            // The edge axis and the slot axis. A labeller proposes nothing on either, so both are
            // empty here — and empty is not permissive: an invented token is refused, naming the axis.
            edge: emerge_core::vocab::Vocabulary::default(),
            slot: emerge_core::vocab::Vocabulary::default(),
        }
    }

    fn suggestion() -> Suggestion {
        Suggestion {
            what: "a wall lamp".to_owned(),
            kind: vec!["light".to_owned()],
            effects: vec!["emit".to_owned()],
            look: vec![],
            offers_surfaces: vec![],
            mount: Some(emerge_core::descriptor::Mount::OnWall { height: 1.8 }),
            front: None,
            needs_turn: None,
            note: Some("A small lamp.".to_owned()),
            rooms: vec![],
            group: None,
            confidence: Confidence::High,
            token_proposals: vec![],
        }
    }

    fn entry(root: &std::path::Path) -> Entry {
        let bytes = std::fs::read(root.join("assets/characters/valkyrie.glb"))
            .unwrap_or_else(|e| panic!("{e}"));
        Entry {
            suggestion: suggestion(),
            provenance: Provenance {
                model: "stub".to_owned(),
                date: "2026-08-06".to_owned(),
                attempts: 1,
            },
            mesh: "characters/valkyrie.glb".to_owned(),
            fingerprint: emerge_core::glb::fnv1a(&bytes),
        }
    }

    fn library_with_valkyrie() -> emerge_core::library::Library {
        let mut d = emerge_core::descriptor::Descriptor::default();
        d.id = "valkyrie".to_owned();
        d.mesh = Some("characters/valkyrie.glb".to_owned());
        emerge_core::library::Library { descriptors: vec![d], ..Default::default() }
    }

    fn write_cache_file(root: &std::path::Path, entries: BTreeMap<String, Entry>, version: u32) {
        let file = CacheFile { version, entries };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(root.join("target")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(root.join(CACHE_PATH), text).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_suggestion_round_trips_through_the_cache() {
        let root = temp_project();
        let mut entries = BTreeMap::new();
        entries.insert("library:valkyrie".to_owned(), entry(&root));
        write_cache_file(&root, entries, CACHE_VERSION);
        let warmed = warm_entries(&root, &library_with_valkyrie(), &vocab());
        let back = warmed
            .get("library:valkyrie")
            .unwrap_or_else(|| panic!("nothing warmed"));
        assert_eq!(back.suggestion, suggestion());
        assert_eq!(back.provenance.attempts, 1);
    }

    #[test]
    fn the_cache_drops_what_no_longer_holds() {
        // (a) a re-exported GLB: fingerprint mismatch.
        let root = temp_project();
        let mut entries = BTreeMap::new();
        entries.insert("library:valkyrie".to_owned(), entry(&root));
        write_cache_file(&root, entries, CACHE_VERSION);
        let glb = root.join("assets/characters/valkyrie.glb");
        let mut bytes = std::fs::read(&glb).unwrap_or_else(|e| panic!("{e}"));
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&glb, bytes).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            warm_entries(&root, &library_with_valkyrie(), &vocab()).is_empty(),
            "a re-export must drop the entry"
        );

        // (b) a retired vocabulary token: the suggestion no longer speaks the language.
        let root = temp_project();
        let mut entries = BTreeMap::new();
        entries.insert("library:valkyrie".to_owned(), entry(&root));
        write_cache_file(&root, entries, CACHE_VERSION);
        let mut small = vocab();
        small.kind = emerge_core::vocab::Vocabulary::of(&[("table", "a flat top")]);
        assert!(
            warm_entries(&root, &library_with_valkyrie(), &small).is_empty(),
            "a vocab edit must drop exactly the suggestions using the retired token"
        );

        // (c) a version bump drops the whole file; (d) a vanished target drops the entry.
        let root = temp_project();
        let mut entries = BTreeMap::new();
        entries.insert("library:valkyrie".to_owned(), entry(&root));
        write_cache_file(&root, entries, CACHE_VERSION + 1);
        assert!(warm_entries(&root, &library_with_valkyrie(), &vocab()).is_empty());

        let root = temp_project();
        let mut entries = BTreeMap::new();
        entries.insert("library:valkyrie".to_owned(), entry(&root));
        write_cache_file(&root, entries, CACHE_VERSION);
        let empty = emerge_core::library::Library { descriptors: vec![], ..Default::default() };
        assert!(
            warm_entries(&root, &empty, &vocab()).is_empty(),
            "a removed library entry must drop its suggestion"
        );
    }

    #[test]
    fn candidate_entries_live_by_their_mesh_file() {
        let root = temp_project();
        let mut entries = BTreeMap::new();
        entries.insert("candidate:characters/valkyrie.glb".to_owned(), entry(&root));
        write_cache_file(&root, entries, CACHE_VERSION);
        let empty = emerge_core::library::Library { descriptors: vec![], ..Default::default() };
        let warmed = warm_entries(&root, &empty, &vocab());
        assert_eq!(warmed.len(), 1, "a candidate's suggestion needs no library row");
        // Delete the GLB: the candidate is gone and so is its suggestion.
        std::fs::remove_file(root.join("assets/characters/valkyrie.glb"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(warm_entries(&root, &empty, &vocab()).is_empty());
    }

    /// One key clears everything the model holds: proposals, batch queue, booth queue — and the
    /// generation bump is what makes `save_cache` empty the disk file too. Idempotent: a second
    /// clear changes nothing and says so with zeros.
    #[test]
    fn clear_all_empties_every_holding_pen() {
        let root = temp_project();
        let mut suggestions = Suggestions::default();
        suggestions.insert(&EditTarget::Library("a".to_owned()), entry(&root));
        suggestions.insert(&EditTarget::Candidate("kit/b.glb".to_owned()), entry(&root));
        let mut generation = LabelGeneration::default();
        let mut queue = LabelQueue::default();
        queue.total = 2;
        queue.queue = vec![
            EditTarget::Library("c".to_owned()),
            EditTarget::Library("d".to_owned()),
        ]
        .into_iter()
        .collect();
        let mut tasks = LabelTasks::default();
        let mut rig = crate::label_booth::ShotRig::default();
        rig.push_unique(crate::label_booth::ShotJob {
            target: EditTarget::Library("e".to_owned()),
            mesh: "kit/e.glb".to_owned(),
            scale: 1.0,
        });

        let said =
            clear_all_labels(&mut suggestions, &mut generation, &mut queue, &mut tasks, &mut rig);
        assert!(said.contains("2 proposal(s)"), "{said}");
        assert!(said.contains("3 queued"), "{said}");
        assert_eq!(suggestions.pending(), 0);
        assert!(!queue.running());
        assert!(rig.is_idle());
        assert_eq!(generation.0, 1, "the bump that empties the disk cache via save_cache");

        // Idempotent, and a no-op clear does not bump the generation.
        let said =
            clear_all_labels(&mut suggestions, &mut generation, &mut queue, &mut tasks, &mut rig);
        assert!(said.contains("0 proposal(s)"), "{said}");
        assert_eq!(generation.0, 1);
    }

    /// The proposals file: dedup by (axis, token), assets accumulate, a hand-mangled file is
    /// refused rather than overwritten, and nothing at all is written for an empty proposal set.
    #[test]
    fn proposal_merging_dedups_and_refuses_a_mangled_file() {
        let hob = crate::vlm::TokenProposal {
            axis: "surfaces".to_owned(),
            token: "hob".to_owned(),
            why: "cooktops are not worktops".to_owned(),
        };
        // First write: fresh file with the readme and one row.
        let text = merge_proposals(None, &[hob.clone()], "kitchen_stove", "stub", "2026-08-06")
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("nothing written"));
        assert!(text.contains("PROPOSALS ONLY"), "the readme rides in the file");
        assert!(text.contains("hob"));
        // Second sighting from another asset: the row grows, no duplicate.
        let text2 = merge_proposals(Some(&text), &[hob.clone()], "camp_stove", "stub", "2026-08-07")
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("nothing written"));
        let parsed: ProposalsFile = ron::from_str(&text2).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parsed.proposals.len(), 1);
        assert_eq!(parsed.proposals[0].assets, vec!["kitchen_stove", "camp_stove"]);
        assert_eq!(parsed.proposals[0].first_seen, "2026-08-06", "first sighting keeps its date");
        // The same asset again: nothing changed, nothing written.
        assert!(
            merge_proposals(Some(&text2), &[hob], "camp_stove", "stub", "2026-08-08")
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
        // No proposals: no write at all.
        assert!(
            merge_proposals(None, &[], "x", "stub", "2026-08-06")
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
        // A hand-mangled file is refused, never overwritten.
        let refused = merge_proposals(
            Some("this is not ron ("),
            &[crate::vlm::TokenProposal {
                axis: "look".to_owned(),
                token: "rusty".to_owned(),
                why: "".to_owned(),
            }],
            "x",
            "stub",
            "2026-08-06",
        );
        assert!(refused.is_err());
    }

    /// The batch's membership test: any empty judgement axis or a missing note qualifies; a fully
    /// judged piece does not. Mount is deliberately not in the test.
    #[test]
    fn needs_labels_is_any_missing_judgement() {
        let mut d = emerge_core::descriptor::Descriptor::default();
        assert!(needs_labels(&d), "an unlabeled piece needs labels");
        d.kind = vec!["light".to_owned()];
        d.effects = vec!["emit".to_owned()];
        d.look = vec!["metal".to_owned()];
        d.note = Some("a lamp".to_owned());
        assert!(!needs_labels(&d), "a fully judged piece does not");
        d.look.clear();
        assert!(needs_labels(&d), "one empty axis is enough");
        d.look = vec!["metal".to_owned()];
        d.mount = None;
        assert!(!needs_labels(&d), "mount is not part of the test");
    }

    #[test]
    fn the_batch_queue_reports_progress_and_cancels_clean() {
        let mut q = LabelQueue::default();
        assert!(!q.running());
        q.total = 3;
        q.queue = vec![
            EditTarget::Library("a".to_owned()),
            EditTarget::Library("b".to_owned()),
            EditTarget::Library("c".to_owned()),
        ]
        .into_iter()
        .collect();
        assert!(q.running());
        assert_eq!(q.progress(), (0, 3));
        let _ = q.queue.pop_front();
        assert_eq!(q.progress(), (1, 3));
        q.queue.clear();
        q.total = 0;
        assert!(!q.running());
    }

    /// Apply semantics: axis lists REPLACE (the model answered with the complete proposed state —
    /// union would make review a merge puzzle); `Some`-carrying scalars overwrite; everything the
    /// suggestion does not carry is untouched.
    #[test]
    fn apply_replaces_axes_and_touches_only_what_the_suggestion_carries() {
        let mut d = emerge_core::descriptor::Descriptor::default();
        d.kind = vec!["table".to_owned()];
        d.look = vec!["worn".to_owned()];
        d.note = Some("hand-written".to_owned());
        d.mount = Some(emerge_core::descriptor::Mount::OnFloor);
        d.placement.rooms = vec!["office".to_owned()];
        d.placement.group = Some("desk_set".to_owned());

        let mut s = suggestion(); // kind: [light], effects: [emit], look: [], mount: OnWall, note: Some
        s.rooms = vec![];
        s.group = None;
        apply_fields(&mut d, &s);

        assert_eq!(d.kind, vec!["light".to_owned()], "kind replaced");
        assert_eq!(d.effects, vec!["emit".to_owned()], "effects replaced");
        assert!(d.look.is_empty(), "an empty proposed axis clears — replacement, not union");
        assert_eq!(
            d.mount,
            Some(emerge_core::descriptor::Mount::OnWall { height: 1.8 }),
            "a carried mount overwrites"
        );
        assert_eq!(d.note.as_deref(), Some("A small lamp."), "a carried note overwrites");
        // Fields the suggestion did NOT carry stay the author's.
        assert_eq!(d.placement.rooms, vec!["office".to_owned()]);
        assert_eq!(d.placement.group.as_deref(), Some("desk_set"));

        // And carried rooms/group land.
        let mut s = suggestion();
        s.rooms = vec!["kitchen".to_owned()];
        s.group = Some("cook_set".to_owned());
        apply_fields(&mut d, &s);
        assert_eq!(d.placement.rooms, vec!["kitchen".to_owned()]);
        assert_eq!(d.placement.group.as_deref(), Some("cook_set"));

        // A carried front lands on align; a flagged turn NEVER touches the descriptor HERE —
        // `suggestion_keys` intercepts it before `apply_fields` is reached, turns the piece
        // through `rotate_mesh` (the same re-measure the N/P keys run), and re-asks the model.
        let mut s = suggestion();
        s.front = Some(emerge_core::descriptor::Face::South);
        s.needs_turn = Some(crate::vlm::NeedsTurn {
            axis: "x".to_owned(),
            why: "lying down".to_owned(),
        });
        let before_align_rotate = d.align.rotate;
        let before_extent = d.extent.clone();
        apply_fields(&mut d, &s);
        assert_eq!(d.align.front, Some(emerge_core::descriptor::Face::South));
        assert_eq!(d.align.rotate, before_align_rotate, "no rotation applied");
        assert_eq!(d.extent, before_extent, "no re-measure smuggled in");
    }

    #[test]
    fn suggestion_keys_are_ids_and_mesh_paths_never_indices() {
        let mut s = Suggestions::default();
        let lib = EditTarget::Library("crt_a".to_owned());
        let cand = EditTarget::Candidate("kit/crt_b.glb".to_owned());
        assert_eq!(key_of(&lib), "library:crt_a");
        assert_eq!(key_of(&cand), "candidate:kit/crt_b.glb");
        let root = temp_project();
        s.insert(&lib, entry(&root));
        assert!(s.get(&lib).is_some());
        assert!(s.get(&cand).is_none());
        assert_eq!(s.pending(), 1);
        assert!(s.remove(&lib).is_some());
        assert_eq!(s.pending(), 0);
    }
}
