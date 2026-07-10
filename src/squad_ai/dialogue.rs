//! Squad **dialogue** — interactions driven by *what a unit does*. A role action emits a
//! [`SquadUtterance`] (a structured observation: who, what, where); it is filed into the speaker's
//! [`MemoryStream`] (a memory-record store keyed by importance + recency, after Park et al.,
//! "Generative Agents: Interactive Simulacra of Human Behavior", 2023) and turned into a line by the
//! selected [`DialogueProvider`]. The default [`TemplateProvider`] is a deterministic role-flavoured
//! grammar (offline, testable, one path per build); [`LlmProvider`] builds a persona-grounded prompt
//! (Shanahan et al., "Role-Play with LLMs", 2023; grounded in game state per Gallotta et al. 2024) for
//! a local LLM — its transport is the opt-in integration point.
//!
//! Output is a [`SquadLine`] message (speaker + text + emotion). `crate::dialogue::bark_squad_lines`
//! adapts it to a `Bark` and the bubble layer renders it above the speaker's head. That adapter is
//! registered by `DialoguePlugin` (windowed only), so generation still runs — and stays deterministic —
//! in the headless harness, where nothing renders and a line's text never reaches `snapshot_hash`.

use bevy::prelude::*;

use crate::dialogue::Emotion;
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
    /// The affect the bubble layer tints the balloon with.
    ///
    /// A typed [`Emotion`], not a `&'static str` tone tag. Both modules live in this crate, so the string
    /// bought nothing and cost a *partial* mapping: the adapter would have had to decide what to do with a
    /// tone it didn't recognise, and "unknown tone → Neutral" is exactly the silent-default the one-path
    /// rule forbids. Typed, the mapping is total by construction and a new emotion is a compile error.
    pub emotion: Emotion,
}

/// The last few line keys a unit spoke, so the template provider can avoid repeating itself.
///
/// `TemplateProvider` picks by hashing `(speaker, event, recent memory)`, which is deterministic — a
/// virtue for replay, but it means an identical situation always yields the identical phrase. Without this
/// the squad reads as a set of voice-clip triggers rather than people.
///
/// Lives on every unit (uniform, so no archetype split) and holds no simulation state: nothing in
/// `snapshot_hash` reads it, and it is written only from `generate_dialogue` on `Update`.
#[derive(Component, Default)]
pub struct SpokenLines {
    recent: Vec<u64>,
}

/// How many of a speaker's most recent lines to avoid reusing. Held small on purpose: with a 2–3 phrase
/// table, a longer memory would exclude every candidate and force a repeat anyway.
const RECENT_LINE_MEMORY: usize = 3;

impl SpokenLines {
    /// Every remembered key, most recent last. Providers narrow this with [`avoid_window`].
    fn keys(&self) -> &[u64] {
        &self.recent
    }

    fn remember(&mut self, key: u64) {
        self.recent.push(key);
        if self.recent.len() > RECENT_LINE_MEMORY {
            self.recent.remove(0);
        }
    }
}

/// The suffix of `recent` a provider should refuse to repeat when choosing among `n_choices` phrases.
///
/// Capped at `n_choices - 1` so at least one candidate always survives the filter. Without the cap, a
/// two-phrase table plus a three-deep memory would exclude everything and the provider would quietly fall
/// back to repeating — anti-repetition that silently stops working is worse than none.
pub fn avoid_window(recent: &[u64], n_choices: usize) -> &[u64] {
    let keep = n_choices.saturating_sub(1).min(recent.len());
    &recent[recent.len() - keep..]
}

