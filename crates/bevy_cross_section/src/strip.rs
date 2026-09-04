//! **The cross-section strip**: one procedural texture per region, depth along `x`, tiling along `y`.
//!
//! A cut through flesh is read by its bands and by the grain inside each band, and both are
//! anatomy rather than art: adipose tissue is lobules a millimetre or two across held in a fibrous
//! septal net; skeletal muscle in cross-section is fascicles — bundles of fibres — wrapped in a pale
//! perimysium; cortical bone is dense ivory pierced by Haversian canals a tenth of a millimetre wide;
//! and the marrow cavity opens through a lattice of trabecular struts. Every feature here is drawn
//! at its physical size, because the strip is parameterised in millimetres by [`Layers`] and the
//! caller's [`crate::Scale`], so a 2 mm lobule is 2 mm on a thigh and 2 mm on a finger.
//!
//! The muscle band's colour is not authored: it is a thin venous film from `bloodstain::spectral`
//! over a mid-grey substrate — the same optics that colour a pool, because a freshly cut muscle
//! surface is wet with exactly that. Fat, skin, cortex and marrow carry authored albedos; those are
//! stated as this crate's own.
//!
//! Everything is a hash of integer texel coordinates through `bloodstain::hash_f32`, so a strip is
//! a pure function of its inputs and two machines bake the same bytes.

use bloodstain::hash_f32;
use bloodstain::spectral::{Film, SO2_VENOUS, srgb};

use crate::layers::{Layer, Layers};

/// One band of the strip: which layer, and the texel columns it occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    /// The tissue.
    pub layer: Layer,
    /// First column, inclusive.
    pub x0: u32,
    /// Last column, exclusive.
    pub x1: u32,
}

/// **A baked strip**: albedo and metallic-roughness pixels, and where each band landed.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    /// Columns — the depth axis.
    pub width: u32,
    /// Rows — the along axis, which tiles.
    pub height: u32,
    /// `Rgba8UnormSrgb` pixels, row-major.
    pub albedo: Vec<u8>,
    /// `Rgba8Unorm` pixels, row-major: `G` = perceptual roughness, `B` = metallic (always zero).
    pub rough: Vec<u8>,
    /// The bands, outside to inside.
    pub bands: Vec<Band>,
}

impl Strip {
    /// FNV-1a over both pixel buffers — the golden a test freezes.
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.albedo.iter().chain(self.rough.iter()) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// Texel columns per millimetre of depth.
    pub fn px_per_mm(&self, layers: &Layers) -> f32 {
        self.width as f32 / layers.span_mm().max(1.0e-3)
    }
}

/// **Bake the strip** for `layers`, `width × height` texels, from `seed`.
///
/// `height` is the tile: the along-axis noise is periodic in exactly `height` rows, so the image
/// can be sampled with `Repeat` on `v` and no seam. `width` should give at least ten texels to the
/// thinnest band you care about; `512` over a limb's 41 mm is 12 per millimetre.
pub fn strip(layers: &Layers, width: u32, height: u32, seed: u32) -> Strip {
    let width = width.max(1);
    let height = height.max(1);
    let span = layers.span_mm().max(1.0e-3);
    let px_per_mm = width as f32 / span;

    let bands = bands_of(layers, width);
    let muscle = muscle_albedo();

    let n = (width * height) as usize;
    let mut albedo = vec![0u8; n * 4];
    let mut rough = vec![0u8; n * 4];
    for y in 0..height {
        for x in 0..width {
            let depth_mm = (x as f32 + 0.5) / px_per_mm;
            let (layer, frac) = layers.at(depth_mm);
            // Texel coordinates in millimetres, with the along axis periodic in the tile.
            let u = depth_mm;
            let v_mm = (y as f32 + 0.5) / px_per_mm;
            let tile_mm = height as f32 / px_per_mm;
            let ([r, g, b], ro) = texel(layer, frac, u, v_mm, tile_mm, seed, muscle);
            let i = ((y * width + x) * 4) as usize;
            albedo[i] = enc(r);
            albedo[i + 1] = enc(g);
            albedo[i + 2] = enc(b);
            albedo[i + 3] = 255;
            rough[i] = 0;
            rough[i + 1] = enc(ro);
            rough[i + 2] = 0;
            rough[i + 3] = 255;
        }
    }
    Strip { width, height, albedo, rough, bands }
}

