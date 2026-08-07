//! **The diagnostic plots** — where in the cycle, not just by how much.
//!
//! The four checks answer *whether* the numbers agree; a scalar verdict ("off by 0.08s") says a
//! problem exists without saying where. Because this runtime has no transitions — one shared phase,
//! weights only — a wrong duration *"doesn't glitch, it skates"*: a continuous, low-amplitude error
//! smeared across the cycle. These plots make it pointable. Every curve is drawn against the
//! **shared phase**, each slot sampled at `wrap01(phi + declared_offset)` — exactly the runtime's
//! seek formula — so correct offsets align every stance vertically and a wrong one is a visibly
//! displaced trough.
//!
//! The top-down trace draws each clip's measured travel arrow at the declared cycle distance, which
//! is what settles the Valkyrie's backwards-named strafes by looking rather than by folklore.
//!
//! CPU-rasterized into stable [`Image`] handles (the `thumbs` idiom: handles never change identity,
//! data is rewritten), shown as `ImageNode`s in the pane. The raster arithmetic lives engine-free in
//! `emerge_core::plot`; this module owns only sizes, colors, and the phase convention.
//!
//! **Why not more than 128 bins**: `PHASE_BINS` is the *measurement* grid — contact labels,
//! stance fractions and phase offsets are all born bin-quantized in `emerge_core::clips` — so a
//! render-only densification has nothing denser to draw, and raising the core constant changes
//! measurement semantics (a `BENCH_TOOL_VERSION` bump orphaning every provenance stamp). The
//! display is not the bottleneck: 128 bins across 712 raster px is over 5 px per bin.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use emerge_core::clips::GaitCurves;
use emerge_core::plot::Raster;
use emerge_core::rigs::Playback;

use crate::anim_tab::BenchState;
use crate::anim_watch::BenchReports;

/// Raster sizes: 2× the displayed logical size (356×96 / 356×224 under the panel width), so lines
/// stay crisp under `UiScale 1.2` and retina factors.
pub(crate) const PLOT_W: u32 = 712;
pub(crate) const PLOT_H: u32 = 192;
pub(crate) const TRACE_W: u32 = 712;
pub(crate) const TRACE_H: u32 = 448;

/// The displayed (logical) sizes the pane's `Node`s use.
pub(crate) const SHOW_W: f32 = 356.0;
pub(crate) const SHOW_PLOT_H: f32 = 96.0;
pub(crate) const SHOW_TRACE_H: f32 = 224.0;

/// Plot background — `chrome::SLOT_BG`'s hue as bytes.
const BG: [u8; 4] = [36, 34, 32, 255];
/// Axis and grid ink, a step above the background.
const GRID: [u8; 4] = [70, 68, 64, 255];
/// The root-motion threshold line — `chrome::DANGER`'s hue.
const DANGER_INK: [u8; 4] = [219, 92, 77, 255];

/// One color per gait slot, raster and legend alike — the table is the contract between them.
/// Entry 0 is `chrome::ACCENT`'s hue; the rest are distinct hues at comparable value.
pub(crate) const SLOT_COLORS: [[u8; 4]; 8] = [
    [230, 168, 61, 255],
    [86, 180, 190, 255],
    [170, 120, 220, 255],
    [120, 190, 110, 255],
    [220, 110, 100, 255],
    [110, 140, 220, 255],
    [200, 200, 90, 255],
    [220, 130, 180, 255],
];

/// The k-th gait slot's color (wrapping past eight — no rig has that many gaits).
pub(crate) fn slot_color(k: usize) -> [u8; 4] {
    SLOT_COLORS[k % SLOT_COLORS.len()]
}

/// The same color as a UI `Color`, for legends.
pub(crate) fn slot_ui_color(k: usize) -> Color {
    let [r, g, b, _] = slot_color(k);
    Color::srgb_u8(r, g, b)
}