/// A stable, dependency-free key for a phrase (FNV-1a). `DefaultHasher` would also do, but its output is
/// explicitly not stable across Rust releases, and this feeds a deterministic pick.
fn line_key(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
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

/// Everything a provider needs: who is speaking, what just happened, their salient memory, and the lines
/// they have most recently spoken (so a provider can avoid repeating itself).
pub struct DialogueContext<'a> {
    pub speaker: Entity,
    pub persona: &'a Persona,
    pub event: ObsEvent,
    pub recent: Option<MemoryRecord>,
    /// Keys of this speaker's recent lines — see [`SpokenLines::avoid`]. Never excludes every candidate.
    pub recent_lines: &'a [u64],
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
        let (phrases, emotion): (&[&str], Emotion) = match (ctx.event, ctx.persona.role) {
            (ObsEvent::ExaminedBody, RoleId::Researcher) => (
                &[
                    "Post-mortem lacerations. Something fed here.",
                    "Tissue's necrotic. This one's been dead a while.",
                    "No containment tags. Unlogged specimen.",
                ],
                Emotion::Neutral,
            ),
            (ObsEvent::ExaminedObject, _) => (
                &[
                    "Nothing anomalous. Just furniture.",
                    "Residue on the surface — bag it later.",
                    "Structurally sound. Moving on.",
                ],
                Emotion::Neutral,
            ),
            (ObsEvent::SensedAnomaly, RoleId::Psionic) => (
                &[
                    "It's watching. Don't look back at it.",
                    "The signature's close. My teeth ache.",
                    "Something's wrong with the air in here.",
                ],
                Emotion::Fear,
            ),
            (ObsEvent::Communed, RoleId::Psionic) => (
                &["It knows we're here.", "...it's smiling. It's always smiling."],
                Emotion::Fear,
            ),
            (ObsEvent::Warded, RoleId::Psionic) => (
                &["Hold together. I've got us.", "Ward's up. Breathe."],
                Emotion::Calm,
            ),
            (ObsEvent::HealedAlly, RoleId::Medic) => (
                &["You're patched. Stay on me.", "Pressure's holding. Walk it off."],
                Emotion::Calm,
            ),
            (ObsEvent::ThreatSpotted, RoleId::Gunman) => (
                &["Contact. Hold your lane.", "Eyes up. I've got the angle."],
                Emotion::Anger,
            ),
            (ObsEvent::Regrouped, _) => (
                &["Forming up.", "Back on you."],
                Emotion::Neutral,
            ),
            // Any event a role has no special voice for → generic acknowledgement (still in-character
            // enough via tone). Keeps a line for every (event, role) without a giant table.
            _ => (&["Copy.", "Moving."], Emotion::Neutral),
        };
        let seed = (ctx.speaker.to_bits() as u32)
            .wrapping_mul(2_654_435_761)
            .wrapping_add(ctx.event.seed())
            .wrapping_add(ctx.recent.map(|r| r.event.seed() * 31).unwrap_or(0));
        let base = (hash01_u32(seed) * phrases.len() as f32) as usize % phrases.len();
        // Walk forward from the hashed pick to the first phrase this speaker hasn't just used. The hash
        // alone is a pure function of (speaker, event, recent memory), so an identical situation would
        // otherwise replay the identical phrase forever. `avoid_window` never excludes every candidate, so
        // this always lands on something — deterministically, keeping headless generation reproducible.
        let avoid = avoid_window(ctx.recent_lines, phrases.len());
        let pick = (0..phrases.len())
            .map(|k| (base + k) % phrases.len())
            .find(|&i| !avoid.contains(&line_key(phrases[i])))
            .unwrap_or(base);
        Some(SquadLine {
            speaker: ctx.speaker,
            text: phrases[pick].to_string(),
            emotion,
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
    mut speakers: Query<(&Persona, &mut MemoryStream, &mut SpokenLines)>,
) {
    // A monotonic tick proxy for memory timestamps (elapsed 60 Hz frames since start).
    let tick = (time.elapsed_secs() * 60.0) as u64;
    for u in utterances.read() {
        // An utterance whose speaker died (or was despawned) between the action and this system has
        // nobody to say it. Dropping it is correct, not a swallowed error.
        let Ok((persona, mut memory, mut spoken)) = speakers.get_mut(u.speaker) else {
            continue;
        };
        let recent = memory.top(tick);
        memory.push(u.event, tick);
        // Verbosity gate: a taciturn member speaks a FRACTION of the time. Folding the tick into the
        // hash gives each utterance an independent pseudo-random draw, so over many barks roughly
        // `verbosity` of them pass — a real frequency throttle. Hashing only (speaker, event) — as the
        // old gate did — is a single constant per (speaker, event-type): it made a unit's signature
        // line either fire on EVERY cooldown window forever or stay permanently mute, never "less
        // often". Still deterministic/reproducible (tick is the elapsed-frame count), and cosmetic —
        // this runs on `Update`, never entering `snapshot_hash`.
        let draw = hash01_u32(
            (u.speaker.to_bits() as u32)
                .wrapping_add(u.event.seed())
                .wrapping_add((tick as u32).wrapping_mul(0x9E37_79B1)),
        );
        if draw >= persona.verbosity {
            continue;
        }
        let ctx = DialogueContext {
            speaker: u.speaker,
            persona,
            event: u.event,
            recent,
            recent_lines: spoken.keys(),
        };
        if let Some(line) = provider.0.line(&ctx) {
            debug!("[{}] {}", persona.name, line.text);
            spoken.remember(line_key(&line.text));
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

    fn ctx<'a>(p: &'a Persona, event: ObsEvent, recent_lines: &'a [u64]) -> DialogueContext<'a> {
        DialogueContext {
            speaker: Entity::PLACEHOLDER,
            persona: p,
            event,
            recent: None,
            recent_lines,
        }
    }

    #[test]
    fn template_provider_is_deterministic_and_in_character() {
        let p = persona(RoleId::Researcher, 1.0);
        let c = ctx(&p, ObsEvent::ExaminedBody, &[]);
        let a = TemplateProvider.line(&c).unwrap();
        let b = TemplateProvider.line(&c).unwrap();
        assert_eq!(a.text, b.text); // deterministic given the same (speaker, event, memory, recent lines)
        assert!(!a.text.is_empty());
        assert_eq!(a.emotion, Emotion::Neutral); // a Researcher over a corpse is clinical
    }

    #[test]
    fn a_speaker_does_not_repeat_the_line_it_just_said() {
        // Without this, `TemplateProvider`'s hash of (speaker, event, memory) makes an identical situation
        // replay the identical phrase forever — the squad reads as voice-clip triggers, not people.
        let p = persona(RoleId::Researcher, 1.0);
        let first = TemplateProvider.line(&ctx(&p, ObsEvent::ExaminedBody, &[])).unwrap();

        let mut spoken = SpokenLines::default();
        spoken.remember(line_key(&first.text));
        let second = TemplateProvider.line(&ctx(&p, ObsEvent::ExaminedBody, spoken.keys())).unwrap();
        assert_ne!(first.text, second.text, "the Researcher repeated itself verbatim");

        // ...and it keeps finding fresh lines as its memory fills.
        spoken.remember(line_key(&second.text));
        let third = TemplateProvider.line(&ctx(&p, ObsEvent::ExaminedBody, spoken.keys())).unwrap();
        assert_ne!(third.text, first.text);
        assert_ne!(third.text, second.text);
    }

    #[test]
    fn anti_repetition_never_starves_a_small_phrase_table() {
        // `Regrouped` has only two phrases. A three-deep memory must not exclude both and leave the
        // provider with nothing — `avoid_window` caps the exclusion at `n_choices - 1`.
        let p = persona(RoleId::Gunman, 1.0);
        let mut spoken = SpokenLines::default();
        let mut seen = Vec::new();
        for _ in 0..8 {
            let line = TemplateProvider.line(&ctx(&p, ObsEvent::Regrouped, spoken.keys())).unwrap();
            spoken.remember(line_key(&line.text));
            seen.push(line.text);
        }
        // Every beat produced a line, and consecutive beats never repeated.
        assert!(seen.windows(2).all(|w| w[0] != w[1]), "consecutive repeat in {seen:?}");
        assert!(seen.iter().collect::<std::collections::HashSet<_>>().len() >= 2, "stuck on one phrase");
    }

    #[test]
    fn avoid_window_always_leaves_a_candidate() {
        // The invariant the whole scheme rests on: filtering can never exclude the entire table.
        let recent = [1, 2, 3, 4, 5];
        for n_choices in 1..=6 {
            assert!(avoid_window(&recent, n_choices).len() < n_choices.max(1));
        }
        assert!(avoid_window(&[], 3).is_empty());
    }

    #[test]
    fn spoken_lines_memory_is_bounded() {
        let mut spoken = SpokenLines::default();
        for i in 0..100 {
            spoken.remember(i);
        }
        assert_eq!(spoken.keys().len(), RECENT_LINE_MEMORY);
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
            assert!(TemplateProvider.line(&ctx(&p, event, &[])).is_some(), "no line for {event:?}");
        }
    }

    #[test]
    fn llm_prompt_grounds_persona_and_event() {
        let p = persona(RoleId::Psionic, 1.0);
        let prompt = LlmProvider::build_prompt(&ctx(&p, ObsEvent::SensedAnomaly, &[]));
        assert!(prompt.contains("Test"));
        assert!(prompt.contains("Psionic"));
        assert!(prompt.contains("SensedAnomaly"));
    }
}
