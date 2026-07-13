//! Conversation runtime: walks an authored graph, spawns speaker/choice bubbles, and turns the
//! player's clicks into branch choices.
//!
//! A modal conversation freezes the sim by entering [`MenuState::Conversation`] (reusing the single
//! `SimBlocked` writer in `ui::state`) and installs [`ConversationLock`] so `selection::command_input`
//! yields the left-click for the duration — exactly one owner of the click at a time. Choice bubbles
//! are clickable 3D quads (`MeshPickingPlugin` + `Pickable` + a per-entity `On<Pointer<Click>>`
//! observer), mirroring the UI-button `On<Activate>` convention. Ambient [`Bark`]s share the same
//! renderer but never block. In-world dialogue keeps the exchange spatial rather than on a screen HUD,
//! which raises immersion (Kennedy et al., *Removing the HUD*, 2015, DOI 10.1145/2793107.2793120).

use std::collections::VecDeque;

use bevy::picking::Pickable;
use bevy::picking::events::{Click, Out, Over, Pointer};
use bevy::prelude::*;
use bevy::time::Real;

use crate::squad::{Leader, SquadMember, Unit};
use crate::ui::state::MenuState;

use super::bubble::{
    Bubble, BubbleAssets, BubbleStyle, BubbleTtl, build_bubble, dwell_secs,
};
use super::model::{BubbleKind, DialogueScript, Emotion, Node};

/// Start (or ignore, if one is already running) the named conversation.
#[derive(Message)]
pub struct StartConversation {
    pub id: String,
}

/// Fire a one-off ambient bubble over a squad member — barks, reactions, LLM chatter later. Never
/// blocks play.
#[derive(Message, Clone)]
pub struct Bark {
    pub speaker: usize,
    pub kind: BubbleKind,
    pub emotion: Emotion,
    pub text: String,
}

/// Internal: a choice option was clicked (index within the current choice node).
#[derive(Message)]
struct ChoicePicked {
    index: usize,
}

/// Present while a conversation is active. Its presence makes `selection::command_input` yield the
/// left-click (harness-safe: the resource simply never exists in the headless build).
#[derive(Resource)]
pub struct ConversationLock;

/// The live conversation cursor.
#[derive(Resource)]
struct Active {
    conv: String,
    node: String,
    /// Set once the current node's bubbles are on screen (so a node is presented exactly once).
    presented: bool,
    /// World time at which a modal line auto-advances (lines only).
    advance_at: f32,
}

/// Marks every bubble owned by the current conversation, so a node transition / end can clear them.
#[derive(Component)]
struct ConversationBubble;

/// Marks a bubble that lingers on a timer (ambient barks) — never a conversation bubble.
#[derive(Component)]
struct AmbientBubble {
    owner: Entity,
}

/// Vertical gap (world units) between stacked choice bubbles and above the leader's head.
const CHOICE_GAP: f32 = 0.12;
const CHOICE_BASE: f32 = 0.15;

/// Minimum seconds between two ambient bubbles appearing, squad-wide.
///
/// The squad's barks are produced by independent per-unit systems, so two units acting on the same
/// frame used to pop their balloons simultaneously — a reply landing 16 ms after its cue reads as
/// noise, not as conversation. Human turn-taking is fast (modal gap ≈ 200 ms; Stivers et al.,
/// *Universals and cultural variation in turn-taking*, PNAS 2009, DOI 10.1073/pnas.0903616106) but
/// speech carries prosody and the listener already knows when a turn is ending. A *read* balloon has
/// neither cue, so the gap must cover the eye-movement + comprehension cost of noticing the new
/// balloon; ~1 s is the low end of that, and matches the dwell model's per-word budget in `bubble`.
const BARK_GAP: f32 = 1.0;

/// How long a bark may wait its turn before it is no longer worth saying. A line is a reaction to a
/// world event; narrated late it describes a situation that has already moved on.
const BARK_STALE: f32 = 3.0;

/// Backlog cap. Beyond this the *oldest* pending barks are dropped: in a burst the freshest reactions
/// are the relevant ones, and a deep queue would trickle out stale chatter for many seconds.
const BARK_QUEUE_CAP: usize = 4;

/// A bark waiting its turn to be spoken, with the time it was generated (for staleness).
struct PendingBark {
    bark: Bark,
    queued_at: f32,
}

