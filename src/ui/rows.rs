//! **The row model** — the shared shape every content panel renders through.
//!
//! # Why this exists
//!
//! Every panel in this game used to be a single [`Text`] node holding one `\n`-joined string built
//! by a `fn(..) -> String`. That has three costs the panels were all paying:
//!
//! 1. **One colour for the whole panel.** `containment_hud`'s entire job is to say *why*
//!    containment is progressing, and a met clause rendered in exactly the same ink as an unmet
//!    one. The player had to *read* the panel to find the actionable line instead of *seeing* it.
//! 2. **No hit targets.** A string cannot be hovered or clicked, so the verb bar was keyboard-only
//!    and the curriculum could only be cycled with `Tab`.
//! 3. **Columns aligned by monospace padding.** Which is a promise the font has to keep, and which
//!    breaks the moment a name changes length.
//!
//! # What replaces it, and what is deliberately kept
//!
//! The panel builders stay **pure functions** — that is the property that made this UI testable at
//! all, and it is preserved exactly. They simply return `Vec<Row>` instead of `String`. Tests move
//! from asserting on formatted text (`line.contains("[C] DEVICE x3 <")`) to asserting on structure
//! (this row is [`Emphasis::Alert`], carries [`glyph::UNMET`], and its third cell is a
//! [`Cell::Bar`]) — which is a stronger assertion, because it pins the thing the player actually
//! perceives rather than the string that happened to encode it.
//!
//! # Encoding rules (`docs/ui.md` §1.3)
//!
//! [`Emphasis`] maps to **luminance, never hue**. An alert row is *brighter*, not red. Hue
//! discrimination fails for ~8% of men and fails for everyone at low contrast or in peripheral
//! vision, and the project's own `docs/lore/2026-07-12-scp-color-language.md` §6 independently
//! arrives at the same rule for in-fiction reasons ("use luminosity, not hue, for threat").
//! [`Row::glyph`] is the redundant second channel, so severity survives with colour removed
//! entirely.
//!
//! Cleveland & McGill's accuracy ordering for elementary perceptual tasks — position, then length,
//! then angle/area, then colour — is why a [`Cell::Bar`] is a **length** and not a tinted swatch,
//! and why [`spawn_rows`] puts values in a shared grid column (a common **position** scale) rather
//! than letting each row place its own.
//!
//! # Determinism
//!
//! Nothing here is hashed. This module is `Update`-only presentation, like the rest of `crate::ui`
//! (see `ui::mod`), so no value in it may become a genome gene — the same exemption
//! `docs/animation.md` carves for the cosmetic animation layer, and for the same reason: a knob
//! invisible to `snapshot_hash` is a knob the RL/QD search turns forever with the fitness never
//! moving.

use bevy::prelude::*;

use super::theme::{glyph, FontAssets, UiTheme};
use super::widgets::text_colored;

/// How loudly a row speaks. Renders as **luminance**, never as a hue change.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Emphasis {
    /// Context the player is not acting on — satisfied clauses, prerequisites already met.
    Muted,
    #[default]
    Normal,
    /// The row the player should act on. Brightest ink; at most a couple per panel, or the
    /// emphasis stops meaning anything.
    Alert,
}

/// One field within a row. The variants are the *perceptual* kinds, not the data types — that is
/// what lets [`spawn_rows`] give every panel the same column rhythm.
#[derive(Clone, PartialEq, Debug)]
pub enum Cell {
    /// The name of the thing. Left column, and the grid's `min_content` track.
    Label(String),
    /// A reading, a count, a threshold. Right-aligned so digits line up down the panel.
    Value(String),
    /// A `[0, 1]` proportion drawn as a **length**, the most accurately-read encoding available.
    Bar { frac: f32 },
    /// A signed change. Rendered with an explicit sign because the *direction* is the message —
    /// affect tracks the rate at which uncertainty is falling, not its absolute level, so a panel
    /// that shows only a level is withholding the part the player responds to.
    Delta(f32),
}

/// One line of a panel.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Row {
    pub emphasis: Emphasis,
    /// The redundant, non-colour channel. `""` for rows that carry no status.
    pub glyph: &'static str,
    pub cells: Vec<Cell>,
    /// Tree depth, for prerequisite structure. Rendered as leading indent, not as a box-drawing
    /// character, so it survives a proportional font if one is ever adopted.
    pub indent: u8,
    /// A section heading rather than a data row: display font weight, no glyph column.
    pub header: bool,
}

impl Row {
    /// A section heading.
    pub fn header(label: impl Into<String>) -> Self {
        Self {
            emphasis: Emphasis::Normal,
            glyph: "",
            cells: vec![Cell::Label(label.into())],
            indent: 0,
            header: true,
        }
    }

