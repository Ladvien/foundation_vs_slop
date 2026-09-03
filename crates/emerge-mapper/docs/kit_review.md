# Kit door review — 2026-09-03

The seed, reported at the keyboard: *"I don't see a list of meshes as I press up and down arrows, but I
see the mesh change. Lots of issues here."*

Method: two read-only code reviews (MESHES + the shared selection/scroll machinery; TILES + COMPOSE),
cross-checked against a live editor driven over BRP — no window raised, no screen taken. Every entry
cites `file:line`. Where this crate's own doc comment states a decision, the entry quotes it, because
a documented decision is not a defect until the author says it is.

**38 findings: 26 DEFECT, 12 USABILITY.** Nothing has been changed. This document is the triage gate.

**The seed symptom is real and has two independent causes**, and neither is the one you would guess:
the scroll follower reveals a row **flush to the viewport edge** with zero context rows
(`chrome.rs:1631`), and the follower **only fires on a frame where the selection did not move**
(`chrome.rs:1599-1606`) — which a held arrow at a 30 ms repeat floor (`keys.rs:2933`) starves. Measured
live: after ~40 presses the selection is glued to the bottom pixel with 26 rows of already-passed
context above it and **zero rows of what is coming below**.

**Two findings outrank it**, and one of them I reproduced on camera.

---

## Live reproductions

Driven over BRP against the real `furniture` kit, 3396×1356.

### L1 — A second kit opened in one session shows the first kit's everything *(confirms M1, 1/1)*

Opened `furniture`, `Ctrl+O` back, opened `scp` — **a kit with 0 pieces**. The chrome bar correctly
reads `KIT · scp`. Every panel still showed `furniture`:

- inspector on `id hoover`, mesh `low_poly_furniture/glb/Miscellaneous/Hoover.glb`
- `270 mesh(es) not in the library — 89 with warnings`
- shelf chips `NOT IMPORTED (270)` / `MESHES (90)`
- the right panel on the **Tiles page** showing `kit/tile_1  2 member(s)`
- Hoover rendered on the stage, `Hoover.glb staged` in the status band

Capture: `kit_review/L1_second_kit.png`.

### L2 — The chooser offers kits that cannot be opened

The KITS column lists `furniture`, `scp`, `site`, `site_greybox`. `Enter` on `site` refuses:

```
cannot open …/foundation_vs_slop: no kit `site` in this project.
…/assets/emerge/kits.ron binds `furniture`, `scp`. Add it there, or open one of those.
```

The refusal is correct and well-worded — but the row was offered anyway, **and KIT INFO happily read
`pieces 45` and POLICY drew eight patch rows for a kit the project does not bind.** So two info panels
describe a kit that cannot be entered. Found live; on neither reviewer's list. Capture:
`kit_review/L2_unbindable_kit.png`.

*Scope note: this is the chooser's kit column, which you scoped out. Reported, not assumed in.*

### L3 — The TILES tab shows the MESHES tab's readout

