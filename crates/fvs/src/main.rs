//! **`cargo fvs`** — one entry point for the things this repo can do.
//!
//! There are three programs here and they were invoked three different ways: the game as
//! `cargo run`, the editor as `cargo run -p emerge-mapper -- . <name>`, the trainer as `cargo train`.
//! Two of those are memorable and one is not, and putting a map in the game meant knowing that
//! `FVS_EMERGE_MAP` exists at all.
//!
//! ```text
//! cargo run -p fvs -- play --map break_room --at 80,112
//! cargo run -p fvs -- edit break_room
//! cargo run -p fvs -- train behavior --generations 2
//! cargo run -p fvs -- test --harness
//! ```
//!
//! # The `cargo fvs` alias is opt-in, per machine
//!
//! `.cargo/config.toml` is **deliberately not committed** — a hardcoded machine-specific
//! `build.target-dir` in it once broke CI on every commit, so each machine keeps its own. That means
//! the shorter spelling has to be pasted in once rather than arriving with a pull:
//!
//! ```toml
//! [alias]
//! fvs = "run --quiet -p fvs --"
//! ```
//!
//! Everything below works either way; `cargo run -p fvs --` needs no setup at all.
//!
//! # Why this dispatches rather than linking everything
//!
//! One binary that *did* all of it would have to link the trainer, and the trainer needs the
//! `test-harness` feature — which Cargo.toml turns off by default on purpose, *"so the shipped binary
//! carries none of it."* A unified binary would force that scaffolding into every build of the game.
//! So this runs `cargo` the way you would have, and its whole job is to know which incantation.
//!
//! It is also its own crate with **no dependencies**, for a smaller reason that turned out to matter:
//! as a `src/bin/` target of the game package, printing `--help` first rebuilt the entire game
//! library. A launcher that takes two minutes to tell you the flags is worse than remembering them.
//!
//! # Why the arguments are parsed by hand
//!
//! `clap` is behind the same `test-harness` gate. Making it unconditional to dispatch five
//! subcommands would be a dependency bought for nothing, and everything past the subcommand is
//! forwarded verbatim anyway — `fvs train` re-declaring the trainer's forty flags is exactly the
//! second description this repo keeps deleting.
//!
//! # The environment variables are still the mechanism
//!
//! `--map` sets `FVS_EMERGE_MAP`; it does not add a second way for the game to hear about a map.
//! `src/emerge_map.rs` reads one variable and always did. This is a nicer way to spell it, not a
//! parallel path — which matters, because the dev-tool family (`FVS_RESEARCH_ROOM`, `FVS_AUTORUN`,
//! and the rest) all work the same way and would otherwise start diverging one flag at a time.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The repository root, resolved at build time.
///
/// **Not `.`**, and not the shell's working directory. `cargo run -p <pkg>` runs the binary with the
/// *package* directory as its cwd, so passing `.` to the editor pointed it at
/// `crates/emerge-mapper/assets/` and every mesh in the palette failed to load. Every path this hands
/// to a child is absolute for that reason, and it means `cargo fvs` also works from a subdirectory.
fn repo_root() -> PathBuf {
    // `crates/fvs` → the workspace. `env!` is the crate's own manifest directory at compile time,
    // which is the one thing that cannot be wrong about where this repo lives.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

const USAGE: &str = "\
fvs — Foundation vs. Slop

USAGE:
    cargo run -p fvs -- <command> [options]
    cargo fvs <command> [options]        (with the optional alias; see below)

COMMANDS:
    play              Run the game.
        --map <name>      Load a map authored in the editor (assets/emerge/<name>.map.ron).
        --at <x,z>        Put that map where the camera can see it. Maps are authored at
                          (0,0,0) and the camera is not there.
        --research-room   Boot straight into the WFC dungeon dev room (F6 debug panel).
        --release         Build with optimisations.

    edit [name]       Open the map editor on assets/emerge/<name>.map.ron.
                      A name that does not exist yet is a new map. Defaults to `untitled_map`.
        --kit <name>      Open a kit under assets/emerge/ instead of the default furniture
                          library — `--kit site` is the 45-piece architectural set.
        --fullscreen      Borderless fullscreen.
        --release         Build with optimisations.

    train ...         The offline QD/RL driver. Every argument is forwarded verbatim;
                      `cargo fvs train --help` is the trainer's own help.

    test              The deterministic core suite (fast, no GPU).
        --harness         Instead run the headless replay/liveness suite. Needs a GPU,
                          runs single-threaded, and takes about an hour.

EXAMPLES:
    cargo run -p fvs -- play --map break_room --at 80,112
    cargo run -p fvs -- edit break_room --fullscreen
    cargo run -p fvs -- edit site_67 --kit site --fullscreen
    cargo run -p fvs -- train behavior --generations 2 --batch 8
    cargo run -p fvs -- test

THE SHORTER SPELLING:
    `.cargo/config.toml` is not committed (a machine-specific target-dir in it once broke
    CI), so paste this into yours once and every command above loses six characters:

        [alias]
        fvs = \"run --quiet -p fvs --\"
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let rest = &args[1..];

    let result = match command {
        "play" => play(rest),
        "edit" => edit(rest),
        "train" => train(rest),
        "test" => test(rest),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!(
            "unknown command `{other}`.\n\n{USAGE}"
        )),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("cargo fvs: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Pull `--flag <value>` out of the argument list, or `None`.
///
/// Errors rather than defaulting when the flag is present with nothing after it: `--map` with no name
/// is a typed-and-forgotten argument, and silently launching the game without a map is the failure
/// mode `emerge_map`'s own parser was hardened against.
fn take_value(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let Some(at) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    if at + 1 >= args.len() || args[at + 1].starts_with("--") {
        return Err(format!("{flag} needs a value"));
    }
    let value = args.remove(at + 1);
    args.remove(at);
    Ok(Some(value))
}

/// Pull a bare `--flag`, returning whether it was there.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    match args.iter().position(|a| a == flag) {
        Some(at) => {
            args.remove(at);
            true
        }
        None => false,
    }
}

