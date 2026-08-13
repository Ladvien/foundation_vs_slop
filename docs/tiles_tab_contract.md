# The Tiles tab — what it promises

**This is the contract, not the implementation.** `crates/emerge-mapper/tests/headless.rs`'s
`no_reachable_tiles_state_leaves_the_arrows_doing_nothing` is its executable form; every clause below
that can be checked is checked there, and a clause that cannot be checked says so.

Written 2026-08-12, after five bugs were reported from the keyboard in one afternoon. Every one was a
requirement nobody had written down — including two that were not bugs but *absences*: no way to
change which member the verbs act on, and no way to empty a tile. The tab had a selection, five verbs
that acted on it, and no way to move it.

## The nouns

| | |
|---|---|
| **the tile** | `Build::open` — a `Bounded` composition, one cell across unless its contents grow it |
| **a member** | a piece inside it, positioned relative to the tile's centre |
| **the focus** | `Build::focus` — the member the verbs act on, drawn in amber |
| **the library row** | `ImportState::selected_library_id` — what the next drop brings in |
| **the rung** | `Build::rung` — the lattice the arrows step, latched, drawn |

## The two states

The tab is always in exactly one, and it is `keys::Stance`:

- **Choosing** (`Stance::Idle`) — no piece in hand. The arrows walk the library. `Enter` brings the
  selected row in.
- **Placing** (`Stance::Holding`) — a member is focused *and* you are placing it. The arrows act on
  the member. `Esc` returns to Choosing.

**Both facts are required.** Each alone was tried and each broke the opposite end: intent alone left
the arrows live over an emptied tile, and focus alone made Placing permanent so the library could
never be walked again. `Placing` means *there is a piece and you are placing it*.

## What each verb promises

### Choosing

| key | promise |
|---|---|
| `up` / `down` | walk the library. `Shift` strides five |
| `Enter` | bring the selected row in, focus it, and enter Placing |
| `Shift+Enter` | leave a declared hole instead |
| `N` | open a **new** tile — a different document, with its own history |
| `J` | step the rung |
| `Cmd+Z` / `Shift+Cmd+Z` | step this tile's history |

### Placing

| key | promise |
|---|---|
| `up` / `down` | move the focused member one rung, in screen axes |
| `left` / `right` | **step to the previous / next member** |
| `Shift`+arrows | put the focused member flush against that side |
| `[` / `]` | move it a layer |
| `R` | turn it a quarter |
| `Delete` | remove it |
| `Shift+Delete` | empty the tile |
| `Esc` | put the piece back and return to Choosing |
| `Enter` | bring another piece in |

**A flush along an axis the piece already spans is a no-op**, and says so, naming the axis that would
move instead. The arithmetic is right — a piece filling the tile on that axis is already as flush as
it can be — and silence there is indistinguishable from a dead key.

## The invariants

1. **The key list does not lie.** A key the census offers in a state must do something in that state.
   This is the property all three stance bugs violated, and it is what the matrix asserts.
2. **`Esc` always returns to Choosing**, from anywhere in Placing. The tab prints this promise twice.
3. **The focus is always a real member, or there is no focus.** `Build::focus < members.len()`, or
   the tile is empty and the tab is Choosing.
4. **A run of adjustments to one member is one undo step.** The arrows repeat at
   `keys::REPEAT_SECS`, so per-keystroke history buries a drop under seven entries in a second.
   Ousterhout §6.7 — the grouping policy belongs to the layer that knows what one act is.
5. **A tile's history is its own.** Opening a tile starts a new one, because `Cmd+S` saves under the
   open tile's id and an undo that crossed the boundary could write one tile's members under
   another's name. `TileHistory` already makes this argument about the two *tabs*.
6. **Undo removes the most recent drop**, whatever the MEMBERS list order was. `place` uses
   `insert_sorted`, so the list is in id order and the piece you added second can sit at the top —
   which is why undo names what it removed rather than only counting what is left.
7. **A tile bigger than one cell cannot be generated**, and the panel says so beside the size. It is
   a property, not an event: it was a sticky problem raised on every size change, fifteen deep from
   one nudge and still on screen after the tile was emptied.

## Deliberately absent

- **`left`/`right` do nothing while Choosing.** There is one list on this tab, so there is nothing to
  switch between; the census does not offer them, and invariant 1 is about what it *does* offer.
- **No verb sets the tile's size.** The envelope is read off the contents — a size authored
  separately from the members it contains is a second source of truth, and the two drift.
- **No `paint` writer.** Decal ordering is unauthorable; FVS-R-22.

## Where this is enforced

- `no_reachable_tiles_state_leaves_the_arrows_doing_nothing` — invariants 1 and 2, over eight states
  reached by the key sequences that reach them.
- `a_dropped_member_moves_under_the_arrows_without_space_first` — invariant 1, the `Enter`-only route.
- `a_tile_survives_being_emptied_and_refilled` — invariant 3, both cycles.
- `undo_after_two_drops_removes_the_second_mesh` — invariants 4 and 6.
- `undo_removes_the_most_recent_drop_not_the_first_row` — invariant 6, including the naming.
- `a_new_tile_does_not_undo_into_the_one_before_it` — invariant 5.
- `a_flush_along_the_axis_a_piece_already_fills_says_why_nothing_moved` — the flush clause.
- `dropping_an_oversized_mesh_grows_the_tile` — invariant 7, asserted on the panel text.
- `the_focus_walks_the_members_and_shift_delete_empties_the_tile` — the two Placing clauses that did
  not exist until the contract was written down: `left`/`right` and `Shift+Delete`.

**Every one of these was verified by putting the bug back.** A test that cannot fail reads as a
guarantee; this crate has three source ratchets whose notes say so, and the same rule applies here.
