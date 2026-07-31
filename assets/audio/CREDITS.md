
## `foot/concrete_1..4.ogg`

Source: **horror_sfx_vol_1 / "Concrete Footsteps"**, via `/mnt/codex_fs/game_assets/audio/sfx/`.
Transcoded MP3 → mono Vorbis (`-q:a 4`) to match the shipped carpet set — the footstep system plays a
single shared, non-spatialised voice (`audio::footsteps`), so stereo would have been discarded anyway.
Selected by the surface biome under the walking squad's centroid (`dungeon::Biome`).

## `enemy/growl_5..8.ogg`, `ambience/oneshot/creak_4..6.ogg`

Source: **horror_sfx_vol_1** (`Monster Growl`, `Creaking Door`), via
`/mnt/codex_fs/game_assets/audio/sfx/`. Transcoded MP3 → mono Vorbis (`-q:a 4`).

Both pools were widened rather than replaced, because both are sounds the player hears constantly and
both were thin enough to stamp: the growl fires on the false→true edge of an enemy entering sight
range (4 clips → 8), and the ambient layer scatters a one-shot every 7–18 s around the squad (6 → 9).
Sample variation is the first rung against audible repetition — Böttcher & Serafin, already cited in
`src/audio.rs` for the splash pool.

## `enemy/flesh_drone.ogg`, `containment/cordon_{place,seal,breach}.ogg`

Source: **400 Sounds Pack**, via `/mnt/codex_fs/game_assets/audio/sfx/`. Transcoded WAV → mono Vorbis
(`-q:a 4`), matching the rest of the tree.

Added for FVS-K-1, which found the containment verb had **no sound of its own at all** — placing a
quarantine borrowed `Sfx::MoveOrder`, the click for ordering someone to walk, and sealing or breaching
a cordon made no sound whatsoever.

| Clip | Source | Transform |
|---|---|---|
| `enemy/flesh_drone.ogg` | `Environment/water_boiling_loop.wav` | `asetrate=22050` (down an octave, 10 s → 20 s) + `lowpass=850` to strip the kettle hiss. A seething wet bed for an SCP-610 bloom, not a kettle. Chosen because it is authored to loop, which a 2 s gurgle is not — the bloom denies a room for the whole expedition, so an audible loop point would be unbearable rather than unsettling. |
| `containment/cordon_place.ogg` | `UI/sci_fi_select_big.wav` | `lowpass=7000` |
| `containment/cordon_seal.ogg` | `UI/synth_process_complete.wav` | `lowpass=6000` |
| `containment/cordon_breach.ogg` | `UI/synth_warning.wav` | none |

`containment/` is a new directory rather than a corner of `ui/` or `enemy/`: a cordon is neither a
menu blip nor a creature. Seal and breach are **spatialized at the anomaly** (`ui/` is the
non-spatial bucket by convention), because "which room just lost its cordon" is the question they
exist to answer.
