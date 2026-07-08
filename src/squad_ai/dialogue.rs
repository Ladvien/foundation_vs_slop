//! Squad **dialogue** — interactions driven by *what a unit does*. A role action emits a
//! [`SquadUtterance`] (a structured observation: who, what, where); it is filed into the speaker's
//! [`MemoryStream`] (a memory-record store keyed by importance + recency, after Park et al.,
//! "Generative Agents: Interactive Simulacra of Human Behavior", 2023) and turned into a line by the
//! selected [`DialogueProvider`]. The default [`TemplateProvider`] is a deterministic role-flavoured
//! grammar (offline, testable, one path per build); [`LlmProvider`] builds a persona-grounded prompt
//! (Shanahan et al., "Role-Play with LLMs", 2023; grounded in game state per Gallotta et al. 2024) for
//! a local LLM — its transport is the opt-in integration point.
//!
//! Output is a self-contained [`SquadLine`] event (speaker + text + emotion), so this compiles on
//! `main` without the dialogue-bubble crate (PR #12). A thin adapter maps `SquadLine` → that crate's
//! `Bark` when it lands — the render path is content-agnostic (`build_bubble(&str)`).

use bevy::prelude::*;

use crate::util::hash01_u32;

use super::persona::Persona;
use super::role::RoleId;

/// What a unit just did/observed — the semantic content a line is generated from. Compact + `Copy` so
/// it rides in memory records and events cheaply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsEvent {
    /// Researcher/Engineer studied furniture or machinery.
    ExaminedObject,
    /// Researcher studied a creature corpse.
    ExaminedBody,
    /// Psionic sensed the watcher's anomaly signature.
    SensedAnomaly,
    /// Psionic communed with / read the watcher.
    Communed,
    /// Psionic warded the squad (steadied morale).
    Warded,
    /// Medic stabilised a wounded ally.
    HealedAlly,
    /// Gunman acquired / called a threat bearing.
    ThreatSpotted,
    /// A unit strayed and rejoined the squad.
    Regrouped,
}

impl ObsEvent {
    /// Salience `[0,1]` — how memorable/likely-to-be-spoken this is (Park et al. importance score).
    pub fn importance(self) -> f32 {
        match self {
            ObsEvent::SensedAnomaly => 0.9,
            ObsEvent::ExaminedBody => 0.8,
            ObsEvent::ThreatSpotted => 0.7,
            ObsEvent::HealedAlly => 0.6,
            ObsEvent::Communed => 0.6,
            ObsEvent::ExaminedObject => 0.5,
            ObsEvent::Warded => 0.4,
            ObsEvent::Regrouped => 0.2,
        }
    }

    /// A stable discriminant for deterministic line selection (not `as usize`-fragile across edits).
    fn seed(self) -> u32 {
        match self {
            ObsEvent::ExaminedObject => 1,
            ObsEvent::ExaminedBody => 2,
            ObsEvent::SensedAnomaly => 3,
            ObsEvent::Communed => 4,
            ObsEvent::Warded => 5,
            ObsEvent::HealedAlly => 6,
            ObsEvent::ThreatSpotted => 7,
            ObsEvent::Regrouped => 8,
        }
    }
}

/// Emitted by the actions layer when a unit does something worth remarking on.
#[derive(Message, Clone, Copy)]
pub struct SquadUtterance {
    pub speaker: Entity,
    pub role: RoleId,
    pub event: ObsEvent,
    /// Where the subject is (a corpse, an anomaly bearing), if spatial.
    pub subject: Option<Vec3>,
}

/// The generated, ready-to-render line. Consumed by the dialogue-bubble adapter (→ `Bark`).
#[derive(Message, Clone)]
pub struct SquadLine {
    pub speaker: Entity,
    pub text: String,
    /// A coarse tone tag the bubble layer can map to its `Emotion` (kept as a string to avoid a
    /// compile dependency on the dialogue crate).
    pub tone: &'static str,
}

/// One remembered observation. A trimmed [`SquadUtterance`] plus when it happened.
#[derive(Clone, Copy)]
pub struct MemoryRecord {
    pub event: ObsEvent,
    pub tick: u64,
    pub importance: f32,
}

/// A unit's memory stream — a bounded, recency-ordered store of what it has observed (Park et al.).
/// Retrieval combines recency + importance; dialogue and (later) reflection read it.
#[derive(Component, Default)]
pub struct MemoryStream {
    records: Vec<MemoryRecord>,
}

/// Keep memory bounded so it never grows without limit over a long run.
const MEMORY_CAP: usize = 64;

