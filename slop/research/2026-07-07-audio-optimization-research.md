# Audio Optimization Research — Using Sound Optimally in *Foundation vs Slop*

**Date:** 2026-07-07
**Scope:** (1) SOTA / best-practice research mined from the home-still corpus on game audio, and
(2) a catalog of the audio assets we already have (in-repo + `codex_fs`) mapped to how they can
support the game. Ends with a prioritized, gap-analysed backlog against our current
`src/audio.rs`.

> Per `CLAUDE.md`: research first, then implement. Every claim below is backed by a paper in the
> home-still library (DOI/stem given so it can be re-pulled with `paper_download` /
> `markdown_read` / `distill_search`). This doc is the reference; nothing here is committed to code
> yet.

---

## 0. TL;DR — what "optimal sound" means for this game

Our game is a top-down squad-vs-swarm dungeon crawler with a **backrooms / uncanny-horror** tone
(wind bed, growl stingers, a "watcher" boss). The literature says the biggest wins for a game
like ours, in rough ROI order, are:

1. **Spatialize the world sounds** (pan + distance-attenuate growls, squitters, footsteps, muzzle
   fire). Right now every sound is mono at fixed volume — we are leaving the single most-studied
   immersion + information channel on the table. *(Grimshaw & Schott 2007; Zotkin et al. 2004)*
2. **Make the calm↔combat music transition adaptive, not a hard cut** — crossfade or layer
   (vertical remixing / stems), and drive intensity continuously off threat, not a binary flag.
   *(Kaushik 2025; Khan et al. 2023)*
3. **Add a mix bus with sidechain ducking** so gunfire/growls punch through the wind+music bed
   automatically instead of us hand-tuning constants that fight each other. *(implied by the
   mixing discussion in Boettcher & Serafin; Nacke/Grimshaw 2010)*
4. **Exploit tempo/mode/percussiveness deliberately** — these are the *measured* levers on player
   arousal (tempo), valence (major/minor mode) and startle (percussive attacks). Pick horror
   assets accordingly. *(van der Zwaag, Westerink & van den Broek 2011)*
5. **Kill repetition fatigue** with more sample variation and/or light procedural layering
   (we already pitch-jitter; the corpus says that's step one of several). *(Boettcher & Serafin;
   Kaushik 2025)*
6. **Sound-off is measurably worse UX** — players rate a game *tenser and less pleasant* with no
   SFX, and *music without SFX is worse than silence* because it removes feedback while adding
   distraction. Our SFX feedback layer is therefore load-bearing, not decoration. *(Nacke/Grimshaw
   2010)*

---

## 1. The home-still library — what's actually in there for game audio

The corpus holds **6,475 documents / ~201k chunks** (bge-m3 embeddings, Qdrant). Semantic search
across it surfaced a tight cluster of directly-relevant papers. These are the ones worth reading in
full before we build:

### 1.1 Core game-audio papers (read these)