/// Refuse leftovers rather than ignoring them — a mistyped flag that silently does nothing is how you
/// spend ten minutes wondering why `--reserch-room` did not open the research room.
fn nothing_left(args: &[String], command: &str) -> Result<(), String> {
    match args.first() {
        None => Ok(()),
        Some(a) => Err(format!(
            "`{command}` does not take `{a}`.\n\n{USAGE}"
        )),
    }
}

fn play(args: &[String]) -> Result<ExitCode, String> {
    let mut args = args.to_vec();
    let map = take_value(&mut args, "--map")?;
    let at = take_value(&mut args, "--at")?;
    let research_room = take_flag(&mut args, "--research-room");
    let release = take_flag(&mut args, "--release");
    nothing_left(&args, "play")?;

    if at.is_some() && map.is_none() {
        return Err("--at positions a map, so it needs --map too".into());
    }

    let mut cargo = cargo(release);
    cargo.args(["--bin", "foundation_vs_slop"]);
    // The variables ARE the mechanism; this only spells them. See the module docs.
    if let Some(map) = map {
        cargo.env("FVS_EMERGE_MAP", map);
    }
    if let Some(at) = at {
        cargo.env("FVS_EMERGE_MAP_AT", at);
    }
    if research_room {
        cargo.env("FVS_RESEARCH_ROOM", "1");
    }
    run(cargo)
}

fn edit(args: &[String]) -> Result<ExitCode, String> {
    let mut args = args.to_vec();
    let fullscreen = take_flag(&mut args, "--fullscreen");
    let release = take_flag(&mut args, "--release");
    // A kit is a directory under `assets/emerge/` holding a library and its policy layer: the
    // default is furniture, `--kit site` is the 45-piece architectural set. Forwarded rather than
    // interpreted — `Project::open` is what decides whether the kit exists, and duplicating that
    // check here would be a second answer to one question.
    let kit = take_value(&mut args, "--kit")?;
    // Whatever is left is the map name — one positional, and a second is a typo rather than a map
    // called "break room" with a space in it (names are snake_case; the editor forces that anyway).
    let name = args.first().cloned();
    nothing_left(&args[name.iter().len()..], "edit")?;

    let mut cargo = cargo(release);
    cargo.args(["-p", "emerge-mapper", "--"]);
    // The project root, ABSOLUTE. The editor treats the directory it opens as the asset root because
    // meshes are named relative to the project — and `cargo run -p` starts the child in the package
    // directory, so a relative `.` would be `crates/emerge-mapper/`.
    cargo.arg(repo_root());
    if let Some(name) = name {
        cargo.arg(name);
    }
    if let Some(kit) = kit {
        cargo.args(["--kit", &kit]);
    }
    if fullscreen {
        cargo.env("EMERGE_FULLSCREEN", "1");
    }
    run(cargo)
}