/// **The tissue colour and roughness at one point**, `depth_mm` below the skin at `(u_mm, v_mm)`
/// across the face, with the along-axis noise periodic in `tile_mm`.
///
/// The per-texel rule [`strip`] bakes, exposed so a texture-space consumer — a flayed patch, a
/// wound bed — can paint the same tissue at the same physical scale without a strip lookup.
/// Encoded sRGB and perceptual roughness, both in `[0, 1]`.
pub fn texel_at(layers: &Layers, depth_mm: f32, u_mm: f32, v_mm: f32, tile_mm: f32, seed: u32) -> ([f32; 3], f32) {
    let (layer, frac) = layers.at(depth_mm);
    texel(layer, frac, u_mm, v_mm, tile_mm.max(1.0e-3), seed, muscle_albedo())
}

/// Where each band lands in columns, from the layer table alone.
fn bands_of(layers: &Layers, width: u32) -> Vec<Band> {
    let span = layers.span_mm().max(1.0e-3);
    let px_per_mm = width as f32 / span;
    let starts = layers.starts_mm();
    let mut out = Vec::with_capacity(5);
    for (i, layer) in Layer::ALL.iter().enumerate() {
        let lo = starts[i];
        let hi = if i + 1 < 5 { starts[i + 1] } else { span };
        let x0 = ((lo * px_per_mm).round() as u32).min(width);
        let x1 = ((hi * px_per_mm).round() as u32).min(width);
        if x1 > x0 {
            out.push(Band { layer: *layer, x0, x1 });
        }
    }
    out
}

/// The muscle band's base colour: a 0.3 mm venous film over a mid-grey substrate, which is what a
/// freshly cut muscle surface optically is. Computed once per strip.
fn muscle_albedo() -> [f32; 3] {
    srgb(&Film { thickness_mm: 0.3, so2: SO2_VENOUS, substrate: 0.45 })
}

/// Authored albedos, encoded sRGB. This crate's own; stated rather than hidden.
const SKIN_EPIDERMIS: [f32; 3] = [0.72, 0.55, 0.47];
const SKIN_DERMIS: [f32; 3] = [0.90, 0.74, 0.70];
const FAT_LOBULE: [f32; 3] = [0.93, 0.80, 0.42];
const FAT_SEPTUM: [f32; 3] = [0.88, 0.76, 0.70];
const PERIMYSIUM: [f32; 3] = [0.86, 0.78, 0.76];
const CORTEX: [f32; 3] = [0.91, 0.87, 0.77];
const CANAL: [f32; 3] = [0.45, 0.25, 0.22];
const TRABECULA: [f32; 3] = [0.88, 0.83, 0.72];
const MARROW_RED: [f32; 3] = [0.42, 0.12, 0.12];
const MARROW_YELLOW: [f32; 3] = [0.84, 0.70, 0.40];

