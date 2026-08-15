# Vetting the `emerge-mapper` chooser plan

Checked against the home-still corpus, `docs/ui.md` §3.5/§5/§7, and the crate source on disk.

**Verdict: the architecture is right and under-argued; the research is mostly right and
mis-sourced; two of the "measured" facts are not what was measured.** The one thing that should
actually change in the design is the flat ban on recency ordering — it rests on the only citation
that does not cover this widget class, and the corpus's best evidence for linear lists runs the
other way.

---

## Outcome — added after the work, 2026-08-15

*The review below is kept verbatim as it was written, before the chooser existed. This block is the
only thing added; it records what came of it, so the "What to change" list at the end is not read as
outstanding.* Shipped in **`b8f9138`**.

**Every item was acted on.** §1's ordering recommendation is the one **not** taken, and the reason is
that its own paper settled it the other way. Sears & Shneiderman 1994 was fetched, converted and
indexed exactly as §1 asked — and it declines to split a menu of this shape: *"if all items are
selected with near equal frequency, minimal benefits would be expected"*, the advantage *"will
increase with menu length and with more skewed distributions"*, and their guideline caps the
high-frequency zone at four items, which with four kits is the whole list. Their experiment ran
fifteen-item menus for "a few tenths of a second". So the list stayed fixed-alphabetical — but the
justification moved off Samp, which was §1's actual complaint and was correct.

The rest, briefly: per-row letter keys were dropped entirely rather than made stable (§2a — arrows
and `Enter`, so there are no positional bindings to reshuffle); the rehearsal claim is re-sourced on
ExposeHK and softened (§2b); the counts were wrong and are gone, with the architecture argued from
gating **cost** rather than impossibility (§4, §5); the three repurposed invocations are now stated,
and `--kit` preselects (§6); the NAME field starts empty (§7); spawn-and-wait replaced `exec` (§8).

**Two of §8's "unverified" items are now measured**, and both were right to flag:

- The 2,524 figure was the sum of `test result: ok. N passed` across a real `cargo test --workspace`
  run — every target, not `#[test]` attributes. It is **2,548** after this work.
- The kit piece counts are `emerge` 75, `site` 45, `site_greybox` 45, `site_v2` 0 — pinned by
  `the_chooser_sees_the_shipped_kits`, which deliberately does **not** assert the exact numbers for a
  populated kit, since importing a mesh is what the editor is *for*.

**`docs/ui.md` was corrected in three places** on the strength of this review: §3.5's Samp
over-generalisation now carries the Sears & Shneiderman finding, and §7's two stale entries
(Itthipuripat is on PMC and is now in the library full-text; the VR-menu paper had in fact converted)
are fixed. Samp itself still will not download; §7 records the manual route.

---

## 1. The load-bearing citation is about radial menus

`ui.md` §3.5, verbatim:

> Samp 2011 adds that radial's cost is paid at first sight — so fix item positions permanently and
> never reorder by recency.

Samp 2011 is *The Design and Evaluation of Graphical Radial Menus*, Krystian Samp, NUI Galway PhD
(`10.13025/17344`). Its own abstract:

> the improved navigation performance in **radial menus** comes at the cost of slower visual search
> … with a consistent pattern of organizing items **in radial menus**, search and navigation
> performance can be further improved.

That is a finding about layouts whose cost is paid at first sight. The chooser is a two-column
linear list. §3.5's sentence is a design conclusion drawn *in a radial context*, and the plan
promotes it to a general rule — "This rules out the obvious 'recent maps first' list" — which is the
single most design-determining move in the document.

For **linear** menus the corpus points the other way:

| Work | Finding |
|---|---|
| Sears & Shneiderman 1994, *Split menus* (`10.1145/174630.174632`) | "split menus were **significantly faster than alphabetic menus** and yielded significantly higher subjective preferences"; 17–58% time reductions in two in-situ studies |
| Liu, Bailly & Howes 2017 (`10.1145/3025453.3025707`) | menu usage is Zipfian; an item's selection time depends on the frequency distribution of the *whole* menu, not just its own rank |
| Findlater & Gajos 2009 (`10.1609/aimag.v30i4.2268`) | adaptive-GUI results are **mixed** and hinge on prediction accuracy — the real caution, and it is a caution about *automatic* prediction |