/// Paces ambient chatter. Barks are enqueued as they arrive and released one at a time, at least
/// [`BARK_GAP`] apart, turning simultaneous utterances into a turn-taking exchange.
///
/// This is the single gate on bark presentation — barks are never spawned directly. Ordering is FIFO,
/// so a reply still follows its cue.
#[derive(Resource, Default)]
struct BarkQueue {
    pending: VecDeque<PendingBark>,
    /// Real-time stamp before which no bark may be released.
    next_at: f32,
}

impl BarkQueue {
    /// File a bark, evicting the oldest if the backlog is full.
    fn push(&mut self, bark: Bark, now: f32) {
        self.pending.push_back(PendingBark { bark, queued_at: now });
        while self.pending.len() > BARK_QUEUE_CAP {
            self.pending.pop_front();
        }
    }

    /// Discard barks that have waited past [`BARK_STALE`], then hand back the next one if the
    /// [`BARK_GAP`] since the last balloon has elapsed. Staleness is applied unconditionally so a dead
    /// bark can never hold the queue head and starve the fresh ones behind it.
    fn take_ready(&mut self, now: f32) -> Option<Bark> {
        while self.pending.front().is_some_and(|p| now - p.queued_at > BARK_STALE) {
            self.pending.pop_front();
        }
        if now < self.next_at {
            return None;
        }
        self.pending.pop_front().map(|p| p.bark)
    }

    /// A balloon actually reached the screen at `now` — open the gap before the next one may.
    fn spoke_at(&mut self, now: f32) {
        self.next_at = now + BARK_GAP;
    }
}

pub fn plugin(app: &mut App) {
    app.add_message::<StartConversation>()
        .add_message::<Bark>()
        .add_message::<ChoicePicked>()
        .init_resource::<BarkQueue>()
        .add_systems(
            Update,
            (
                open_conversation,
                resolve_choice,
                advance_line,
                present_current,
                // The squad-AI adapter feeds `Bark`, so it runs immediately before the consumer and a
                // generated line reaches the queue on the frame it was spoken.
                super::bark_squad_lines,
                // Enqueue, then release: a bark spoken this frame can still be shown this frame, but
                // only one balloon appears per `BARK_GAP` (see `BarkQueue`).
                queue_barks,
                release_bark,
            )
                .chain(),
        );
}

/// Resolve a speaker index to its living unit entity (falls back to the leader as narrator if that
/// member is dead, so a scripted line never silently vanishes).
fn speaker_entity(
    speaker: usize,
    members: &Query<(Entity, &SquadMember), With<Unit>>,
    leader: &Query<Entity, (With<Unit>, With<Leader>)>,
) -> Option<Entity> {
    if let Some((e, _)) = members.iter().find(|(_, m)| m.0 == speaker) {
        return Some(e);
    }
    let l = leader.single().ok();
    if l.is_some() {
        warn!("dialogue: speaker {speaker} not alive; anchoring on leader");
    }
    l
}

fn open_conversation(
    mut commands: Commands,
    mut starts: MessageReader<StartConversation>,
    active: Option<Res<Active>>,
    script: Res<DialogueScript>,
    mut menu: ResMut<NextState<MenuState>>,
) {
    // One conversation at a time; take the first request and drain the rest of the queue.
    let id = {
        let Some(start) = starts.read().next() else {
            return;
        };
        start.id.clone()
    };
    starts.clear();
    if active.is_some() {
        return;
    }
    let Some(conv) = script.conversation(&id) else {
        warn!("dialogue: no conversation '{id}'");
        return;
    };
    commands.insert_resource(Active {
        conv: id.clone(),
        node: conv.start.clone(),
        presented: false,
        advance_at: 0.0,
    });
    commands.insert_resource(ConversationLock);
    menu.set(MenuState::Conversation);
}

