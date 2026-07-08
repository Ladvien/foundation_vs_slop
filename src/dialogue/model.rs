//! Dialogue data model — the authored conversation graph deserialized from RON.
//!
//! A [`DialogueScript`] is a set of named [`Conversation`]s; each conversation is a graph of
//! [`Node`]s keyed by id. A node is either a spoken/thought [`Node::Line`] (advances to `next`) or a
//! [`Node::Choice`] that presents options above the leader and branches on the player's pick. Two
//! bubble kinds carry distinct meaning — speech = directed dialogue/story, thought = ambient inner
//! state — following the Comic-Strip-Conversation convention that holds thoughts "equal to spoken
//! words" (Gray; Rajendran & Mitchell, *Bubble Dialogue*, 2000).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;

/// Highest valid speaker index (the squad has five members, 0..=4). See `squad::SquadMember`.
pub const MAX_SPEAKER: usize = 4;

/// Balloon style — drives both the drawn shape and the semantic channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum BubbleKind {
    /// Spoken aloud: rounded-rect balloon with a pointed tail. Directed dialogue / story beats.
    Speech,
    /// Inner voice: soft pill balloon with a trailing dot-tail. Ambient feeling / intent / emotion.
    Thought,
}

fn default_speech() -> BubbleKind {
    BubbleKind::Speech
}

/// Optional affect on a line — tints the balloon and (later) its spawn animation. Grounded in
/// An et al., *AniBalloons* (arXiv:2408.06294): balloon color/animation reliably conveys emotion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum Emotion {
    #[default]
    Neutral,
    Joy,
    Anger,
    Sadness,
    Surprise,
    Fear,
    Calm,
}

/// One selectable option in a [`Node::Choice`]: its label and the node id it jumps to.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub text: String,
    pub next: String,
}

/// A single node in a conversation graph.
#[derive(Debug, Clone, Deserialize)]
pub enum Node {
    /// A spoken/thought line by `speaker`; advances to `next` (or ends the conversation on `None`).
    Line {
        speaker: usize,
        #[serde(default = "default_speech")]
        kind: BubbleKind,
        #[serde(default)]
        emotion: Emotion,
        text: String,
        next: Option<String>,
    },
    /// A prompt spoken by `speaker`, followed by clickable option bubbles above the leader unit.
    Choice {
        speaker: usize,
        #[serde(default)]
        emotion: Emotion,
        prompt: String,
        options: Vec<Choice>,
    },
}

/// A named conversation: the id of its `start` node plus the node graph.
#[derive(Debug, Clone, Deserialize)]
pub struct Conversation {
    pub start: String,
    pub nodes: HashMap<String, Node>,
}

/// The whole authored dialogue set, one RON file, deserialized once into this resource.
#[derive(Resource, Debug, Clone, Deserialize)]
pub struct DialogueScript {
    pub conversations: HashMap<String, Conversation>,
}

impl DialogueScript {
    pub fn conversation(&self, id: &str) -> Option<&Conversation> {
        self.conversations.get(id)
    }
}

/// Validate a parsed script: every conversation's `start` exists, every `next`/option target
/// resolves, choices are non-empty, and speaker indices are in range. One path: a malformed script
/// is a loud error at load, never a half-broken conversation at runtime.
pub fn validate_script(s: &DialogueScript) -> Result<(), String> {
    if s.conversations.is_empty() {
        return Err("dialogue: no conversations defined".into());
    }
    for (cid, conv) in &s.conversations {
        if !conv.nodes.contains_key(&conv.start) {
            return Err(format!(
                "dialogue '{cid}': start node '{}' not found",
                conv.start
            ));
        }
        for (nid, node) in &conv.nodes {
            match node {
                Node::Line { speaker, next, .. } => {
                    check_speaker(*speaker, cid, nid)?;
                    if let Some(n) = next
                        && !conv.nodes.contains_key(n)
                    {
                        return Err(format!("dialogue '{cid}' node '{nid}': next '{n}' not found"));
                    }
                }
                Node::Choice {
                    speaker, options, ..
                } => {
                    check_speaker(*speaker, cid, nid)?;
                    if options.is_empty() {
                        return Err(format!("dialogue '{cid}' node '{nid}': choice has no options"));
                    }
                    for o in options {
                        if !conv.nodes.contains_key(&o.next) {
                            return Err(format!(
                                "dialogue '{cid}' node '{nid}': option next '{}' not found",
                                o.next
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_speaker(s: usize, cid: &str, nid: &str) -> Result<(), String> {
    if s > MAX_SPEAKER {
        return Err(format!(
            "dialogue '{cid}' node '{nid}': speaker {s} out of range 0..={MAX_SPEAKER}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
(
  conversations: {
    "intro": (
      start: "a",
      nodes: {
        "a": Line(speaker: 0, kind: Speech, text: "Move up.", next: Some("b")),
        "b": Line(speaker: 1, kind: Thought, emotion: Fear, text: "This place is wrong.", next: Some("c")),
        "c": Choice(speaker: 0, prompt: "Which way?", options: [
          (text: "Left", next: "d"),
          (text: "Hold", next: "d"),
        ]),
        "d": Line(speaker: 0, text: "Copy.", next: None),
      },
    ),
  },
)
"#;

    #[test]
    fn parses_and_validates_sample() {
        let script: DialogueScript = ron::from_str(SAMPLE).expect("sample parses");
        validate_script(&script).expect("sample validates");
        let conv = script.conversation("intro").expect("intro exists");
        assert_eq!(conv.start, "a");
        assert_eq!(conv.nodes.len(), 4);
    }

    #[test]
    fn kind_defaults_to_speech() {
        // Node "d" omits `kind` — must default to Speech.
        let script: DialogueScript = ron::from_str(SAMPLE).unwrap();
        let conv = script.conversation("intro").unwrap();
        match conv.nodes.get("d").unwrap() {
            Node::Line { kind, .. } => assert_eq!(*kind, BubbleKind::Speech),
            _ => panic!("d is a Line"),
        }
    }

    #[test]
    fn rejects_dangling_next() {
        let bad = r#"( conversations: { "x": ( start: "a", nodes: {
            "a": Line(speaker: 0, text: "hi", next: Some("nope")),
        } ) } )"#;
        let script: DialogueScript = ron::from_str(bad).unwrap();
        assert!(validate_script(&script).is_err());
    }

    #[test]
    fn rejects_missing_start() {
        let bad = r#"( conversations: { "x": ( start: "z", nodes: {
            "a": Line(speaker: 0, text: "hi", next: None),
        } ) } )"#;
        let script: DialogueScript = ron::from_str(bad).unwrap();
        assert!(validate_script(&script).is_err());
    }

    #[test]
    fn rejects_out_of_range_speaker() {
        let bad = r#"( conversations: { "x": ( start: "a", nodes: {
            "a": Line(speaker: 9, text: "hi", next: None),
        } ) } )"#;
        let script: DialogueScript = ron::from_str(bad).unwrap();
        assert!(validate_script(&script).is_err());
    }
}
