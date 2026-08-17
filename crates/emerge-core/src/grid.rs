//! **The authoring grid** — one definition of what a cell is.
//!
//! Two things need it and they must agree: the flood fill decides how far apart to place copies of a
//! piece, and the importer tells an author how many cells a mesh will occupy. When those disagree the
//! importer is lying, and it did: the fill rounded a span to the nearest cell while the importer
//! ceilinged it, so a 0.74 m piece was reported as occupying two cells and then packed into one.
//!
//! Nothing here is clever. It is here so there is exactly one of it.

/// The authoring grid, metres. Half a metre is the unit this project's kits are authored on.
///
/// **This is the FOOTPRINT quantum, not the placement lattice.** [`snap_span`] and [`cells`] round a
/// piece's extent to it, which is what the flood fill and the importer must agree on. Where a piece
/// *lands* is [`SnapLevel`]'s ladder, which divides [`TILE`] instead — see that type for why the two
/// are no longer the same number.
pub const SNAP: f32 = 0.5;

/// **The tile — level 0 of the placement ladder, and the solver's cell.**
///
/// One number, here rather than in the editor: `grammar::solve` lays prototypes at this spacing and
/// `from_compositions` refuses any composition that is not exactly this across, so a second statement
/// of it in the editor was a second chance to disagree.
pub const TILE: f32 = 1.0;

/// **Where a piece may land: the tile, or a rung below it.**
///
/// # Why a ladder at all
///
/// The placement lattice used to be [`SNAP`] — half a tile — for a documented reason that was about
/// *furniture*: a 0.55 m bench should tile on 0.5 m rather than 1.0 m. It was never a decision about
/// cell-sized tiles, and it was wrong for them: `Placed::at` is a piece's CENTRE, and a 1 m tile
/// centred on a whole metre spans `[0.5, 1.5]` — straddling two solver cells, which `to_cell` then
/// floors into one it only half covers. Half the reachable positions were wrong and nothing on screen
/// said which.
///
/// # Why the corner and not the centre
///
/// `grammar::cell_centre` is `min + (c + 0.5) * TILE`, so level 0's lattice for a 1 x 1 piece is
/// **not** multiples of `TILE` — it is multiples of `TILE` offset by half the footprint. A ladder that
/// divides the pitch and forgets that phase reintroduces the straddle at every rung. So the rule is
/// stated on the piece's **minimum corner** ([`snap_corner`]), which is correct by construction for
/// any footprint and any pitch.
///
/// # Why not "subgrid" or "nesting"
///
/// Both words are taken. `Descriptor::subgrid` is the per-mesh edge-token lattice and *nesting* is
/// `Body::Composition` at depth ≤ 8; FVS-R-15 records that a third claimant retires a word for new
/// things.
///
/// # A rung is not a licence to put a tile anywhere
///
/// A piece the solver can see — a composition stamp — is pinned to [`SnapLevel::Tile`] and may not use
/// the rungs below. `pcgbook-ch11-mixed-initiative-content-creation`: *"All content that a human can
/// produce using a mixed-initiative PCG system must be possible for the computer to generate on its
/// own."* A stamp at a third of a cell is a state the solver cannot produce, so it is not offered.
/// The rungs are a dressing tool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnapLevel {
    /// The tile itself. The only level a solver-visible piece may use.
    #[default]
    Tile,
    /// One rung down — `TILE / divisor`.
    Fine,
    /// Two rungs down — `TILE / divisor²`.
    Finer,
}

impl SnapLevel {
    /// How many times the tile is divided at this level.
    pub fn depth(self) -> u32 {
        match self {
            SnapLevel::Tile => 0,
            SnapLevel::Fine => 1,
            SnapLevel::Finer => 2,
        }
    }

    /// **One rung down, and it stops at the bottom.**
    ///
    /// Saturating rather than wrapping, because this is what `Shift` means at a call site — *"finer
    /// than what I am on"* — and a modifier that jumps from the finest rung back to the whole tile
    /// would be the largest possible movement dressed as the smallest. Wrapping belongs to a key
    /// that cycles, and that is [`SnapLevel::next`].
    pub fn finer(self) -> SnapLevel {
        match self {
            SnapLevel::Tile => SnapLevel::Fine,
            SnapLevel::Fine | SnapLevel::Finer => SnapLevel::Finer,
        }
    }

