# Handoff — emerge-mapper usability, and the guided-feedback loop that found it

**Written 2026-08-15, branch `tiles-arrows-and-kit-tab`.** Everything described as shipped is
committed-ready and green: `cargo test -p emerge-mapper` 300 passed, `cargo test --workspace` 2,499
passed / 0 failed.

Read this before touching `crates/emerge-mapper/src/{editor,build,keys,tiles,chrome}.rs`. The
crate's own rules are in `crates/emerge-mapper/CLAUDE.md`; the Tiles tab's contract is
`docs/tiles_tab_contract.md`; the guide channel's fuller writeup is `docs/2026-08-13-handoff.md`.

---

## 1. What this session actually was

Two days of **guided feedback sessions**: the agent posts a script over `bevy_debugger/guide`, the
editor renders one step at a time on the author's own window, host-named checkpoints advance it, and
the transcript is `k/n` per step. The author drives; the agent watches state, asks the judgement
questions in chat, and fixes what is reported — often within the same sitting.

**It works, and the evidence is the defect list below.** Nine distinct usability defects were found
in about three hours at the keyboard. Six of them had *passing tests over the exact code path*.
None would have been found by reading the source, and several had survived months of green suites.

The loop's own mechanics, and its traps, are in `docs/2026-08-13-handoff.md` and
`~/.claude/.../memory/guide-channel-agent-to-human.md`. The one rule worth repeating here: **a step
with `checkpoint: null` never self-advances**. Ask it in chat, by name, and wait for a real answer.

### Guides that exist (`crates/emerge-mapper/guides/`)

| file | steps | state |
|---|---|---|
| `author_a_tile.json` | 8 | the original; still current |
| `author_the_site_kit.json` | 8 | uses `with` args |
| `tile_feedback.json` | 13 | **walked to step 7 of 13**; steps 1–7 approved |
| `repair_the_kit.json` | 5 | **complete, 4/4 checkpoints passed**; tile_4 repaired on disk |
| `place_and_generate.json` | 8 | **walked to step 3 of 8** — the Map half is unexercised |
| `derive_edges.json` | 6 | **never posted** — the Meshes/lattice loop is unexercised |

Every one is driven headlessly before it reaches a person
(`the_*_script_can_actually_be_followed` in `tests/headless.rs`). Do not skip that: the first time a
script was written from memory of the editor, **4 of its 10 steps were wrong** and every one passed
the name check.

---

## 2. The through-line: this is a discoverability problem, not a keybinding problem

The author's own words, 2026-08-15, after asking for two verbs that already existed:

> "It sounds like all of this is a usability concern. Could we research in home still how to make it
> easier for users? While not giving up shortcut keys. Maybe we look at adding UI guidance, but
> ensure there are shortcut keys for everything."

That is the brief. **Two decisions are already made** (asked and answered in session):

1. **Always-on contextual hint line.** A persistent line in the panel naming the 3–4 verbs that
   apply to *what you are holding right now*, changing with stance. No key required to see it. The
   `K` overlay stays as the full reference.
2. **The per-tile clear (`Shift+Delete`) is not missing — it is unfindable.** The ask was
   explicitly "make it findable", **not** a new kit-wipe verb. Do not build a destructive
   wipe-the-kit command; it was offered and declined.

**Nothing about the keyboard is to be given up.** Every verb keeps a shortcut; the hint line is
additive.

### Why the author kept hitting this

Two verbs were asked for that had been bound the whole time:

- **`R`** — turn the focused member a quarter, on the Tiles tab.
- **`Shift+Delete`** (`Shift+Backspace` on macOS) — empty the tile.

Both live on **one collapsed census row** reading `"turn / remove this / Shift: empty the tile"`,
visible only while `K` is held. The census's row-collapsing — which exists to keep each context
under a 12-row learnability cap — is *itself* part of the problem: it compresses three verbs into
one line whose phrases you must pair off against chords by position.

---

## 3. The research (home-still), and the honest gap

**What the corpus already has, and what it says:**

- **Carroll, *Creating Minimalist Instruction*** (`10.14434/ijdl.v5i2.12887`) — guided-exploration
  cards: a few hints toward a concern, a **checkpoint**, and **error recovery**, conveying that
  "errors and error recovery are standard and routine, not failures or crises." This is already the
  `Step` schema (`label`, `goal`, `do`, `checkpoint`, `recovery`) field for field.
