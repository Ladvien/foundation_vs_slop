//! **A spatial FPS probe — "a radar for frame drops".** Debug builds only (gated like `devshot`,
//! `region_capture` and `perf_hud`).
//!
//! # Why this exists
//!
//! `perf_hud` (F4) tells you the frame rate *now*. That is enough to notice a drop and useless for
//! diagnosing one, because by the time you have read the number you have already walked somewhere
//! else. The reported symptom was "it drops to 26 in places" — a claim about **locations**, which a
//! scalar readout structurally cannot answer.
//!
//! So this samples continuously and tags every sample with **where the camera was looking and what
//! was rendering there**. Two artifacts land in `debug_screenshots/`:
//!
//! * **`fps_trace.csv`** — one row per sample, appended live. Survives a crash, opens in anything,
//!   and is the raw material for any correlation you want to run afterwards.
//! * **`fps_hotspots.md`** — the aggregate, rewritten periodically: dungeon cells ranked by how slow
//!   they were, with the scene census that was live while you stood there.
//!
//! # The measurement decisions, and why they are not arbitrary
//!
//! **Sampled at 2 Hz, not per frame.** A per-frame log of a ten-minute session is ~36,000 rows of
//! mostly-identical data, and writing it would itself perturb the thing being measured. Half a second
//! is short enough to localise a room and long enough that the I/O is free.
//!
//! **Mean frame time inverted, never the mean of per-frame FPS.** Averaging FPS over-weights fast
//! frames and flatters exactly the stuttering capture this is built to catch.
//!
//! **Mean *and* 1% low, always together.** A 60 mean with a 12 low is a stutter; a flat 30 is a
//! budget problem. They want opposite fixes, and either number alone hides which one you have.
//!
//! **Visible entities, not resident ones.** The census counts things with a true
//! [`ViewVisibility`] — what the renderer actually drew this frame. A resident-but-culled entity
//! costs memory, not frame time, and mixing the two is how you end up optimising the wrong thing.
//! (The *resident* count is still worth having, and `region_capture`'s Ctrl+P report gives it.)
//!
//! # Determinism
//!
//! Windowed-only `Update` measurement, on `Time<Real>`. It reads scene state and writes nothing but
//! files; the headless `sim_harness` never registers this plugin, and the module is stripped from
//! release with `perf_hud` (see `lib.rs`). Goldens are untouched by construction.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

use bevy::camera::visibility::ViewVisibility;
use bevy::prelude::*;
use bevy::time::Real;

use crate::camera::{CameraRig, ISO_OFFSET};
use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::nest::Nest;
use crate::parasite::Manca;
use crate::placement::PlacedIn;
use crate::squad::Unit;

/// Where both artifacts land — beside the region captures, because they answer the same question and
/// a player who is chasing a drop is already looking in that directory.
const OUT_DIR: &str = "debug_screenshots";
const TRACE_PATH: &str = "debug_screenshots/fps_trace.csv";
const HOTSPOT_PATH: &str = "debug_screenshots/fps_hotspots.md";

/// Seconds between samples. See the header for why this is not per-frame.
const SAMPLE_SECS: f32 = 0.5;
/// Seconds of frame history each sample summarises. Matches `region_capture`'s window so a Ctrl+P
/// note and the trace row taken at the same moment report the same numbers.
const WINDOW_SECS: f32 = 5.0;
/// Below this, a sample is a **drop**: it is flagged in the CSV and logged to the console with its
/// coordinates, so a drop announces itself during play instead of waiting to be found in a file.
const DROP_FPS: f32 = 50.0;
/// How often the aggregate is rewritten. Cheap (it is a few hundred cells at most) but pointless to
/// redo every sample.
const HOTSPOT_EVERY_SECS: f32 = 10.0;
/// Rows in the hotspot table.
const MAX_HOTSPOTS: usize = 25;
/// A cell needs this many samples before it can be ranked. One unlucky sample during an asset load
/// is not a hotspot, and without this the table fills with noise from cells you walked through once.
///
/// **24 = one full FVS-N-24 cycle (~12 s at 2 Hz), and that is the load-bearing property.** At the
/// original 3, the 2026-07-31 hotspot table was an aliasing artifact: the game-wide ~11.7 s
/// oscillation collapses frame rate everywhere for ~4.4 s per cycle, so a cell walked through
/// during a slow phase ranked as a "hotspot" on 3-of-3 slow samples while a cell occupied for a
/// minute (119 samples, 0 drops) ranked clean. A cell can only be ranked once it has been observed
/// across at least one whole cycle, so no cell can any longer be indicted by phase luck alone.
const MIN_SAMPLES_TO_RANK: usize = 24;