/// The plot images. Handles are created once at `Startup` and NEVER replaced — the pane's
/// `ImageNode`s bind them by identity, and only the pixel data moves (the `Thumbnails` idiom).
#[derive(Resource)]
pub struct BenchPlots {
    pub height: Handle<Image>,
    pub speed: Handle<Image>,
    pub drift: Handle<Image>,
    pub trace: Handle<Image>,
    /// **One shared hover overlay** for the three phase plots — a transparent image carrying just
    /// the cursor line. Shared on purpose: the plots share the phase axis, so one vertical cursor
    /// across the stack is a feature, and one image is one repaint.
    pub hover: Handle<Image>,
    /// The bin the cursor sits on, if any — the repaint-only-on-change latch.
    pub hovered_bin: Option<usize>,
    /// Which rig the pixels currently describe; `None` = blank.
    pub plotted: Option<String>,
}

fn blank(images: &mut Assets<Image>, w: u32, h: u32, fill: [u8; 4]) -> Handle<Image> {
    let raster = Raster::new(w as usize, h as usize, fill);
    images.add(Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        raster.px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

pub(crate) fn create_plot_images(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(BenchPlots {
        height: blank(&mut images, PLOT_W, PLOT_H, BG),
        speed: blank(&mut images, PLOT_W, PLOT_H, BG),
        drift: blank(&mut images, PLOT_W, PLOT_H, BG),
        trace: blank(&mut images, TRACE_W, TRACE_H, BG),
        hover: blank(&mut images, PLOT_W, PLOT_H, [0, 0, 0, 0]),
        hovered_bin: None,
        plotted: None,
    });
}

/// Marker on the three phase-plot `ImageNode`s — the hover surfaces. The trace is deliberately
/// not one: its axes are spatial, and a phase cursor over it would be a lie. Public for the
/// headless wiring test.
#[derive(Component)]
pub struct PhasePlotNode;

/// Marker on the readout `Text` under the plots. Public for the headless wiring test.
#[derive(Component)]
pub struct PlotReadout;

/// The centered cursor x ([`bevy::ui::RelativeCursorPosition`]'s convention: 0 at the node's
/// middle, ±0.5 at its edges) as a phase bin.
pub(crate) fn hover_bin(centered_x: f32) -> Option<usize> {
    let frac = centered_x + 0.5;
    if !(0.0..=1.0).contains(&frac) {
        return None;
    }
    let bins = emerge_core::clips::PHASE_BINS;
    Some(((frac * bins as f32) as usize).min(bins - 1))
}

/// **Hover-to-inspect.** Ungated — it early-outs to nothing every frame the bin is unchanged, so
/// the repaint-on-change idiom holds: moving within one bin costs two query reads.
pub(crate) fn drive_plot_hover(
    bench: Option<Res<BenchState>>,
    reports: Option<Res<BenchReports>>,
    plots: Option<ResMut<BenchPlots>>,
    images: Option<ResMut<Assets<Image>>>,
    nodes: Query<&bevy::ui::RelativeCursorPosition, With<PhasePlotNode>>,
    mut readouts: Query<&mut Text, With<PlotReadout>>,
) {
    let (Some(bench), Some(reports), Some(mut plots), Some(mut images)) =
        (bench, reports, plots, images)
    else {
        return;
    };
    let bin = nodes
        .iter()
        .find(|r| r.cursor_over)
        .and_then(|r| r.normalized)
        .and_then(|n| hover_bin(n.x));
    if bin == plots.hovered_bin {
        return;
    }
    plots.hovered_bin = bin;

    // The overlay: transparent but for one vertical cursor line.
    let mut r = Raster::new(PLOT_W as usize, PLOT_H as usize, [0, 0, 0, 0]);
    if let Some(b) = bin {
        let bins = emerge_core::clips::PHASE_BINS;
        let x = (b * PLOT_W as usize / bins).min(PLOT_W as usize - 1);
        r.vspan(x, 0, PLOT_H as usize - 1, GRID);
    }
    commit(&plots.hover, r, &mut images);

    // The readout: each plotted slot's height and speed at the hovered phase, world units, read
    // through the same declared-offset resample the raster draws.
    let text = match bin {
        None => String::new(),
        Some(b) => {
            let names = bench.names();
            let selected = names.get(bench.selected).copied();
            let rig = selected.and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)));
            let report = selected.and_then(|n| reports.by_rig.get(n));
            match (rig, report) {
                (Some(rig), Some(report)) => {
                    let bins = emerge_core::clips::PHASE_BINS;
                    let mut heights = Vec::new();
                    let mut speeds = Vec::new();
                    for (i, slot) in rig.slots.iter().enumerate() {
                        let offset = match slot.playback {
                            Playback::Gait { phase_offset, .. } => phase_offset,
                            _ => 0.0,
                        };
                        let Some((_, c)) = report.curves.iter().find(|(s, _)| *s == i) else {
                            continue;
                        };
                        // The seek formula, one sample: value at wrap01(phi + offset).
                        let at = ((b as f32 / bins as f32 + offset).rem_euclid(1.0)
                            * bins as f32) as usize
                            % bins;
                        if let (Some(h), Some(v)) =
                            (c.foot_height.get(at), c.ground_speed.get(at))
                        {
                            heights.push(format!("{:.2}", h * rig.scale));
                            speeds.push(format!("{:.2}", v * rig.scale));
                        }
                    }
                    format!(
                        "phase {:.3} | h {} | v {}",
                        b as f32 / bins as f32,
                        heights.join("/"),
                        speeds.join("/")
                    )
                }
                _ => String::new(),
            }
        }
    };
    for mut t in &mut readouts {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
}

