# Bake history

Append-only. One record per `train apply` / `train all --apply` phase, written by
`train`'s `record_bake`. **Do not hand-edit** — and do not delete a record to make a diff look
tidy; a bake you would rather not have recorded is exactly the one worth reading later.

Git already holds the *values* a bake changed (`git log -p assets/config/config.ron`) and the
goldens (`git log -p tests/replay.rs`). This file adds the two things git cannot: WHICH elite
caused a change (the archives are gitignored and the next run overwrites them — the snapshot
under `assets/config/bake_history/` is the only surviving copy), and per-phase attribution
inside a single `train all` run, which git otherwise collapses into one diff.

A moved golden is only reviewable because of this trail. Read it before you trust a run.

## 2026-07-19T06:56:08Z — audio

- elite:    audio <- assets/config/elites_audio.ron (cell (2, 7), fitness 0.144)
- archive:  assets/config/elites_audio.ron
- snapshot: assets/config/bake_history/2026-07-19T06-56-08Z-audio.ron
- goldens:  unchanged (snapshot 0xe11eed83902ee648, field 0xd504e6a2f019f3fb)
- files:    assets/config/config.ron, src/audio_tuning.rs