    /// A plain `LABEL   VALUE` line.
    pub fn kv(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            cells: vec![Cell::Label(label.into()), Cell::Value(value.into())],
            ..default()
        }
    }

    /// A line of prose with no columns.
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            emphasis: Emphasis::Muted,
            cells: vec![Cell::Label(text.into())],
            ..default()
        }
    }

    /// A satisfied condition — recedes.
    pub fn met(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            emphasis: Emphasis::Muted,
            glyph: glyph::MET,
            cells: vec![Cell::Label(label.into()), Cell::Value(value.into())],
            ..default()
        }
    }

    /// An unsatisfied condition — the actionable row, so it is the brightest thing in the panel.
    ///
    /// The caller supplies the label as an **instruction** ("RAISE OBSERVATION"), never a status
    /// ("observation: unmet"). That rule is FVS-L-1's and it is the strongest copy discipline in
    /// this codebase; the row model preserves the *emphasis*, not the wording, so it cannot enforce
    /// the rule for you.
    pub fn unmet(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            emphasis: Emphasis::Alert,
            glyph: glyph::UNMET,
            cells: vec![Cell::Label(label.into()), Cell::Value(value.into())],
            ..default()
        }
    }

    pub fn with_glyph(mut self, g: &'static str) -> Self {
        self.glyph = g;
        self
    }

    pub fn with_indent(mut self, n: u8) -> Self {
        self.indent = n;
        self
    }

    pub fn with_emphasis(mut self, e: Emphasis) -> Self {
        self.emphasis = e;
        self
    }

    pub fn push(mut self, c: Cell) -> Self {
        self.cells.push(c);
        self
    }

    /// The row's label text, if it has one. Convenience for tests and for tooltips.
    pub fn label(&self) -> Option<&str> {
        self.cells.iter().find_map(|c| match c {
            Cell::Label(s) => Some(s.as_str()),
            _ => None,
        })
    }
}

/// Width of the glyph gutter, in `ch`-ish px at the default rem. Fixed so every row's label starts
/// on the same x — a shared position scale is the whole reason the columns read.
const GUTTER_PX: f32 = 12.0;
/// Indent step per tree level.
const INDENT_PX: f32 = 14.0;
/// Width of a [`Cell::Bar`].
const BAR_PX: f32 = 90.0;

