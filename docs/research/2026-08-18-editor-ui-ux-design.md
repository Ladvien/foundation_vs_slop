# Reusable scroll and tab widgets for the editor — design

**Prepared 2026-08-18.** Brief: reusable, embeddable scroll view (with scroll items) and tab view for
`emerge-mapper`, built on Bevy 0.19, following modern-browser UX convention. Decisions taken at the
keyboard before this was written: **Feathers wholesale**, **BSN templates**, and scope covering
**virtualization**, **keyboard-first / a11y**, and **polish (momentum, sticky, animation)** — explicitly
*not* "actual browser ability".

That last qualifier is load-bearing and this document holds it. We are not building a compositor-threaded
scroller with rubber-band overscroll and scroll-anchoring. We are building two widgets that behave the way
an author's hands already expect, because their hands were trained by browsers.

---

## 0. TL;DR — what this proposes

| | Decision |
|---|---|
| **Foundation** | `bevy_feathers` 0.19 for machinery, `chrome.rs` for the palette. Feathers' `UiTheme` is *seeded from* `chrome.rs`, not replacing it. |
| **Authoring** | BSN `SceneComponent` + `Props` — `@ScrollView { @content: ... }`, `@TabView { @tabs: ... }`. |
| **Scroll view** | Thin wrapper over `bevy_ui_widgets::{ScrollArea, Scrollbar, ScrollbarThumb}` + `FeathersScrollbar`. Adds: auto-hide bar, stable gutter, sticky headers, keyboard paging **routed through `keys.rs`**, momentum on wheel only. |
| **Virtual list** | A **separate** widget, not a mode of `ScrollView`. Uniform row height, one-shot-system row source, opt-in above ~200 rows. Feathers' `FeathersListView` does not virtualize and cannot be made to. |
| **Tab view** | Built from scratch — **nothing tabbed exists in Bevy 0.19**, in `bevy_ui_widgets` or in Feathers. Roving tabindex, `Role::TabList`/`Tab`/`TabPanel`, `Display::None` panel swap, animated indicator. |
| **Biggest risk** | BSN is spawn-time only. It removes *spawn* boilerplate, not *update* boilerplate — and the editor's panel code is mostly update systems. See §8.1. |
| **Second biggest** | Every API name below was read from Bevy **0.19.1** docs/sources. This repo pins **0.19.0**. See §9.1 before writing a line of it. |

---

## 1. What Bevy 0.19 actually gives us

Verified against docs.rs for `bevy` 0.19.1 and the `v0.19.1` sources. Deltas against the pinned 0.19.0 are
§9.1's problem, not this section's.

### 1.1 Scrolling — complete and headless

`bevy_ui_widgets` (default-on feature) ships the whole stack:

- **`ScrollArea`** — zero-sized marker, `#[require(ScrollPosition)]`. Turns `Pointer<Scroll>` into
  `ScrollPosition` writes. It does **not** set `Node.overflow` for you.
- **`Scrollbar { target: Entity, orientation: ControlOrientation, min_thumb_length: f32 }`** — goes on the
  *track* entity, points at the scrolled entity. No `Default` (holds an `Entity`).
- **`ScrollbarThumb { border_radius, border }`** — **deliberately has no `Node`**. It is not laid out by
  taffy; `update_scrollbar_thumb` writes its `ComputedNode` and `UiGlobalTransform` directly in `PostUpdate`
  after `ui_layout_system`. Adding a `Node` to a thumb is a bug.
- **`ScrollbarDragState { dragging, .. }`** — on the thumb; the hook for hover/drag styling.
- **`ScrollIntoView { entity }`** — an `EntityEvent`. `commands.trigger(ScrollIntoView { entity: row })`
  walks ancestors for the nearest `ScrollArea` and adjusts by the minimum amount. **This replaces
  `chrome::scroll_to_reveal` entirely.** See §6.3.
- **`IgnoreScroll(BVec2)`** on `bevy::ui` — per-axis "don't apply my parent's scroll offset". This is
  sticky headers, for free.
- **`ComputedNode`** exposes `size()`, `content_size()`, `scrollbar_size`, `scroll_position`,
  `inverse_scale_factor`, `normalize_point()`. The canonical clamp, from the shipped source:

  ```rust
  let visible = (computed.size() - computed.scrollbar_size) * computed.inverse_scale_factor;
  let content = computed.content_size() * computed.inverse_scale_factor;
  let max     = (content - visible).max(Vec2::ZERO);
  ```

- **`Node.scrollbar_width: f32`** — reserves gutter space inside the node. This is CSS
  `scrollbar-gutter: stable`.

Two shipped inconsistencies worth knowing before you trust the stack:

1. **`ScrollArea`'s wheel handler uses `computed_node.size()`; `Scrollbar`'s code uses
   `size() - scrollbar_size`.** With `scrollbar_width > 0` the wheel's max scroll and the thumb's max scroll
   disagree by the gutter width. Overlay gutters (`scrollbar_width == 0`) sidestep it; §3.3 does.
2. **`scrollarea_on_scroll` calls `propagate(false)` unconditionally on hit** — even at the clamp limit, even
   on a non-scrollable axis. **Nested scroll areas do not chain.** The editor has nesting today (a detail
   pane inside a panel), so this matters; §3.6 handles it.

### 1.2 Focus and a11y — everything the keyboard-first brief needs

- **`bevy_input_focus`**: `InputFocus`, `InputFocusVisible`, `TabIndex(i32)`, `TabGroup { order, modal }`,
  `TabNavigation` (a `SystemParam` with `navigate(&InputFocus, NavAction)`), `AutoFocus`, `AcquireFocus`,
  `FocusedInput<T>`.
- **`AcquireFocus` bubbles until it hits something with `TabIndex`** — that is what makes clicking a tab's
  *text child* focus the *tab*.
- **`bevy_ui::accessibility::AccessibleLabel(String)`** — new in 0.19, `#[require(AccessibilityNode)]`,
  immutable with insert/remove hooks. Lets a widget *user* set the label without owning the whole
  `accesskit::Node`. This is the right prop type for tab captions.
- **AccessKit roles we need all exist** in `accesskit` 0.24: `ScrollView`, `ScrollBar`, `TabList`, `Tab`,
  `TabPanel`, `ListBox`, `ListItem`. Setters: `set_scroll_y/_min/_max`, `set_active_descendant`,
  `set_controls`, `set_labelled_by`, `set_position_in_set`, `set_size_of_set`, `set_selected`.
