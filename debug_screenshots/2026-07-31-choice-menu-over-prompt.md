# 2026-07-31 — the choice menu was drawn on top of the speaker's own line

*From `region_2026-07-31_18-06-02-725`. Player note: "Text bubbles are overlapping." Fixed the same
day; capture cleared. Tracked as **FVS-L-8** in `BACKLOG_ARCHIVE.md`.*

## What the capture showed

A conversation with a `Choice` node. Three bubbles in the crop:

```
        2. Cordon the room.      <- option, no tail, over the LEADER
        1. Burn it out.          <- option, no tail, over the LEADER
           Call it.              <- prompt, TAILED, over the SPEAKER
              \/
```

`1. Burn it out.` and `Call it.` visibly intersect — the option box's lower edge crosses the prompt's
upper edge, and the prompt's tail passes behind the option.

## Root cause

Not "two speakers' bubbles collided", which was the first guess. It is **one conversation whose two
kinds of bubble hang off two different entities**:

- the prompt is spawned over the **speaker** at `Vec2::ZERO` (`dialogue/runtime.rs`, `Node::Choice` arm);
- the clickable options are stacked over the **leader**, starting from a bare `CHOICE_BASE = 0.15`.

Neither column knew the other existed. `Bubble.offset` was only ever built to stack one speaker's own
options against *each other*. In play the leader is very often standing next to — or is adjacent to —
whoever is talking, so the two columns project into the same screen space and overlap.

## Why the fix is not "put them on the same entity"

The two-owner split is deliberate and was kept: **the line belongs to them, the options belong to
you.** Collapsing it would have removed a real piece of readability to fix a layout bug.

Instead the option stack now clears the prompt's **measured** height:

- `spawn_line_bubble` returns its `RenderedBubble` size;
- the `Choice` arm starts `offset_y` at `CHOICE_BASE + prompt_height`.

A bubble's footprint is only known *after* rasterisation, because the text wraps — which is exactly
why a compile-time constant could never have been correct here.

Vertical offsets are applied along **camera up** in `bubble::track_bubbles`, so clearing the prompt's
own height is precisely the screen-space separation needed, at any camera angle. When the speaker is
far from the leader there was no overlap to begin with and the options simply ride one bubble higher,
which costs nothing — they already float well above the leader's head.

## Determinism

None. `src/dialogue/` is cosmetic and `Update`-only, touches no pinned state, and the goldens did not
move.