/// Forwarded verbatim, including `--help`. The trainer's forty flags are described in exactly one
/// place, which is the trainer.
fn train(args: &[String]) -> Result<ExitCode, String> {
    let mut cargo = Command::new("cargo");
    cargo.args(["run", "--manifest-path", &manifest()]);
    cargo.args(["--release", "--features", "test-harness", "--bin", "train", "--"]);
    cargo.args(args);
    run(cargo)
}

fn test(args: &[String]) -> Result<ExitCode, String> {
    let mut args = args.to_vec();
    let harness = take_flag(&mut args, "--harness");
    nothing_left(&args, "test")?;

    let mut cargo = Command::new("cargo");
    cargo.args(["test", "--manifest-path", &manifest()]);
    if harness {
        // `--test-threads=1` is not optional here: the harness pins Bevy's IO pool to one thread, and
        // omitting it stacks the load generators — 23 cores against 2. See TESTING.md.
        cargo.args(["--features", "test-harness", "--", "--test-threads=1"]);
    } else {
        cargo.args(["--workspace", "--no-fail-fast"]);
    }
    run(cargo)
}

fn cargo(release: bool) -> Command {
    let mut cargo = Command::new("cargo");
    cargo.arg("run");
    cargo.args(["--manifest-path", &manifest()]);
    if release {
        cargo.arg("--release");
    }
    cargo
}

/// The workspace manifest, so `cargo fvs` works from any directory rather than only the root.
fn manifest() -> String {
    repo_root().join("Cargo.toml").display().to_string()
}

/// Hand the terminal over and pass the child's exit status back up, so `cargo fvs test` fails a
/// script exactly as `cargo test` would.
fn run(mut cargo: Command) -> Result<ExitCode, String> {
    let status = cargo
        .status()
        .map_err(|e| format!("could not start cargo: {e}"))?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        // A `u8` is all `ExitCode` carries, and a signal death has no code at all; both become a
        // plain failure rather than a number that means something else.
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bug this const exists for.** `cargo run -p emerge-mapper` starts the child in the
    /// PACKAGE directory, so the `.` originally passed as the project root resolved to
    /// `crates/emerge-mapper/` and every mesh in the palette failed to load — reported from a Mac
    /// within minutes of the command shipping.
    #[test]
    fn the_repo_root_is_the_workspace_and_not_a_package() {
        let root = repo_root();
        assert!(
            root.join("Cargo.toml").is_file(),
            "{} has no Cargo.toml",
            root.display()
        );
        assert!(
            root.join("assets/emerge").is_dir(),
            "{} is not the workspace root — the editor's project lives at assets/emerge",
            root.display()
        );
        assert!(
            root.is_absolute(),
            "the root must be absolute; a relative one is what broke"
        );
        // Specifically NOT a package directory, which is the shape the bug had.
        assert!(
            !root.ends_with("emerge-mapper") && !root.ends_with("fvs"),
            "{} looks like a package rather than the workspace",
            root.display()
        );
    }

    /// A flag with nothing after it is a typed-and-forgotten argument, not a request for a default.
    #[test]
    fn a_valueless_flag_is_refused() {
        let mut args = vec!["--map".to_owned()];
        assert!(take_value(&mut args, "--map").is_err());

        let mut args = vec!["--map".to_owned(), "--at".to_owned(), "1,2".to_owned()];
        assert!(
            take_value(&mut args, "--map").is_err(),
            "the next flag is not a map name"
        );

        let mut args = vec!["--map".to_owned(), "break_room".to_owned()];
        assert_eq!(
            take_value(&mut args, "--map").unwrap_or_default(),
            Some("break_room".to_owned())
        );
        assert!(args.is_empty(), "the flag and its value are consumed");
    }

    /// A mistyped flag is refused rather than ignored — silently doing nothing is how ten minutes go
    /// missing wondering why `--reserch-room` did not open the research room.
    #[test]
    fn a_leftover_argument_is_refused() {
        assert!(nothing_left(&[], "play").is_ok());
        let leftover = ["--reserch-room".to_owned()];
        let err = nothing_left(&leftover, "play")
            .err()
            .unwrap_or_default();
        assert!(err.contains("--reserch-room"), "{err}");
    }

    /// `--at` positions a map, so it is meaningless alone — and the game would launch normally,
    /// which reads as the flag being ignored.
    #[test]
    fn positioning_a_map_that_was_not_asked_for_is_refused() {
        let err = play(&["--at".to_owned(), "80,112".to_owned()])
            .err()
            .unwrap_or_default();
        assert!(err.contains("needs --map"), "{err}");
    }
}