| # | Paper | DOI / stem | Why it matters to us |
|---|-------|-----------|----------------------|
| A | **Situating Gaming as a Sonic Experience: The acoustic ecology of First-Person Shooters** — Grimshaw & Schott, 2007 | `10.26503/dl.v2007i1.313` | The theoretical backbone. Defines *navigational listening*, *audio beacons*, and the *perceptual surety vs surprise* taxonomy (attractors / connectors / retainers). Directly tells us how footsteps, gunfire and growls function as **information**, not just flavour. |
| B | **Adaptive Background Music for a Fighting Game: A Multi-Instrument Volume Modulation Approach** — Khan, Nguyen, Nimpattanavong & Thawonmas, 2023 | `10.48550/arXiv.2303.15734` | The concrete recipe for **vertical remixing**: one piece, N instrument stems, each stem's volume bound to a game variable (HP, distance, energy). Proven to convey game state through music. This is exactly the pattern to replace our binary calm/combat swap. |
| C | **Procedural Music Generation in Video Games** — Kaushik, 2025 | `10.36948/ijfmr.2025.v07i02.39384` | Survey of adaptive/generative music: dynamic layering, state machines, ADSR envelope shaping, Markov/CA/GA/DL techniques, and the **pre-generated-variation vs real-time** trade-off (store variations to save CPU). Good menu of options + the CPU-budget argument. |
| D | **Procedural Audio for computer games** — Böttcher & Serafin (chapter/journal) | `Boettcher_Procedural_Audio` | The case for **procedural SFX**: footsteps/weapons/collisions eat huge sample budgets to avoid audible repetition; procedural + granular synthesis solves repetition and memory. Also argues the winning move is **hybrid** (procedural texture layered on real samples). Cites Raghuvanshi et al. 2007 "Real-time sound synthesis and propagation for games" (CACM) and granular-synthesis-in-games work. |
| E | **Rendering Localized Spatial Audio in a Virtual Auditory Space** — Zotkin, Duraiswami & Davis, 2004 | `10.1109/tmm.2004.827516` | The spatial-audio reference: HRTF-based 3D positioning, distance/reverb cues for **externalization** ("out of the head"), real-time on commodity hardware. Localization accuracy improved ~15–30% with good HRTF. Justifies *why* even simple pan+attenuation pays off. |
| F | **Effect of game sound & music on player experience** (Interaction with Computers) — Nacke / Grimshaw et al., 2010 | `10.1016/j.intcom.2010.04.005` | Empirical, psychophysiological (EMG/EDA + GEQ). Key findings: **sound present → higher positive experience, lower tension**; **music-on/SFX-off is the *worst* condition** (distraction without feedback). Grounds our SFX layer as load-bearing. |
| G | **Emotional and psychophysiological responses to tempo, mode, and percussiveness** — van der Zwaag, Westerink & van den Broek, 2011 | `10.1177/1029864911403364` | The measured emotion levers: **tempo → arousal**, **minor mode → higher arousal / negative valence**, **percussiveness → skin-conductance (startle)**, **staccato → fear/anger vs legato → tenderness/sadness**, **expectancy violation → surprise**. This is our horror-tuning cheat sheet. |
| H | **DareFightingICE Competition: A Fighting Game Sound Design and AI Competition** — Khan et al., 2022 | `10.1109/cog51982.2022.9893624` | Context for B; also the **Blind-AI** angle — a game whose audio carries enough state that an agent can play from sound alone is a game whose audio is genuinely informative. A good design test. |
| I | **Improving Digital Accessibility Through Audio-Game Co-Design** — Mason, Green, Lindley & Coulton, 2023 | `10.26503/dl.v2023i1.1922` | Accessibility: building a sense of game space through sound feedback; learnable audio playstyles. Relevant if we want the dungeon to be legible with the screen dimmed / for low-vision players. |

### 1.2 Adjacent papers (present but lower priority)

- Real-time **deep noise suppression** (`10.1109/ICASSP39728.2021.9413580`, `10.21437/interspeech.2020-2631`, `10.1109/LSP.2019.2955818`) — DNN speech enhancement. Not relevant unless we add voice chat / mic input.
- **Movement sonification & rhythmic auditory cueing** (`W2781500572`) — from a Parkinson's-gait context, but the underlying principle (biologically-*variable* rhythmic cues beat perfectly-isochronous ones for motor entrainment) is a nice justification for our footstep pitch/timing jitter.

> To pull any of these into readable markdown: `paper_download <doi>` → `scribe_convert <stem>` →
> `markdown_read <stem>` (several are already converted + embedded, so `distill_search` hits them
> directly).

---

## 2. What the research says — best practices, distilled

### 2.1 Spatial audio is the highest-value gap