/// One aggregated dungeon cell.
#[derive(Default, Clone)]
struct CellStat {
    samples: usize,
    /// Summed frame TIME, not summed FPS — see the header.
    total_dt: f32,
    worst_dt: f32,
    /// Scene census summed over samples, divided out when reported.
    units: usize,
    hostiles: usize,
    props: usize,
    lights: usize,
    tris: usize,
    drops: usize,
}

#[derive(Resource)]
struct PerfProbe {
    /// Rolling frame times (seconds), newest last, trimmed to [`WINDOW_SECS`].
    frames: std::collections::VecDeque<f32>,
    since_sample: f32,
    /// Frames observed since the last sample, and their summed time — the LOCAL window.
    local_frames: usize,
    local_dt: f32,
    since_hotspots: f32,
    elapsed: f32,
    cells: HashMap<(i32, i32), CellStat>,
    /// False until the CSV header has been written for this run.
    started: bool,
}

impl Default for PerfProbe {
    fn default() -> Self {
        Self {
            frames: std::collections::VecDeque::new(),
            since_sample: 0.0,
            local_frames: 0,
            local_dt: 0.0,
            since_hotspots: 0.0,
            elapsed: 0.0,
            cells: HashMap::new(),
            started: false,
        }
    }
}

impl PerfProbe {
    fn push_frame(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.frames.push_back(dt);
        let mut total: f32 = self.frames.iter().sum();
        while total > WINDOW_SECS && self.frames.len() > 1 {
            if let Some(front) = self.frames.pop_front() {
                total -= front;
            }
        }
    }

    /// `(mean fps, 1% low fps, worst frame secs)`.
    fn stats(&self) -> Option<(f32, f32, f32)> {
        if self.frames.is_empty() {
            return None;
        }
        let n = self.frames.len();
        let mean_dt = self.frames.iter().sum::<f32>() / n as f32;
        let mut sorted: Vec<f32> = self.frames.iter().copied().collect();
        // SORT-OK: descending frame times for a percentile in a dev-only readout; touches no
        // simulation state.
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let worst_n = (n / 100).max(1);
        let low_dt = sorted[..worst_n].iter().sum::<f32>() / worst_n as f32;
        Some((1.0 / mean_dt, 1.0 / low_dt, sorted[0]))
    }
}

pub struct PerfProbePlugin;

impl Plugin for PerfProbePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerfProbe>()
            .add_systems(Update, sample);
    }
}