- **Andersen et al. 2012, *The impact of tutorials on games of varying complexity***
  (`10.1145/2207676.2207687`, N=45,318) — **in-context instruction beats up-front manuals**;
  restricting freedom bought nothing; an **on-demand help button cost 12% of levels**. This is the
  direct argument for an always-on line over a help affordance, and it is why the guide channel has
  no gate and no help button.
- **Kennedy et al. 2015, *Removing the HUD*** (`10.1145/2793107.2793120`) — non-diegetic elements
  scaffold novices but **distract experts**, who did better without them. **This is the caveat on
  the hint line**: it must be dismissible/fadeable, or it becomes the thing that costs the author
  once they know the verbs. Do not ship a permanent line with no way off.
- **Vicente & Rasmussen, *Ecological interface design*** (`10.1109/21.156574`) — already cited in
  `BACKLOG.md` (FVS-R-27): the perceptual cues in the interface should *directly specify* process
  constraints. The colour half of the author's ask lands here.
- **Horn et al. 2017, *Adapting Cognitive Task Analysis…*** (`10.1145/3116595.3116640`) — "power
  tools" and shortcuts can *prevent* skills from being learned, because the shortcut satisfies the
  early cases. Relevant to which verb the hint line should name first.

**The gap, stated plainly: the corpus does not have the hotkey-adoption literature.** Searches for
shortcut discoverability and feedforward returned noise (LLM-coding-tool adoption papers, planning
papers). The two canonical works were located externally:

| paper | DOI | status |
|---|---|---|
| Cockburn, Gutwin, Scarr, Malacria 2014, **Supporting Novice to Expert Transitions in User Interfaces** | `10.1145/2659796` | **paywalled — no OA PDF.** `paper_download` fails. Get it another way. **This is the single most relevant paper to the brief.** |
| Malacria, Bailly, Harrison, Cockburn, Gutwin 2013, **Promoting Hotkey use through rehearsal with ExposeHK** | `10.1145/2470654.2470735` | **downloaded, converted and INDEXED** (19 chunks) — searchable now. |

The Cockburn survey's frame is the one to design against — it names four routes to expertise:
*intramodal* (practice with one method), *intermodal* (switching to a higher-ceiling method — the
mouse→hotkey transition), **vocabulary extension** (learning more verbs), and *task mapping*.

**Read the ExposeHK paper before designing the hint line — it is indexed, and it reframes the
brief.** What it establishes, in its own words:

- **Kurtenbach's principle of rehearsal**, which is the design rule for this entire ask:
  *"guidance should be a physical rehearsal of the way an expert would issue a command."*
  The damning corollary: *"Traditional hotkey methods require users to discover hotkeys using a
  non-hotkey modality (pointing), and consequently **users rehearse pointing, not hotkey use**."*
  Any guidance that teaches a verb through a channel other than its own keystroke trains the wrong
  motion.
- **Pointer compulsion is real and strong.** In their pilots, participants *"completed selections
  using the pointer despite clearly displayed instructions to use hotkeys, and continued to do so
  until verbally instructed"*. Displaying a shortcut does not, on its own, cause its use.
- **EHK's own weakness is discoverability**: it *"has no visual representation to aid discovery
  until the user accidentally or deliberately presses its modifier-key trigger."* So rehearsal and
  discovery are two problems, and solving one does not solve the other.
- **It supplements rather than replaces**: *"largely compatible with existing designs… allowing
  users to maintain existing interaction strategies without performance detriment, but also
  offering a higher performance ceiling."* That is exactly the author's "without giving up shortcut
  keys" constraint, stated from the research side.
- They deliberately avoided **post-action feedback** as a distraction, and avoided imposing a cost
  on the pointer path. Do not "helpfully" nag after the fact.

**The distinction that decides the design here:** ExposeHK targets *intermodal* transition — a
mouse user who should become a keyboard user. **emerge-mapper's author is already a keyboard user
who does not know the vocabulary.** That is Cockburn's **vocabulary extension**, a different
quadrant. So:

- For **verbs with no mouse path at all** (`R`, `Shift+Delete`, `J`, `,`/`.` — most of this editor),
  ExposeHK's overlay has nothing to attach to. The always-on hint line **is** the right instrument,
  and the rehearsal principle is satisfied trivially: the line names a key, and pressing that key is
  the expert action.