- **`bevy_a11y` does not re-export `accesskit`.** Add `accesskit = "0.24"` as a direct dependency.
- **Nothing auto-populates scroll offsets for AT.** If we want a screen reader to know where the list is
  scrolled, we write `set_scroll_y` ourselves. See §5.2.

### 1.3 Feathers — what it is and what it is not

`bevy_feathers` 0.19 is an **optional** cargo feature (`bevy = { features = ["bevy_feathers"] }`), currently
**absent from this workspace's feature list** — `Cargo.lock` resolves it, but nothing enables it. Its own
docs describe it as *"a collection of styled and themed widgets for building editors and inspectors"*, and
explicitly discourage it for game UI. That is exactly our case, and it is why "wholesale" is a defensible
call here even though the crate is marked experimental.

What it gives us:

- **Containers**: `pane()`, `pane_header()`, `pane_header_divider()`, `pane_body()`, `subpane*()`, `group*()`,
  `flex_spacer()` — all `impl Scene` functions. These map almost one-to-one onto `chrome::panel_root` /
  `chrome::title` / `chrome::section`.
- **`FeathersScrollbar { @target: EntityTemplate, @orientation: ControlOrientation }`** — the themed track +
  thumb, 3 px radius, tokens `SCROLLBAR_BG` / `SCROLLBAR_THUMB` / `SCROLLBAR_THUMB_HOVER`.
- **`FeathersListView` / `FeathersListRow`** — `ScrollArea` + overlay `FeathersScrollbar` + `ListBox` +
  `Role::ListBox` + `TabIndex(0)`, rows carrying `Role::ListItem`, `Hovered`, `ListItem`, `Selected`.
- **`focus::{FocusIndicator, FocusWithinIndicator, FocusOutlinesPlugin}`** — inserts a `bevy_ui::Outline`
  (2 px width, 2 px offset, `tokens::FOCUS_RING`) on the focused entity. **This is the visible focus ring the
  keyboard-first brief needs, and we do not have to write it.**
- **`cursor::{EntityCursor, DefaultCursor, OverrideCursor}`** — per-entity system cursors.
- **`theme::{UiTheme, ThemeProps, ThemeToken, ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor,
  InheritableThemeTextColor, ThemedText}`** — 137 tokens, flat `SmolStr` keys.

What it does **not** give us, confirmed against the complete 0.19.1 item list:

- **No tab widget of any kind.** No `Tab`, `TabList`, `TabPanel`, `TabView`, `TabStrip`. (`TabNavigationPlugin`
  in `FeathersPlugins` is *keyboard tab-key* navigation — an unrelated concept sharing a word.)
- **`FeathersListView` does not virtualize.** `listview.rs` has no recycling and no visible-range logic;
  `@rows` is a `SceneList` spawned in full. Every row is a live entity with `Hovered`, `ListItem`,
  `AccessibilityNode` and two theme components. One labelling walk in this repo queued 778 meshes.
- **No light theme helper**, no tree view, no table, no splitter, no tooltip, no modal, no progress bar.
- **`UiTheme::default()` is empty.** Every token misses, warns once, and renders **fuchsia**. Seeding is not
  optional.
- **`ThemeProps` covers colours only.** Spacing, sizing and radii live in `constants::size` and are not
  themable. `chrome`'s `GAP_TIGHT`/`GAP_ROW`/`GAP_GROUP`/`MARGIN`/`PAD` scale stays ours.

### 1.4 BSN — the shape of the authoring API

BSN shipped in 0.19 as `bevy::scene` (it *replaced* the old scene crate; the old one is now
`WorldAsset`/`DynamicWorld`/`WorldAssetRoot`). The RFC names `Construct`/`ConstructContext`/`Patch` do not
exist — they shipped as `Template`/`TemplateContext` in `bevy::ecs::template`.

The widget pattern is `SceneComponent` + a props struct:

```rust
pub trait SceneComponent: Component + FromTemplate where Self::Template: Default {
    type Props: Default;
    fn scene(props: Self::Props) -> impl Scene;
}
```

```rust
#[derive(SceneComponent, Default, Clone)]
#[scene(ScrollViewProps)]
pub struct ScrollView;

// caller side
commands.spawn_scene(bsn! {
    @ScrollView { @content: { bsn_list![ row("alpha"), row("beta") ] } }
    Node { flex_grow: 1.0 }              // patch the widget's own Node
    on(|_: On<Activate>| info!("hi"))    // embedded observer
});
```

Rules that shape the design below:

- **Props are evaluated immediately and are not patchable.** They produce patchable output. So anything a
  caller must be able to override after the fact belongs on a component, not in props.
- **Non-`@` fields inside `@Widget { ... }` patch the widget's own components.** `Node { flex_grow: 1.0 }`
  written alongside works. That is the escape hatch that makes these embeddable.
- **`Box<dyn SceneList>` is the idiomatic slot type** for injected children. `Box<dyn Scene>` for a
  single-entity slot.
- **`#Name` gives a compile-time reference within one `bsn!` scope** — `@FeathersScrollbar { @target: #inner }`
  is how the list view binds its bar to its viewport. It also inserts a runtime `Name` component, but
  **post-spawn lookup is by marker component, not by BSN name.** Feathers finds its own sub-parts with
  `iter_descendants(root).find(|e| q_marker.contains(*e))`. Every widget below therefore ships markers.
- **No `..` spread, no `if`/`match`.** Conditionals are Rust: build a `Box<dyn Scene>` or a `Vec<_>` and
  splice with `{ expr }`.
- **`bsn!` scopes are per-invocation**, so name collisions across widgets are impossible.
- **There is no reactivity.** §8.1.

---

## 2. The theme decision — Feathers machinery, `chrome.rs` colour

"Feathers wholesale" has one collision with this repo, and it is worth naming before anything is built.

`chrome.rs` is not an accidental palette. Every constant carries a reason (`PANEL_BG` opaque because *"a
researcher in a white coat behind a translucent one is unreadable — measured"*; `SUGGEST` a cool slate
because *"a proposal is a question, and it must not read as either an answer or an alarm"*). The
2026-08-17 audit's central finding is that four tabs are **drifting into four dialects** and its central
remedy is *one name per fact*. Dropping Feathers' greys on top of that would create a fifth dialect and
throw away the reasoning.

**So: adopt Feathers' theme *mechanism*, seed it from `chrome.rs`.**

