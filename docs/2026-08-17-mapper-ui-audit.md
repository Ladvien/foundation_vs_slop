# emerge-mapper UI audit — 2026-08-17

Four tabs reviewed in code and in captured frames, judged against the editor's own shared vocabulary in `crates/emerge-mapper/src/chrome.rs` and the standards in `docs/ui.md`. Method: one code catalog per tab (editor/fill, tiles/filter, compose, anim_tab/anim_plots) cross-checked against chrome.rs, plus one whole-frame devshot capture per tab from a live windowed build at `297edc7`, driven through `drive.request`. Every line number below was spot-verified against HEAD before this doc was written. The illustrated version with the four annotated captures is published at https://claude.ai/code/artifact/98c4df5d-b80d-4d30-8ee6-d7365fc05e73; the backlog items it produced were minted FVS-R-19 through FVS-R-23 and renumbered FVS-R-35 through FVS-R-39 at the 2026-08-17 merge (a parallel session had independently assigned 19-23; see BACKLOG_ARCHIVE.md for the mapping).

The one-line verdict: the vocabulary is good and the Status discipline is excellent, but adoption of the palette, spacing scale, and row shapes is uneven enough that the four tabs are drifting toward four dialects — the exact failure chrome.rs's own module doc was written against, one level up.

## What already works — the standard the rest should be raised to

- **The Status model** (`chrome.rs:190–369`): severity declared at ~210 write sites, problems outliving notes, banner + log as one list with one clearing rule, capped-and-counted overflow. Uniformly adopted.
- **The key census** and hold-`K` overlay: every binding is data, collision-tested, never retyped per panel.
- **The panel skeleton**: `panel_root` / `title` / `problem_banner` / `shortcut_hint` are used by all four tabs.
- **The spacing doc** in chrome.rs (crowding is ratio, not count — van den Berg, Cornelissen & Roerdink 2009, `10.1167/9.4.24`) states the right rule; the finding below is that two of four tabs never call the constants it governs.

## Behavioral defects (seven)

1. **The Compose body clips but cannot scroll.** `compose.rs:235–246` hand-copies `chrome::scroll_list` field-for-field but omits `ScrollArea` (`grep ScrollArea compose.rs` is empty), and `bevy_ui_widgets`' wheel handler only serves `With<ScrollArea>` (`bevy_ui_widgets-0.19.0/src/scrollarea.rs:23`). This is the longest generated pane in the editor; once it overflows, the overflow is unreachable. `scroll_list` takes `impl Bundle`, so `scroll_list(p, (ComposeBody, notice::CopyPane(Mode::Compose)))` is a drop-in fix.
2. **The triangle-count readout overlaps other tabs' content.** `editor.rs:854` spawns a second absolute root (bottom-right, `GlobalZIndex(100)`) tagged with no tab's root marker; in the Tiles capture "79,520 tris drawn" sits on the candidate list's bottom row. Either it is the Map's and should hide with `MapRoot`, or it is global and must dodge the list panel.
3. **Compose lines wrap to column zero, breaking their own indentation.** Structure in the pane is leading-space indentation on flat `Text` rows with no stated `TextLayout` and no `min_width: 0` — visible in the capture as an orphaned word at the left margin. The panel was widened to `TILES_CONTROLS_W` as a workaround (`compose.rs:208–211`); hex STALE lines and long fault messages still wrap wrong.
4. **A stage label clips against the panel edge.** `place_labels` projects world-space slot labels to screen space without testing against panel bounds; the Compose capture shows a half-hidden glyph at the panel's right edge.
5. **The STALE badge whispers on the tab strip.** `anim_watch.rs:340–366` rewrites the tab label to "ANIM (N STALE)" in the tab's normal color; the pane's own doc calls STALE "the one word here allowed to shout" and paints it `DANGER` there (`anim_tab.rs:947`).
6. **One VLM ghost line breaks the SUGGEST rule.** `tiles.rs:~4860` renders `token_proposals` in `DIM` under a comment saying everything in the block renders `SUGGEST`. Related: `tiles.rs:5419` sets `BorderColor::all(SUGGEST)` unconditionally and gates only the border width — one condition stated in two places.
7. **Hover exists as a hit-test everywhere and as feedback almost nowhere.** Every row, chip and field carries `Hovered` for the "is the pointer over UI" question, but only the tab strip and the map palette restyle on it — and both use the same unnamed color literal (below). Whether rows and chips get hover feedback is a design decision, not assumed here.

## Palette leaks — facts stated twice

| Leak | Where | Remedy |
|---|---|---|
| Row-hover grey, unnamed, twice | `editor.rs:1653` == `tiles.rs:2728`, byte-identical `srgb(0.16, 0.15, 0.14)` | a `chrome::ROW_HOVER` |
| `ENVELOPE_IDLE` is hand-halved ACCENT | `compose.rs:830` — exactly `ACCENT` ÷ 2, transcribed | derive it, or name it in chrome |
| Plot palette mirrors three chrome colors as bytes | `anim_plots.rs:49,53,57` — `BG`=SLOT_BG, `DANGER_INK`=DANGER, `SLOT_COLORS[0]`=ACCENT, each hand-transcribed with a comment claiming the link | a `const fn` chrome→`[u8;4]` bridge; slots 1–7 stay a legitimate categorical palette |
| `GRID_LINE` lives outside the palette | defined `editor.rs:63`, consumed by Compose (`compose.rs:1096`) | move to chrome — it is a two-tab word |
| One warm grey, two names | `editor::BOUNDS_LINE` == `tiles::CELLS` (0.42, 0.38, 0.30) | one name |
| `SLOT_BG` moonlights as the focus state | thumbnail ground (`editor.rs:1161`) and text-field-focused (`editor.rs:1420`, `filter.rs`) | a `FOCUS_BG` word, even at the same value |
| `dim_ink` re-inlined | `anim_plots.rs:585` re-derives ink/3 instead of calling `dim_ink()` | call it |