/// A curve resampled onto the SHARED phase axis: displayed value at φ is the clip's value at
/// `wrap01(φ + declared_offset)` — the seek formula, bin-quantized.
fn shifted(ys: &[f32], offset: f32) -> Vec<f32> {
    let n = ys.len();
    if n == 0 {
        return Vec::new();
    }
    let shift = ((offset.rem_euclid(1.0)) * n as f32).round() as usize % n;
    (0..n).map(|i| ys[(i + shift) % n]).collect()
}

fn shifted_contact(cs: &[bool], offset: f32) -> Vec<bool> {
    let n = cs.len();
    if n == 0 {
        return Vec::new();
    }
    let shift = ((offset.rem_euclid(1.0)) * n as f32).round() as usize % n;
    (0..n).map(|i| cs[(i + shift) % n]).collect()
}

/// Everything the raster pass needs about one gait slot, in WORLD units, already shifted onto the
/// shared phase axis.
struct SlotCurves {
    /// The k-th gait of the rig — the color index.
    rank: usize,
    declared_cycle: f32,
    height: Vec<f32>,
    speed: Vec<f32>,
    drift: Vec<f32>,
    contact: Vec<bool>,
    trace: Vec<[f32; 2]>,
    body_velocity: [f32; 2],
    /// The same curves at the MEASURED phase offset (and the measured cycle distance) — the plots'
    /// half of the A/B ghost, drawn dimmed under the declared curves when the toggle is on.
    /// `None` when nothing was measured or measured equals declared to the grid: a rotation of the
    /// same bins would overdraw itself invisibly.
    measured: Option<MeasuredShift>,
}

/// See [`SlotCurves::measured`].
struct MeasuredShift {
    cycle: f32,
    height: Vec<f32>,
    speed: Vec<f32>,
    drift: Vec<f32>,
}

/// **Repaint the plots for the selected rig.** Runs on the same dirt as the pane rebuild; a rig
/// with no gait curves paints the background only (and the pane hides the section).
pub(crate) fn render_plots(
    bench: Option<Res<BenchState>>,
    reports: Option<Res<BenchReports>>,
    ab: Option<Res<crate::anim_stage::BenchAb>>,
    plots: Option<ResMut<BenchPlots>>,
    images: Option<ResMut<Assets<Image>>>,
) {
    let (Some(bench), Some(reports), Some(mut plots), Some(mut images)) =
        (bench, reports, plots, images)
    else {
        return;
    };
    let ab_on = ab.is_some_and(|a| a.0);
    let names = bench.names();
    let selected = names.get(bench.selected).map(|s| (*s).to_owned());
    let rig = selected
        .as_deref()
        .and_then(|n| bench.rigs.as_ref().and_then(|r| r.get(n)));
    let report = selected.as_deref().and_then(|n| reports.by_rig.get(n));

    // Gather the plotted slots' curves in world units, on the shared phase axis: the gait slots
    // when the rig has any, else every free slot the measurer found a joint for (offset 0 and no
    // cycle — nothing was declared, so nothing declared is drawn).
    let mut slots: Vec<SlotCurves> = Vec::new();
    if let (Some(rig), Some(report)) = (rig, report) {
        let scale = rig.scale;
        let mut rank = 0usize;
        for (i, slot) in rig.slots.iter().enumerate() {
            let (phase_offset, cycle_distance) = match slot.playback {
                Playback::Gait {
                    phase_offset,
                    cycle_distance,
                    ..
                } => (phase_offset, cycle_distance),
                Playback::Free { .. } if !rig.has_gaits() => (0.0, 0.0),
                _ => continue,
            };
            if let Some((_, c)) = report.curves.iter().find(|(s, _)| *s == i) {
                let measure = report.slots.iter().find(|m| m.slot == i);
                slots.push(world_curves(
                    rank,
                    phase_offset,
                    cycle_distance,
                    c,
                    scale,
                    measure,
                ));
            }
            rank += 1;
        }
    }

    // Always repaint when dirtied: the same rig may have been re-measured, and the raster is a few
    // hundred microseconds — cheaper than proving it unchanged.
    let plotted = if slots.is_empty() {
        None
    } else {
        selected.clone()
    };
    paint_height(&plots.height, &slots, ab_on, &mut images);
    paint_speed(&plots.speed, &slots, ab_on, &mut images);
    paint_drift(&plots.drift, &slots, rig.map_or(1.0, |r| r.scale), ab_on, &mut images);
    paint_trace(&plots.trace, &slots, ab_on, &mut images);
    plots.plotted = plotted;
}

