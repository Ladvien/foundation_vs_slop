//! **The authoring grid** — one definition of what a cell is.
//!
//! Two things need it and they must agree: the flood fill decides how far apart to place copies of a
//! piece, and the importer tells an author how many cells a mesh will occupy. When those disagree the
//! importer is lying, and it did: the fill rounded a span to the nearest cell while the importer
//! ceilinged it, so a 0.74 m piece was reported as occupying two cells and then packed into one.
//!
//! Nothing here is clever. It is here so there is exactly one of it.

/// The authoring grid, metres. Half a metre is the unit this project's kits are authored on and what
/// the editor snaps translation to.
pub const SNAP: f32 = 0.5;

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
}