On TILES with no tile open, the inspector's second line reads `270 mesh(es) not in the library — 89
with warnings, 0 unmeasurable`. That is the mesh scan summary, on a page about tiles, where it means
nothing. `refresh_lines` (`tiles.rs:6871`) repaints it unconditionally with no mode gate. Capture:
`kit_review/L3_tiles_scan_summary.png`.

### L4 — `↓` on the TILES page does nothing, twice, with no refusal

Two down-presses on a 2-row list (`+ New Tile`, `kit/tile_1`) left the cursor on `kit/tile_1` and said
nothing. `+ New Tile` is above the cursor and never takes it. Whether the walk should include the
`+ New Tile` row is a design question — see Q3.

---

## DEFECTS — high

| # | Finding | Where |
|---|---|---|
| **M1** | **A second kit of a session never rescans.** Both scan entrances gate on `!state.scanned`; `ImportState` is `Ownership::Door` but *nothing resets it* — `remove_from` drops only `Project`/`OpenMap`/`Door`/`Mode`. `candidates`, `folded_packs` and both cursors survive too, and `Enter` would import a mesh the new kit never scanned. `screen.rs`'s own `Ownership` doc names the hazard and declines to close it: *"fifty-six are not touched at all."* **Reproduced as L1.** | `tiles.rs:4008, 4071`; `args.rs:48-53` |
| **M2** | **`Shift+R` excludes the pack the cursor is not on.** `exclude_pack` derives the pack from the *mesh* cursor and never consults `focused_pack`, so standing on pack A's heading with `selected` still in pack B excludes **B** — and writes `project.ron` to do it. | `tiles.rs:4092-4095` |
| **M3** | **The seed.** (a) `scroll_to_reveal` puts the row's edge exactly on the viewport's edge — zero context. (b) `Follow` returns `pending` only on a frame where the selection is unchanged, so back-to-back moves at the 30 ms repeat floor silently drop a scroll; at any sustained frame time above 30 ms it fires **never**. The unit test only ever steps with a still frame between moves, and a second test pins flush as correct. | `chrome.rs:1628-1636, 1598-1606`; `keys.rs:2933` |
| **M4** | **Two rows highlighted at once whenever the cursor is on a heading.** `draw_pack` paints the mesh row from `ix == state.selected` without consulting `focused_pack`, so the mesh you came from keeps its fill *and* its accent rail. `put_cursor`'s own doc exists to prevent exactly this. The test that claims to hold the line only checks a field, never what is drawn. | `tiles.rs:7708` vs `7653` |
| **M5** | **`MESHES (90)` counts rows the list does not draw.** The chip counts the whole merged library; `library_rows` additionally filters to what this kit measured. The comment three lines above the count reads *"**The counts are of what is SHOWN.**"* | `tiles.rs:7482-7487` vs `6285-6292` |
| **T1** | **Every tile verb is a silent no-op with no tile open — including `Cmd+S`.** `build_keys` bails at the size guard with no status write, killing eleven verbs. Two routes reach that state. The refusal strings for it exist (`"no tile open — nothing to save"`) and are unreachable from the keyboard. | `build.rs:1424-1429` |
| **T2** | **Opening an anchored composition from the Tiles page bricks the tab.** `tile_rows` lists every composition unfiltered; an `Anchored` one (what the Map's `M` produces) opens, fails the size guard, and `refit` never converts it. There is no verb that closes a tile, so the only escape is opening a different one. | `build.rs:1424`; `tiles.rs:7160-7181` |
| **T3** | **Switching to MESHES with the Tiles page up leaves the shared list showing the kit.** `enter_tab` never clears `Build::browsing`, so the shelf resolves to `Shelf::Tiles` while the strip draws only `[Candidates, Library]` — **neither chip active**, the arrows walking a list that is not drawn, the follower chasing a third. The code contradicts the comment three lines above it. | `tiles.rs:7516-7527` |
| **T4** | **Destructive verbs fire on the hidden open tile while the Tiles page is showing.** `Del`, `Shift+Del` and `R` are stanceless, so they pass at `Stance::Browsing`: `Shift+Del` empties an off-screen tile. `Space` prints *"placing — arrows move the tile"* while the arrows still walk the tile list. | `build.rs:1281-1300`; `keys.rs:2035-2085` |
| **T12** | **The three red tile tests are macOS-only tests running on Linux — not a product bug.** All eleven `KeyCode::SuperLeft` literals sit inside exactly those three tests; `MOD_KEYS` is `ControlLeft` off macOS, so `UndoBuild`/`RedoBuild`/`Save` never pass `allowed()`. Every symptom follows exactly, including "expects 1, gets 2". The green sibling drives the same path through `MOD_KEYS[0]`. **`keys.rs:4183-4185` already states the rule these three broke.** Fix is ten substitutions. | `headless.rs:4100…4451`; `keys.rs:828-831` |
| **C1** | **Clicking a composition row leaves the member cursor pointing into the previous group.** The only one of four writers of `selected` that does not reset `member`; the other three each say why in a comment. | `compose.rs:1739-1749` |

## DEFECTS — medium

| # | Finding | Where |
|---|---|---|
| **M6** | A click on a candidate row leaves the heading cursor set — three readers, two answers. `put_cursor` and `focus_on` exist so this cannot happen; the click observers go through neither. | `tiles.rs:6390-6400` |
| **M7** | Both visibility guards switch off exactly when the filter matches nothing, leaving `Enter`/`Del` armed on an invisible row. Leaving the *cursor* put is the documented decision and is right; leaving the *verbs* armed is not. | `tiles.rs:6203, 6346` |
| **M8** | The `EXCLUDED` group cannot be opened from the keyboard — mouse-only, breaking the §4.2 parity rule on the one path back from an accidental `Shift+R`. | `tiles.rs:5990` |
| **M9** | The follower's "one frame late" contract is unordered against the rebuild. No `.before`/`.after` between `move_selection`, `rebuild_candidates` and `keep_selection_on_screen`. Wrong whenever a rebuild changes the row set. | `tiles.rs:3326/3379/3399` |
| **M10** | `autoscan_candidate` remembers an *index*; an import shifts the vector, so the next candidate silently has no lattice. `labels.rs` already learned this and keys by mesh path. | `tiles.rs:1853, 1866-1870` |
| **M11** | Undo restores the lists and not the cursor — the highest-trust verb reports success while pointing at the wrong piece. | `tiles.rs:266-269, 1258-1260` |
| **T6** | The MEMBERS list is the one list you cannot click, and still draws an ASCII `>` cursor — the exact defect `kit_row` fixed 700 lines up, in the list the destructive verbs act on. | `tiles.rs:7873-7900` |
| **T7** | A clicked row keeps keyboard focus, so the next `Enter` fires its `Activate` too — both `build_keys` and `on_kit_row` write, and whichever lands second wins. The crate documented paying for this once and made the tab chip not a `Button`; `KitRow` was then made one. `[INFERENCE]` on ordering. | `tiles.rs:7243-7250` |
| **T8** | A tile name prompt survives a tab change and takes the keyboard with it — `naming_keys` drains `Enter`/`Escape` off-tab while `Fields::typing()` still reports true. Recovery needs a mouse. | `build.rs:1897-1900`; `editor.rs:2546` |
| **C4** | The member ring is skipped for anchored groups, so walking members of one moves a panel highlight and nothing in the viewport. Anchored is what the Map's `M` produces. | `compose.rs:1305-1310` |

## DEFECTS — low

| # | Finding | Where |
|---|---|---|
| **T10** | `build_detail` reads `Project` values the pane is not rebuilt for — masked today by an unrelated resource being written most frames. | `tiles.rs:7755-7800` |
| **T11** | Thirteen observers take non-optional `ResMut` of `Ownership::Door` resources. Safe **only** because the teardown removes four resources and not the fifty-six the table declares. Latent; high the day teardown is completed. Also: `Build`/`TileHistory` survive a door change, so one kit's undo stack reaches the next. | `tiles.rs` ×11, `compose.rs` ×2 |
| **C7** | `clamp_selection` clamps the group cursor and never the member cursor; `rebuild` hides it by clamping the highlight while the scroll follower searches unclamped. | `compose.rs:248-259` |

## USABILITY

| # | Finding | Where |
|---|---|---|
| **T5** | The flush-reveal half of M3, reaching the kit list and both Compose lists. `every_list_follows_its_selection.rs` asserts the row is *inside* the viewport, which a flush row satisfies — the ratchet cannot see it. | `chrome.rs:1626-1646` |
| **C2** | Focus can be handed to an empty MEMBERS list; the arrows then answer nothing while the pane still says `<- arrows`. | `compose.rs:457-472, 503-505` |
| **C3** | `Enter` arms a composition and puts the receipt below the fold — the one verb the tab exists for confirms itself where you may not be looking. | `compose.rs:1668-1671` |
| **M12** | `Space` stops folding packs while a derivation is staged, and only the keyboard loses it — the mouse still folds. | `keys.rs:1499-1508` |
| **M13** | `←`/`→` go dead in the filter box while `↑`/`↓` do not, on a shared badge row that promises both. | `keys.rs:1412-1449` |
| **M14** | Two readouts name their chord by hand instead of reading the census — `"Del removes it"` and `"Shift+L resumes"`. Correct on Linux today, a lie the moment a chord moves. | `tiles.rs:6386, 7311-7313` |
| **M15** | A comment on the walk states a false fact about `Repeat` — a constraint that does not exist, which the next reader will preserve. | `tiles.rs:5777-5780` |
| **M16** | `←` has no refusal on an empty candidate shelf where `→` has one; the chip arms have the same asymmetry, and the code demanding the symmetry is three lines away. | `tiles.rs:5799-5802` |
| **T9** | `Esc` on the Tiles page goes *forward*, not back — into T1's dead state. The page's own back key is `←`. | `build.rs:1390-1398` |
| **C5** | The Compose lists wrap where every other list stops; the crate argues against wrapping twice, in the two places it made the choice. | `compose.rs:510-519` |
| **C6** | Compose's Shift×5 stride is read outside the census, so no badge can announce it — a working accelerator nobody can discover. | `compose.rs:481` |
| **C8** | `split_indent`'s doc names a caller deleted in the same commit that removed it. | `compose.rs:1767-1770` |

---

## What is *not* wrong

Recorded so nobody re-opens it: the pack header counts and the `NOT IMPORTED (n)` count are honest;
the scan summary, action line, severity marks and label progress all repaint on every path that
changes them; every `Context::Meshes` binding has a live handler with no collisions and every
`ControlId` has an anchor; all six `compose::Line` variants are reachable and `Line::Prose` no longer
stands in for the face table; and there is **no user-reachable `unwrap`/`expect`/panic** in the
production paths of `tiles.rs`, `build.rs` or `compose.rs`.

---

## Part 2 — Triage and spec

Answered at the keyboard, 2026-09-03. **Everything is in scope** — all ten bundles, A through J —
plus standing latitude: *"any other missing pieces we determine would make the user experience
better."* Where this section disagrees with a finding above, this section wins.

**Verification is visual and continuous, not final.** Every fix is confirmed with a zoomed, cropped
`bevy_debugger/screenshot` of the surface it changes, read back — the same method that found the
flush edge and the `KIT · scp` bleed in the first place. A fix that compiles and passes a test but
was never looked at is not done.

**Baseline**, measured on a clean worktree of `HEAD` and re-measured after pulling 13 commits (which
touched neither `emerge-mapper` nor `emerge-core`, so every `file:line` above still resolves):
`emerge-mapper --test headless` at **152 passed / 4 failed**, every other binary green.

### S1 — Scrolling: two rows of context, and a page-jump re-centres *(A: M3, T5)*

Reveal so **at least two rows** sit between the selection and the edge it is approaching, so you can
always see what is coming. A move of more than a page — `Shift`×5, a click, a filter change —
**re-centres** instead of creeping. And the follower must survive **back-to-back moves**: arming on a
change and firing only on a still frame is what starves it at the 30 ms repeat floor.

### S2 — A door change is a new session *(B: M1, T11's resource half)*

Opening a kit resets everything the previous kit owned: rescan from scratch, candidates, both
cursors, staged mesh, undo stack. **Folds are re-seeded by the rule `scan` already documents** — only
the packs this kit builds from start open. Per-kit memory of cursors was considered and declined: it
needs somewhere to keep per-kit UI state and this is already the biggest change on the list.

### S3 — A verb acts on the visible cursor, or refuses *(C: M2, M7)*

`Shift+R` excludes the pack **the highlight is on** — the heading if one is focused, otherwise the
selected mesh's pack. `Enter`/`Del` **refuse out loud** when a filter has hidden the selection rather
than acting on a row nobody can see. **The cursor still never jumps on its own** — the crate's
existing argument against jumping on a half-typed query stands; the refusal goes in the verb.

### S4 — One cursor, one writer, keyed by identity *(D: M4, M6, M10, M11, C1, C7)*

Every path that moves the selection — arrow, click, import, undo, filter — goes through the single
writer that already exists (`put_cursor` / `focus_on`). Remembered cursors key on the **mesh path or
id, never a vector index**, which is the lesson `labels.rs` already learned. **Exactly one row is
ever highlighted.**

### S5 — The Tiles page *(E: T1, T2, T3, T4, T9)*

- Anchored compositions **are listed and marked**, and `Enter` refuses by name — *"`{id}` claims no
  tile — open it on COMPOSE"*. Not hidden (the row is real) and not silently converted (that would
  rewrite what the author captured).
- Every verb that declines **says so**, using the refusal strings that already exist and are
  currently unreachable. **A close verb is added**, so "no tile open" is a state you chose rather
  than one you fell into.
- **A page you can see owns the keys.** Tile-editing verbs are gated on the tile being on screen, and
  `enter_tab` clears the browsing state so MESHES always shows meshes. Verbs that do not apply vanish
  from the badge overlay.

### S6 — A number counts exactly what is under it *(G: M5, L3, M14)*

The chip count comes from the same builder that draws the rows; a readout belonging to one tab is
drawn only on that tab; chords are read from the key census, never hand-written. **And a filter says
so**: `MESHES (12 of 90)` while narrowing, so a short list cannot be mistaken for the end of a list.

### S7 — The MEMBERS list is a list *(H: T6)*

`chrome::list_row`, clickable, accent-rail selection, ASCII `>` gone — the same treatment `kit_row`
got 700 lines up, in the list `Del` and `R` act on.

### S8 — The chooser shows unbindable kits, marked *(I: L2)*

The row stays visible — the directory does exist — dimmed and marked unbindable, with the refusal
naming `kits.ron`. **Info panels stay blank for it**, where today KIT INFO reads `pieces 45` and
POLICY draws eight rows for a kit that cannot be entered. Offering to bind it was declined: that
writes a project file from a row whose whole job today is to refuse.

### S9 — `+ New Tile` is a row you can walk onto *(L4)*

The arrows reach it and `Enter` starts a tile, the same shape the chooser's `+ new kit` / `+ new map`
rows already have.

### S10 — The remaining parity items, all in *(H)*

- The name prompt can **always** be escaped (T8) — today `Esc` is drained off-tab and recovery needs
  a mouse.
- `Space` keeps folding packs while a derivation is staged (M12) — today the keyboard loses it and
  the mouse keeps it.
- `←`/`→` keep working in a filter box (M13) — one badge row promises both halves.
- The `EXCLUDED` band opens from the keyboard (M8) — the one path back from an accidental `Shift+R`.
- Compose lists **clamp** instead of wrapping (C5) — the crate argues against wrapping twice.
- Arming a composition confirms itself **in view** (C3).
- Compose's `Shift`×5 goes through the key census (C6), so the badge overlay can announce it.

### S11 — The rest of the list, unchanged from the finding *(F, J)*

T12 (ten `MOD_KEYS[0]` substitutions), T10, the observer half of T11, C4, C8, M15 — each is built as
the finding describes it, with no design question outstanding.

---

## Part 3 — What changed

*(Filled in as the work lands, with before/after captures.)*