Read together these do not say "reorder by recency". They say: a small **stable** high-frequency
zone above an otherwise fixed list beats a purely alphabetic list, and the risk lives in *automatic*
prediction rather than in reordering as such.

**Recommended change.** Replace the blanket ban with a split: a short top zone that is
*user-pinned* (or simply last-opened, one row), then the full alphabetical list, fixed, below.
Positions in the main list never move — which is what §3.5 was protecting — and the fast case stops
costing a scan. If the split is rejected, reject it on the Findlater & Gajos prediction-accuracy
argument, which is the one that applies, not on Samp.

None of these four are in the library. Samp is openly downloadable (`hdl.handle.net/10379/2672`);
the others are one `paper_search` away.

---

## 2. The better source for the third finding is already in the library, full text

`10.1145_2470654.2470735` — Malacria, Bailly, Harrison, **Cockburn** & Gutwin, *Promoting Hotkey Use
through Rehearsal with ExposeHK*, CHI 2013 — is **downloaded, converted and indexed**. It shares two
authors with the Cockburn 2014 survey and is one of the empirical papers that survey summarises.
§7 lists Cockburn 2014 under "no open-access PDF for the DOI at all" and §3.5 falls back on
Kurtenbach & Buxton 1994, also not in the library. ExposeHK quotes Kurtenbach's principle verbatim —

> guidance should be a physical rehearsal of the way an expert would issue a command

— and supplies three controlled studies of exactly the design under discussion. **The plan's
abstract-only caveat is correct about Cockburn 2014 and unnecessary about the claim**: it can be
sourced from full text today.

### 2a. And it changes the design

ExposeHK's fourth goal is "maximise expert performance by using **consistent shortcuts in a flat
command hierarchy**", concluding: "the ultimate performance of experts is improved through **stable
bindings** and flat hierarchies."

The plan renders "its own key" on every row of an alphabetically sorted list. Those keys are
**positional**. Press `N`, create `barn`, and every key below `barn` shifts by one. The plan pins
alphabetical order in a test specifically so positions stay put — and then lets the *bindings* move,
which is the thing this literature says costs you, because the binding is what gets memorised.

**Fix:** derive each row's key from the item (first free letter of its name), so `site_v2` is `V`
whatever else exists — or drop per-row keys and keep arrows + `Enter`.

### 2b. It is also doing less work than the plan claims

ExposeHK's "performance dip" is **cross-modal**: pointer-trained users failing to switch to the
keyboard, because "users reinforce pointing even while trying to learn a faster non-pointer method."
The chooser is keyboard-primary from the first frame. There is no slow path to be trapped on, so
Cockburn 2014's intermodal-transition failure is being applied to a screen with no intermodal
transition. Per-row keys are still right — §3.5's own "the readout has to show it happened" carries
it — but say that, rather than leaning on a finding about a problem this screen does not have.

---

## 3. Itthipuripat is used correctly; §7's status note about it is stale

`10.1523/jneurosci.0440-18.2018` resolves to Itthipuripat, Cha, Deering, Salazar & Serences,
*Having More Choices Changes How Human Observers Weight Stable Sensory Evidence*, J. Neurosci. From
the abstract:

> having more choices did not alter SSVEP amplitude and led to a larger LPD … having more options
> largely spares early sensory processing and slows down decision-making via a selective **increase
> in decision thresholds**.

Plan's reading matches §3.5's, which matches the source. Nothing to fix.

**But §7 is wrong about its availability.** It lists Itthipuripat under "**No open-access PDF for
the DOI at all**, needs a manual fetch". `paper_get` returns two free full texts:
`https://europepmc.org/articles/PMC6170981?pdf=render` and
`https://www.jneurosci.org/content/jneuro/38/40/8635.full.pdf`. It is PMC-deposited. One
`paper_download` → `scribe_convert` → `distill_index` retires the abstract-only caveat for §1.3,
§3.5 **and** this plan. §7's own pip-and-pop precedent — "Reading it upgraded the claim materially"
— is the argument for doing it.

**Also stale:** §7 lists `10.1109/tvcg.2024.3420236` (Lakier/Wentzel et al., *VR Menu Archetypes*)
under "conversion failing". It converted — `distill_search` returns full prose chunks from it.

---

## 4. The "measured" numbers are not the measured numbers

Plan: "**107 systems take `Res<Project>`** across 10 files."

Measured on disk in `crates/emerge-mapper`:

