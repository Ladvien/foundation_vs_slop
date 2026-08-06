//! **A raster the bench can draw curves into** — RGBA8 pixels and nothing else.
//!
//! Engine-free like everything in this crate: the editor wraps the pixel buffer in a texture, but
//! what a curve *looks like* — where the line lands, how a span fills — is arithmetic, and testable
//! without a renderer. No coordinate here can panic: everything off the raster is clipped, not
//! clamped onto the edge, so an out-of-range polyline vanishes instead of smearing along a border.

/// An RGBA8 image, row-major, `w * h * 4` bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Raster {
    pub fn new(w: usize, h: usize, bg: [u8; 4]) -> Raster {
        let mut px = Vec::with_capacity(w * h * 4);
        for _ in 0..w * h {
            px.extend_from_slice(&bg);
        }
        Raster { w, h, px }
    }

    /// Write one pixel. Off-raster coordinates are clipped — ignored, never clamped.
    pub fn set(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let at = (y as usize * self.w + x as usize) * 4;
        self.px[at..at + 4].copy_from_slice(&c);
    }

    /// A full-width horizontal rule — axis lines, thresholds.
    pub fn hline(&mut self, y: usize, c: [u8; 4]) {
        for x in 0..self.w as i32 {
            self.set(x, y as i32, c);
        }
    }

    /// A vertical run of pixels at `x`, both ends included, in either order.
    pub fn vspan(&mut self, x: usize, y0: usize, y1: usize, c: [u8; 4]) {
        let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for y in lo..=hi {
            self.set(x as i32, y as i32, c);
        }
    }

    /// A straight segment, Bresenham. Endpoints are pre-clamped into a band around the raster so a
    /// wild coordinate bounds the walk instead of spinning it; pixels outside still clip.
    pub fn line(&mut self, a: [i32; 2], b: [i32; 2], c: [u8; 4]) {
        let cap = |v: i32, dim: usize| v.clamp(-(dim as i32), 2 * dim as i32);
        let (mut x0, mut y0) = (cap(a[0], self.w), cap(a[1], self.h));
        let (x1, y1) = (cap(b[0], self.w), cap(b[1], self.h));
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.set(x0, y0, c);
            if x0 == x1 && y0 == y1 {
                return;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// **A sampled curve across the full width**, `lo` at the bottom row and `hi` at the top,
    /// consecutive columns joined by vertical spans so a steep curve stays connected. Non-finite
    /// samples and a degenerate range draw nothing — a blank is honest, a made-up line is not.
    pub fn curve(&mut self, ys: &[f32], lo: f32, hi: f32, c: [u8; 4]) {
        if ys.is_empty() || !(hi > lo) || self.w == 0 || self.h == 0 {
            return;
        }
        let (w, h) = (self.w, self.h);
        let to_row = move |v: f32| -> Option<i32> {
            if !v.is_finite() {
                return None;
            }
            let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
            Some(((1.0 - t) * (h - 1) as f32).round() as i32)
        };
        let mut prev: Option<i32> = None;
        for x in 0..w {
            let at = x as f32 / w as f32 * ys.len() as f32;
            let row = to_row(ys[(at as usize).min(ys.len() - 1)]);
            match (prev, row) {
                (Some(p), Some(r)) => self.vspan(x, p.max(0) as usize, r.max(0) as usize, c),
                (None, Some(r)) => self.set(x as i32, r, c),
                _ => {}
            }
            prev = row;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: [u8; 4] = [255, 255, 255, 255];
    const BG: [u8; 4] = [0, 0, 0, 255];

    fn painted(r: &Raster) -> usize {
        r.px.chunks(4).filter(|p| *p == INK).count()
    }

    #[test]
    fn a_curve_touches_every_column() {
        let mut r = Raster::new(64, 16, BG);
        let ys: Vec<f32> = (0..32).map(|i| (i as f32 * 0.3).sin()).collect();
        r.curve(&ys, -1.0, 1.0, INK);
        for x in 0..64usize {
            let hit = (0..16).any(|y| {
                let at = (y * 64 + x) * 4;
                r.px[at..at + 4] == INK
            });
            assert!(hit, "column {x} has no ink");
        }
    }

    #[test]
    fn lines_land_on_their_endpoints_and_wild_coordinates_cannot_spin() {
        let mut r = Raster::new(32, 32, BG);
        r.line([2, 3], [29, 17], INK);
        let at = |x: usize, y: usize| (y * 32 + x) * 4;
        assert_eq!(&r.px[at(2, 3)..at(2, 3) + 4], &INK);
        assert_eq!(&r.px[at(29, 17)..at(29, 17) + 4], &INK);
        // A far-off segment is clipped to nothing, in bounded time, without panicking.
        let before = painted(&r);
        r.line([-1_000_000, 5], [5, 1_000_000], INK);
        assert!(painted(&r) >= before);
    }

    #[test]
    fn off_raster_pixels_clip_rather_than_clamp() {
        let mut r = Raster::new(8, 8, BG);
        r.set(-1, 0, INK);
        r.set(0, -1, INK);
        r.set(8, 0, INK);
        r.set(0, 8, INK);
        assert_eq!(painted(&r), 0, "nothing may smear onto an edge");
    }

    #[test]
    fn empty_and_degenerate_inputs_draw_nothing() {
        let mut r = Raster::new(8, 8, BG);
        r.curve(&[], 0.0, 1.0, INK);
        r.curve(&[0.5, f32::NAN, 0.5], 1.0, 1.0, INK); // hi == lo
        let nan_only = [f32::NAN, f32::NAN];
        r.curve(&nan_only, 0.0, 1.0, INK);
        assert_eq!(painted(&r), 0);
    }
}