## Spacing — one scale, two tabs ignore it

Adoption of `GAP_TIGHT`/`GAP_ROW`/`GAP_GROUP`/`MARGIN`/`PAD` by grep: **tiles.rs 23 references, editor.rs 0, anim_* 0, compose ~0.**

- The 6/3 chip-and-row padding is an undeclared constant at twelve sites in five files (`tiles.rs:4520,4581,4635,5300,5324,5349,5411`, `filter.rs:107`, `editor.rs:1096`, `anim_stage.rs:726,754`, `anim_tab.rs:744`); the skip chip uses 4/1 (`anim_tab.rs:1036`), the cost readout and palette rows 8/4, the size field 6/2 — four padding rhythms with no rule between them.
- Inline restatements of named values: `MARGIN`'s 12.0 at `tiles.rs:2639`, `editor.rs:859`, `anim_tab.rs:961`; `GAP_TIGHT`'s 3.0 inside every 6/3 pair; the panel z-index 100 restated at `editor.rs:865`; `anim_plots::SHOW_W` = 356 is silently `TILES_CONTROLS_W − 2·PAD`, undeclared.
- Alignment facts stated N times: the anim slot column's 84px three times (`anim_tab.rs:1002,1078,1098`); `min_height: 18` five times; label columns 48px ×4 and 62px ×2.
- Compose has no spacing at all: five blank-`Text` rows stand in for `GAP_GROUP`, so every row is 2px from every other and the crowding rule can do nothing for the panel.
- `scroll_list`'s own `row_gap: 2.0` is off its own scale and copied inline in three tabs.

## Typography — six sizes, no role map

Sizes in use: 9/10/11/13/15/18. Role assignments diverge per tab: section headings are 9 (`chrome::section`), 10 (editor's hand-rolled "MAP SIZE"/"PLACE", anim's "MEASURED"/"RIGS"), or 11 (Compose's three hand-made headings — and the Compose panel shows two "COMPOSITIONS" headings, the real `section` plus an 11px twin). Label/value pairs are 10/11 (editor status, tiles MEASURED/mount) or 10/10 (tiles PLACEMENT) or flat 11 (Compose) — and anim renders its central declared/measured pairing inverted, declared 10 over measured 9. The filter box is 11px above 10px rows in Tiles/Anim. Compose's world labels introduce 12px, a size nothing else uses.

## The missing builders, with copy counts

`chrome.rs:554` already predicts the first ("the shape is repeated four times… not written yet because nothing has been moved onto it"); the count has since doubled.

| Builder that doesn't exist | Hand-rolled copies at HEAD |
|---|---|
| `label_value_row` — fixed label column + value | ~8 (editor status block `editor.rs:920–942`; tiles MEASURED/mount/mount-height/PLACEMENT, one already a local closure at `tiles.rs:5453`; anim slot rows) |
| `list_row(selected)` — 6/3 padding, `ROW_BG`/`ROW_SELECTED` | ~7 (map palette, tiles library/pack/candidate rows, filter box, anim rig rows) |
| `chip(state)` — button box + one 10px text child | 5 variants, two paddings |
| `text_field()` — `ROW_BG` box, focus tint, `{raw}_` ACCENT caret | 6 (`tiles.rs:4732,4755,4946,5046`, `filter.rs:191`, map size fields; `NameBox` a 7th cousin) |
| `list_heading(text)` — the 10px list-panel header | 3 ("PLACE" `editor.rs:1043`, "RIGS" `anim_tab.rs:312`, tiles' two 9px in-list headers) |
| `severity_rail(sev)` | 2 dialects: tiles (2px rail, Warn→ACCENT, Note→DIM, `tiles.rs:5522–5556`) vs anim (3px rail, Note→LABEL, `anim_tab.rs:856–903`) — same severity words, forked tint map |
| `scroll_list` (exists!) | hand-copied 3× anyway (tiles DetailPane `tiles.rs:2816`, anim SlotPane `anim_tab.rs:281`, compose — the copy that dropped `ScrollArea`) |

## Compose is a different program

The largest divergence is structural: Compose renders structure as ASCII in flat text (`">*"` gutters, leading-space indents, blank rows as spacers, monospace column padding via `{:>5}`/`" ".repeat(7)`) while the other three tabs render structure as layout (row backgrounds, `ROW_SELECTED`, padded nodes, section builders). Selection on Map/Tiles/Anim is a filled row; on Compose it is a `>` and a color. `docs/ui.md` §3.1 ("panels are rows, not strings") records the game paying for exactly this with one-TextColor panels. Any re-lay must be sequenced with FVS-R-15, which cuts most of the tab's verbs — restyle what survives, not what is about to be deleted.
