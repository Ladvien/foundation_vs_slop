# One application

A design for making the editor feel like one program rather than three that share a window. Written
after measuring the current behaviour rather than from memory; every number below was read out of the
code on 2026-08-17.

**Status: designed, not built.** Reported at the keyboard the same day, against a running editor:
*"It still doesn't feel like a unified application. When I enter kit editing, there's no clear way to
get back to the main menu."* And then the requirement, in one sentence: *"That process should be
largely seamless, smooth."*

---

## 1. The finding that changes the design

`screen.rs` states the door model and defends it:

> *"leaving a door despawns everything it made and drops the project; entering the next one loads it
> fresh."*
>
> *"A reload cannot be wrong; a partial teardown can be, silently, and the bug lands weeks later
> looking like something else."*

**That is not what the code does.** Measured:

| What | Cleared on a door change? | How |
|---|---|---|
| Entities | **Yes, all of them** | `scene_roots` — every root with `Transform` or `Node`, a reachability rule |
| `Project`, `OpenMap`, `Door`, `Mode` | **Yes** | `args::Opened::remove_from`, named one by one |
| **The other 58 resources** | **No** | `init_resource` at plugin build; nothing removes or resets them |

Fifty-eight resources created by the editor's own plugins survive every door change, and **nothing
resets any of them on entry** — all eleven `OnEnter(Screen::Editor)` systems are spawns (panels,
cameras, the compass, the booth, a cache warm). Among the survivors: `build::TileHistory` (the tile
undo stack), `EditorState`, `ComposeState`, `ImportState`, `Build`, `LabelQueue`, `Filters`.

So the door change **is already a partial teardown** — an unnamed, unchecked one. The safety the
comment claims is not the safety the code has, and the bug class it is spent to prevent is already
open: edit a tile in kit A, leave, open kit B, and A's undo stack is still there to be replayed into
B.

This matters beyond the tidiness of it. `screen.rs` declines to keep the project warm *because* a
partial teardown is unsafe. **The premise is false**, so the refusal does not hold: keeping the
project warm is not taking on a new class of risk, it is making an existing one explicit and
checkable. The work is the same work either way, and today it is not being done.

## 2. What the doors doc already charged, now collected

`docs/2026-08-16-doors.md` §7 named this exact risk when the split was designed, and priced it:

> three doors to label a mesh, build a tile from it and place it is three menu round-trips, against
> Lai, Latham & Leymarie's second pillar — *"it is important that this feedback loop is as short as
> possible"* (`10.1145/3402942.3402946`)

It was mitigated *"by a door key, not by a cross-door hop"* — and then the door key was never built,
because §10's own measurement cut five doors to two and the mitigation went with the doors it was
mitigating. The risk survived the cut; the mitigation did not.

The corpus has the general form of this failure. Smelik, Tutenel, de Kraker & Bidarra
(`10.1145/1814256.1814258`, 2010) describe splitting a modelling workflow into phases with no way
back — *"they could even be implemented as separate systems"* — and reject it in the same breath:

> *"the serious problems [are avoided] by disrupting the iterative workflow, which can be quite
> restrictive and cumbersome in use … thus it is not a satisfying approach."*

That is a description of the shipped editor, written sixteen years early. The kit door and the map
door are one iterative loop — author a piece, place it, see it is wrong, go back — and the loop
currently runs through a menu and a reload.

## 3. Why the way out is not found

It exists on all four panels (`chrome::back_button`, called from `editor.rs:1237`, `tiles.rs:3380`,
`compose.rs:337`, `anim_tab.rs:317`) and it is labelled with its chord. It is still not found, and
the reason is where it is drawn rather than how brightly.

- It is **inside the left panel**, under that panel's own heading, so it reads as that panel's
  content — a row in a list of rows.
- It is on **`SLOT_BG`**, the ground this editor uses for an inspector slot. The encoding says
  "a field of the thing you are looking at", not "a way out of it".
- **Nothing at window level is navigation.** The topmost element on screen is the door's tab strip,
  which is door-local by design (`Door::tabs`).

**This is the second failure of the same affordance**, and that is the argument for changing the
encoding rather than the contrast. `chrome.rs:1679` records the first:

> *"an author looking for the exit found neither — they pressed `Esc` three times instead, which is
> the one key deliberately wired to mean 'not that' rather than 'out'."*

The fix then was to name the chord in the hint line. The report above is the same defect after that
fix. `d02e4aa` already settled what to do when a signal fails twice — *"the encoding was not weak, it
was wrong"* — and it settled it about this same editor.

## 4. The three changes