fn world_curves(
    rank: usize,
    declared_offset: f32,
    declared_cycle: f32,
    c: &GaitCurves,
    scale: f32,
    measure: Option<&emerge_core::rig_check::SlotMeasure>,
) -> SlotCurves {
    // The measured variant: the SAME bins rotated to the measured offset, and the measured cycle
    // distance for the trace arrow. Built only when it would draw something the declared curves do
    // not — a shift under half a bin rounds to the same rotation.
    let measured = measure.and_then(|m| {
        let m_offset = m.phase_offset?;
        let half_bin = 0.5 / c.foot_height.len().max(1) as f32;
        let offset_differs =
            emerge_core::clips::signed_offset((m_offset - declared_offset).rem_euclid(1.0)).abs()
                >= half_bin;
        let m_cycle = m.cycle_distance.map(|d| d * scale).unwrap_or(declared_cycle);
        let cycle_differs = (m_cycle - declared_cycle).abs() > 0.01 * declared_cycle.max(1.0e-3);
        (offset_differs || cycle_differs).then(|| MeasuredShift {
            cycle: m_cycle,
            height: shifted(&c.foot_height, m_offset)
                .into_iter()
                .map(|v| v * scale)
                .collect(),
            speed: shifted(&c.ground_speed, m_offset)
                .into_iter()
                .map(|v| v * scale)
                .collect(),
            drift: shifted(&c.root_drift, m_offset)
                .into_iter()
                .map(|v| v * scale)
                .collect(),
        })
    });
    SlotCurves {
        rank,
        declared_cycle,
        height: shifted(&c.foot_height, declared_offset)
            .into_iter()
            .map(|v| v * scale)
            .collect(),
        speed: shifted(&c.ground_speed, declared_offset)
            .into_iter()
            .map(|v| v * scale)
            .collect(),
        drift: shifted(&c.root_drift, declared_offset)
            .into_iter()
            .map(|v| v * scale)
            .collect(),
        contact: shifted_contact(&c.contact, declared_offset),
        trace: c.trace.iter().map(|p| [p[0] * scale, p[1] * scale]).collect(),
        body_velocity: [c.body_velocity[0] * scale, c.body_velocity[1] * scale],
        measured,
    }
}

/// The A/B under-curve ink: the slot's color at a third strength — the trace's swing-dim idiom.
fn dim_ink(rank: usize) -> [u8; 4] {
    let ink = slot_color(rank);
    [ink[0] / 3, ink[1] / 3, ink[2] / 3, 255]
}

#[cfg(test)]
mod hover_tests {
    use super::*;

    #[test]
    fn hover_bins_cover_the_node_and_refuse_the_outside() {
        let bins = emerge_core::clips::PHASE_BINS;
        // The node's left edge is centered -0.5, its right edge +0.5.
        assert_eq!(hover_bin(-0.5), Some(0));
        assert_eq!(hover_bin(0.0), Some(bins / 2));
        assert_eq!(hover_bin(0.499), Some(bins - 1));
        // The right edge itself still lands on the last bin, never past it.
        assert_eq!(hover_bin(0.5), Some(bins - 1));
        assert_eq!(hover_bin(0.51), None);
        assert_eq!(hover_bin(-0.51), None);
    }
}