```rust
// chrome.rs — new
pub mod token {
    use bevy_feathers::theme::ThemeToken;
    pub const LIST_ROW_BG:       ThemeToken = ThemeToken::new_static("emerge.list.row.bg");
    pub const LIST_ROW_HOVER:    ThemeToken = ThemeToken::new_static("emerge.list.row.hover");
    pub const LIST_ROW_SELECTED: ThemeToken = ThemeToken::new_static("emerge.list.row.selected");
    pub const TAB_ACTIVE_TEXT:   ThemeToken = ThemeToken::new_static("emerge.tab.active.text");
    pub const TAB_IDLE_TEXT:     ThemeToken = ThemeToken::new_static("emerge.tab.idle.text");
    pub const TAB_INDICATOR:     ThemeToken = ThemeToken::new_static("emerge.tab.indicator");
    // ...
}

/// The editor's palette as a Feathers theme. Overrides Feathers' own tokens where they exist,
/// adds ours where they don't. One table, one source, and a missing entry is FUCHSIA — loud.
pub fn theme() -> ThemeProps {
    let mut props = bevy_feathers::dark_theme::create_dark_theme();
    props.color.extend([
        (feathers_tokens::WINDOW_BG,      VOID),
        (feathers_tokens::PANE_BODY_BG,   PANEL_BG),
        (feathers_tokens::PANE_HEADER_BG, HEADER_BG),
        (feathers_tokens::TEXT_MAIN,      TEXT),
        (feathers_tokens::TEXT_DIM,       DIM),
        (feathers_tokens::FOCUS_RING,     ACCENT.with_alpha(0.5)),
        (feathers_tokens::SCROLLBAR_BG,   HEADER_BG),
        (feathers_tokens::SCROLLBAR_THUMB, scaled(ROW_SELECTED, 0.8)),
        (feathers_tokens::SCROLLBAR_THUMB_HOVER, ROW_SELECTED),
        (feathers_tokens::LISTROW_BG,          ROW_BG),
        (feathers_tokens::LISTROW_BG_HOVER,    ROW_HOVER),
        (feathers_tokens::LISTROW_BG_SELECTED, ROW_SELECTED),
        (token::TAB_ACTIVE_TEXT, ACCENT),
        (token::TAB_IDLE_TEXT,   DIM),
        (token::TAB_INDICATOR,   ACCENT),
        // ...
    ]);
    props
}
```

Two arguments for this beyond taste.

**It converts the audit's whole "palette leaks" table into a compile-or-shout failure.** A hand-transcribed
`srgb(0.16, 0.15, 0.14)` in a second file is invisible. A missing `ThemeToken` renders fuchsia and warns.
`chrome::scaled` and `chrome::ink` already answer their own error case *"loudly in magenta rather than with
a silent guess"* — Feathers picked the same convention independently. The mechanism is already the house style.

**It makes `Hovered`-as-hit-test into `Hovered`-as-feedback for free** (audit defect 7). Every Feathers
widget already restyles on `Hovered`; nothing has to be decided per row.

The cost is honest and should be recorded: **spacing and typography are not covered.** Feathers' `size::`
constants are its own, and the audit's spacing and typography findings ("six sizes, no role map") remain
open. Widgets below take spacing from `chrome`, not from `feathers::constants::size`.

### 2.1 Fonts

Feathers embeds Fira Sans/Mono via `embedded_asset!` and refers to them through `InheritableFont`. The editor
has its own `install_font`. Options: (a) let Feathers' fonts win inside Feathers widgets and accept two
typefaces in one panel; (b) override `InheritableFont` on each widget root with the editor's font; (c) adopt
Fira wholesale. **(b) is recommended** — one line per widget root, and it keeps the editor's existing panels
unchanged during migration. Decide before the first widget lands, because retrofitting is a sweep.

---

## 3. `ScrollView`

### 3.1 What browsers do that we are copying

The behaviours below are *convention*, and copying them is why an author's hands work on first contact. They
are cheap. The behaviours we are **not** copying — scroll anchoring, overscroll rubber-banding, compositor
threading, `scroll-behavior: smooth` for anchor jumps — are expensive and buy an editor nothing.

| Browser behaviour | Ours | Cost |
|---|---|---|
| Wheel/trackpad scroll, correct axis, correct sign | `ScrollArea` (free) | — |
| Drag the thumb; click the track to page | `Scrollbar` (free) | — |
| Bar hides when content fits | small system | ~20 lines |
| Overlay bars by default, stable gutter opt-in | `Node.scrollbar_width` + a prop | ~10 lines |
| `position: sticky` headers | `IgnoreScroll` (free) | — |
| PageUp/PageDown/Home/End on the focused scroller | **must route through `keys.rs`** — see §3.5 | design cost, §3.5 |
| Focus scrolls into view | `ScrollIntoView` (free) | — |
| Wheel momentum | opt-in, wheel only, §3.7 | ~40 lines + a caveat |
| Nested scrollers chain at the limit | opt-in observer, §3.6 | ~30 lines |
| Shift+wheel scrolls horizontally | **not free** — `Pointer<Scroll>` carries no modifier state | skip unless asked |

### 3.2 Shape

```rust
#[derive(SceneComponent, Default, Clone, Reflect)]
#[scene(ScrollViewProps)]
#[reflect(Component, Default, Clone)]
pub struct ScrollView;

#[derive(Default, Clone, Copy, PartialEq, Reflect)]
pub enum ScrollAxis { #[default] Y, X, Both }

#[derive(Default, Clone, Copy, PartialEq, Reflect)]
pub enum Gutter {
    /// Bar floats over the content. No layout shift ever, content can slide under the bar.
    #[default] Overlay,
    /// Bar reserves `Node.scrollbar_width` px. Content never slides under; the panel's usable
    /// width is constant whether or not it currently overflows.
    Stable,
}

#[derive(Default, Clone, Copy, PartialEq, Reflect)]
pub enum BarVisibility {
    /// Present only while `content_size > visible_size`. Browser default.
    #[default] Auto,
    Always,
    Never,
}

pub struct ScrollViewProps {
    pub content: Box<dyn SceneList>,
    /// Rows pinned to the top, above the scrolled content. `IgnoreScroll` does the pinning.
    pub sticky: Box<dyn SceneList>,
    pub axis: ScrollAxis,
    pub gutter: Gutter,
    pub bar: BarVisibility,
    pub row_gap: Val,
    /// Bound into `keys.rs` so PageUp/Home/End appear in the census. `None` = no keyboard paging.
    pub keys: Option<keys::Context>,
}
```

