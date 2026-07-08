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
#[derive(Message)]
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

pub fn plugin(app: &mut App) {
    app.add_message::<StartConversation>()
        .add_message::<Bark>()
        .add_message::<ChoicePicked>()
        .add_systems(
            Update,
            (
                open_conversation,
                resolve_choice,
                advance_line,
                present_current,
                emit_barks,
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
                        m.base_color = Color::srgb(0.8, 0.8, 0.8);
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
                                    m.base_color = Color::srgb(0.8, 0.8, 0.8);
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

fn emit_barks(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut barks: MessageReader<Bark>,
    assets: Res<BubbleAssets>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    members: Query<(Entity, &SquadMember), With<Unit>>,
    leader: Query<Entity, (With<Unit>, With<Leader>)>,
    ambient: Query<(Entity, &AmbientBubble)>,
) {
    for bark in barks.read() {
        let Some(owner) = speaker_entity(bark.speaker, &members, &leader) else {
            continue;
        };
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
                expires_at: time.elapsed_secs() + dwell_secs(&bark.text),
            },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(rendered.material),
            Transform::from_scale(Vec3::new(rendered.size.x, rendered.size.y, 1.0)),
        ));
    }
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