/// Push a finished raster into the image asset. `get_mut` marks the asset changed, which is what
/// re-uploads the texture.
fn commit(handle: &Handle<Image>, raster: Raster, images: &mut Assets<Image>) {
    if let Some(mut image) = images.get_mut(handle) {
        image.data = Some(raster.px);
    }
}

/// The value range across every slot's curve, padded so nothing hugs an edge.
fn range(slots: &[SlotCurves], pick: impl Fn(&SlotCurves) -> &[f32]) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for s in slots {
        for v in pick(s) {
            if v.is_finite() {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
    }
    if !(hi > lo) {
        return (0.0, 1.0);
    }
    let pad = (hi - lo) * 0.05;
    (lo - pad, hi + pad)
}

/// Foot height vs phase, with a per-slot contact tick row along the bottom — stance is the flat
/// part, and the ticks say where each clip believes its feet are down.
fn paint_height(handle: &Handle<Image>, slots: &[SlotCurves], ab: bool, images: &mut Assets<Image>) {
    let mut r = Raster::new(PLOT_W as usize, PLOT_H as usize, BG);
    // The measured variant is a rotation of the same bins, so it never widens the range.
    let (lo, hi) = range(slots, |s| &s.height);
    r.hline(PLOT_H as usize - 1, GRID);
    for s in slots {
        // Dim = at the measured offset, under the declared curve — the plots' half of the ghost.
        if ab {
            if let Some(m) = &s.measured {
                r.curve(&m.height, lo, hi, dim_ink(s.rank));
            }
        }
        r.curve(&s.height, lo, hi, slot_color(s.rank));
        // Contact ticks: one two-pixel row per slot, stacked up from the bottom edge.
        let y = (PLOT_H as usize).saturating_sub(2 + 2 * s.rank);
        for (x_bin, planted) in s.contact.iter().enumerate() {
            if !planted {
                continue;
            }
            let x0 = x_bin * PLOT_W as usize / s.contact.len().max(1);
            let x1 = (x_bin + 1) * PLOT_W as usize / s.contact.len().max(1);
            for x in x0..x1 {
                r.set(x as i32, y as i32, slot_color(s.rank));
            }
        }
    }
    commit(handle, r, images);
}

/// Foot ground speed vs phase. During stance this should sit at the body's speed — deviation
/// inside the stance IS foot skate, visible as a wobble where a plateau should be.
fn paint_speed(handle: &Handle<Image>, slots: &[SlotCurves], ab: bool, images: &mut Assets<Image>) {
    let mut r = Raster::new(PLOT_W as usize, PLOT_H as usize, BG);
    let (_, hi) = range(slots, |s| &s.speed);
    r.hline(PLOT_H as usize - 1, GRID);
    for s in slots {
        if ab {
            if let Some(m) = &s.measured {
                r.curve(&m.speed, 0.0, hi.max(1.0e-3), dim_ink(s.rank));
            }
        }
        r.curve(&s.speed, 0.0, hi.max(1.0e-3), slot_color(s.rank));
    }
    commit(handle, r, images);
}

/// Root drift vs phase, with the in-place threshold drawn as a line — check 3 as a curve. A
/// compliant clip is flat along the bottom; a violation towers over the red line.
fn paint_drift(
    handle: &Handle<Image>,
    slots: &[SlotCurves],
    scale: f32,
    ab: bool,
    images: &mut Assets<Image>,
) {
    let mut r = Raster::new(PLOT_W as usize, PLOT_H as usize, BG);
    let threshold = emerge_core::rig_check::ROOT_MOTION_EPS * scale;
    let (_, measured_hi) = range(slots, |s| &s.drift);
    let hi = (2.0 * threshold).max(measured_hi);
    let threshold_row = ((1.0 - threshold / hi) * (PLOT_H - 1) as f32) as usize;
    r.hline(threshold_row.min(PLOT_H as usize - 1), DANGER_INK);
    for s in slots {
        if ab {
            if let Some(m) = &s.measured {
                r.curve(&m.drift, 0.0, hi, dim_ink(s.rank));
            }
        }
        r.curve(&s.drift, 0.0, hi, slot_color(s.rank));
    }
    commit(handle, r, images);
}

/// Top-down (looking along −Y): +Z — the rig's forward — is image-up, +X is image-left. Per slot:
/// the foot's path (contact bins full-strength, swing dimmed) and an arrow from the origin along
/// the measured body travel, drawn at the DECLARED cycle distance — so a declared number that
/// disagrees with the measured direction or magnitude is visible as an arrow that does not fit its
/// own footprints. The grid is spaced at the reference slot's declared cycle distance.
fn paint_trace(handle: &Handle<Image>, slots: &[SlotCurves], ab: bool, images: &mut Assets<Image>) {
    let (w, h) = (TRACE_W as usize, TRACE_H as usize);
    let mut r = Raster::new(w, h, BG);
    if slots.is_empty() {
        commit(handle, r, images);
        return;
    }
    // Fit: every trace point and every arrow tip, plus one reference cycle of breathing room.
    let ref_cycle = slots[0].declared_cycle.max(1.0e-3);
    let mut reach = ref_cycle;
    for s in slots {
        for p in &s.trace {
            reach = reach.max(p[0].abs()).max(p[1].abs());
        }
        let v = (s.body_velocity[0].powi(2) + s.body_velocity[1].powi(2)).sqrt();
        if v > 1.0e-6 {
            reach = reach.max(s.declared_cycle);
            if ab {
                if let Some(m) = &s.measured {
                    reach = reach.max(m.cycle);
                }
            }
        }
    }
    let margin = 24.0;
    let k = ((w.min(h) as f32) / 2.0 - margin) / reach;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let to_px = |p: [f32; 2]| -> [i32; 2] {
        // +X left, +Z up.
        [(cx - p[0] * k) as i32, (cy - p[1] * k) as i32]
    };

    // The grid, one line per reference cycle distance out from the origin.
    let step = ref_cycle * k;
    if step >= 8.0 {
        let mut d = 0.0f32;
        while d <= (w.max(h) as f32) / 2.0 {
            for x in [(cx - d) as usize, (cx + d) as usize] {
                if x < w {
                    r.vspan(x, 0, h - 1, GRID);
                }
            }
            for y in [(cy - d) as i32, (cy + d) as i32] {
                if (0..h as i32).contains(&y) {
                    r.hline(y as usize, GRID);
                }
            }
            d += step;
        }
    }

    for s in slots {
        let ink = slot_color(s.rank);
        let dim = [ink[0] / 3, ink[1] / 3, ink[2] / 3, 255];
        // The path: swing dimmed, contact full.
        for i in 0..s.trace.len() {
            let a = to_px(s.trace[i]);
            let b = to_px(s.trace[(i + 1) % s.trace.len()]);
            let planted = *s.contact.get(i).unwrap_or(&false);
            r.line(a, b, if planted { ink } else { dim });
        }
        // The travel arrow: origin → measured direction × declared distance.
        let v = (s.body_velocity[0].powi(2) + s.body_velocity[1].powi(2)).sqrt();
        if v > 1.0e-6 {
            let dir = [s.body_velocity[0] / v, s.body_velocity[1] / v];
            // The A/B: a dimmed second arrow at the MEASURED cycle distance — the declared arrow
            // keeps full ink, so which number fits the footprints is a glance.
            if ab {
                if let Some(m) = &s.measured {
                    let m_tip = [dir[0] * m.cycle, dir[1] * m.cycle];
                    r.line(to_px([0.0, 0.0]), to_px(m_tip), dim);
                }
            }
            let tip = [dir[0] * s.declared_cycle, dir[1] * s.declared_cycle];
            let (a, b) = (to_px([0.0, 0.0]), to_px(tip));
            r.line(a, b, ink);
            // A simple head: two short strokes back from the tip, perpendicular-ish.
            let back = 0.12 * s.declared_cycle;
            let side = 0.06 * s.declared_cycle;
            for sgn in [-1.0f32, 1.0] {
                let p = [
                    tip[0] - dir[0] * back + sgn * -dir[1] * side,
                    tip[1] - dir[1] * back + sgn * dir[0] * side,
                ];
                r.line(b, to_px(p), ink);
            }
        }
    }
    commit(handle, r, images);
}