Markers shipped for post-spawn lookup (BSN names are not addresses):

```rust
#[derive(Component)] pub struct ScrollViewRoot;
#[derive(Component)] pub struct ScrollViewport;   // the `overflow: scroll` + `ScrollArea` node
#[derive(Component)] pub struct ScrollViewBar;    // the track
```

### 3.3 The tree

Modelled on `FeathersListView`, which is the shipped reference wiring, with an overlay bar rather than the
grid frame the `scrollbars.rs` example uses. Overlay is chosen deliberately: the grid frame gives a stable
gutter but forces `scrollbar_width`, which walks straight into the §1.1 gotcha where the wheel and the thumb
disagree about max scroll. `Gutter::Stable` costs a padding change, not a layout mode change.

```rust
impl ScrollView {
    fn scene(props: ScrollViewProps) -> impl Scene {
        let overflow = match props.axis {
            ScrollAxis::Y    => Overflow::scroll_y(),
            ScrollAxis::X    => Overflow::scroll_x(),
            ScrollAxis::Both => Overflow::scroll(),
        };
        let gutter = match props.gutter {
            Gutter::Overlay => px(0),
            Gutter::Stable  => px(BAR_W + BAR_INSET),
        };
        bsn! {
            ScrollViewRoot
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                // The one thing everyone gets wrong: a flex item's automatic minimum size is its
                // content, so without `min_height: 0` the node grows to fit every row and
                // `overflow` has nothing left to clip. `chrome::scroll_list` already carries this
                // comment; here it is carried once for everyone.
                flex_grow: 1.0,
                min_height: px(0),
                padding: UiRect { right: gutter, ..default() },
            }
            AccessibilityNode(accesskit::Node::new(Role::ScrollView))
            TabIndex(0)
            FocusWithinIndicator
            Children [
                { props.sticky }            // IgnoreScroll(BVec2::new(false, true)) applied per row
                (
                    #viewport
                    ScrollViewport
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Stretch,
                        row_gap: { props.row_gap },
                        overflow: { overflow },
                        flex_grow: 1.0,
                        min_height: px(0),
                    }
                    ScrollArea
                    Children [ { props.content } ]
                ),
                (
                    ScrollViewBar
                    @FeathersScrollbar { @target: #viewport, @orientation: {ControlOrientation::Vertical} }
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(BAR_INSET), top: px(0), bottom: px(0), width: px(BAR_W),
                    }
                    Pickable::default()     // the bar IS interactive; content under it is not
                ),
            ]
        }
    }
}
```

Notes on the details:

- **`TabIndex(0)` on the root, not the viewport.** The scroll container is the focusable, keyboard-accepting
  thing; `bevy_ui_widgets::Scrollbar`'s own doc says scrollbars deliberately have no `AccessibilityNode` and
  take no keyboard, *"which is also the responsibility of the scrollable container."*
- **`FocusWithinIndicator`, not `FocusIndicator`.** A list whose row has focus should show the list is live;
  `FocusIndicator` would only ring the container when the container itself is focused.
- **`Pickable` on the bar.** Content nodes that should not eat drags get
  `Pickable { is_hoverable: false, should_block_lower: true }`; decorative overlays get `Pickable::IGNORE`.
- **`BAR_W`/`BAR_INSET` come from `chrome`**, not `feathers::constants::size`, per §2.

### 3.4 Auto-hide, and the change-detection budget

```rust
/// Show the bar only while there is somewhere to scroll. `Display::None` rather than alpha,
/// so a bar that is not needed costs no draw and no hit-test.
fn bar_visibility(
    viewports: Query<(&ComputedNode, &ChildOf), (With<ScrollViewport>, Changed<ComputedNode>)>,
    children: Query<&Children>,
    mut bars: Query<(&mut Node, &BarPolicy), With<ScrollViewBar>>,
) { /* content_size().y > (size().y - scrollbar_size.y) ? Flex : None, written only on change */ }
```

The guard rail that governs every system in this document: **`chrome::Follow`'s doc comment is a standing
constraint.** It exists because two systems re-armed themselves every frame off `Res::is_changed`, and it
records that writing `ScrollPosition` once per move rather than sixty times a second *"is what keeps
`ScrollPosition`'s change detection meaningful for anything else reading it."* Any widget system here that
writes `ScrollPosition`, `Node` or `BackgroundColor` unconditionally per frame breaks a contract the editor
already relies on. Every system below is `Changed<..>`-gated or compares-before-writing, and the review
checklist in §10 makes that a gate.

### 3.5 Keyboard — and the census

This is the sharpest repo-specific constraint in the document, and it is worth stating plainly.

The editor's `keys.rs` holds **every binding as data**, renders it in the key census, and tests it for
collisions. `chrome::key_census` is *"the key list, read from the census and never retyped"*, and its doc
cites `docs/ui.md` §3.5 for what happens otherwise: key allocation lived in five prose censuses and all five
drifted to the same wrong answer. The 2026-08-15 usability work chose an always-on hint line over an overlay
specifically because a keyboard-only editor has nothing for an overlay to attach to, and settled that per-row
keys must be stable per item.

**A widget that installs its own `On<FocusedInput<KeyboardInput>>` observer for PageUp/PageDown/Home/End
binds keys behind the census's back.** They would not appear in the hint line, they would not be
collision-tested, and the census would start lying — a sixth prose census, which is the exact failure
`key_census`'s own doc comment names.

So the design is: **the widget declares its keys; `keys.rs` owns them.**

```rust
// keys.rs — new actions, registered like every other
pub enum Action {
    // ...
    ScrollPageUp, ScrollPageDown, ScrollHome, ScrollEnd,
}

// The widget's props take a Context; the plugin registers the actions into it at spawn.
pub struct ScrollViewProps { /* ... */ pub keys: Option<keys::Context> }
```

and one system, not one observer per widget, translates a dispatched `Action` into a write on **the scroll
view that currently owns focus** (`InputFocus` → nearest `ScrollViewRoot` ancestor). Paging arithmetic is the
browser's: one page = `visible_height - overlap`, with `overlap ≈ 2 rows` so the reader keeps a landmark.

`Home`/`End` collide with text-field editing — `EditableText` in 0.19 consumes Home/End for cursor motion.
`keys.rs` already has the stance machinery to scope that; it must be used, not worked around.

### 3.6 Nested scrollers