    /// **The next rung, wrapping** — what a key that cycles the ladder steps through.
    ///
    /// Coarsest first, so an author who presses it once from the default lands on the rung they are
    /// most likely to want next rather than on the finest one.
    pub fn next(self) -> SnapLevel {
        match self {
            SnapLevel::Tile => SnapLevel::Fine,
            SnapLevel::Fine => SnapLevel::Finer,
            SnapLevel::Finer => SnapLevel::Tile,
        }
    }

    /// The lattice pitch in metres, dividing [`TILE`] by `divisor` once per rung.
    ///
    /// `divisor` is a project policy rather than a constant, so a kit authored on halves and one
    /// authored on thirds can both say so. Values below 2 would make every rung the tile, which is a
    /// ladder with no rungs — clamped rather than refused, because a policy file is authored by hand
    /// and a silently-1 divisor should still place pieces somewhere sane.
    pub fn pitch(self, divisor: u32) -> f32 {
        let d = divisor.max(2);
        TILE / (d.pow(self.depth()) as f32)
    }
}

/// **Snap a piece so its minimum corner lands on the lattice.**
///
/// `centre` and the answer are both piece centres (what `Placed::at` holds); `span` is the piece's
/// extent along that axis. Returns the centre that puts the near edge on a multiple of `pitch`.
///
/// Stated on the corner rather than the centre because the phase differs per footprint — see
/// [`SnapLevel`]. A 1 m piece at the tile pitch can then only ever land filling exactly one cell,
/// whatever the author aimed at.
pub fn snap_corner(centre: f32, span: f32, pitch: f32) -> f32 {
    if !(pitch.is_finite() && pitch > 0.0) || !centre.is_finite() || !span.is_finite() {
        return centre;
    }
    let half = span * 0.5;
    let min = centre - half;
    (min / pitch).round() * pitch + half
}

/// **Snap a piece's centre to the lattice**, so the tile's own centre is always reachable.
///
/// The counterpart to [`snap_corner`], and the one an author positioning something *inside* a cell
/// wants. Reachable positions are multiples of `pitch`, which includes zero — so a piece can always
/// be returned to the middle, and every cell centre is on the lattice at every rung.
///
/// **Why both exist.** `snap_corner` aligns a piece's near edge, which is what butts architecture
/// against a cell boundary; this aligns its middle, which is what centres a lamp in a cell. Neither
/// subsumes the other and neither is a fallback for the other: they answer different questions and
/// the caller knows which it is asking.
///
/// The failure that produced it: nudging in the tile assembler is *relative*, so a piece keeps
/// whatever phase it starts on. `shift`+arrow flushes to an absolute edge — 0.40 for a 0.2 m piece in
/// a 1 m tile — and from there every nudge at the 333 mm rung lands on 0.067 or 0.733. The centre
/// becomes unreachable, permanently, and the author who asked *"shouldn't the movements include a
/// centre placement too"* had found exactly that.
pub fn snap_centre(centre: f32, pitch: f32) -> f32 {
    if !(pitch.is_finite() && pitch > 0.0) || !centre.is_finite() {
        return centre;
    }
    (centre / pitch).round() * pitch
}

/// The cell size a span of `span` metres occupies: the nearest whole number of cells, never zero.
///
/// **Nearest, not next-largest.** A 0.55 m bench should tile on 0.5 m rather than 1.0 m — rounding up
/// would leave 450 mm of floor between every pair, which is the striping the flood fill was fixed for.
/// The cost is that a piece slightly wider than its cell overlaps its neighbour by the difference, and
/// that is the right trade for set dressing: a 5 cm overlap is invisible and a 45 cm gap is not.
pub fn snap_span(span: f32) -> f32 {
    let cells = (span / SNAP).round().max(1.0);
    cells * SNAP
}