- For the few verbs that **do** have a mouse path — palette rows (clickable, and arrow-walkable),
  the KIT list, tab chips — apply EHK properly: **display the chord on the control itself**, so
  clicking the thing teaches the key that replaces clicking it. This is cheap here because
  `keys::rows()` already knows every chord, and it is the one place this editor genuinely has the
  pointer-vs-keyboard split the paper is about.

**Also worth ingesting** (not yet in corpus, not yet searched for DOIs): Vermeulen et al. 2013 on
**feedforward** (telling the user what an action *will* do before they do it — distinct from
feedback), and marking-menu rehearsal work (Kurtenbach & Buxton; M3 Gesture Menu
`10.1145/3173574.3173823` is in reach and cites the lineage).

---

## 4. What shipped this session (all green, all pinned)

Archived as **FVS-R-28 … FVS-R-33** in `BACKLOG_ARCHIVE.md`, each with the author's verbatim words
and the test that holds it. Summary:

**The arrow ladder (FVS-R-28)** — the big one, and **explicitly approved**: *"THIS IS WHAT I
WANT!!"*. Plain arrows on the Tiles tab walk stops dividing the span between the tile's **centre**
and the focused piece's **flush** position; `J` deepens by thirds (span → ⅓ → ⅑ → wraps,
`DEPTHS = 3`). Centre and flush are exact stops at every depth. `Build::rung: SnapLevel` became
`Build::depth: u32`. `build::flush_reach` is the single expression both `aligned()` and the ladder
read, so the flush verb and the ladder's terminal stop are **bit-identical** by construction.

**Held-piece brightness (FVS-R-29)** — `editor::HeldPiece` + `brighten_held`, emissive +0.10,
hue-neutral, resolving on `Escape`. **The value is a first guess at "very subtle" and has not been
judged at the keyboard.**

**The judgement card (FVS-R-30)** — the `-> yours to judge` line rendered in the title's own gold at
smaller size; the author stalled on it anyway (*"I'm stuck on step four"*). Now full-size, in a blue
nothing else on the card uses. **Second finding on the same line** — camouflage is a real failure
mode.

**Dead-key strings (FVS-R-31)** — the Tiles arrival note named `T F G H` (Meshes keys); the KIT
strip promised `left back` with no such binding. **The census cannot catch prose**, which is how
both survived.

**The shared list panel (FVS-R-32)** — scroll-follow regated to the panel not the tab (it was fixed
for one of the two tabs sharing the list); `MESHES | KIT` strip moved out of the scroll container;
the picked mesh now ghosts **while choosing**, not only while placing.

**Click-to-move, one rotate cluster, one grid (FVS-R-33)** —
- A clear cursor (Place tool, no brush, no armed composition) makes a click pick a piece up and the
  next click set it down. `editor::cursor_is_clear` is the named predicate.
- `Z`/`C` **retired**. `R`/`T`/`Y`/`U`/`V` now address **the ghost when one is armed, the piece
  under the cursor otherwise** (`editor::ghost_is_armed`). `AimReset` → `Straighten`. The ghost
  gained a **tip** (`EditorState::brush_tip`) — it had a yaw and no tip, so `Y`/`U` could only ever
  act on a placed piece. Map census 12 → 11 rows.
- **One grid at the live rung**, over the whole map. The old major/minor pair (tile rung everywhere
  + fine window at camera focus) read as two answers.

**The fill box (FVS-R-34)** — `fill::covered_rect`: the drag outline now
covers the **ground the fill will cover** rather than the anchors it will place on, so on a
cell-sized brush its corners land on cell corners. Reported as *"falls in the center of each tile…
I would expect it to fall on the corners"*.

**`chrome::scroll_to_reveal`** — the fold arithmetic both list-follows share, six unit tests.

---

## 5. Open — in the order I would take them

### 5.1 The hint line (decided, unbuilt)

Build the always-on contextual line. Constraints, from the research and from this codebase:

- **It must be generated from the census** (`keys::rows`), never hand-written. Two hand-written
  strings are exactly what FVS-R-31 was, and `crates/emerge-mapper/CLAUDE.md` records the repo's
  standing rule that a second census is the thing this crate keeps deleting.
- **Name the chord and the verb, and nothing else** — the line's job is vocabulary extension, and
  per the rehearsal principle the key it names must be the same key an expert presses. No "click
  here instead" affordance, no post-action nagging (ExposeHK deliberately avoided both).