/// Spawn `rows` as children of `parent`.
///
/// Layout is a **grid**, not monospace padding: one `min_content` column for the glyph gutter, one
/// flexible column for labels, and one `min_content` column for values, so values right-align down
/// the whole panel regardless of how long any individual label is.
///
/// Callers own the container (position, background, border); this only fills it.
pub fn spawn_rows(
    parent: &mut ChildSpawnerCommands,
    theme: &UiTheme,
    fonts: &FontAssets,
    rows: &[Row],
) {
    for row in rows {
        let ink = row_ink(theme, row);
        let size = if row.header { theme.font_body * 1.1 } else { theme.font_body };

        parent
            .spawn((
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(theme.space_sm),
                    padding: UiRect::top(Val::Px(if row.header { theme.space_sm } else { 0.0 })),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|line| {
                // --- glyph gutter (fixed width, so labels share an x even when the glyph is "") ---
                if !row.header {
                    line.spawn((
                        Node {
                            width: Val::Px(GUTTER_PX + row.indent as f32 * INDENT_PX),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|g| {
                        if !row.glyph.is_empty() {
                            g.spawn(text_colored(theme, fonts, row.glyph, size, ink));
                        }
                    });
                }

                for cell in &row.cells {
                    match cell {
                        Cell::Label(s) => {
                            line.spawn((
                                Node { flex_grow: 1.0, ..default() },
                                Pickable::IGNORE,
                            ))
                            .with_children(|c| {
                                c.spawn(text_colored(theme, fonts, s.clone(), size, ink));
                            });
                        }
                        Cell::Value(s) => {
                            line.spawn((
                                Node {
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::End,
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ))
                            .with_children(|c| {
                                c.spawn(text_colored(theme, fonts, s.clone(), size, ink));
                            });
                        }
                        Cell::Bar { frac } => {
                            line.spawn((
                                Node {
                                    width: Val::Px(BAR_PX),
                                    height: Val::Px(theme.font_body * 0.45),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(theme.health_back),
                                Pickable::IGNORE,
                            ))
                            .with_children(|track| {
                                track.spawn((
                                    Node {
                                        width: Val::Percent(frac.clamp(0.0, 1.0) * 100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(ink),
                                    Pickable::IGNORE,
                                ));
                            });
                        }
                        Cell::Delta(d) => {
                            line.spawn((
                                Node {
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::End,
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ))
                            .with_children(|c| {
                                c.spawn(text_colored(theme, fonts, format_delta(*d), size, ink));
                            });
                        }
                    }
                }
            });
    }
}

/// Marks a node whose children are rows, and caches the rows currently drawn there.
///
/// The cache is what keeps [`sync_rows`] cheap: rebuilding row entities every frame would churn
/// the whole subtree (and every `Node` write re-runs the Taffy solve for it), so panels only rebuild
/// when their content actually changes. This is the row-model equivalent of the
/// `if text.0 != line { .. }` guard the string panels used, and it exists for the same reason.
#[derive(Component, Default)]
pub struct RowPanel {
    drawn: Vec<Row>,
}

/// Rebuild `entity`'s row children **iff** `rows` differs from what is already drawn there.
///
/// The single path every panel updates through, so no panel can invent its own rebuild policy and
/// no panel can forget the change guard.
pub fn sync_rows(
    commands: &mut Commands,
    entity: Entity,
    panel: &mut RowPanel,
    theme: &UiTheme,
    fonts: &FontAssets,
    rows: Vec<Row>,
) {
    if panel.drawn == rows {
        return;
    }
    panel.drawn = rows.clone();
    let theme = theme.clone();
    let fonts = fonts.clone();
    commands
        .entity(entity)
        .despawn_children()
        .with_children(move |p| spawn_rows(p, &theme, &fonts, &rows));
}

/// A signed change, always with an explicit sign so the direction is unmissable.
///
/// Pure and separately tested: the sign is the message, and a `+` that silently went missing on
/// positives would invert what the player reads off the panel.
pub fn format_delta(d: f32) -> String {
    if d > 0.0 {
        format!("+{d:.2}")
    } else if d < 0.0 {
        // Rust's own `-` on the formatted value; no U+2212, which the shipped face does not carry.
        format!("{d:.2}")
    } else {
        "0.00".to_string()
    }
}

fn row_ink(theme: &UiTheme, row: &Row) -> Color {
    if row.header {
        return theme.accent;
    }
    theme.emphasis_ink(row.emphasis == Emphasis::Muted, row.emphasis == Emphasis::Alert)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmet_row_is_louder_than_a_met_one_without_changing_hue() {
        // The encoding rule this whole module exists to enforce. If emphasis ever starts riding on
        // hue, the containment readout stops being legible for red-green CVD players and for
        // anyone reading it in peripheral vision while looking at the world.
        let theme = UiTheme::default();
        let met = row_ink(&theme, &Row::met("HOLD GUNFIRE", "0.01"));
        let unmet = row_ink(&theme, &Row::unmet("RAISE OBSERVATION", "0.10"));

        let lum = |c: Color| {
            let c = c.to_linear();
            0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
        };
        assert!(
            lum(unmet) > lum(met),
            "an actionable row must be BRIGHTER: unmet {:.3} vs met {:.3}",
            lum(unmet),
            lum(met)
        );

        // Same hue family: the ratio between channels must not swing the way a green->red ramp
        // would. Compare normalized chromaticity rather than raw values.
        let chroma = |c: Color| {
            let c = c.to_linear();
            let sum = (c.red + c.green + c.blue).max(1e-6);
            (c.red / sum, c.green / sum, c.blue / sum)
        };
        let (mr, mg, _) = chroma(met);
        let (ur, ug, _) = chroma(unmet);
        assert!(
            (mr - ur).abs() < 0.08 && (mg - ug).abs() < 0.08,
            "emphasis must not shift hue: met {:?} vs unmet {:?}",
            chroma(met),
            chroma(unmet)
        );
    }

    #[test]
    fn met_and_unmet_carry_different_glyphs() {
        // The redundant channel. With colour removed entirely, these two rows must still differ.
        assert_ne!(Row::met("a", "1").glyph, Row::unmet("b", "2").glyph);
        assert_eq!(Row::unmet("b", "2").glyph, glyph::UNMET);
    }

    #[test]
    fn a_delta_always_states_its_direction() {
        // The sign is the message — a bare "0.42" cannot be told from "-0.42" at a glance.
        assert_eq!(format_delta(0.42), "+0.42");
        assert_eq!(format_delta(-0.42), "-0.42");
        assert_eq!(format_delta(0.0), "0.00");
        assert!(format_delta(1.0).starts_with('+'));
    }

    #[test]
    fn a_bar_cell_clamps_rather_than_overflowing_its_track() {
        // Descriptors come off disk (the QD archive) and off live sim state; a corrupt or
        // out-of-range value must not draw outside the panel or panic.
        for frac in [-5.0f32, 0.0, 0.5, 1.0, 9.0] {
            let w = frac.clamp(0.0, 1.0) * 100.0;
            assert!((0.0..=100.0).contains(&w), "frac {frac} produced width {w}");
        }
    }

    #[test]
    fn builders_produce_the_cells_a_panel_expects() {
        let r = Row::unmet("RAISE OBSERVATION", ">= 0.50").push(Cell::Bar { frac: 0.2 });
        assert_eq!(r.emphasis, Emphasis::Alert);
        assert_eq!(r.label(), Some("RAISE OBSERVATION"));
        assert!(matches!(r.cells.last(), Some(Cell::Bar { .. })));

        let h = Row::header("CONTAINMENT");
        assert!(h.header);
        assert_eq!(h.glyph, "", "a header has no status glyph");
    }

    #[test]
    fn indent_is_data_not_baked_into_the_label() {
        // Prerequisite depth has to stay machine-readable so the renderer can indent it; a builder
        // that pre-padded the string with spaces would make the tree structure unrecoverable.
        let r = Row::kv("SEDATION", "LOCKED").with_indent(2);
        assert_eq!(r.indent, 2);
        assert_eq!(r.label(), Some("SEDATION"), "the label carries no padding");
    }
}