#[allow(clippy::too_many_arguments)]
fn sample(
    time: Res<Time<Real>>,
    mut probe: ResMut<PerfProbe>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>>,
    rig: Option<Res<CameraRig>>,
    dungeon: Option<Res<Dungeon>>,
    // The census: only entities the renderer actually drew this frame.
    seen: Query<(
        &ViewVisibility,
        Has<Unit>,
        Has<Enemy>,
        Has<Crab>,
        Has<Manca>,
        Has<Nest>,
        Has<PlacedIn>,
    )>,
    lights: Query<&ViewVisibility, With<PointLight>>,
    meshes_of: Query<(&ViewVisibility, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
) {
    let dt = time.delta_secs();
    probe.push_frame(dt);
    probe.elapsed += dt;
    probe.since_sample += dt;
    if dt.is_finite() && dt > 0.0 {
        probe.local_frames += 1;
        probe.local_dt += dt;
    }
    probe.since_hotspots += dt;
    if probe.since_sample < SAMPLE_SECS {
        return;
    }
    probe.since_sample = 0.0;

    let Some((mean_fps, low_fps, worst_dt)) = probe.stats() else { return };

    // **The LOCAL rate — mean over just the last `SAMPLE_SECS`, not the 5 s window.**
    //
    // This column exists because the first real trace was uninterpretable without it. Every other
    // field in a row (cell, biome, the whole visible census) describes *this instant*, while
    // `fps_mean` describes the previous five seconds — so bucketing rows by, say, visible triangles
    // and averaging `fps_mean` mixes each sample's scene with the frame times of wherever the player
    // was standing five seconds earlier. It produced a flatly wrong reading (fewer triangles looked
    // *slower*) purely from that lag.
    //
    // `fps_local` is the column to correlate against. `fps_mean` is the one to read as "how did the
    // last few seconds feel". Both are kept: the smoothed one is what a player perceives, the local
    // one is what the row actually measured.
    let local_fps = if probe.local_frames > 0 && probe.local_dt > 0.0 {
        probe.local_frames as f32 / probe.local_dt
    } else {
        mean_fps
    };
    probe.local_frames = 0;
    probe.local_dt = 0.0;

    // Where the player is LOOKING, not where the camera is. `CameraRig::focus` is the rig's own
    // look-at point; the camera itself sits `ISO_OFFSET` (12, 12, 12) up and back from it, so its
    // translation would report a cell nobody is looking at — off the map entirely near an edge.
    // Falling back to `camera_translation - ISO_OFFSET` is the same recovery `audio::sync_listener`
    // documents for the spatial listener, so the two agree on what "here" means.
    let focus = match (&rig, &camera) {
        (Some(rig), _) => Some(rig.focus),
        (None, Some(cam)) => {
            let (_, cam_tf) = **cam;
            Some(cam_tf.translation() - ISO_OFFSET)
        }
        _ => None,
    };
    let Some(focus) = focus else { return };

    let (cell, region, biome) = match dungeon.as_deref() {
        Some(d) => {
            let c = d.world_to_cell(focus);
            let region = d
                .regions
                .iter()
                .position(|r| r.rect.contains([c.x, c.y]))
                .map(|i| i as i32)
                .unwrap_or(-1);
            (c, region, format!("{:?}", d.biome(c)))
        }
        None => (IVec2::ZERO, -1, "n/a".to_string()),
    };

    let mut units = 0usize;
    let mut hostiles = 0usize;
    let mut props = 0usize;
    for (vis, unit, enemy, crab, manca, nest, prop) in &seen {
        if !vis.get() {
            continue;
        }
        units += unit as usize;
        hostiles += (enemy || crab || manca || nest) as usize;
        props += prop as usize;
    }
    let light_count = lights.iter().filter(|v| v.get()).count();
    let tris: usize = meshes_of
        .iter()
        .filter(|(v, _)| v.get())
        .filter_map(|(_, m)| meshes.get(&m.0))
        .map(|m| match m.indices() {
            Some(i) => i.len() / 3,
            None => m.count_vertices() / 3,
        })
        .sum();

    // On the LOCAL rate, so a drop is attributed to where it happened rather than trailing the
    // player for five seconds after they leave.
    let dropped = local_fps < DROP_FPS;
    if dropped {
        // Loud, with coordinates, so a drop is reported the moment it happens rather than only in a
        // file the player has to know to open.
        warn!(
            "fps drop: {local_fps:.1} fps here ({mean_fps:.1} 5s mean, 1% low {low_fps:.1}, worst {:.1} ms) at cell ({}, {}) \
             region {region} biome {biome} — visible: {units} units, {hostiles} hostiles, {props} props, \
             {light_count} lights, {tris} tris",
            worst_dt * 1000.0,
            cell.x,
            cell.y
        );
    }

    // Accumulate into the cell map for the aggregate.
    let stat = probe.cells.entry((cell.x, cell.y)).or_default();
    stat.samples += 1;
    stat.total_dt += 1.0 / local_fps;
    stat.worst_dt = stat.worst_dt.max(worst_dt);
    stat.units += units;
    stat.hostiles += hostiles;
    stat.props += props;
    stat.lights += light_count;
    stat.tris += tris;
    stat.drops += dropped as usize;

    // Append the raw row. Opened per write rather than held: a held handle loses whatever is still
    // buffered when the game is killed, and a session that ends in a hang is exactly the one whose
    // trace you want.
    let elapsed = probe.elapsed;
    let need_header = !probe.started;
    probe.started = true;
    if std::fs::create_dir_all(OUT_DIR).is_ok()
        && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(TRACE_PATH)
    {
        if need_header {
            let _ = writeln!(
                f,
                "t_secs,fps_local,fps_mean,fps_1pct_low,worst_ms,cell_x,cell_y,region,biome,\
                 vis_units,vis_hostiles,rev_props,vis_lights,vis_tris,drop"
            );
        }
        let _ = writeln!(
            f,
            "{elapsed:.1},{local_fps:.2},{mean_fps:.2},{low_fps:.2},{:.2},{},{},{region},{biome},\
             {units},{hostiles},{props},{light_count},{tris},{}",
            worst_dt * 1000.0,
            cell.x,
            cell.y,
            dropped as u8
        );
    }

    if probe.since_hotspots >= HOTSPOT_EVERY_SECS {
        probe.since_hotspots = 0.0;
        write_hotspots(&probe);
    }
}

/// Rewrite the aggregate: the slowest cells, with the census that was live in them.
fn write_hotspots(probe: &PerfProbe) {
    let mut ranked: Vec<((i32, i32), CellStat)> = probe
        .cells
        .iter()
        .filter(|(_, s)| s.samples >= MIN_SAMPLES_TO_RANK)
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    if ranked.is_empty() {
        return;
    }
    // Slowest first — by mean frame time, so the ordering matches the thing being optimised.
    // SORT-OK: dev-only report ordering, cell coordinate breaks ties into a total order.
    ranked.sort_by(|a, b| {
        let am = a.1.total_dt / a.1.samples as f32;
        let bm = b.1.total_dt / b.1.samples as f32;
        bm.partial_cmp(&am)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    let total_samples: usize = probe.cells.values().map(|s| s.samples).sum();
    let total_drops: usize = probe.cells.values().map(|s| s.drops).sum();

    let mut md = String::new();
    let _ = writeln!(md, "# FPS hotspots\n");
    let _ = writeln!(
        md,
        "Live aggregate, rewritten every {HOTSPOT_EVERY_SECS:.0}s. Raw rows in `fps_trace.csv`.\n"
    );
    let _ = writeln!(
        md,
        "- Session: {:.0}s · {total_samples} samples · **{total_drops} below {DROP_FPS:.0} fps** \
         ({:.0}%)",
        probe.elapsed,
        100.0 * total_drops as f32 / total_samples.max(1) as f32
    );
    let _ = writeln!(
        md,
        "- Cells visited: {} (ranked below: those with ≥{MIN_SAMPLES_TO_RANK} samples)\n",
        probe.cells.len()
    );
    let _ = writeln!(
        md,
        "Counts are **visible** (drawn) averages while standing in that cell — except `props↑`,\n\
         which is **revealed-so-far**, not on-screen: prop roots are `WorldAssetRoot`s with no\n\
         `Aabb`, so Bevy never frustum-culls them and their `ViewVisibility` is a one-way\n\
         Hidden→Visible reveal latch. It only ever rises across a session; do not read a\n\
         props↔fps correlation out of it (measured 2026-07-31: it is confounded with elapsed\n\
         time, and collapses under partial correlation while `lights` survives).\n"
    );
    let _ = writeln!(
        md,
        "| cell | mean fps | worst ms | samples | drops | units | hostiles | props↑ | lights | tris |"
    );
    let _ = writeln!(md, "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for ((x, y), s) in ranked.iter().take(MAX_HOTSPOTS) {
        let n = s.samples as f32;
        let _ = writeln!(
            md,
            "| ({x}, {y}) | {:.1} | {:.1} | {} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {} |",
            n / s.total_dt,
            s.worst_dt * 1000.0,
            s.samples,
            s.drops,
            s.units as f32 / n,
            s.hostiles as f32 / n,
            s.props as f32 / n,
            s.lights as f32 / n,
            s.tris / s.samples
        );
    }
    if std::fs::create_dir_all(OUT_DIR).is_ok() {
        let _ = std::fs::write(Path::new(HOTSPOT_PATH), md);
    }
}