/// How many cells a span occupies, and the signed slack in the last one.
///
/// Positive slack is a gap when tiled; negative is an overlap. Both are worth seeing, which is why
/// this is signed rather than an absolute "error".
pub fn cells(span: f32) -> (u32, f32) {
    let size = snap_span(span);
    ((size / SNAP).round() as u32, size - span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_snaps_to_the_nearest_whole_cell() {
        assert_eq!(snap_span(0.5), 0.5);
        assert_eq!(snap_span(1.0), 1.0);
        // 0.55 rounds DOWN to one cell: rounding up would leave 450 mm between benches.
        assert_eq!(snap_span(0.55), 0.5);
        assert_eq!(snap_span(0.8), 1.0);
        assert_eq!(snap_span(1.45), 1.5);
    }

    /// Never zero, and never a step a flood fill could loop on.
    #[test]
    fn a_tiny_or_zero_span_still_occupies_a_cell() {
        assert_eq!(snap_span(0.0), SNAP);
        assert_eq!(snap_span(0.01), SNAP);
        assert_eq!(cells(0.0), (1, SNAP));
    }

    #[test]
    fn slack_is_signed_so_a_gap_and_an_overlap_read_differently() {
        let (n, slack) = cells(1.45);
        assert_eq!(n, 3);
        assert!((slack - 0.05).abs() < 1e-5, "{slack}");

        // Wider than its cell: the slack is negative, meaning neighbours overlap.
        let (n, slack) = cells(0.55);
        assert_eq!(n, 1);
        assert!((slack + 0.05).abs() < 1e-5, "{slack}");
    }

    /// **The bug the ladder exists for.** A 1 m tile must land filling exactly one cell, whatever the
    /// author aimed at — never spanning `[0.5, 1.5]`, which `grammar::to_cell` would floor into a cell
    /// the tile only half covers.
    #[test]
    fn a_tile_sized_piece_can_only_land_filling_one_cell() {
        for aim in [0.0, 0.1, 0.4, 0.49, 0.5, 0.51, 0.9, 1.0, 1.4, 1.6, 2.3, -0.3, -1.2] {
            let got = snap_corner(aim, TILE, SnapLevel::Tile.pitch(3));
            let min = got - TILE * 0.5;
            assert!(
                (min - min.round()).abs() < 1e-5,
                "aiming at {aim} put the near edge at {min}, which is not a cell boundary"
            );
            // And the centre is therefore always a half-metre offset, never a whole one.
            assert!(
                (got.fract().abs() - 0.5).abs() < 1e-5,
                "aiming at {aim} gave centre {got}, which straddles two cells"
            );
        }
    }

    /// The invariant the whole design rests on, over every rung and a spread of footprints: **the
    /// piece's minimum corner is a multiple of the pitch.** Stated as a property rather than as
    /// examples, because the phase is what examples keep getting wrong.
    #[test]
    fn the_minimum_corner_always_lands_on_the_lattice() {
        for divisor in [2u32, 3, 4, 5] {
            for level in [SnapLevel::Tile, SnapLevel::Fine, SnapLevel::Finer] {
                let pitch = level.pitch(divisor);
                for span in [0.1f32, 0.25, 0.5, 0.55, 1.0, 1.5, 2.0, 3.0] {
                    for aim in [-2.7f32, -0.4, 0.0, 0.13, 0.5, 0.97, 1.0, 4.62] {
                        let got = snap_corner(aim, span, pitch);
                        let min = got - span * 0.5;
                        let k = min / pitch;
                        assert!(
                            (k - k.round()).abs() < 1e-3,
                            "divisor {divisor} {level:?} span {span} aim {aim}: near edge {min} is \
                             {k} pitches, not a whole number"
                        );
                        // And it is the NEAREST such position, never a jump of more than half a pitch.
                        assert!(
                            (got - aim).abs() <= pitch * 0.5 + 1e-3,
                            "moved {} m, more than half a pitch of {pitch}",
                            (got - aim).abs()
                        );
                    }
                }
            }
        }
    }

    /// Thirds by default, and the rungs are the divisor applied once per level.
    #[test]
    fn the_ladder_divides_the_tile_once_per_rung() {
        assert_eq!(SnapLevel::Tile.pitch(3), 1.0);
        assert!((SnapLevel::Fine.pitch(3) - 1.0 / 3.0).abs() < 1e-6);
        assert!((SnapLevel::Finer.pitch(3) - 1.0 / 9.0).abs() < 1e-6);

        // Halves, for a kit that wants the old behaviour: the middle rung IS the old SNAP.
        assert_eq!(SnapLevel::Fine.pitch(2), SNAP);
        assert_eq!(SnapLevel::Finer.pitch(2), 0.25);

        // A ladder with no rungs is clamped rather than dividing by one forever.
        assert_eq!(SnapLevel::Finer.pitch(1), SnapLevel::Finer.pitch(2));
        assert_eq!(SnapLevel::Finer.pitch(0), SnapLevel::Finer.pitch(2));
    }

    /// Finer rungs are strictly finer, which is what makes the modifier ladder mean anything.
    #[test]
    fn each_rung_is_finer_than_the_one_above() {
        for divisor in [2u32, 3, 7] {
            let (t, f, ff) = (
                SnapLevel::Tile.pitch(divisor),
                SnapLevel::Fine.pitch(divisor),
                SnapLevel::Finer.pitch(divisor),
            );
            assert!(t > f && f > ff, "divisor {divisor}: {t} {f} {ff}");
        }
    }

    /// Nonsense in, the author's own position out — never a NaN written into a map.
    #[test]
    fn a_degenerate_pitch_or_position_returns_the_input() {
        assert_eq!(snap_corner(1.7, 1.0, 0.0), 1.7);
        assert_eq!(snap_corner(1.7, 1.0, -1.0), 1.7);
        assert_eq!(snap_corner(1.7, 1.0, f32::NAN), 1.7);
        assert!(snap_corner(f32::NAN, 1.0, 0.5).is_nan(), "a NaN aim is the caller's problem");
        assert_eq!(snap_corner(1.7, f32::INFINITY, 0.5), 1.7);
    }

    /// The bug this module exists for: two callers rounding differently. Whatever `cells` reports must
    /// be what `snap_span` lays down.
    #[test]
    fn the_cell_count_and_the_cell_size_cannot_disagree() {
        for span in [0.1, 0.24, 0.26, 0.5, 0.74, 0.76, 1.0, 1.45, 2.29, 7.5] {
            let (n, _) = cells(span);
            assert_eq!(
                n as f32 * SNAP,
                snap_span(span),
                "{span} m: reported {n} cells but lays down {}",
                snap_span(span)
            );
        }
    }

    /// **`finer` saturates and `next` wraps**, which is the difference between a modifier and a key
    /// that cycles.
    ///
    /// A `Shift` that jumped from the finest rung back to the whole tile would be the largest
    /// movement available dressed as the smallest, so the two step differently on purpose and this
    /// pins both. `emerge-mapper`'s `snap_level` reads the first; its `J` reads the second.
    #[test]
    fn the_ladder_steps_one_way_for_a_modifier_and_another_for_a_cycle() {
        assert_eq!(SnapLevel::Tile.finer(), SnapLevel::Fine);
        assert_eq!(SnapLevel::Fine.finer(), SnapLevel::Finer);
        assert_eq!(
            SnapLevel::Finer.finer(),
            SnapLevel::Finer,
            "the bottom rung stays put — a modifier must never be the biggest jump on offer"
        );

        assert_eq!(SnapLevel::Tile.next(), SnapLevel::Fine);
        assert_eq!(SnapLevel::Fine.next(), SnapLevel::Finer);
        assert_eq!(
            SnapLevel::Finer.next(),
            SnapLevel::Tile,
            "a key that cycles must come back round, or the third press does nothing"
        );

        // Every rung is reachable from every other by cycling, which is what makes one key enough.
        let mut seen = vec![SnapLevel::Tile];
        let mut at = SnapLevel::Tile;
        for _ in 0..2 {
            at = at.next();
            seen.push(at);
        }
        assert_eq!(seen, vec![SnapLevel::Tile, SnapLevel::Fine, SnapLevel::Finer]);
    }
}