| | Count |
|---|---|
| `Res<Project>` | 67 |
| `ResMut<Project>` | 32 |
| **Total occurrences** | **99** |
| **Files** | **8** — `anim_cache`, `build`, `compose`, `editor`, `guided`, `labels`, `thumbs`, `tiles` |
| of which `Option<Res<..>>` (do **not** panic) | 12 |
| roughly, inside `#[cfg(test)]` | ~29 |

Workspace-wide is identical — nothing outside the crate takes it. Occurrences are also not systems:
helper signatures and test fixtures count in that grep. The honest figure is "on the order of 60
production systems across 8 files."

The conclusion does not change. But a plan that leads with "Measured:" and is 8% high on the count,
25% high on the file count, and wrong about the unit is spending credibility it will want later.

---

## 5. The architecture recommendation is right — argue it from the code, not the count

> **Corrected 2026-08-15.** This section originally claimed that gating on
> `.run_if(resource_exists::<Project>)` would itself panic, because run conditions are evaluated
> without short-circuiting. **That was wrong**, and the plan's author caught it.
> `resource_exists` takes **`Option<Res<T>>`**, not `Res<T>`:
>
> ```rust
> pub fn resource_exists<T>(res: Option<Res<T>>) -> bool where T: Resource
> ```
>
> (`bevy_ecs-0.19.0/src/schedule/condition.rs`, confirmed against docs.rs.) It is safe on a missing
> resource. `lib.rs:31` says a **bare** `Res<T>` in a `.run_if` panics, and `resource_exists` is not
> bare — I over-read it.
>
> The no-short-circuit warning is still real, just narrower: it bites when a *second* condition is
> chained after the guard. `.run_if(resource_exists::<Project>).run_if(resource_changed::<Project>)`
> **does** panic when the resource is absent, because `resource_changed` takes a bare `Res<T>` and
> is evaluated regardless of the first condition's result.

So gating is **feasible**, not impossible. The argument against it is cost, not correctness: ~60
production systems each need `.in_set(..)`, the crate has exactly one `SystemSet` today
(`keys::Phase`, `keys.rs:2068`) so the set machinery is new, and a single missed system is a
first-frame panic. That is a real argument for a second `App`. It is not the knockdown this section
originally claimed.

`ui.md` §5 trap 2 confirms the engine behaviour — "A missing `Res<T>` **panics** the system in 0.19
rather than skipping it" — consistent with Bevy's `ValidationOutcome::Invalid` →
`default_error_handler` path introduced in 0.16 (`Single`/`Populated` skip; a missing resource
errors).

**Cite `lib.rs:31` and §5 trap 2 for the panic behaviour. Drop the 107. Argue the App split on
gating cost, not on gating being impossible.**

---

## 6. "`main.rs` grows one branch … otherwise today's path, byte-for-byte" is not accurate

`main.rs` today:

```rust
let root = PathBuf::from(positional.first().cloned().unwrap_or_else(|| ".".to_owned()));
let map_name = positional.get(1).cloned().unwrap_or_else(|| "untitled_map".to_owned());
```

So three currently-working invocations change meaning under "no positional map argument → chooser":

| Command | Today | Under the plan |
|---|---|---|
| `emerge-mapper` | opens `untitled_map`, no kit | chooser |
| `emerge-mapper .` | opens `untitled_map`, no kit | chooser |
| `emerge-mapper . --kit site` | opens `untitled_map` in `site` | chooser — **discarding an explicitly named kit** |

The first two are probably wanted. The third is a wrong turn: `--kit site` is the one invocation
that has already answered the question the screen exists to ask. **Open the chooser with `site`
preselected.**

Related: `Project::open(kit: None)` resolves to `assets/emerge` itself as the kit (`project.rs`), a
mode the chooser's `assets/emerge/*/` scan can never produce. Either list it as a row or accept that
the no-kit mode becomes unreachable from the chooser — and say which.

Either way, the plan should say it changes these three, not that it preserves them byte-for-byte.

---

## 7. `Map::default()` is not a valid map, and the mockup contradicts `emerge-core`

The reuse table says: "a new, valid, empty map | `Map::default()` — already 'a new, empty, VALID
map', not a zeroed struct."

