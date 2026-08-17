//! **One parser for what opens a door**, whether the menu chose it or the command line did.
//!
//! The menu produces exactly the argv `main` accepts — `[root, map?, --door <d>, --kit <k>]` — so
//! there is one thing to get wrong rather than two that agree until they do not. That mattered less
//! when the menu launched the editor as a child process and argv was literally the interface; it
//! matters more now that both live in one application (`crate::screen`) and the temptation is a
//! second, tidier struct on the inside.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::project::{OpenMap, Project};
use crate::tiles::{Door, Mode};

/// A door, loaded and ready to insert.
pub struct Opened {
    pub project: Project,
    /// `None` on every door but Maps — see [`crate::project::OpenMap`].
    pub open_map: Option<OpenMap>,
    pub door: Door,
    /// Which of the door's panels it opens on.
    pub mode: Mode,
}

impl Opened {
    /// Put it in the World. Synchronous, because `OnEnter(Editor)` is the next schedule and
    /// everything in it assumes a loaded project.
    pub fn insert_into(self, world: &mut World) {
        world.insert_resource(self.project);
        world.insert_resource(self.door);
        world.insert_resource(self.mode);
        match self.open_map {
            Some(m) => world.insert_resource(m),
            // **Removed, not left stale.** A door with no map that inherited the last one would be
            // the exact failure the full teardown exists to prevent.
            None => {
                world.remove_resource::<OpenMap>();
            }
        }
    }

    /// Drop everything a door owns.
    ///
    /// Named one by one because there is no reachability rule for resources the way there is for
    /// entities — a resource nobody removes simply stays, and a stale `Project` outliving its door
    /// is the bug a full teardown is spent to make impossible.
    pub fn remove_from(world: &mut World) {
        world.remove_resource::<Project>();
        world.remove_resource::<OpenMap>();
        world.remove_resource::<Door>();
        world.remove_resource::<Mode>();
    }
}

/// **What the door needs, out of the argv both callers speak.**
///
/// `--door` is required for anything but the Maps door, because a bare positional cannot say which
/// door it names. A missing `--door` with a map name is the Maps door, which is what
/// `emerge-mapper . site_67` always meant.
/// **What an argv says, split once.**
///
/// Every question this module answers — where the root is, whether a door was named, which door —
/// is answered off this one split. Three hand-rolled scans is how `root_of` came to treat `--kit`'s
/// *value* as the first positional: `emerge-mapper --kit site` pointed Bevy's asset root at
/// `./site/assets` while `open` correctly resolved the project at `.`, and every mesh 404'd. The
/// module note promised "one thing to get wrong rather than two that agree until they do not"; this
/// is that one thing.
pub struct Split<'a> {
    pub positional: Vec<&'a str>,
    pub kit: Option<&'a str>,
    pub door: Option<&'a str>,
}

/// Split an argv into its positionals and the two flags this editor takes.
///
/// A flag's value is consumed with it, so it can never be mistaken for a positional.
pub fn split(args: &[String]) -> Split<'_> {
    let mut out = Split {
        positional: Vec::new(),
        kit: None,
        door: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--kit" => out.kit = it.next().map(String::as_str),
            "--door" => out.door = it.next().map(String::as_str),
            other => out.positional.push(other),
        }
    }
    out
}

pub fn open(args: &[String]) -> Result<Opened, String> {
    let Split {
        positional,
        kit,
        door: door_flag,
    } = split(args);
    let root = root_from(&positional);

    let door = match door_flag {
        None => Door::default(),
        Some(name) => Door::from_flag(name).ok_or_else(|| {
            format!(
                "no door called `{name}`. The doors are {}.",
                Door::ALL
                    .iter()
                    .map(|d| d.label().to_lowercase())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?,
    };

    let project = Project::open(&root, kit)
        .map_err(|e| format!("cannot open {}: {e}", root.display()))?;

    // **Only the Maps door takes a map**, and it is the only one that needs the second positional.
    let open_map = if door == Door::Map {
        let name = positional.get(1).copied().unwrap_or("untitled_map");
        Some(OpenMap::open(&project, name).map_err(|e| format!("cannot open map `{name}`: {e}"))?)
    } else {
        None
    };

    Ok(Opened {
        mode: door.opens_on(),
        project,
        open_map,
        door,
    })
}

/// The project root out of the positionals: the first one, or the working directory.
fn root_from(positional: &[&str]) -> PathBuf {
    let root = PathBuf::from(positional.first().copied().unwrap_or("."));
    // A path that cannot be canonicalised is one that does not exist; keep it as written so
    // `Project::open` names it in the refusal rather than reporting an absolute path nobody typed.
    std::fs::canonicalize(&root).unwrap_or(root)
}

/// Where the project is, for the asset root. `main` needs this before anything is opened.
pub fn root_of(args: &[String]) -> PathBuf {
    root_from(&split(args).positional)
}

/// **Whether these arguments name a door directly**, rather than asking for the menu.
///
/// A `--door`, or a second positional — which is the map name, and only the Maps door takes one.
/// Counted off [`split`], so `--kit site` is a flag and its value, never "a second positional".
pub fn names_a_door(args: &[String]) -> bool {
    let split = split(args);
    split.door.is_some() || split.positional.len() >= 2
}
