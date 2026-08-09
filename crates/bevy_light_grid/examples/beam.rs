//! **The field, and the creatures reading it — on screen.**
//!
//! A flashlight sweeps a room. The wedge is wall-occluded, so pillars throw hard shadows that travel
//! with the beam. Pale dots **flee** the light and amber dots **chase** it, and neither one is being
//! told where the beam is: each samples the same CPU grid and steers along `light_push_at`, which is
//! `signed_gain · ∇illuminance`. Flip the sign and you have flipped the creature.
//!
//! **Nothing here is a lighting pass.** Every lit cell you see is a `f32` this crate computed on the
//! CPU, drawn as a flat sprite so you can look at the number. Your renderer already knows how bright a
//! pixel is; the point of this crate is that a *creature* cannot read a framebuffer, and this is what
//! it reads instead.
//!
//! Watch what the shadows do. A dot that has escaped into the dark behind a pillar sits still — the
//! gradient there is flat, so `light_push_at` returns zero and an unlit creature is *unbiased* rather
//! than pushed somewhere arbitrary. Then the beam rotates, the shadow moves off it, and it runs.
//!
//! Two passes, split on cost: `bake` recomputes the static lamps and is event-driven; `compose` re-adds
//! only the moving cone and runs every frame. Occlusion is the caller's, handed in as a closure — this
//! crate never learns what a wall is.
//!
//! This is the only example here that needs a GPU; `shadow` and `taxis` print to a terminal.
//!
//! Run: `cargo run -p bevy_light_grid --example beam`

use bevy::prelude::*;
use bevy_light_grid::{FlashlightCone, LightGrid, light_push_at};

const W: usize = 64;
const H: usize = 40;
/// Screen pixels per cell.
const CELL: f32 = 12.0;

/// Where the sweeping beam sits.
const BEAM: IVec2 = IVec2::new(32, 20);
/// Radians per second the beam rotates.
const SWEEP_RATE: f32 = 0.55;
/// Velocity bleed per second. Framerate-independent on purpose.
const DRAG: f32 = 3.0;
/// Cells per second a creature can manage at full tilt.
const MAX_SPEED: f32 = 7.0;

/// Border, four pillars and two spur walls — enough that the travelling shadows are the whole show.
fn is_wall(c: IVec2) -> bool {
    let (x, y) = (c.x, c.y);
    if x <= 0 || y <= 0 || x >= W as i32 - 1 || y >= H as i32 - 1 {
        return true;
    }
    for (px, py) in [(14, 10), (14, 29), (49, 10), (49, 29)] {
        if (px..px + 4).contains(&x) && (py..py + 4).contains(&y) {
            return true;
        }
    }
    // Two spurs reaching in from the sides, so the beam is cut at some angles and not others.
    if (6..=24).contains(&x) && y == 20 {
        return true;
    }
    if (39..=57).contains(&x) && y == 20 {
        return true;
    }
    false
}

fn floor_cells() -> impl Iterator<Item = IVec2> {
    (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| IVec2::new(x, y)))
        .filter(|c| !is_wall(*c))
}