Taken at the keyboard, 2026-08-17, all three.

### 4.1 The way out becomes application chrome

A persistent bar **above** the tab strip, outside every panel, carrying where you are and the way
back. It is not a fourth panel and not a row: it is the one piece of the window that does not belong
to a door.

```
┌──────────────────────────────────────────────┐
│  ‹ kits & maps          KIT · furniture      │   <- chrome, always, every door
├──────────────────────────────────────────────┤
│  1 MESHES   2 TILES   3 COMPOSE              │   <- the door's own strip
├──────────────────────────────────────────────┤
│  MESHES AND TILES                            │
```

`chrome::back_button`'s four call sites collapse to one, which is the point: a way out that each
panel places is a way out each panel can forget, and the Rigs door already draws it in a different
place from the Map door. The window title already carries `— kit — furniture` (`main::name_the_window`)
and nobody reads a title bar; this is that same fact put where the eye is.

### 4.2 Doors switch directly

`Door::ALL` is three. The strip in 4.1 is where they go, so a door change is a click or a chord, and
the menu stops being on the path. This reverses `screen.rs`'s *"No direct Kit↔Map key"*, and the
reversal is honest rather than quiet: that refusal was argued on the saving being *"the menu
round-trip rather than the reload"*, and §5 removes the reload from the comparison.

The refusal's other half — *"a second way to do one thing is the pattern this crate spends its
refusals on"* — is answered by deleting the first way, not by adding a second. The menu becomes where
you choose **what** to open. Changing **which lens** you look at it through is not a menu question.

### 4.3 The project stays warm, and what a door owns becomes a list

Entering a door with the same kit already loaded must not re-read the library, the vocabulary or the
thumbnails. What it must do is reset exactly what the previous door owned — and per §1 that list does
not currently exist, which is why the refusal was written.

**So the list is the deliverable, not the optimisation.** Every resource in §1's fifty-eight is
classified, once, in one place:

| Class | Meaning | On a door change |
|---|---|---|
| `Project` | derived from files on disk | kept if the kit is unchanged, reloaded if not |
| `Door` | this door's own working state — selections, drags, edit buffers, undo stacks | **reset** |
| `Session` | true for as long as the app runs — caches, generations, the injected cursor | kept |

And a ratchet, on the pattern `chrome_census.rs` and `census_is_the_one_counter.rs` already
establish: **every resource the editor's plugins register must appear in that classification**, or
the test fails naming it. A new resource is then a deliberate answer to "what happens to this when
the door changes", asked at the moment it is added rather than three weeks later when its stale value
surfaces as something else.

That ratchet is worth building **even if 4.3 is abandoned**, because §1's fifty-eight survivors are
live today and unclassified.

## 5. Blast radius

| Touches | What |
|---|---|
| `chrome.rs` | `back_button` becomes the chrome bar; four call sites become one |
| `screen.rs` | `close_the_door` resets by class rather than dropping four names; §1's comment is rewritten against what the code does |
| `args.rs` | `Opened::remove_from` is replaced by the classification |
| `tiles.rs` | `Door` gains its strip; `Door::ALL` becomes reachable UI |
| `keys.rs` | a door chord; `MainMenu` keeps `Cmd+O` |
| `editor.rs`, `compose.rs`, `anim_tab.rs` | their `back_button` calls go |
| **a new `tests/every_resource_says_what_a_door_does_to_it.rs`** | the ratchet |

## 6. Staging

1. **The classification and its ratchet.** No behaviour change: name all 58, assert the test can see
   them, prove it fails when one is added without an answer. This is the step that makes 3 safe, and
   it stands alone if the rest is dropped.
2. **The chrome bar** (4.1). Self-contained, visible immediately, and it is where 4.2 will live.
3. **Direct door switching** (4.2), still tearing down as today — so the routing is proven before the
   lifecycle changes underneath it.
4. **Warm project** (4.3). Last, because until 1 is green it is the thing `screen.rs` correctly
   refused.

## 7. Open

- **What `Esc` means once there is a visible way out.** It currently peels layers and then asks
  (`editor.rs:1086`). With the exit on screen the question may be redundant, or may be the only thing
  standing between a stray keystroke and a lost map. Not decided here.
- **Whether the map survives a door change.** 4.3 keeps the *project* warm; whether an open map
  stays open while you visit the Kit door is a separate question, and it is the one an author is most
  likely to have an opinion about.
- **Whether the menu keeps a door column at all.** If doors switch directly, the menu's job shrinks
  to picking a kit or a map, and the door strip on it may be one place too many to say the same thing.