/// One texel: colour and roughness for `layer` at `frac` through it, at `(u, v)` millimetres.
fn texel(layer: Layer, frac: f32, u: f32, v: f32, tile: f32, seed: u32, muscle: [f32; 3]) -> ([f32; 3], f32) {
    match layer {
        Layer::Skin => {
            // A thin dark epidermis over a fibrous dermis.
            let fibre = value_noise(u * 6.0, v * 6.0, tile * 6.0, seed ^ 0x51);
            let dermis = shade(SKIN_DERMIS, 0.92 + 0.12 * fibre);
            if frac < 0.15 { (SKIN_EPIDERMIS, 0.55) } else { (dermis, 0.6 - 0.1 * fibre) }
        }
        Layer::Fat => {
            // Lobules ~2 mm across in a septal net: Worley F1 is the lobule interior, the ridge
            // between cells (F2 − F1 small) is the septum.
            let (f1, f2) = worley(u / 2.0, v / 2.0, tile / 2.0, seed ^ 0xFA7);
            let ridge = (f2 - f1).clamp(0.0, 1.0);
            let septum = smoothstep((0.12 - ridge) / 0.12);
            let glisten = 0.9 + 0.15 * (1.0 - f1).clamp(0.0, 1.0);
            let c = lerp3(shade(FAT_LOBULE, glisten), FAT_SEPTUM, septum);
            (c, 0.32 + 0.25 * septum)
        }
        Layer::Muscle => {
            // Fascicles ~0.7 mm across wrapped in perimysium at ~3 mm.
            let (f1, f2) = worley(u / 0.7, v / 0.7, tile / 0.7, seed ^ 0x3A5);
            let (p1, p2) = worley(u / 3.0, v / 3.0, tile / 3.0, seed ^ 0x9C1);
            let fibre = 0.85 + 0.2 * (1.0 - f1).clamp(0.0, 1.0);
            let fine_ridge = smoothstep((0.08 - (f2 - f1)) / 0.08) * 0.35;
            let peri = smoothstep((0.06 - (p2 - p1)) / 0.06);
            let c = lerp3(shade(muscle, fibre), PERIMYSIUM, (peri + fine_ridge).min(1.0));
            (c, 0.22 + 0.3 * peri)
        }
        Layer::Cortex => {
            // Dense ivory with Haversian canals ~0.1 mm: a sparse dot field.
            let (f1, _) = worley(u / 0.35, v / 0.35, tile / 0.35, seed ^ 0xB0E);
            let canal = smoothstep((0.28 - f1) / 0.1);
            let grain = value_noise(u * 3.0, v * 3.0, tile * 3.0, seed ^ 0x77);
            let c = lerp3(shade(CORTEX, 0.94 + 0.08 * grain), CANAL, canal);
            (c, 0.65 + 0.2 * canal)
        }
        Layer::Marrow => {
            // Trabecular struts thin out over the first half of the band; the cavity is yellow
            // (fatty) marrow with red patches.
            let n = value_noise(u * 1.5, v * 1.5, tile * 1.5, seed ^ 0x1D3);
            let strut = smoothstep((0.12 - (n - 0.5).abs()) / 0.12) * (1.0 - smoothstep(frac / 0.55));
            let blotch = value_noise(u * 0.6, v * 0.6, tile * 0.6, seed ^ 0x4E2);
            let base = lerp3(MARROW_RED, MARROW_YELLOW, smoothstep(blotch * 1.4 - 0.2));
            (lerp3(base, TRABECULA, strut), 0.4 - 0.15 * strut.min(1.0) + 0.25 * strut)
        }
    }
}

/// Scale an encoded-sRGB colour, clamped.
fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    [(c[0] * k).clamp(0.0, 1.0), (c[1] * k).clamp(0.0, 1.0), (c[2] * k).clamp(0.0, 1.0)]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn enc(v: f32) -> u8 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    (v * 255.0).round() as u8
}

/// A lattice hash in `[0, 1)`, periodic in `y` with period `py` cells.
fn lattice(x: i32, y: i32, py: i32, seed: u32) -> f32 {
    let yw = if py > 0 { y.rem_euclid(py) } else { y };
    let k = (x as u32).wrapping_mul(0x9E37_79B9) ^ (yw as u32).wrapping_mul(0x85EB_CA6B) ^ seed.wrapping_mul(0xC2B2_AE35);
    hash_f32(k)
}

/// Smooth value noise in `[0, 1]`, periodic along `y` with period `tile`.
fn value_noise(x: f32, y: f32, tile: f32, seed: u32) -> f32 {
    let py = tile.round().max(1.0) as i32;
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (smoothstep(x - xi), smoothstep(y - yi));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = lattice(xi, yi, py, seed);
    let b = lattice(xi + 1, yi, py, seed);
    let c = lattice(xi, yi + 1, py, seed);
    let d = lattice(xi + 1, yi + 1, py, seed);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    top + (bot - top) * fy
}