/// Bresenham line-of-sight: blocked if any cell strictly between `a` and `b` is a wall.
///
/// Exactly the shape the crate asks for — `impl Fn(IVec2, IVec2) -> bool`, monomorphised at the call
/// site and returning a plain `bool`, so it cannot perturb a float in the light sum.
fn line_of_sight(a: IVec2, b: IVec2) -> bool {
    let (mut x, mut y) = (a.x, a.y);
    let (dx, dy) = ((b.x - a.x).abs(), -(b.y - a.y).abs());
    let (sx, sy) = (if a.x < b.x { 1 } else { -1 }, if a.y < b.y { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        if (x, y) == (b.x, b.y) {
            return true;
        }
        if (x, y) != (a.x, a.y) && is_wall(IVec2::new(x, y)) {
            return false;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Grid cell → screen position, with the grid's +y running *down* the screen.
fn world_of(gx: f32, gy: f32) -> Vec3 {
    Vec3::new(
        (gx - W as f32 * 0.5 + 0.5) * CELL,
        (H as f32 * 0.5 - gy - 0.5) * CELL,
        0.0,
    )
}

#[derive(Resource)]
struct Field(LightGrid);

#[derive(Resource, Default)]
struct Sweep(f32);

/// One drawn cell of the grid.
#[derive(Component)]
struct Tile(IVec2);

/// A creature that reads the field. `gain` is the whole difference between the two species: negative
/// descends the gradient into the dark, positive climbs it toward the light.
#[derive(Component)]
struct Creature {
    pos: Vec2,
    vel: Vec2,
    gain: f32,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy_light_grid — light your AI can read".into(),
                // 0.19 takes physical pixels as `u32` — there is no `(f32, f32)` conversion.
                resolution: ((W as f32 * CELL) as u32, (H as f32 * CELL) as u32).into(),
                ..default()
            }),
            ..default()
        }))
        .init_resource::<Sweep>()
        .add_systems(Startup, setup)
        .add_systems(Update, (sweep_beam, paint_tiles, steer_creatures).chain())
        .run();
}

fn setup(mut commands: Commands) {
    let mut grid = LightGrid::new(W, H, floor_cells());

    // The static half: four corner lamps, baked once. `(cell, intensity, range)`.
    grid.bake(
        &[
            (IVec2::new(8, 5), 0.55, 16.0),
            (IVec2::new(55, 5), 0.55, 16.0),
            (IVec2::new(8, 34), 0.55, 16.0),
            (IVec2::new(55, 34), 0.55, 16.0),
        ],
        line_of_sight,
    );
    commands.insert_resource(Field(grid));

    commands.spawn(Camera2d);
    spawn_legend(&mut commands);

    // One sprite per cell. Flat, unlit, deliberately: this is a readout of a number, not a render.
    for c in (0..H as i32).flat_map(|y| (0..W as i32).map(move |x| IVec2::new(x, y))) {
        commands.spawn((
            Tile(c),
            Sprite {
                color: Color::BLACK,
                custom_size: Some(Vec2::splat(CELL)),
                ..default()
            },
            Transform::from_translation(world_of(c.x as f32, c.y as f32)),
        ));
    }

    // Creatures. Pale ones flee, amber ones chase — same field, same call, opposite sign.
    let mut spawn = |x: f32, y: f32, gain: f32| {
        let colour = if gain < 0.0 {
            Color::srgb(0.72, 0.90, 0.95)
        } else {
            Color::srgb(1.0, 0.72, 0.25)
        };
        commands.spawn((
            Creature { pos: Vec2::new(x, y), vel: Vec2::ZERO, gain },
            Sprite { color: colour, custom_size: Some(Vec2::splat(CELL * 0.72)), ..default() },
            Transform::from_translation(world_of(x, y).with_z(1.0)),
        ));
    };
    for i in 0..16 {
        let t = i as f32;
        spawn(9.0 + (t * 2.7) % 44.0, 6.0 + (t * 5.3) % 27.0, -26.0);
    }
    for i in 0..5 {
        let t = i as f32;
        spawn(20.0 + t * 6.0, 33.0 - (t * 3.0) % 9.0, 18.0);
    }
}

/// A legend, because a field of coloured dots is only self-explanatory to whoever wrote it.
///
/// ASCII only, deliberately: Bevy's embedded default font carries 95 codepoints, so an arrow or a
/// middle dot renders as nothing at all and the label silently loses a word.
fn spawn_legend(commands: &mut Commands) {
    fn row(parent: &mut ChildSpawnerCommands, swatch: Color, label: &str) {
        parent
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    Node { width: Val::Px(12.0), height: Val::Px(12.0), ..default() },
                    BackgroundColor(swatch),
                ));
                r.spawn((
                    Text::new(label),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    TextColor(Color::srgb(0.88, 0.88, 0.92)),
                ));
            });
    }

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::all(Val::Px(9.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("bevy_light_grid - illuminance your AI reads"),
                TextFont { font_size: FontSize::Px(13.0), ..default() },
                TextColor(Color::srgb(1.0, 0.93, 0.78)),
            ));
            row(p, Color::srgb(0.72, 0.90, 0.95), "photophobic: runs DOWN the gradient");
            row(p, Color::srgb(1.0, 0.72, 0.25), "photophilic: climbs UP it");
            row(p, Color::srgb(0.75, 0.64, 0.42), "lit cell: one f32, wall-occluded");
            row(p, Color::srgb(0.10, 0.11, 0.15), "wall: shadow via your line-of-sight fn");
        });
}