- **The collapsed row is the enemy of this.** `"turn / remove this / Shift: empty the tile"` is
  three verbs in one string, paired to chords by position — the form that hid `R` and
  `Shift+Delete` from the author for two sessions. The hint line should emit **one verb per line**
  even though the `K` census collapses them; `rows()` keeps the chord list, so the split is
  available without a second source of truth.
- **Stance-aware** — `keys::Live(Context, Stance)` already carries what is in hand. The `K` overlay
  is already rebuilt on stance change (`chrome.rs`, `ShowingFor`); reuse that trigger.
- **3–4 verbs, not the whole row set** — the alert-budget argument in `docs/ui.md` §3.4, and
  Kennedy's expert-distraction finding. Rank by what the current stance makes *possible* and what
  has not been used yet this session.
- **Dismissible.** Kennedy et al. is the reason. An expert must be able to turn it off, and the off
  switch must itself be a shortcut.
- Panel space is scarce; `docs/ui.md` §5 has the layout traps.

### 5.2 The colour half (undecided)

The author asked for *"color to make it more intuitive"* and chose the hint line **first**, not
instead. The colour language today: amber `ACCENT` for focus, the new held-brightness, `DIM`/`TEXT`
for panel copy, red for refusals. Vicente & Rasmussen is the grounding. **Ask before building** —
the repo rule against assuming design decisions is explicit, and a global palette change was
rejected once already on this project (see the SCP colour-scheme memory).

### 5.3 Unexercised surface — the guides are written, just unwalked

- `place_and_generate.json` from step 3: the **Map** half — placing by mouse, the piece verbs, and
  **`Cmd+G` → the solver lays your own kit → Enter/Esc commit door**. Untested at the keyboard:
  **click-to-move, the new rotate cluster, and the single grid all shipped after the last time the
  author drove the Map.**
- `derive_edges.json`, never posted: select `site/floor`, `B` to stage a derivation, `Enter` to
  write tokens onto the measured lattice. The least-walked loop in the editor.
- `tile_feedback.json` from step 7.

### 5.4 Judgement calls waiting on the author

- Is the held-piece brightness right at +0.10?
- Does the single grid **wash** at the finest rung? (288 lines each way over 32 m — the measured
  reason the window existed. If it washes, the fix is extent or colour, **never** a second grid.)
- Do the ladder stops read correctly on the Map now that the grid draws only the live rung?

### 5.5 Known gaps found while working, not yet fixed

- **`fill::box_fill` takes a yaw and no tip**, so a box fill with a tipped brush lays untipped rows.
  Pre-existing; the new `brush_tip` makes it reachable.
- **The removal drag box** has the same anchor-to-anchor drawing the fill box just had — but
  removal genuinely *deletes by anchor containment*, so fixing the drawing means deciding what it
  deletes. Not touched: that is a design call, not a drawing bug.
- **Tiles cannot be renamed** (`site/tile_N` is generated). Named in
  `docs/2026-08-13-handoff.md` as next-thing #2, and the new KIT list makes it worse.

---

## 6. Operational notes that will cost time if missed

- **Run the editor:** `cargo run -p emerge-mapper -- . untitled_map --kit site`. The `debugger`
  feature is **on by default**, so BRP is live on 15702.
- **Post a guide:** `scripts/guide_post.sh crates/emerge-mapper/guides/<file>.json` — it waits for
  BRP. **Never `cargo run &` then curl.**
- **A watcher must be attached for checkpointed steps to advance.** `bevy_debugger/guide+watch` is
  SSE (`data: {json}\n\n`), single-consumer, and **latched once per step**. `{"read": true}` is the
  authoritative, reconnect-safe position poll. The watcher dying silently is why one session
  appeared to stall.
- **The window will not raise itself and must not be raised.** The author clicks it. Closing the
  window ends the guide — its state lives in the app.
- **`cargo test --workspace` matters**, not bare `cargo test` (root package; crate tests would not
  compile). Check `lsof -nP -iTCP:15702 -sTCP:LISTEN` first — a live editor inverts
  `bevy_debugger_mcp::test_highlight_entities`, and you must not kill the author's editor to make it
  pass.
- **Test-driving keys: latch your press systems.** Two tests here held a chord down every frame and
  passed only by Bevy's arbitrary system ordering; adding one unrelated system flipped them. Use the
  `Local<bool> done` pattern.
- **`Node` requires `ScrollPosition` in Bevy 0.19**, so "has a scrolling ancestor" must be asserted
  on `Node::overflow`, never on the presence of `ScrollPosition`.