impl MemoryStream {
    pub fn push(&mut self, event: ObsEvent, tick: u64) {
        self.records.push(MemoryRecord { event, tick, importance: event.importance() });
        if self.records.len() > MEMORY_CAP {
            self.records.remove(0);
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The most salient recent record: importance blended with recency (Park et al. retrieval). Used to
    /// give a generated line memory context ("still no sign of the specimen").
    pub fn top(&self, now: u64) -> Option<MemoryRecord> {
        self.records
            .iter()
            .copied()
            .max_by(|a, b| score(a, now).total_cmp(&score(b, now)))
    }
}

/// Retrieval score: importance + a recency term decaying over ~600 ticks (10 s at 60 Hz).
fn score(r: &MemoryRecord, now: u64) -> f32 {
    let age = now.saturating_sub(r.tick) as f32;
    r.importance + (1.0 - (age / 600.0)).max(0.0) * 0.5
}

/// The dialogue generation interface — one path per build (template or LLM), no runtime fallback.
pub trait DialogueProvider: Send + Sync {
    /// Produce a line for `ctx`, or `None` to stay silent this beat.
    fn line(&self, ctx: &DialogueContext) -> Option<SquadLine>;
}

/// Everything a provider needs: who is speaking, what just happened, and their salient memory.
pub struct DialogueContext<'a> {
    pub speaker: Entity,
    pub persona: &'a Persona,
    pub event: ObsEvent,
    pub recent: Option<MemoryRecord>,
}

/// The selected provider, inserted at startup by config (default = [`TemplateProvider`]).
#[derive(Resource)]
pub struct ActiveDialogueProvider(pub Box<dyn DialogueProvider>);

impl Default for ActiveDialogueProvider {
    fn default() -> Self {
        ActiveDialogueProvider(Box::new(TemplateProvider))
    }
}

/// Deterministic role-flavoured grammar. Picks a phrase by hashing the speaker + event + persona, so a
/// given situation always yields the same line (the harness can exact-check it, and it never needs a
/// network). SCP Mobile-Task-Force voice.
pub struct TemplateProvider;

impl DialogueProvider for TemplateProvider {
    fn line(&self, ctx: &DialogueContext) -> Option<SquadLine> {
        let (phrases, tone): (&[&str], &str) = match (ctx.event, ctx.persona.role) {
            (ObsEvent::ExaminedBody, RoleId::Researcher) => (
                &[
                    "Post-mortem lacerations. Something fed here.",
                    "Tissue's necrotic. This one's been dead a while.",
                    "No containment tags. Unlogged specimen.",
                ],
                "clinical",
            ),
            (ObsEvent::ExaminedObject, _) => (
                &[
                    "Nothing anomalous. Just furniture.",
                    "Residue on the surface — bag it later.",
                    "Structurally sound. Moving on.",
                ],
                "flat",
            ),
            (ObsEvent::SensedAnomaly, RoleId::Psionic) => (
                &[
                    "It's watching. Don't look back at it.",
                    "The signature's close. My teeth ache.",
                    "Something's wrong with the air in here.",
                ],
                "afraid",
            ),
            (ObsEvent::Communed, RoleId::Psionic) => (
                &["It knows we're here.", "...it's smiling. It's always smiling."],
                "afraid",
            ),
            (ObsEvent::Warded, RoleId::Psionic) => (
                &["Hold together. I've got us.", "Ward's up. Breathe."],
                "calm",
            ),
            (ObsEvent::HealedAlly, RoleId::Medic) => (
                &["You're patched. Stay on me.", "Pressure's holding. Walk it off."],
                "steady",
            ),
            (ObsEvent::ThreatSpotted, RoleId::Gunman) => (
                &["Contact. Hold your lane.", "Eyes up. I've got the angle."],
                "clipped",
            ),
            (ObsEvent::Regrouped, _) => (
                &["Forming up.", "Back on you."],
                "flat",
            ),
            // Any event a role has no special voice for → generic acknowledgement (still in-character
            // enough via tone). Keeps a line for every (event, role) without a giant table.
            _ => (&["Copy.", "Moving."], "flat"),
        };
        let seed = (ctx.speaker.to_bits() as u32)
            .wrapping_mul(2_654_435_761)
            .wrapping_add(ctx.event.seed())
            .wrapping_add(ctx.recent.map(|r| r.event.seed() * 31).unwrap_or(0));
        let pick = (hash01_u32(seed) * phrases.len() as f32) as usize % phrases.len();
        Some(SquadLine {
            speaker: ctx.speaker,
            text: phrases[pick].to_string(),
            tone,
        })
    }
}

/// Builds a persona-grounded prompt for a local LLM (Shanahan role-play preamble + game-state
/// grounding). The *transport* (an async POST to the bmb llama-swap endpoint) is the opt-in
/// integration; the prompt construction is pure and tested here so the grounding is verifiable offline.
pub struct LlmProvider;

impl LlmProvider {
    /// The system/user prompt a request would carry. Grounds the model in persona + situation so it
    /// stays in character and on-world (Gallotta et al. 2024).
    pub fn build_prompt(ctx: &DialogueContext) -> String {
        let mem = match ctx.recent {
            Some(r) => format!(" Recently: {:?}.", r.event),
            None => String::new(),
        };
        format!(
            "You are {name}, the {role:?} of an SCP Mobile Task Force. Temperament: {temp}. \
             You just observed: {event:?}.{mem} Say one short, in-character line (max 12 words).",
            name = ctx.persona.name,
            role = ctx.persona.role,
            temp = ctx.persona.temperament,
            event = ctx.event,
        )
    }
}

/// Turn each [`SquadUtterance`] into a spoken [`SquadLine`] via the active provider, filing it into the
/// speaker's memory first so the line can reference context. Cosmetic → `Update` (never pinned; a
/// line's text must never enter `snapshot_hash`).
pub fn generate_dialogue(
    time: Res<Time>,
    provider: Res<ActiveDialogueProvider>,
    mut utterances: MessageReader<SquadUtterance>,
    mut lines: MessageWriter<SquadLine>,
    mut speakers: Query<(&Persona, &mut MemoryStream)>,
) {
    // A monotonic tick proxy for memory timestamps (elapsed 60 Hz frames since start).
    let tick = (time.elapsed_secs() * 60.0) as u64;
    for u in utterances.read() {
        let Ok((persona, mut memory)) = speakers.get_mut(u.speaker) else {
            continue;
        };
        let recent = memory.top(tick);
        memory.push(u.event, tick);
        // Verbosity gate: a taciturn member speaks less. Deterministic per (speaker, event) so it's
        // reproducible — not a random suppression.
        let chatty =
            hash01_u32((u.speaker.to_bits() as u32).wrapping_add(u.event.seed())) < persona.verbosity;
        if !chatty {
            continue;
        }
        let ctx = DialogueContext { speaker: u.speaker, persona, event: u.event, recent };
        if let Some(line) = provider.0.line(&ctx) {
            debug!("[{}] {}", persona.name, line.text);
            lines.write(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona(role: RoleId, verbosity: f32) -> Persona {
        Persona {
            name: "Test".into(),
            role,
            temperament: "clinical".into(),
            verbosity,
        }
    }

    #[test]
    fn importance_orders_events() {
        assert!(ObsEvent::SensedAnomaly.importance() > ObsEvent::Regrouped.importance());
    }

    #[test]
    fn memory_is_bounded_and_retrieves_salient() {
        let mut m = MemoryStream::default();
        for t in 0..100 {
            m.push(ObsEvent::Regrouped, t);
        }
        assert!(m.len() <= MEMORY_CAP);
        m.push(ObsEvent::SensedAnomaly, 100);
        // The high-importance recent record wins retrieval.
        assert_eq!(m.top(100).map(|r| r.event), Some(ObsEvent::SensedAnomaly));
    }

    #[test]
    fn template_provider_is_deterministic_and_in_character() {
        let p = persona(RoleId::Researcher, 1.0);
        let ctx = DialogueContext {
            speaker: Entity::PLACEHOLDER,
            persona: &p,
            event: ObsEvent::ExaminedBody,
            recent: None,
        };
        let a = TemplateProvider.line(&ctx).unwrap();
        let b = TemplateProvider.line(&ctx).unwrap();
        assert_eq!(a.text, b.text); // deterministic
        assert!(!a.text.is_empty());
        assert_eq!(a.tone, "clinical");
    }

    #[test]
    fn every_event_yields_a_line() {
        let p = persona(RoleId::Gunman, 1.0);
        for event in [
            ObsEvent::ExaminedObject,
            ObsEvent::ExaminedBody,
            ObsEvent::SensedAnomaly,
            ObsEvent::Communed,
            ObsEvent::Warded,
            ObsEvent::HealedAlly,
            ObsEvent::ThreatSpotted,
            ObsEvent::Regrouped,
        ] {
            let ctx = DialogueContext { speaker: Entity::PLACEHOLDER, persona: &p, event, recent: None };
            assert!(TemplateProvider.line(&ctx).is_some(), "no line for {event:?}");
        }
    }

    #[test]
    fn llm_prompt_grounds_persona_and_event() {
        let p = persona(RoleId::Psionic, 1.0);
        let ctx = DialogueContext {
            speaker: Entity::PLACEHOLDER,
            persona: &p,
            event: ObsEvent::SensedAnomaly,
            recent: None,
        };
        let prompt = LlmProvider::build_prompt(&ctx);
        assert!(prompt.contains("Test"));
        assert!(prompt.contains("Psionic"));
        assert!(prompt.contains("SensedAnomaly"));
    }
}