`scrollarea_on_scroll` calls `propagate(false)` on hit, unconditionally. An inner list at its bottom will
therefore *not* hand the wheel to the panel behind it — which is not what a browser does and not what an
author expects. The editor already nests (`tiles.rs`'s DetailPane inside the tiles panel).

Opt-in fix, per the official `scroll.rs` example's `on_scroll_handler`: replace `ScrollArea` with
`ScrollChain` on inner views, which consumes only the delta it can absorb and calls `propagate(false)` only
when the axis is fully consumed. ~30 lines. Recommend shipping it as the default for any `ScrollView` spawned
inside another, and leaving `ScrollArea` for the outermost.

### 3.7 Momentum — and why it is wheel-only

Bevy 0.19 has no inertia anywhere. `update_scrollbar_thumb` has a comment about clamping *"during inertial
scroll"*, which anticipates a user implementation writing out-of-range `ScrollPosition`.

Two hard constraints shape ours:

1. **macOS trackpads already deliver OS momentum**, as a long tail of `MouseScrollUnit::Pixel` events.
   Adding software inertia on top double-applies it and feels greasy. **Momentum applies only when
   `scroll.unit == MouseScrollUnit::Line`** — i.e. a discrete mouse wheel.
2. **It must stop writing.** Per §3.4, an integrator that writes `ScrollPosition` every frame forever
   destroys change detection for `Follow`, for `bar_visibility`, and for the virtual list's recycler. The
   component is inserted on fling and **removed** when `|velocity| < 0.5 px/s`.

```rust
#[derive(Component)]
pub struct ScrollMomentum { velocity: Vec2, decay: f32 }   // decay ≈ 0.92 per 60 Hz frame
```

No rubber-banding at the limits: clamp and drop velocity to zero. Overscroll bounce is a phone gesture, and
an editor panel that springs is an editor panel that moved when you did not ask it to.

**Recommendation: build it, ship it behind a default-off flag, and measure it at the keyboard before making
it default.** It is the one item in this document whose value is entirely a matter of feel.

---

## 4. `VirtualList`

### 4.1 Why it is a separate widget

Because `FeathersListView` cannot become one and `ScrollView` should not.

Feathers spawns every row: at the 778 meshes one labelling walk queued
(`docs/2026-08-15-blank-slate-handoff.md`) that is 778 entities, each carrying `Hovered`, `ListItem`,
`AccessibilityNode`, `ThemeBackgroundColor` and `InheritableThemeTextColor`, plus a `Text` child — and each
of them participating in taffy layout, in picking, and in the AccessKit tree, every frame. Making
`ScrollView` optionally virtual would mean one widget with two incompatible internal structures and two
incompatible sets of guarantees, which is the shape a reader cannot hold in their head.

**Threshold to virtualize: ~200 rows.** Below that, `ScrollView` with live rows is simpler, keeps
`ScrollIntoView` working, keeps a11y complete, and costs nothing measurable.

### 4.2 The structure — top spacer, live window, bottom spacer

Absolute positioning inside a scrolled node fights the scroll transform. Two spacer nodes keep everything in
normal flex flow and make the arithmetic trivial:

```
viewport (overflow: scroll_y, ScrollArea)
├── top spacer      Node { height: px(first * row_h) }
├── live row  first
├── live row  first+1
│   ...
├── live row  first+n-1
└── bottom spacer   Node { height: px((total - first - n) * row_h) }
```

**Uniform row height is a requirement, not a simplification.** Variable heights need a measured prefix-sum
table maintained across respawns; that is a different, much larger widget. Every list in the editor today is
a uniform row.

### 4.3 The row source

The hard part in Rust: a virtual list needs to build row *i* on demand, and a `Box<dyn Fn>` prop cannot touch
the World. Three options:

| Option | Verdict |
|---|---|
| `Box<dyn Fn(usize) -> Box<dyn Scene>>` in props | Cannot read `Res<Project>`. Dead on arrival for every real list here. |
| Trait + per-list impl | Needs dynamic dispatch through a registry; more machinery than the problem. |
| **Registered one-shot system taking `In<RowRange>`** | Reads any `Res`/`Query`, testable headless, matches the house style. **Recommended.** |

```rust
#[derive(Component)]
pub struct VirtualRows {
    pub source: SystemId<In<RowRange>, Vec<Box<dyn Scene>>>,
    pub row_height: f32,
    pub total: usize,          // written by the owning tab when its data changes
}

pub struct RowRange { pub first: usize, pub count: usize }
```

The recycler runs on `Or<(Changed<ScrollPosition>, Changed<VirtualRows>, Changed<ComputedNode>)>`, computes
`first = floor(scroll_y / row_h)` and `count = ceil(view_h / row_h) + OVERSCAN` (`OVERSCAN = 2`, one row of
headroom either side so a fast fling does not show a gap), diffs against the live window, and despawns/spawns
only the delta. A pure-arithmetic `visible_window(scroll_y, view_h, row_h, total) -> RowRange` is the unit-
testable core, exactly as `chrome::scroll_to_reveal` is today.

### 4.4 What virtualization costs — state it in the doc comment

1. **`ScrollIntoView` stops working**, because the target row may not exist. Replaced by an index API, which
   is *simpler* than today's `scroll_to_reveal`: `scroll_y = (index * row_h).clamp(0, max)` with the same
   above-fold / below-fold branch. `chrome::Follow<K>` survives unchanged — it arms on selection change, and
   that is orthogonal.
2. **Selection cannot live on the row entity.** A row scrolled out of the window is despawned, and its
   `Selected` marker with it. Selection lives in the tab's own resource (which is already how
   `EditorState`/`ImportState` work here) and the recycler paints it on spawn.
3. **The a11y tree is incomplete.** A screen reader sees `count` items, not `total`. Mitigation is exactly
   what AccessKit provides for this: `set_size_of_set(total)` and `set_position_in_set(i + 1)` on each live
   row, plus `set_scroll_y/_min/_max` on the container. That is what those setters are for.
4. **Hover and focus flicker across a recycle boundary.** If the focused row is despawned, `InputFocus` points
   at a dead entity. Recycler must clamp the live window to always include the focused/selected index — cheap,
   and it also makes (1) mostly moot.
5. **No `Changed<T>` on rows.** Anything that reacts to row content changes must react to the source data,
   not to the row entities.

---

## 5. `TabView`

### 5.1 Why this one is from scratch