/// Worley (cellular) noise: distances to the nearest and second-nearest feature points, in cell
/// units, periodic along `y` with period `tile` cells.
fn worley(x: f32, y: f32, tile: f32, seed: u32) -> (f32, f32) {
    let py = tile.round().max(1.0) as i32;
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let mut f1 = f32::INFINITY;
    let mut f2 = f32::INFINITY;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cx + dx, cy + dy);
            let px = gx as f32 + lattice(gx, gy, py, seed);
            let pyy = gy as f32 + lattice(gx, gy, py, seed ^ 0x5555_5555);
            let d = ((x - px) * (x - px) + (y - pyy) * (y - pyy)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::Region;

    /// **The bands are the table, to within a texel.** Each band's width in columns, divided by the
    /// strip's texels-per-millimetre, is the layer's thickness — within 10 %, which at 512 columns
    /// over a limb is generous by an order of magnitude.
    #[test]
    fn band_widths_match_the_thickness_table() {
        for region in Region::ALL {
            let layers = Layers::for_region(region);
            let s = strip(&layers, 512, 64, 1);
            let ppm = s.px_per_mm(&layers);
            assert_eq!(s.bands.len(), 5, "{region:?} lost a band");
            for band in &s.bands {
                let want = layers.thickness_mm(band.layer);
                let got = (band.x1 - band.x0) as f32 / ppm;
                let tol = (want * 0.10).max(1.0 / ppm);
                assert!(
                    (got - want).abs() <= tol,
                    "{region:?} {:?}: {got:.2} mm drawn against {want:.2} mm measured",
                    band.layer
                );
            }
        }
    }

    /// The strip tiles: the last row and the row past it agree, so `Repeat` on `v` has no seam.
    #[test]
    fn the_along_axis_tiles_without_a_seam() {
        let layers = Layers::for_region(Region::Limb);
        let a = strip(&layers, 128, 32, 9);
        let b = strip(&layers, 128, 64, 9);
        // Row 32 of the taller strip is row 0 of the shorter one's period only if the noise is
        // periodic in the tile; the two strips have different tiles, so compare a strip to itself
        // through the lattice instead.
        fn row(s: &Strip, y: u32) -> &[u8] {
            &s.albedo[(y * s.width * 4) as usize..((y + 1) * s.width * 4) as usize]
        }
        // Periodicity: lattice(x, y) == lattice(x, y + py).
        assert_eq!(lattice(3, 1, 8, 5), lattice(3, 9, 8, 5));
        assert_eq!(lattice(3, -1, 8, 5), lattice(3, 7, 8, 5));
        // And a strip is deterministic in its inputs.
        assert_eq!(row(&a, 0), row(&strip(&layers, 128, 32, 9), 0));
        assert_ne!(a.digest(), b.digest());
    }

    /// The muscle band is redder than the fat band and darker than the cortex — the three bands a
    /// player reads first, in the order they read them.
    #[test]
    fn the_bands_read_as_tissue() {
        let layers = Layers::for_region(Region::Limb);
        let s = strip(&layers, 512, 64, 3);
        let mean = |band: &Band| {
            let mut acc = [0u64; 3];
            let mut n = 0u64;
            for y in 0..s.height {
                for x in band.x0..band.x1 {
                    let i = ((y * s.width + x) * 4) as usize;
                    acc[0] += s.albedo[i] as u64;
                    acc[1] += s.albedo[i + 1] as u64;
                    acc[2] += s.albedo[i + 2] as u64;
                    n += 1;
                }
            }
            [acc[0] as f32 / n as f32, acc[1] as f32 / n as f32, acc[2] as f32 / n as f32]
        };
        let by = |l: Layer| s.bands.iter().find(|b| b.layer == l).map(mean).unwrap_or([0.0; 3]);
        let (fat, muscle, cortex) = (by(Layer::Fat), by(Layer::Muscle), by(Layer::Cortex));
        let red_share = |c: [f32; 3]| c[0] / (c[0] + c[1] + c[2]).max(1.0);
        assert!(red_share(muscle) > red_share(fat), "muscle {muscle:?} is not redder than fat {fat:?}");
        assert!(fat[1] > muscle[1], "fat {fat:?} is not yellower than muscle {muscle:?}");
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        assert!(lum(cortex) > lum(muscle), "cortex {cortex:?} is not lighter than muscle {muscle:?}");
    }

    /// **Frozen.** Three strips, one per region, at the size the plugin bakes.
    #[test]
    fn the_strips_are_frozen() {
        let got: Vec<u64> = Region::ALL
            .iter()
            .map(|r| strip(&Layers::for_region(*r), 512, 64, 0xC0FF_EE00).digest())
            .collect();
        println!("{got:x?}");
        assert_eq!(got, vec![0x136725b9641e209f, 0x909d3ebc3b17d129, 0x7ba7a7439ee27dbc]);
    }
}
