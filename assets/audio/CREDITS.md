
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