Confirmed against the complete 0.19.1 item lists for `bevy_ui_widgets` and `bevy_feathers`: **there is no
tabbed-panel widget in Bevy 0.19.** The closest shipped reference implementation is `ListBox` /
`listbox_on_key_input`, which is the same keyboard state machine (roving active-descendant, arrows, Home/End,
activate) over a different a11y role. We copy its shape.

### 5.2 Shape

```rust
#[derive(SceneComponent, Default, Clone, Reflect)]
#[scene(TabViewProps)]
pub struct TabView;

pub struct TabViewProps {
    pub tabs: Vec<TabSpec>,
    pub orientation: ControlOrientation,
    /// What happens when the strip is narrower than its tabs.
    pub overflow: TabOverflow,
    /// Whether a hidden panel keeps its entities (and therefore its scroll position and
    /// half-typed field) or is despawned.
    pub retain: PanelRetention,
    pub keys: Option<keys::Context>,
}

pub struct TabSpec {
    pub id: TabId,                    // stable per tab, never positional — see §5.4
    pub label: String,
    /// The STALE-style annotation. A slot, so a badge is a component with its own colour,
    /// not a rewritten label string.
    pub badge: Option<Box<dyn Scene>>,
    pub panel: Box<dyn Scene>,
    pub enabled: bool,
}

#[derive(Default, Clone, Copy)]
pub enum TabOverflow {
    /// The strip becomes a horizontal `ScrollView` with `BarVisibility::Never`; the active tab is
    /// kept in view. What Chrome and Firefox do.
    #[default] Scroll,
    /// Overflowing tabs collapse into a `@FeathersMenuButton` dropdown. What VS Code does.
    Menu,
}

#[derive(Default, Clone, Copy)]
pub enum PanelRetention {
    /// Hidden panels stay spawned at `Display::None`. Scroll position, selection and half-typed
    /// text survive a round trip. Their systems keep running.
    #[default] Keep,
    /// Hidden panels are despawned. Cheap; loses all panel-local state.
    Despawn,
    /// Spawned on first activation, kept thereafter.
    Lazy,
}
```

Markers: `TabStrip`, `TabButton(TabId)`, `TabPanelNode(TabId)`, `TabIndicator`.

### 5.3 Roving tabindex — the pattern

This is what browsers and the ARIA authoring practices do, and it is the difference between a tab strip that
takes one Tab press to skip and one that takes eight.

- **The strip** carries `TabIndex(0)`, `ActiveDescendant`, `AccessibilityNode(Role::TabList)` with
  `set_orientation` and `set_active_descendant`.
- **Each tab** carries **no `TabIndex`** — so the Tab key skips over the whole strip in one press — plus
  `Selectable`, `AccessibilityNode(Role::Tab)` with `set_position_in_set`/`set_size_of_set`/`set_controls`,
  `Pickable`, `Hovered`, and `ActivateOnPress`.
- **Each panel** carries `AccessibilityNode(Role::TabPanel)` with `set_labelled_by([tab_id])`.
- **One observer on the strip**, on `FocusedInput<KeyboardInput>`: Left/Right (or Up/Down per orientation)
  move `ActiveDescendant`; Home/End jump; Space/Enter activate; `propagate(false)` when handled;
  `InputFocusVisible(true)`; `commands.trigger(ScrollIntoView { entity: next })` so an off-screen tab
  scrolls in.
- **Clicking a tab's text child focuses the tab**, because `AcquireFocus` bubbles until it finds a `TabIndex`
  — but the tabs have none, so it reaches the strip. That is the correct roving behaviour and it is free.

Whether Left/Right *activates* or merely *moves focus* is a real fork: ARIA calls the first "automatic
activation" and the second "manual". **Automatic is right here** — the editor's tabs are cheap views of the
same project, the audit records `left`/`right` already switching them, and manual activation would add a
confirm press to a motion authors do constantly.

### 5.4 Stable ids, and the badge slot

Two requirements come straight out of the repo's own record.

**`TabId` must be stable per tab, never positional.** `docs/research/2026-08-15-chooser-plan-vetting.md`
settled this for per-row keys (*"stable per item, not per position"*) and the same argument applies: a tab
whose keyboard shortcut moves when a sibling appears is a tab an author cannot build a habit on.

**A badge is a slot, not a rewritten label.** Audit defect 5: `anim_watch.rs` rewrites the tab label to
`"ANIM (N STALE)"` in the tab's normal colour, while the pane itself calls STALE *"the one word here allowed
to shout"* and paints it `DANGER`. `TabSpec::badge` is a `Box<dyn Scene>` so the shout survives into the
strip, and no code path rewrites a label string to smuggle state.

### 5.5 The animated indicator, and its one-frame lag

An absolutely positioned underline whose `left`/`width` chase the selected tab's `ComputedNode`:

```rust
fn drive_indicator(
    selected: Query<(&ComputedNode, &UiGlobalTransform), (With<TabButton>, With<Selected>)>,
    mut indicator: Query<&mut Node, With<TabIndicator>>,
    time: Res<Time>,
) { /* exponential approach, stop writing inside 0.5 px */ }
```

Two things to get right, both instances of a defect class this repo has already paid for:

- **It must run in `PostUpdate` after `UiSystems::Layout`**, reading the layout it is chasing.
- **On the frame the selection changes, `ComputedNode` still describes the previous layout.** That is exactly
  `chrome::Follow<K>`'s founding observation. The indicator's easing hides it (it is chasing a target, not
  snapping to it), but a `PanelRetention::Despawn` strip that re-lays out on switch will show one frame of
  wrong width unless the first frame is skipped.
- **Stop writing inside half a pixel**, per §3.4.

### 5.6 Panel retention — the recommendation

**`PanelRetention::Keep` (`Display::None`) as the default.** Browsers keep the DOM; the editor's panels hold
scroll position, a selection cursor and sometimes a half-typed field, and losing those on a tab switch is the
kind of small betrayal that makes a tool feel hostile. The costs are real and should be written into the
widget's doc comment: hidden panels' systems keep running (queries still match, `Changed<T>` still fires), and
memory is the sum of all tabs rather than the max.

`Lazy` is the right default for a panel that is expensive to build — the Compose pane, on the audit's account
— and `Despawn` for anything holding a GPU resource.

### 5.7 Reconciling with what exists

The editor has two tab-like things today and neither is a `TabView` yet:

- **`Door`** (Kit / Map / Rigs) — a top-level mode with a `Res<Door>`, run conditions keyed off it, and its
  own strip in `tiles::spawn_tab_strip`. This is a *window/workspace* switch, not a tab bar, and it should
  probably stay bespoke; forcing it into `TabView` buys a strip and inherits a panel-retention model it does
  not want (each door owns different resources).
- **`Mode`** within the Kit door (Meshes / Tiles / Compose), plus `tiles::tab_strip`'s two-tab
  MESHES/KIT strip. **These are the migration targets.** Both are hand-rolled `Text` + `TextColor(ACCENT/DIM)`
  strips with an inline hint string, and both are in the audit's drift list.

Suggested order: `tiles::tab_strip` first (two tabs, one file, contained blast radius), `Mode` second, `Door`
last or never.

---

## 6. What this replaces in `chrome.rs`

| Today | After | Note |
|---|---|---|
| `chrome::scroll_list(parent, marker)` | `@ScrollView { @content: ... }` | Fixes audit defect 1 by construction: you cannot forget a `ScrollArea` you never spell. |
| `chrome::scroll_to_reveal(row, list, scroll_y, inv_scale)` | `commands.trigger(ScrollIntoView { entity })` | Upstream does the same minimum-adjustment arithmetic, including the over-tall-row case. **The unit tests should be kept and re-pointed** — they are the only thing that would notice upstream changing its mind. |
| `chrome::Follow<K>` | **kept, unchanged** | `ScrollIntoView` has the same one-frame layout-lag problem `Follow` was built for. It is the arming discipline, not the arithmetic. |
| `chrome::list_row`, `chip`, `text_field` | `@FeathersListRow`, `@FeathersButton { @variant: Plain }`, `@FeathersTextInput` | Out of scope for this document, but this is where the audit's "missing builders" table gets answered. |
| `chrome::PANEL_BG` … as raw `Color` consts | kept as consts, **plus** a `ThemeToken` each | §2. The consts stay so the gizmo/world-ink colours and the CPU plot palette keep working. |

Non-goal for this pass: the audit's typography findings. Six sizes with no role map is a real problem and it
is not a scroll-view problem.

---

## 7. Plugin wiring

```rust
// Cargo.toml — the feature is optional and is not currently enabled anywhere in this workspace.
bevy = { workspace = true, features = ["jpeg", "bevy_feathers"] }
accesskit = "0.24"    // bevy_a11y does NOT re-export it
```

```rust
pub struct EditorWidgetsPlugin;

impl Plugin for EditorWidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FeathersPlugins)                  // TabNavigationPlugin + FeathersCorePlugin
           .insert_resource(UiTheme(chrome::theme()))     // NOT optional — default is empty => fuchsia
           .add_systems(PostUpdate, (
               bar_visibility,
               drive_indicator.after(UiSystems::Layout),
               recycle_virtual_rows,
           ))
           .add_observer(tab_strip_keys)
           .add_observer(scroll_chain);
    }
}
```

Wiring facts that will bite otherwise:

- **`FeathersPlugins` already includes `TabNavigationPlugin`.** Adding it twice is a duplicate-observer bug,
  not a no-op.
- **`TabNavigationPlugin` installs its observer on the primary window at `Startup`.** In `build_headless`
  (`WgpuSettings { backends: None }`, no window) there is no primary window, so tab navigation silently does
  nothing in tests. Any headless assertion about focus must drive `InputFocus` directly rather than
  synthesising a Tab key.
- **This goes in the shared plugin list**, the one `main.rs` and `harness::build_headless` both use — "one
  plugin list, two entry points", per the crate's `CLAUDE.md`.
- **A missing `Res<T>` panics its system in Bevy 0.19**, and all run conditions are evaluated. Every system
  above takes `Option<Res<..>>` or is `init_resource`'d.

---

## 8. The honest risks

### 8.1 BSN removes spawn boilerplate, not update boilerplate

**BSN in 0.19 has no reactivity.** No signals, no diffing, no dependency tracking. `apply_scene` exists and
overwrites rather than diffing, and re-runs observers, so it is not a per-frame update path.

Feathers itself is the proof: `checkbox.rs` maintains its styling with a `Changed<Hovered>`-gated system that
walks `iter_descendants` for marker components, **plus** a second `RemovedComponents`-driven system for the
removal edges — because `Changed`/`Added` do not fire on removal. That is more code than the `bsn!` block it
maintains.

The editor's panel code is roughly half spawn and half `Changed<T>` repaint (`paint_label_progress`,
`light_the_back_button`, the status/problem systems). **BSN improves the first half.** Expect the second half
to look exactly as it does today. If the expectation going in is "BSN will shrink the UI code," it will
disappoint; if it is "BSN will make the tree declarative and keep the repaint systems," it will not.

### 8.2 Feathers is experimental, and says so

Its own docs suggest copying the code rather than depending on it. Two mitigations, and the choice matters:

- **Depend** (recommended): the editor ships to nobody, so an API break costs an afternoon, not a release.
  Pin exactly. Expect churn at 0.20 — the crate went from spawn-functions to BSN inside 0.19, and the
  deprecated `*_bundle` functions are still sitting there with doc notes pointing at names that no longer
  exist in the public API.
- **Vendor** the two files we actually lean on (`scrollbar.rs`, `listview.rs`) the way `bevy_debugger_mcp`
  is vendored, and own them. Costs the theme system and the focus outlines, which are the good parts.

### 8.3 A fifth dialect is the failure mode

The audit's verdict is that four tabs are drifting into four dialects. **A widget set adopted by one tab and
not the others makes it five.** Whatever the migration order, it should be a sweep with a deadline rather than
an opportunistic per-tab thing — and `chrome.rs` should not be left holding two vocabularies for the same
shape for longer than one working session.

### 8.4 Smaller ones

- **`ScrollArea` vs `Scrollbar` disagree about `scrollbar_size`** (§1.1). Overlay gutters avoid it.
- **`Scrollbar`'s drag mapping** uses `distance * content_size / track_length` rather than the exact
  `track - thumb` denominator, so the thumb drifts under the cursor at extreme content ratios. Upstream's
  problem; visible on a list of that size.
- **`ControlOrientation`'s `#[default]` variant is not documented.** Always pass it explicitly.
- **`ScrollbarThumb` must not get a `Node`.** It will look like it works and then fight `update_scrollbar_thumb`.
- **`FeathersColorPlane` is classified as an enum on docs.rs**, which is odd for a `SceneComponent`. Irrelevant
  here, but a sign the crate's shape is still moving.

---

## 9. Before writing code