Grimshaw & Schott (A) frame every world sound as a **perceptual hook** the player uses to build a
mental model of space — they coin **navigational listening** (following a sound to its source) and
call directional sounds **audio beacons**. Sounds are either **perceptual sureties** (expected: a
gun makes a gunshot) or **surprises**, sub-typed as **attractors** (invite action), **connectors**
(aid orientation), and **retainers** (encourage lingering). The taxonomy is a design checklist:
every sound we emit should be classifiable, and its *spatial* rendering is what turns it from
flavour into information.

Zotkin et al. (E) show that even a coarse spatial model — panning (ITD/ILD), distance attenuation,
and a little reverb — is what makes sound feel **externalized** ("out of the head") rather than
flat. Full HRTF is overkill for a top-down game, but the cheap subset (stereo pan by screen-X,
inverse-distance rolloff, low-pass with distance) is directly applicable and is what Bevy's
`SpatialAudioSink` / spatial `AudioPlayer` already supports.

**For us:** growls, squitters, muzzle fire, impacts, and footsteps should pan and attenuate by the
emitter's position relative to the camera/listener. A growl from off-screen-left that gets louder
is an *attractor + connector* telling the player where the threat is — currently it's a flat mono
blip with no positional content.

### 2.2 Adaptive music: layer, don't switch

Two independent sources converge:

- **Khan et al. (B)** — *vertical remixing*: take one coherent piece, split into instrument stems,
  bind each stem's **volume** to a continuous game variable. In their fighting game: violin↔P1 HP,
  cello↔player distance, etc. The music *is* a readout of game state, and it never has a jarring
  cut because you're modulating gains, not swapping tracks. They deliberately paired **calm music
  with fast action** for tension contrast (the "Quicksilver" effect).
- **Kaushik (C)** — same idea generalized: **dynamic layering** (add/remove percussion/strings by
  state), **state machines** for combat/explore/stealth with **event-listener** triggers, ADSR
  envelope shaping for transitions, and — crucially for our CPU budget — **pre-generate variations
  and select/combine at runtime** rather than synthesize live.

Both warn against the thing we currently do: a **hard cut** on a binary flag. Our `update_music`
despawns one loop and spawns another the instant an enemy enters LOS; the wind bed papers over the
seam but the transition is still abrupt and binary.

**For us:** two viable upgrades, cheapest first:
1. **Crossfade** the calm↔combat swap over ~1–2 s (fade the outgoing gain down while the incoming
   fades up) instead of a despawn/spawn cut. Small change to `audio.rs`.
2. **Vertical layering**: author combat music as 2–3 stems (bed / percussion / lead) played
   simultaneously, and modulate stem gains off a **continuous threat scalar** (e.g. number + proximity
   of visible hostiles) rather than a boolean. This makes the music breathe with the fight.

### 2.3 The emotion levers are measured — tune horror deliberately

van der Zwaag et al. (G) give us empirically-validated dials (valence–arousal model):

| Lever | Effect | Horror application |
|-------|--------|--------------------|
| **Tempo ↑** | Arousal ↑, tension ↑, HRV ↓ | Combat/chase layers should be faster than explore. |
| **Minor mode** | Arousal ↑, negative valence | Default our tonal beds to minor. |
| **Percussiveness ↑** (sharp attack-decay) | **Skin-conductance / startle ↑** (below conscious threshold!) | Sudden percussive hits for the watcher's reveal/attack. |
| **Staccato articulation** | Fear & anger | Enemy-proximity stingers. |
| **Legato** | Tenderness / sadness | The "uncanny calm" of the watcher when unobserved. |
| **Expectancy violation** | Surprise → arousal | Irregular ambient events (see §2.5). |

Notably, percussiveness moved skin conductance **without** appearing in self-report — i.e. it
works *subconsciously*, which is exactly what horror wants. This maps cleanly onto the smiley
watcher's design (see memory `smiley-watcher-rework`): legato/sparse when unobserved, sharp
percussive attack when it strikes.

### 2.4 SFX feedback is load-bearing (don't over-quiet it)