fn present_current(
    mut commands: Commands,
    time: Res<Time<Real>>,
    active: Option<ResMut<Active>>,
    script: Res<DialogueScript>,
    assets: Res<BubbleAssets>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<ConversationBubble>>,
    members: Query<(Entity, &SquadMember), With<Unit>>,
    leader: Query<Entity, (With<Unit>, With<Leader>)>,
) {
    let Some(mut active) = active else {
        return;
    };
    if active.presented {
        return;
    }

    // Clear the previous node's bubbles.
    for e in &existing {
        commands.entity(e).despawn();
    }

    let Some(node) = script
        .conversation(&active.conv)
        .and_then(|c| c.nodes.get(&active.node))
    else {
        warn!("dialogue: node '{}' missing; ending", active.node);
        end_conversation(&mut commands, &existing);
        return;
    };

    match node {
        Node::Line {
            speaker,
            kind,
            emotion,
            text,
            ..
        } => {
            if let Some(owner) = speaker_entity(*speaker, &members, &leader) {
                spawn_line_bubble(
                    &mut commands,
                    &assets,
                    &mut images,
                    &mut materials,
                    owner,
                    *kind,
                    *emotion,
                    text,
                    Vec2::ZERO,
                );
            }
            active.advance_at = time.elapsed_secs() + dwell_secs(text);
        }
        Node::Choice {
            speaker,
            emotion,
            prompt,
            options,
        } => {
            // The prompt as a speech bubble over the speaker.
            if let Some(owner) = speaker_entity(*speaker, &members, &leader) {
                spawn_line_bubble(
                    &mut commands,
                    &assets,
                    &mut images,
                    &mut materials,
                    owner,
                    BubbleKind::Speech,
                    *emotion,
                    prompt,
                    Vec2::ZERO,
                );
            }
            // Clickable option bubbles stacked above the leader.
            if let Ok(leader_e) = leader.single() {
                let mut offset_y = CHOICE_BASE;
                for (i, opt) in options.iter().enumerate() {
                    let rendered = build_bubble(
                        &assets,
                        &mut images,
                        &mut materials,
                        &BubbleStyle {
                            kind: BubbleKind::Speech,
                            emotion: Emotion::Neutral,
                            tail: false,
                        },
                        &format!("{}. {}", i + 1, opt.text),
                    );
                    // Slightly dim until hovered, for a clickable affordance.
                    if let Some(mut m) = materials.get_mut(&rendered.material) {
                        m.base_color = crate::palette::PAPER_GRAY;
                    }
                    let mat = rendered.material.clone();
                    let mat_over = mat.clone();
                    let mat_out = mat.clone();
                    let idx = i;
                    let center_offset = offset_y + rendered.size.y * 0.5;
                    commands
                        .spawn((
                            Bubble {
                                owner: leader_e,
                                offset: Vec2::new(0.0, center_offset),
                            },
                            ConversationBubble,
                            Pickable::default(),
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(mat),
                            Transform::from_scale(Vec3::new(rendered.size.x, rendered.size.y, 1.0)),
                        ))
                        .observe(
                            move |_ev: On<Pointer<Click>>, mut picked: MessageWriter<ChoicePicked>| {
                                picked.write(ChoicePicked { index: idx });
                            },
                        )
                        .observe(
                            move |_ev: On<Pointer<Over>>,
                                  mut mats: ResMut<Assets<StandardMaterial>>| {
                                if let Some(mut m) = mats.get_mut(&mat_over) {
                                    m.base_color = Color::WHITE;
                                }
                            },
                        )
                        .observe(
                            move |_ev: On<Pointer<Out>>,
                                  mut mats: ResMut<Assets<StandardMaterial>>| {
                                if let Some(mut m) = mats.get_mut(&mat_out) {
                                    m.base_color = crate::palette::PAPER_GRAY;
                                }
                            },
                        );
                    offset_y += rendered.size.y + CHOICE_GAP;
                }
            }
            active.advance_at = f32::INFINITY; // choices don't auto-advance
        }
    }
    active.presented = true;
}

fn advance_line(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mouse: Res<ButtonInput<MouseButton>>,
    active: Option<ResMut<Active>>,
    script: Res<DialogueScript>,
    existing: Query<Entity, With<ConversationBubble>>,
    mut menu: ResMut<NextState<MenuState>>,
) {
    let Some(mut active) = active else {
        return;
    };
    if !active.presented {
        return;
    }
    // Only Line nodes advance here (choices are resolved by a pick).
    let next = match script
        .conversation(&active.conv)
        .and_then(|c| c.nodes.get(&active.node))
    {
        Some(Node::Line { next, .. }) => next.clone(),
        _ => return,
    };
    let clicked = mouse.just_pressed(MouseButton::Left);
    let timed_out = time.elapsed_secs() >= active.advance_at;
    if !clicked && !timed_out {
        return;
    }
    match next {
        Some(n) => {
            active.node = n;
            active.presented = false;
        }
        None => {
            drop(active);
            finish(&mut commands, &existing, &mut menu);
        }
    }
}