### 9.1 Verify against 0.19.0, not 0.19.1

Every API name, signature and source excerpt in this document was read from **docs.rs for `bevy` 0.19.1** and
the **`v0.19.1` git tag**. This workspace pins **`bevy = "0.19.0"`**, and `crates/emerge-mapper/CLAUDE.md` is
explicit: *"Read the vendored source (`~/.cargo/registry/src/index.crates.io-*/bevy-0.19.0/`, and its
`examples/`), not bevy.org — that documents `main` and has been wrong for this pin more than once."*

That rule applies to this document too. **First implementation task: diff this document's API surface against
the vendored 0.19.0 sources** and correct or annotate anything that moved. Highest-risk items, because they
are newest:

- `bevy_feathers::controls::listview` and `scrollbar` (BSN migration landed inside the 0.19 cycle)
- `SceneComponent` / `#[scene(..)]` / `bsn_list!` exact spellings
- `ScrollIntoView` (its `EntityEvent` propagation config is marked unverified in the research)
- `AccessibleLabel`'s module path — `bevy_ui::accessibility`, re-exported through `bevy::prelude`
- `bevy_feathers::dark_theme::create_dark_theme` and the token constant names in §2

Either the workspace bumps to 0.19.1 (it is a patch release, 2026-08-13) or the doc gets a verified-against-
0.19.0 pass. The bump is cheaper.

### 9.2 Open questions for the keyboard

1. **Fonts** (§2.1) — Fira wholesale, or override `InheritableFont` per widget root? Affects every widget.
2. **Momentum default** (§3.7) — on or off out of the box? Recommend off, measure, then decide.
3. **Migration order** (§5.7) — is `Door` in scope, or does it stay bespoke?
4. **Vendor or depend** (§8.2).
5. **Does `Mode` want `PanelRetention::Keep`?** It means the Compose pane's systems keep running while the
   Meshes tab is showing. That may be desirable (a background solve continues) or not (a background solve
   continues).

---

## 10. Implementation plan

Each step is independently landable and testable headless.

| # | Step | Test |
|---|---|---|
| 1 | Verify §9.1 against vendored 0.19.0; correct this doc | — |
| 2 | Enable `bevy_feathers`, add `accesskit`, `chrome::theme()`, `EditorWidgetsPlugin` in the shared plugin list | `build_headless` survives its first frame |
| 3 | `ScrollView` + markers + `bar_visibility`; port `chrome::scroll_list` callers | headless: spawn, assert `ScrollArea` present on the viewport of every `ScrollViewRoot` — the assertion audit defect 1 would have failed |
| 4 | Retire `scroll_to_reveal` for `ScrollIntoView`; keep and re-point its unit tests; keep `Follow<K>` | existing arithmetic tests, re-pointed |
| 5 | `keys.rs` actions for paging + the focus-owner routing system | census collision test; headless key dispatch |
| 6 | `TabView`, roving tabindex, a11y roles, indicator | headless: activation moves `Selected` and `ActiveDescendant`; arrows wrap at the ends |
| 7 | Migrate `tiles::tab_strip` (2 tabs), then `Mode` | devshot capture per tab, compared against the 2026-08-17 baselines |
| 8 | `VirtualList` + `visible_window` arithmetic + recycler | unit-test `visible_window` at every boundary; headless: 10 000 rows, assert live entity count is bounded |
| 9 | Polish: sticky headers, momentum behind a flag, indicator easing | at the keyboard |

**Review gate for every step** (from §3.4 and the crate's own rules): no system writes `ScrollPosition`,
`Node` or a colour unconditionally per frame; every one is `Changed<..>`-gated or compares before writing.

---

## Sources

Bevy, official:
- [Bevy 0.19 release notes](https://bevy.org/news/bevy-0-19/) · [Migration guide 0.18 → 0.19](https://bevy.org/learn/migration-guides/0-18-to-0-19/)
- [`bevy::ui_widgets`](https://docs.rs/bevy/0.19.1/bevy/ui_widgets/index.html) · [`ScrollPosition`](https://docs.rs/bevy/latest/bevy/ui/struct.ScrollPosition.html) · [`bevy::scene` / `bsn!`](https://docs.rs/bevy/0.19.1/bevy/scene/macro.bsn.html) · [`bevy::ecs::template`](https://docs.rs/bevy/0.19.1/bevy/ecs/template/index.html)
- [`bevy_feathers` 0.19.1](https://docs.rs/bevy_feathers/latest/bevy_feathers/) · [all items](https://docs.rs/bevy_feathers/latest/bevy_feathers/all.html) · [`tokens`](https://docs.rs/bevy_feathers/latest/bevy_feathers/tokens/index.html)
- [`bevy` cargo features](https://docs.rs/crate/bevy/0.19.0/features)
- Sources at tag `v0.19.1`: `crates/bevy_ui_widgets/src/{scrollarea,scrollbar,list}.rs`, `crates/bevy_feathers/src/controls/{listview,scrollbar,button,checkbox}.rs`, `examples/ui/scroll_and_overflow/{scroll,drag_to_scroll,scrollbars}.rs`, `examples/ui/widgets/feathers_gallery.rs`, `examples/scene/bsn.rs`
- [Standard Headless Widgets — discussion #16900](https://github.com/bevyengine/bevy/discussions/16900) · [Next-gen Scene/UI — discussion #14437](https://github.com/bevyengine/bevy/discussions/14437) · [PR #23413](https://github.com/bevyengine/bevy/pull/23413)

This repo:
- `crates/emerge-mapper/src/chrome.rs` — the palette, `scroll_list`, `scroll_to_reveal`, `Follow<K>`, `key_census`
- `crates/emerge-mapper/src/tiles.rs` — `Door`, `Mode`, `tab_strip`, `spawn_tab_strip`
- `crates/emerge-mapper/CLAUDE.md` — the 0.19 pin, the vendored-source rule, headless/BRP discipline
- `docs/2026-08-17-mapper-ui-audit.md` — the seven defects, the palette leaks, the missing builders
- `docs/research/2026-08-17-editor-experience-corpus.md` — colour-guides-more-than-messages, reduce-noise
- `docs/research/2026-08-15-chooser-plan-vetting.md` — stable-per-item keys, the always-on hint line
- `docs/ui.md` §3.1 "Panels are rows, not strings", §3.5 (the five drifted prose censuses)
- `docs/2026-08-15-blank-slate-handoff.md` — the 778-mesh labelling walk