Nacke/Grimshaw (F) is the empirical gut-check. Across GEQ dimensions:
- **Sound on → more positive affect, immersion, flow; less tension.** Sound *off* made players rate
  the game **tenser and less pleasant** — the "hunter and the hunted" scenario with no audio clues
  is disturbing (a *perceptual mismatch* between what the eyes see and the ears hear).
- **Worst condition = music on, SFX off.** Music without feedback distracts while starving the
  player of information. Lesson: **never let the music/ambience bed drown the SFX**. Our low UI/foot
  volumes are correct in spirit, but the fix for "too quiet under action" is **ducking the bed**
  (§2.6), not turning feedback down further.

### 2.5 Ambient soundscape & avoiding the loop-tell

The acoustic-ecology work (A) notes environment sounds are the **predictable ground** against which
**volatile** action sounds (the *figure*) stand out — and that predictability itself is an
information channel (low volatility = low activity nearby). But a *too*-predictable loop becomes a
**tell** and breaks immersion. Kaushik (C) and Böttcher (D) both prescribe **controlled
randomization**: scatter one-shot ambient events (distant drips, creaks, a far-off skitter) on
randomized timers over the wind bed, and vary them, so the ambience reads as a living space rather
than a 12-second WAV on repeat. Expectancy violations (G) in the ambient layer generate low-grade
unease — perfect for backrooms horror.

### 2.6 Mixing: one bus, automatic ducking, and loudness discipline

The mixing thread runs through D and F. The professional pattern the papers assume (and that
middleware like Wwise/FMOD bakes in) is:
- **Buses/submixes** (music, ambience, SFX, UI) with a master, so volumes are *relative* and
  tuneable in one place — instead of ~10 hard-coded `*_VOL` constants that each fight the others.
- **Sidechain ducking / HDR**: when a loud/important sound fires (gunfire, growl, unit death), the
  music+ambience bed automatically dips for its duration, then recovers. This is *the* mechanism
  that lets feedback "punch through" without permanently quieting the bed (§2.4).
- **Loudness normalization** across our sample library so no single clip is wildly hotter/quieter —
  the game-audio equivalent of broadcast LUFS discipline. Our current per-clip `vol` constants are
  a manual, fragile stand-in for this.

Bevy 0.19 doesn't ship a full bus graph, but the pattern is approximable: group gains behind a few
resource-driven multipliers and implement a simple envelope-follower duck on the music/ambience
entities keyed off recent loud-SFX events.

### 2.7 Procedural / hybrid SFX to kill repetition and memory cost

Böttcher & Serafin (D): the #1 time/memory sink in game audio is producing *enough* footstep/weapon
variations to avoid perceived repetition across many surfaces × characters. Procedural + **granular
synthesis** solves both repetition and sample memory, and the interviewed audio programmers'
consensus is **hybrid**: keep the hand-crafted sample's character, add a procedural/granular texture
on top for infinite non-repeating variation. Our pitch-jitter is the poor-man's version of this and
is a legitimate first rung; the next rungs are (a) more sample variants per event, (b) round-robin
without immediate repeats, (c) light granular layering for continuous sounds (the swarm skitter is
an ideal candidate — a grain cloud whose density tracks crab count).

`W2781500572` adds a nice footnote: **biologically-variable** rhythmic cues entrain motor behaviour
better than perfectly-isochronous ones — i.e. our jittered footstep timing isn't just anti-repetition,
it's more natural.

---

## 3. Audio assets we already have

### 3.1 In-repo (`assets/audio/**`) — currently wired

All Ogg Vorbis (one decode path, no extra Cargo features). 22 clips:

| Category | Files | Used by `audio.rs` |
|----------|-------|--------------------|
| **music/** | `calm.ogg`, `combat.ogg` | binary swap in `update_music` |
| **ui/** | `select.ogg`, `select_all.ogg`, `deselect.ogg`, `move_order.ogg`, `invalid.ogg` | `play_sfx` (UI_VOL) |
| **weapon/** | `fire.ogg` | `play_sfx`, voice-capped 4/frame |
| **foot/** | `carpet_1..4.ogg` | `footsteps` shared throttled voice |
| **enemy/** | `growl.ogg`, `squitter.ogg` | `growl_stinger` (edge-triggered), `crab_squitter` (density-throttled) |
| **ambience/** | `wind.ogg` | always-on loop bed |
| **impact/** | `wall.ogg`, `splat.ogg`, `flesh.ogg`, `squelch.ogg`, `crunch.ogg`, `bone_snap.ogg`, `enemy_death.ogg` | `play_sfx` gore layer |

**Gaps in the wired set:** only **4** footstep variants and a **single** growl/squitter/fire clip —
prime repetition-fatigue candidates (§2.7). Only **carpet** footsteps (fine — the backrooms are
carpeted) but other floor materials (concrete/tile) have no variants. `flesh.ogg` and
`enemy_death.ogg` exist on disk but note `play_sfx` uses `splat`/`squelch` for those events — worth
auditing whether they're redundant or should be layered.

### 3.2 `codex_fs` game-assets library (`/mnt/codex_fs/game_assets/`) — the big pool

Catalogued at `/mnt/codex_fs/game_assets/CATALOG.md`. This is where we shop for upgrades.

**Music — `audio/music/` — 21 packs, ~5,048 files** (many are wav/ogg/mp3 mirrors, so unique-track
counts are lower). Most relevant to our horror/dungeon tone:

| Pack | Files | Fit for us |
|------|-------|-----------|
| `HorrorMusic_PT1/` (TheCarnival) | 25 | Uncanny carnival — on-tone for the "watcher"/smiley. WAV. |
| `HorrorMusic_PT2/` (Deadly Winds, Horror Ambiences) | 23 | Horror ambience beds → better than a single `wind.ogg`. WAV. |
| `Dark Fantasy 50 Tracks Pack/` | 300 | Minor-mode dread beds (§2.3). Loops + Tracks. |
| `50 Tracks RPG Music Pack/` | 255 | Has **"Ambiences & Dark Loops"** + **"Ambient & Action (+Loops)"** — ready-made calm/combat pairs *and* stems for vertical layering (§2.2). |
| `30 Open World Ambient Tracks/` / `Open World Ambient Music Pack/` | 183 / 129 | Sparse exploration ambience for the calm layer. |
| `Roguelike Music Pack/` | 120 | Genre-appropriate loops/tracks. |
| `100 Piano Game Tracks/` | 600 | Sparse solo piano = legato/minor "uncanny calm" for the watcher (§2.3). |

**SFX — `audio/sfx/` — 7 folders, ~855 files**:

| Folder | Files | Fit for us |
|--------|-------|-----------|
| `400 Sounds Pack/` | 400 | General library, 14 categories incl. **Footsteps (33)**, **Combat and Gore (17)**, **Weapons (15)**, **UI (30)**, **Materials (33)**, **Environment (24)**, **Human (17)**. This is our variation goldmine for §2.7. |
| `horror_sfx_vol_1/` | 53 | **Footsteps on carpet/concrete/leaves/metal**, creaking doors, **monster growls**, ambient wind. Directly extends our growl + footstep sets, and adds new floor materials. |
| `horror_sfx_vol_2/` | 32 | Footsteps: gravel, mud, stairs, wooden — more surfaces if the dungeon gains floor types. |
| `50 SFX Pack/`, `Game Sounds/`, `ogg/`, `wav/` | ~370 | UI clicks, combat, doors, fire, magic — UI + interaction variety. |

**Practical picks to close our biggest gaps:**
- **Growl variation:** pull several monster growls from `horror_sfx_vol_1` → round-robin the
  `growl_stinger` (currently one clip). *Directly attacks the most-heard repetition.*
- **Footstep variation & materials:** `400 Sounds Pack/Footsteps` + `horror_sfx_vol_1/2` give us
  many carpet/concrete/etc. variants → expand beyond 4, and enable per-floor-material footstep
  selection if the dungeon adds tile/concrete.
- **Ambient one-shots:** creaks/drips/distant skitters from `horror_sfx_vol_1` → the randomized
  ambient-event layer in §2.5.
- **Adaptive-music stems:** `50 Tracks RPG Music Pack`'s "Ambient & Action (+Loops)" and the Dark
  Fantasy pack are the most stem-friendly starting point for §2.2's vertical remix.
- **Watcher stingers:** percussive/uncanny hits (§2.3) — `HorrorMusic_PT1` carnival + `400 Sounds
  Pack/Musical Effects (111)`.

> Licensing note: several packs ship license PDFs (Medieval, Dark Fantasy, 15 Fairytale). Check the
> per-pack license before shipping any of these in a release build.

---

## 4. Gap analysis & prioritized backlog (research → our `src/audio.rs`)

Current `audio.rs` is genuinely good on the fundamentals the corpus cares about: message-driven
one-shots, **pitch jitter** (§2.7 first rung), **voice-capping** fire, **density-throttled shared
voices** for footsteps/squitter (avoids the "5 units = an army" bug), **edge-triggered** growl
stinger, and background-mute. What it lacks maps precisely onto the highest-ROI research findings.

| Prio | Upgrade | Research basis | Effort | Notes |
|------|---------|---------------|--------|-------|
| **P0** | **Spatialize world SFX** (pan + distance attenuation for growl, squitter, fire, impacts, footsteps) | A, E | M | Biggest immersion + information win. Bevy spatial audio; needs a listener on the camera + emitter transforms. Keep UI/music non-spatial. |
| **P0** | **Crossfade** calm↔combat instead of hard cut | B, C | S | Fade gains over ~1–2 s; smallest change with a clearly audible payoff. |
| **P1** | **Sidechain ducking** of music+wind under loud SFX (gunfire/growl/death) | D, F | M | Lets feedback punch through without further quieting the bed; fixes the "too quiet under action" tension the constants currently fight. |
| **P1** | **Continuous threat scalar** driving music (count × proximity of visible hostiles), not a boolean | B, C, G | M | Enables intensity that breathes; pairs with vertical layering. Faster tempo/ minor mode for higher threat (G). |
| **P1** | **Growl & fire sample variation** (round-robin, no immediate repeat) | D, F | S | Pull extra growls from `horror_sfx_vol_1`. Highest-frequency repeat offenders. |
| **P2** | **Randomized ambient one-shot layer** (creaks/drips/distant skitters over the wind bed) | A, C, D, G | M | Turns a looping bed into a living space; expectancy violations = unease. Assets in `horror_sfx_vol_1`. |
| **P2** | **Vertical music layering** (2–3 stems, gain-modulated) | B, C | L | The full adaptive-music build. Author/select stems from `50 Tracks RPG` / `Dark Fantasy`. |
| **P2** | **Mix-bus abstraction** (music/ambience/SFX/UI group gains + a master) replacing scattered `*_VOL` consts | D, F | M | Makes everything above tuneable in one place; foundation for ducking + loudness discipline. |
| **P3** | **Loudness-normalize** the sample library (offline pass) | D (mixing) | S | One-time; removes per-clip volume guesswork. |
| **P3** | **Per-floor-material footsteps** if the dungeon gains non-carpet floors | A, D | M | Assets ready in `400 Sounds Pack` / `horror_sfx_vol_2`. |
| **P3** | **Watcher percussive/legato stingers** tied to observe/attack states | G, H | M | Legato-sparse when unobserved, sharp percussive attack on strike (subconscious startle via percussiveness). Ties into `smiley-watcher-rework`. |
| **P4** | **Accessibility pass** — is the dungeon legible from audio alone? | I, H (Blind-AI test) | L | Nice-to-have; the "can an agent play from sound" test is a good design gauge of how informative our audio is. |

### Determinism / testing caveats (from `TESTING.md` + `CLAUDE.md`)

- Audio systems run on `Update` (they don't touch pinned `FixedUpdate` sim state), so they stay out
  of `snapshot_hash` — **keep it that way**: any spatial/ducking/threat-scalar logic must read sim
  state, never write it, or it'll break the deterministic-core hash gate.
- No `unwrap()` / panic paths — the current `audio.rs` is clean here (uses `is_some_and`,
  `unwrap_or(false)`); new asset loading + round-robin indexing must stay panic-free.
- Background-mute (`mute_when_background`) already keeps headless/CI/devshot runs silent — spatial
  audio must not regress that (a headless run has no listener; guard for it).

---

## 5. Suggested next actions

1. **Read the two anchor papers in full** before building: Grimshaw & Schott (A, spatial/ecology)
   and Khan et al. (B, adaptive music). Both are already embedded — `markdown_read` after
   `paper_download` if not yet converted.
2. **Ship the two P0s** (spatialization + music crossfade) as the first audio PR — highest ROI,
   contained blast radius, both align with the "one path" rule (they *replace* the flat/hard-cut
   paths, not add fallbacks alongside them).
3. **Stage assets from `codex_fs`**: copy a handful of extra growls + ambient one-shots into
   `assets/audio/` (Ogg) to unblock the P1 variation work, checking each pack's license.

---

## Appendix — References (home-still corpus)

- Grimshaw, M. & Schott, G. (2007). *Situating Gaming as a Sonic Experience: The acoustic ecology of First-Person Shooters.* DiGRA. `10.26503/dl.v2007i1.313`
- Khan, I., Nguyen, T. V., Nimpattanavong, C. & Thawonmas, R. (2023). *Adaptive Background Music for a Fighting Game: A Multi-Instrument Volume Modulation Approach.* arXiv. `10.48550/arXiv.2303.15734`
- Kaushik, K. (2025). *Procedural Music Generation in Video Games.* IJFMR 7(2). `10.36948/ijfmr.2025.v07i02.39384`
- Böttcher, N. & Serafin, S. *Procedural Audio for computer games* (home-still stem `Boettcher_Procedural_Audio`; see also Böttcher 2013, *J. Gaming & Virtual Worlds* 5(3):215–234, `10.1386/jgvw.5.3.215_1`). Cites Raghuvanshi, Lauterbach, Chandak, Manocha & Lin (2007), *Real-time sound synthesis and propagation for games*, CACM 50(7):66–73.
- Zotkin, D. N., Duraiswami, R. & Davis, L. S. (2004). *Rendering Localized Spatial Audio in a Virtual Auditory Space.* IEEE Trans. Multimedia. `10.1109/tmm.2004.827516`
- Nacke, L. / Grimshaw, M. et al. (2010). *[Effect of game sound and music on player experience]*, Interaction with Computers. `10.1016/j.intcom.2010.04.005`
- van der Zwaag, M. D., Westerink, J. H. D. M. & van den Broek, E. L. (2011). *Emotional and psychophysiological responses to tempo, mode, and percussiveness.* Musicae Scientiae / Psychology of Music. `10.1177/1029864911403364`
- Khan, I., Nguyen, T. V., Dai, X. & Thawonmas, R. (2022). *DareFightingICE Competition: A Fighting Game Sound Design and AI Competition.* IEEE CoG. `10.1109/cog51982.2022.9893624`
- Mason, Z., Green, D., Lindley, J. & Coulton, P. (2023). *Improving Digital Accessibility Through Audio-Game Co-Design.* DiGRA. `10.26503/dl.v2023i1.1922`
- (Adjacent) *Rhythmic auditory cueing / movement sonification* — home-still stem `W2781500572`.
</content>
</invoke>