fn resolve_choice(
    mut commands: Commands,
    mut picks: MessageReader<ChoicePicked>,
    active: Option<ResMut<Active>>,
    script: Res<DialogueScript>,
    existing: Query<Entity, With<ConversationBubble>>,
    mut menu: ResMut<NextState<MenuState>>,
) {
    let Some(mut active) = active else {
        picks.clear();
        return;
    };
    let Some(pick) = picks.read().next() else {
        return;
    };
    let index = pick.index;
    picks.clear();

    let next = match script
        .conversation(&active.conv)
        .and_then(|c| c.nodes.get(&active.node))
    {
        Some(Node::Choice { options, .. }) => options.get(index).map(|o| o.next.clone()),
        _ => return,
    };
    match next {
        Some(n) => {
            active.node = n;
            active.presented = false;
        }
        None => {
            drop(active);
            finish(&mut commands, &existing, &mut menu);
        }
    }
}

/// Tear down the conversation: despawn its bubbles, drop the lock/cursor, unfreeze the sim.
fn finish(
    commands: &mut Commands,
    existing: &Query<Entity, With<ConversationBubble>>,
    menu: &mut NextState<MenuState>,
) {
    end_conversation(commands, existing);
    menu.set(MenuState::Closed);
}

fn end_conversation(commands: &mut Commands, existing: &Query<Entity, With<ConversationBubble>>) {
    for e in existing {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<Active>();
    commands.remove_resource::<ConversationLock>();
}

/// Queue every incoming [`Bark`]. Runs before [`release_bark`] so a bark generated this frame is a
/// candidate this frame — the queue adds pacing, never a frame of latency to an idle squad.
fn queue_barks(time: Res<Time<Real>>, mut barks: MessageReader<Bark>, mut queue: ResMut<BarkQueue>) {
    let now = time.elapsed_secs();
    for bark in barks.read() {
        queue.push(bark.clone(), now);
    }
}

/// Release at most one queued bark per [`BARK_GAP`], dropping any that waited past [`BARK_STALE`].
///
/// Silent while a modal conversation holds the [`ConversationLock`]: this layer runs on `Time<Real>`
/// (so balloons keep expiring through the freeze), which means the gap would otherwise elapse mid-
/// conversation and pop ambient chatter over the authored scene. The sim is frozen, so no *new* bark is
/// generated during one — only a bark filed on the frame the conversation opened could slip through.
/// Anything still queued when the lock lifts is aged out by [`BARK_STALE`] on the next release.
fn release_bark(
    mut commands: Commands,
    time: Res<Time<Real>>,
    lock: Option<Res<ConversationLock>>,
    mut queue: ResMut<BarkQueue>,
    assets: Res<BubbleAssets>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    members: Query<(Entity, &SquadMember), With<Unit>>,
    leader: Query<Entity, (With<Unit>, With<Leader>)>,
    ambient: Query<(Entity, &AmbientBubble)>,
) {
    if lock.is_some() {
        return;
    }
    let now = time.elapsed_secs();
    let Some(bark) = queue.take_ready(now) else {
        return;
    };
    let Some(owner) = speaker_entity(bark.speaker, &members, &leader) else {
        // Nothing was shown, so the gap is not started — the next bark is eligible immediately.
        return;
    };
    queue.spoke_at(now);

    // Replace any existing ambient bubble on this unit so they don't stack.
    for (e, a) in &ambient {
        if a.owner == owner {
            commands.entity(e).despawn();
        }
    }
    let rendered = build_bubble(
        &assets,
        &mut images,
        &mut materials,
        &BubbleStyle {
            kind: bark.kind,
            emotion: bark.emotion,
            tail: true,
        },
        &bark.text,
    );
    commands.spawn((
        Bubble {
            owner,
            offset: Vec2::ZERO,
        },
        AmbientBubble { owner },
        BubbleTtl {
            expires_at: now + dwell_secs(&bark.text),
        },
        Mesh3d(assets.quad.clone()),
        MeshMaterial3d(rendered.material),
        Transform::from_scale(Vec3::new(rendered.size.x, rendered.size.y, 1.0)),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_line_bubble(
    commands: &mut Commands,
    assets: &BubbleAssets,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    owner: Entity,
    kind: BubbleKind,
    emotion: Emotion,
    text: &str,
    offset: Vec2,
) {
    let rendered = build_bubble(
        assets,
        images,
        materials,
        &BubbleStyle {
            kind,
            emotion,
            tail: true,
        },
        text,
    );
    commands.spawn((
        Bubble { owner, offset },
        ConversationBubble,
        Mesh3d(assets.quad.clone()),
        MeshMaterial3d(rendered.material),
        Transform::from_scale(Vec3::new(rendered.size.x, rendered.size.y, 1.0)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bark(text: &str) -> Bark {
        Bark {
            speaker: 0,
            kind: BubbleKind::Speech,
            emotion: Emotion::Neutral,
            text: text.into(),
        }
    }

    /// The bug this pacing exists for: two units acting on the same frame used to pop both balloons at
    /// once, so a "reply" landed ~16 ms after its cue. The second must now wait a full `BARK_GAP`.
    #[test]
    fn a_second_bark_waits_a_full_gap() {
        let mut q = BarkQueue::default();
        q.push(bark("Contact. Hold your lane."), 0.0);
        q.push(bark("Copy."), 0.0);

        // The first speaks immediately — an idle squad is never made to wait.
        assert_eq!(q.take_ready(0.0).map(|b| b.text), Some("Contact. Hold your lane.".into()));
        q.spoke_at(0.0);

        // ...and the reply is held back until the gap has fully elapsed.
        assert!(q.take_ready(0.5).is_none(), "reply jumped the gap");
        assert!(q.take_ready(BARK_GAP - 0.01).is_none(), "reply jumped the gap");
        assert_eq!(q.take_ready(BARK_GAP).map(|b| b.text), Some("Copy.".into()));
    }

    #[test]
    fn first_bark_is_never_delayed() {
        let mut q = BarkQueue::default();
        q.push(bark("Moving."), 0.0);
        assert!(q.take_ready(0.0).is_some(), "a quiet squad should speak the instant it has a line");
    }

    #[test]
    fn a_bark_that_waited_too_long_is_dropped_not_narrated_late() {
        let mut q = BarkQueue::default();
        q.push(bark("Contact."), 0.0);
        assert!(
            q.take_ready(BARK_STALE + 0.01).is_none(),
            "a stale reaction describes a situation that has moved on"
        );
        assert!(q.pending.is_empty());
    }

    /// Staleness must be applied before the gap check, or a dead bark parked at the head would block
    /// every fresh bark behind it until it happened to be picked.
    #[test]
    fn a_stale_head_does_not_starve_a_fresh_bark() {
        let mut q = BarkQueue::default();
        q.push(bark("old"), 0.0);
        q.push(bark("fresh"), 2.5);
        // At t=3.5 "old" has waited 3.5 s (stale); "fresh" only 1.0 s.
        assert_eq!(q.take_ready(3.5).map(|b| b.text), Some("fresh".into()));
    }

    #[test]
    fn a_burst_keeps_the_freshest_barks() {
        let mut q = BarkQueue::default();
        for i in 0..BARK_QUEUE_CAP + 2 {
            q.push(bark(&i.to_string()), 0.0);
        }
        assert_eq!(q.pending.len(), BARK_QUEUE_CAP);
        // The two oldest were evicted, not the two newest.
        assert_eq!(q.pending.front().map(|p| p.bark.text.clone()), Some("2".into()));
    }

    /// A burst drains one balloon at a time, each a gap apart — the whole point of the queue.
    #[test]
    fn a_burst_drains_one_balloon_per_gap() {
        let mut q = BarkQueue::default();
        for i in 0..BARK_QUEUE_CAP {
            q.push(bark(&i.to_string()), 0.0);
        }
        let mut spoken = Vec::new();
        let mut t = 0.0_f32;
        // Step in small increments across the staleness window; only whole gaps should release.
        while t <= BARK_STALE {
            if let Some(b) = q.take_ready(t) {
                q.spoke_at(t);
                spoken.push((b.text, t));
            }
            t += 0.1;
        }
        assert!(spoken.len() >= 3, "expected a paced drain, got {spoken:?}");
        assert!(spoken.len() <= BARK_QUEUE_CAP, "released more than were queued");
        // Every consecutive pair of balloons is at least a gap apart.
        for w in spoken.windows(2) {
            assert!(w[1].1 - w[0].1 >= BARK_GAP - 1.0e-3, "balloons {:?} and {:?} overlap", w[0], w[1]);
        }
    }
}