`map.rs:125` does say that in its doc comment, but it sets `name: String::new()`, and
`Map::validate` (`map.rs:411`) rejects it — `is_snake_case("")` is false, so validate returns
"``` `` ``` is not a usable name." Default is valid in **version and bounds**, the two things its
comment is contrasting against a derived `Default`, and deliberately invalid in **name**.

The line beside it is the design decision the mockup breaks:

> Empty, not "untitled": **a substituted name is a name nobody chose, and the second one collides
> with the first.** An unnamed map is a map that has to be named before it saves.

If the New-map flow pre-fills `untitled_map`, it reintroduces exactly the substituted name
`emerge-core` refuses. Start the NAME field **empty** and refuse `Enter` until it is snake_case —
which is what the plan's own unit test ("a name that forces to empty is refused by name, not
silently defaulted") already wants.

---

## 8. Smaller, still worth fixing

- **`main.rs:51`** is `Project::open`, not the `insert_resource`. That is `main.rs:92`;
  `add_editor_plugins` is `main.rs:102` and `harness.rs:197`. The `harness.rs:182` cite is exact.
- **`exec` is unix-only.** `std::os::unix::process::CommandExt` would be emerge-mapper's first
  platform `cfg` — there is no `cfg(unix)`/`cfg(windows)` anywhere in the crate today, and the repo
  publishes as "a standalone world-building editor". Either add a `spawn` + `wait` +
  `exit(status)` fallback behind `#[cfg(not(unix))]`, or state that the tool is unix-only. Also
  worth one line: `exec` tears the window down and the new process opens a fresh one, so there is a
  visible flash between choosing and editing.
- **2,524 tests: unverified** — I did not run the suite. Workspace-wide there are 1,257 `#[test]`
  attributes (323 in emerge-mapper), so 2,524 must include doctests and per-target duplication.
  Plausible, but say where the number came from, since it is the gate.
- **The four kits' piece counts are unverified** — `assets/` was not readable from this session. The
  plan's own asset-contract test is the right place to pin them.

### Confirmed exactly as written

`harness.rs:182` `.insert_resource(project)` · the only `SystemSet` in the crate is `keys::Phase`
(`keys.rs:2068`) · `chrome.rs:632` `pub struct NameBox` · `build.rs:130` `pub struct NamePrompt` ·
`emerge_core::ron_surgery::save_atomic` (`ron_surgery.rs:726`) · `naming::{is_snake_case,
to_snake_case, map_file_name}` (`naming.rs:23,41,76`) · `Map::validate` (`map.rs:411`) ·
`Project::open`'s kit rule — `to_snake_case`, then reject unless `dir.join(LIBRARY_FILE).is_file()`
— which is exactly the rule the `Catalog` is told to mirror.

### The corpus-gap admission is accurate

Searching distill for project/file-open UX, launchers, start screens and recent-file lists returns
nothing on topic — top hits are a PCG textbook and a 2001 isometric-programming book. §7
independently records that "the library's classical human-factors shelf is empty." The plan should
keep saying so.

---

## What to change

1. **Drop the flat ban on recency.** Replace with a stable user-pinned (or one-row last-opened) zone
   above a fixed alphabetical list, on Sears & Shneiderman 1994. If rejecting it, reject on
   Findlater & Gajos 2009's prediction-accuracy caution, not on Samp.
2. **Make per-row keys stable per item, not per position** — ExposeHK's goal 4. As written, creating
   a map reshuffles every binding below it.
3. **Re-source the rehearsal claim on ExposeHK** (`10.1145_2470654.2470735`, in the library, full
   text), and soften it: this screen has no intermodal transition to bridge.
4. **Fix the counts** — 99 occurrences / 8 files / 12 already `Option`, or just say "~60 production
   systems". Argue the App split on the cost of gating ~60 systems through a set the crate does not
   yet have, not on gating being impossible — see the correction in §5.
5. **Say what the three repurposed invocations do**, and preselect the kit when `--kit` is given.
6. **Start the NAME field empty**, per `map.rs:125`.
7. **Decide unix-only or add the non-unix `spawn` fallback.**
8. **Run `paper_download` on Itthipuripat** (`PMC6170981`) and Samp (`hdl.handle.net/10379/2672`),
   and correct §7's stale entries for Itthipuripat and `10.1109/tvcg.2024.3420236`.

Nothing here changes the recommendation to make the chooser its own `App`. That call is sound, and
the code says so more forcefully than the plan does.