/// Re-add the moving cone on top of the cached bake. Every frame, because a moving light is the one
/// thing that can never be dirty-gated — which is exactly why `bake` and `compose` are separate.
fn sweep_beam(time: Res<Time>, mut sweep: ResMut<Sweep>, mut field: ResMut<Field>) {
    sweep.0 += time.delta_secs() * SWEEP_RATE;
    let forward = Vec2::new(sweep.0.cos(), sweep.0.sin());
    field.0.compose(
        &[FlashlightCone {
            source: BEAM,
            forward,
            intensity: 1.5,
            range: 30.0,
            // ~28° half-angle, with a soft rim so the gradient creatures read stays smooth.
            cone_cos: 0.88,
            edge_softness: 0.22,
        }],
        line_of_sight,
    );
}

fn paint_tiles(field: Res<Field>, mut tiles: Query<(&Tile, &mut Sprite)>) {
    let peak = field.0.peak().max(1.0e-4);
    for (tile, mut sprite) in &mut tiles {
        if is_wall(tile.0) {
            sprite.color = Color::srgb(0.10, 0.11, 0.15);
            continue;
        }
        // Normalised illuminance, gamma-lifted so the dim tail stays visible rather than crushing to
        // black — the creatures steer on that tail, so it should be legible.
        let v = (field.0.sample_cell(tile.0) / peak).clamp(0.0, 1.0).powf(0.65);
        sprite.color = Color::srgb(
            0.03 + v * 0.97,
            0.04 + v * 0.82,
            0.10 + v * 0.42,
        );
    }
}

/// The whole point, in one call each: sample the gradient, scale by the creature's sign, integrate.
fn steer_creatures(time: Res<Time>, field: Res<Field>, mut creatures: Query<(&mut Creature, &mut Transform)>) {
    let dt = time.delta_secs().min(0.05);
    for (mut c, mut transform) in &mut creatures {
        let cell = IVec2::new(c.pos.x.round() as i32, c.pos.y.round() as i32);

        // `light_push_at` returns the push in the grid plane as world XZ.
        let push = light_push_at(&field.0, cell, c.gain);
        let steer = Vec2::new(push.x, push.z);

        // Framerate-independent damping: a per-frame constant would make the creatures' speed depend on
        // the monitor, which is a good way to ship a demo that only looks right on one machine.
        let damp = (1.0 - DRAG * dt).clamp(0.0, 1.0);
        c.vel = (c.vel + steer * dt) * damp;
        if c.vel.length() > MAX_SPEED {
            c.vel = c.vel.normalize_or_zero() * MAX_SPEED;
        }

        // Walls are the caller's business here too — slide along rather than tunnel through.
        let step = c.vel * dt;
        let try_x = Vec2::new(c.pos.x + step.x, c.pos.y);
        if !is_wall(IVec2::new(try_x.x.round() as i32, try_x.y.round() as i32)) {
            c.pos.x = try_x.x;
        } else {
            c.vel.x = -c.vel.x * 0.4;
        }
        let try_y = Vec2::new(c.pos.x, c.pos.y + step.y);
        if !is_wall(IVec2::new(try_y.x.round() as i32, try_y.y.round() as i32)) {
            c.pos.y = try_y.y;
        } else {
            c.vel.y = -c.vel.y * 0.4;
        }

        transform.translation = world_of(c.pos.x, c.pos.y).with_z(1.0);
    }
}
