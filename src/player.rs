//! The player-controlled duplicant: walking, climbing, gravity and sprite animation.

use bevy::prelude::*;
use bevy::sprite::Anchor;
use serde::Deserialize;

use crate::{world, Level, TILE_SIZE};

/// Rendered ONI duplicant sheets (placeholder art, not in git).
const SHEETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/asset/oni/player");
/// Visible height of the standing duplicant in tiles.
pub const HEIGHT_TILES: f32 = 2.7;
/// Height of a standing duplicant in the rendered sheets, in pixels; sets the
/// sprite scale. Measured on a short-haired dupe; hair makes some a little taller.
const SHEET_HEIGHT_PX: f32 = 154.0;
/// Size of one ONI cell in the rendered sheets, in pixels. Animations that
/// move the duplicant (walk cycle, climbs) move it in whole cells.
const SHEET_CELL_PX: f32 = 100.0;
/// Our tiles per sheet pixel, from scaling the duplicant to [`HEIGHT_TILES`].
const TILES_PER_SHEET_PX: f32 = HEIGHT_TILES / SHEET_HEIGHT_PX;
/// One ONI cell in our tiles.
const CELL_TILES: f32 = SHEET_CELL_PX * TILES_PER_SHEET_PX;
/// Half the width of the collision box, in tiles. Its height is [`HEIGHT_TILES`].
const HALF_WIDTH: f32 = 0.4;
/// Walking speed in tiles per second.
const WALK_SPEED: f32 = 4.0;
/// Highest drop, in tiles, the player walks off without holding down (S).
const MAX_SAFE_DROP: f32 = 3.0;
/// Gravity in tiles per second squared, and the terminal falling speed.
const GRAVITY: f32 = 40.0;
const MAX_FALL_SPEED: f32 = 20.0;

pub struct PlayerPlugin;

/// Moves and animates the player; order systems that read [`Player::focus`] after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSystems;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .add_systems(Update, (move_player, animate_player).chain().in_set(PlayerSystems));
    }
}

/// Index of rendered sheets, `animations.ron` in each sheet folder.
#[derive(Deserialize)]
struct SheetIndex {
    animations: Vec<SheetInfo>,
}

#[derive(Deserialize)]
struct SheetInfo {
    name: String,
    file: String,
    frame_size: (u32, u32),
    /// Pixel within a frame where the feet are, from the frame's top-left.
    origin: (u32, u32),
    columns: u32,
    rows: u32,
    frames: usize,
    fps: f32,
}

struct Clip {
    image: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    anchor: Anchor,
    frames: usize,
    fps: f32,
}

impl Clip {
    fn duration(&self) -> f32 {
        self.frames as f32 / self.fps
    }
}

fn load_clip(
    folder: &str,
    name: &str,
    assets: &AssetServer,
    layouts: &mut Assets<TextureAtlasLayout>,
) -> Clip {
    let index_path = format!("{SHEETS_DIR}/{folder}/animations.ron");
    let source = std::fs::read_to_string(&index_path)
        .unwrap_or_else(|e| panic!("cannot read {index_path}: {e}"));
    let index: SheetIndex =
        ron::from_str(&source).unwrap_or_else(|e| panic!("invalid {index_path}: {e}"));
    let info = index
        .animations
        .into_iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("no animation {name} in {index_path}"));

    let (w, h) = info.frame_size;
    let (ox, oy) = info.origin;
    Clip {
        image: assets.load(format!("oni/player/{folder}/{}", info.file)),
        layout: layouts.add(TextureAtlasLayout::from_grid(
            UVec2::new(w, h),
            info.columns,
            info.rows,
            None,
            None,
        )),
        // Sheet origin (y down from the top-left) as a sprite anchor (y up from the centre).
        anchor: Anchor(Vec2::new(
            ox as f32 / w as f32 - 0.5,
            0.5 - oy as f32 / h as f32,
        )),
        frames: info.frames,
        fps: info.fps,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ClipKind {
    Idle,
    Walk,
    /// Up one ONI cell, used for 2-tile cliffs.
    ClimbLow,
    /// Up two ONI cells, used for 3-tile cliffs.
    ClimbHigh,
}

struct Clips {
    idle: Clip,
    walk: Clip,
    climb_low: Clip,
    climb_high: Clip,
}

impl Clips {
    fn get(&self, kind: ClipKind) -> &Clip {
        match kind {
            ClipKind::Idle => &self.idle,
            ClipKind::Walk => &self.walk,
            ClipKind::ClimbLow => &self.climb_low,
            ClipKind::ClimbHigh => &self.climb_high,
        }
    }
}

/// A climb in progress. The climb animations carry the duplicant a fixed
/// distance (`shift`) within the sprite; the sprite itself is moved so that
/// the animation ends exactly at `to`.
#[derive(Clone, Copy)]
struct Climb {
    clip: ClipKind,
    from: Vec2,
    to: Vec2,
    shift: Vec2,
    elapsed: f32,
}

#[derive(Clone, Copy)]
enum Motion {
    Idle,
    Walk,
    /// Finishing a step under a low ceiling: walking at normal speed into the
    /// next column, through the ceiling corner the head brushes on the way.
    StepAcross { to: Vec2 },
    Climb(Climb),
}

#[derive(Component)]
pub struct Player {
    /// Bottom centre of the body, in tiles.
    position: Vec2,
    fall_speed: f32,
    facing_left: bool,
    motion: Motion,
    /// Where the camera should look, in world units; follows the body smoothly while climbing.
    pub focus: Vec2,
}

#[derive(Component)]
struct Animation {
    clips: Clips,
    showing: ClipKind,
    elapsed: f32,
}

fn spawn_player(
    mut commands: Commands,
    level: Res<Level>,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let (x, y) = level
        .world
        .find_standing_spot(world::WIDTH / 2, HEIGHT_TILES.ceil() as usize)
        .expect("no room for the player in the level");
    let mut load = |folder, name| load_clip(folder, name, &assets, &mut layouts);
    let clips = Clips {
        idle: load("idle", "idle_default"),
        walk: load("loco", "floor_floor_1_0_loop"),
        climb_low: load("loco", "floor_floor_1_1"),
        climb_high: load("loco", "floor_floor_1_2"),
    };

    commands.spawn((
        Player {
            position: Vec2::new(x as f32 + 0.5, y as f32),
            fall_speed: 0.0,
            facing_left: false,
            motion: Motion::Idle,
            focus: Vec2::ZERO,
        },
        Sprite::from_atlas_image(
            clips.idle.image.clone(),
            TextureAtlas { layout: clips.idle.layout.clone(), index: 0 },
        ),
        clips.idle.anchor,
        Transform::default(),
        Animation { clips, showing: ClipKind::Idle, elapsed: 0.0 },
    ));
}

/// Whether the body standing at `feet` overlaps a solid tile.
fn collides(world: &world::World, feet: Vec2) -> bool {
    let x0 = (feet.x - HALF_WIDTH).floor() as i32;
    let x1 = (feet.x + HALF_WIDTH).ceil() as i32;
    let y0 = feet.y.floor() as i32;
    let y1 = (feet.y + HEIGHT_TILES).ceil() as i32;
    (x0..x1).any(|x| (y0..y1).any(|y| world.is_solid(x, y)))
}

/// How far below `feet` the ground is when walking in direction `dir`, in tiles.
/// Measured three quarters of the way along the body, so the player stops at a
/// cliff edge with only its leading quarter over it. The level's bottom edge
/// counts as ground.
fn drop_below(world: &world::World, feet: Vec2, dir: f32) -> f32 {
    let x = (feet.x + dir * HALF_WIDTH / 2.0).floor() as i32;
    let y = feet.y.floor() as i32;
    let landing = (0..=y).rev().find(|&ty| world.is_solid(x, ty - 1)).unwrap_or(0);
    feet.y - landing as f32
}

/// Whether walking from `from` to the free position `to` (direction `dir`) is
/// allowed. The player stops at the edge of a drop higher than [`MAX_SAFE_DROP`]
/// unless `wants_down`, also while dropping down a small step. A move that
/// doesn't deepen the drop ahead is always allowed, so the player can back away.
fn may_walk(world: &world::World, from: Vec2, to: Vec2, dir: f32, wants_down: bool) -> bool {
    let drop = drop_below(world, to, dir);
    wants_down || drop <= MAX_SAFE_DROP || drop <= drop_below(world, from, dir)
}

/// Where the body ends up stepping one tile up (`rise` = 1) or down (`rise` = -1)
/// when walking to `blocked` (direction `dir`) is blocked, if there is room for it
/// there. Only the room at the destination matters: if the body, still overlapping
/// the old column, doesn't fit, it moves fully into the next column.
fn step(world: &world::World, blocked: Vec2, dir: f32, rise: f32) -> Option<Vec2> {
    let wall_x = (blocked.x + dir * HALF_WIDTH).floor();
    // Just inside the ledge column, nudged off the boundary against rounding.
    let inside = if dir > 0.0 {
        wall_x + HALF_WIDTH + 0.001
    } else {
        wall_x + 1.0 - HALF_WIDTH - 0.001
    };
    let y = blocked.y + rise;
    [Vec2::new(blocked.x, y), Vec2::new(inside, y)]
        .into_iter()
        .find(|&stepped| !collides(world, stepped))
}

/// A climb onto the cliff that blocks walking from `from` to `blocked` (direction `dir`),
/// if the cliff is 2 or 3 tiles high and there is room on top.
fn plan_climb(world: &world::World, from: Vec2, blocked: Vec2, dir: f32) -> Option<Climb> {
    let wall_x = (blocked.x + dir * HALF_WIDTH).floor() as i32;
    let y = from.y.round() as i32;
    let height = (0..4).take_while(|&k| world.is_solid(wall_x, y + k)).count();
    let (clip, cells_up) = match height {
        2 => (ClipKind::ClimbLow, 1.0),
        3 => (ClipKind::ClimbHigh, 2.0),
        _ => return None,
    };
    let top = (y + height as i32) as f32;
    // Room to rise up before stepping over the edge.
    if collides(world, Vec2::new(from.x, top)) {
        return None;
    }
    // Land where the animation naturally ends, or just past the edge if that is blocked.
    let natural = from.x + dir * CELL_TILES;
    let edge = wall_x as f32 + 0.5;
    let to = [natural, edge]
        .into_iter()
        .map(|x| Vec2::new(x, top))
        .find(|&to| !collides(world, to))?;
    Some(Climb {
        clip,
        from,
        to,
        shift: Vec2::new(dir, cells_up) * CELL_TILES,
        elapsed: 0.0,
    })
}

fn move_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    level: Res<Level>,
    player: Single<(&mut Player, &Animation)>,
) {
    let (mut player, anim) = player.into_inner();
    let world = &level.world;
    let dt = time.delta_secs();

    // Steps across and climbs run to completion without input or gravity.
    if let Motion::StepAcross { to } = player.motion {
        let remaining = to.x - player.position.x;
        let advance = WALK_SPEED * dt;
        if remaining.abs() > advance {
            player.position.x += advance * remaining.signum();
        } else {
            player.position = to;
            player.motion = Motion::Walk;
        }
        return;
    }
    if let Motion::Climb(mut climb) = player.motion {
        climb.elapsed += dt;
        if climb.elapsed < anim.clips.get(climb.clip).duration() {
            player.motion = Motion::Climb(climb);
            return;
        }
        player.position = climb.to;
        player.motion = Motion::Idle;
    }

    player.fall_speed = (player.fall_speed + GRAVITY * dt).min(MAX_FALL_SPEED);
    let below = player.position - Vec2::new(0.0, player.fall_speed * dt);
    let on_ground = collides(world, below);
    if on_ground {
        // Land on top of the tile we would have sunk into.
        player.position.y = below.y.floor() + 1.0;
        player.fall_speed = 0.0;
    } else {
        player.position = below;
    }

    let mut dir = 0.0;
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir += 1.0;
    }

    player.motion = Motion::Idle;
    if dir != 0.0 {
        player.facing_left = dir < 0.0;
        let target = player.position + Vec2::new(dir * WALK_SPEED * dt, 0.0);
        let wants_down = keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]);
        if !collides(world, target) {
            if may_walk(world, player.position, target, dir, wants_down) {
                player.position = target;
                player.motion = Motion::Walk;
            }
        } else if on_ground
            && let Some(stepped) =
                step(world, target, dir, 1.0).or_else(|| step(world, target, dir, -1.0))
        {
            // A one-tile step up onto a ledge, or down under a low ceiling. The
            // hop is instant; if the body has to move fully into the next column
            // to fit, it walks there at normal speed.
            let hop = Vec2::new(target.x, stepped.y);
            player.position = hop;
            player.motion = if stepped == hop {
                Motion::Walk
            } else {
                Motion::StepAcross { to: stepped }
            };
        } else if on_ground
            && let Some(climb) = plan_climb(world, player.position, target, dir)
        {
            player.motion = Motion::Climb(climb);
        }
    }
}

fn animate_player(
    time: Res<Time>,
    player: Single<(&mut Player, &mut Animation, &mut Sprite, &mut Anchor, &mut Transform)>,
) {
    let (mut player, mut anim, mut sprite, mut anchor, mut transform) = player.into_inner();

    let (kind, sprite_at, body_at, climb_progress) = match player.motion {
        Motion::Idle => (ClipKind::Idle, player.position, player.position, None),
        Motion::Walk | Motion::StepAcross { .. } => {
            (ClipKind::Walk, player.position, player.position, None)
        }
        Motion::Climb(c) => {
            let t = (c.elapsed / anim.clips.get(c.clip).duration()).min(1.0);
            let body = c.from + (c.to - c.from) * t;
            (c.clip, body - c.shift * t, body, Some(t))
        }
    };
    player.focus = body_at * TILE_SIZE;

    transform.translation = (sprite_at * TILE_SIZE).extend(1.0);
    let scale = TILE_SIZE * TILES_PER_SHEET_PX;
    // The sheets face right. Mirroring through the scale keeps the anchor (the feet) in place.
    transform.scale = Vec3::new(if player.facing_left { -scale } else { scale }, scale, 1.0);

    if anim.showing != kind {
        anim.showing = kind;
        anim.elapsed = 0.0;
        let clip = anim.clips.get(kind);
        sprite.image = clip.image.clone();
        sprite.texture_atlas = Some(TextureAtlas { layout: clip.layout.clone(), index: 0 });
        *anchor = clip.anchor;
    }

    let clip = anim.clips.get(kind);
    let frames = clip.frames;
    let index = match climb_progress {
        Some(t) => ((t * frames as f32) as usize).min(frames - 1),
        None => {
            // The walk cycle covers one cell; play it at the speed that keeps the feet planted.
            let fps = match kind {
                ClipKind::Walk => frames as f32 * WALK_SPEED / CELL_TILES,
                _ => clip.fps,
            };
            anim.elapsed = (anim.elapsed + time.delta_secs()) % (frames as f32 / fps);
            (anim.elapsed * fps) as usize % frames
        }
    };
    if let Some(atlas) = &mut sprite.texture_atlas {
        atlas.index = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk right from x = 1.5 on the floor (row y = 0) into the cliff at x = 3.
    fn climb_right(rows: &[&str]) -> Option<Climb> {
        let world = world::World::from_ascii(rows);
        let from = Vec2::new(2.55, 1.0);
        let blocked = from + Vec2::new(0.1, 0.0);
        assert!(collides(&world, blocked), "the cliff should block walking");
        plan_climb(&world, from, blocked, 1.0)
    }

    fn cliff(height: usize) -> Vec<String> {
        let mut rows: Vec<String> = (0..6).map(|_| "........".to_string()).collect();
        rows.extend((0..height).map(|_| "...#####".to_string()));
        rows.push("########".to_string());
        rows
    }

    fn rows(cliff: &[String]) -> Vec<&str> {
        cliff.iter().map(String::as_str).collect()
    }

    #[test]
    fn climbs_two_and_three_tile_cliffs() {
        for (height, clip) in [(2, ClipKind::ClimbLow), (3, ClipKind::ClimbHigh)] {
            let c = climb_right(&rows(&cliff(height))).expect("should climb");
            assert!(c.clip == clip);
            assert_eq!(c.to.y, 1.0 + height as f32);
            assert!(c.to.x > 3.0, "ends on top of the cliff");
        }
    }

    #[test]
    fn does_not_climb_one_or_four_tile_cliffs() {
        // One tile is a step handled by walking; four is too high.
        assert!(climb_right(&rows(&cliff(1))).is_none());
        assert!(climb_right(&rows(&cliff(4))).is_none());
    }

    /// Standing at x = 2.55 (column 2) on the floor, walking into a one-tile step at column 3.
    fn step_right(rows: &[&str]) -> Option<Vec2> {
        let world = world::World::from_ascii(rows);
        let blocked = Vec2::new(2.65, 1.0);
        assert!(!collides(&world, Vec2::new(2.55, 1.0)), "the start should fit");
        assert!(collides(&world, blocked), "the step should block walking");
        step(&world, blocked, 1.0, 1.0)
    }

    #[test]
    fn steps_up_under_a_low_ceiling() {
        // Only 3 tiles of room at the start; 3 tiles on top of the step.
        let stepped = step_right(&[
            "########",
            "###.....",
            "........",
            "........",
            "...#####",
            "########",
        ])
        .expect("should step up");
        assert_eq!(stepped.y, 2.0);
        assert!(stepped.x - HALF_WIDTH >= 3.0, "body fully on the step's column");
    }

    #[test]
    fn steps_up_in_place_when_there_is_room() {
        let stepped = step_right(&["........", "........", "........", "........", "...#####", "########"]);
        assert_eq!(stepped, Some(Vec2::new(2.65, 2.0)));
    }

    #[test]
    fn steps_down_under_a_low_ceiling() {
        // Standing on a one-tile ledge (column 2); beyond it the floor is a tile lower
        // and the ceiling leaves exactly 3 tiles of room, so walking straight on is blocked.
        let world = world::World::from_ascii(&[
            "........",
            "...#####",
            "........",
            "........",
            "###.....",
            "########",
        ]);
        let from = Vec2::new(2.55, 2.0);
        let blocked = from + Vec2::new(0.1, 0.0);
        assert!(!collides(&world, from), "the start should fit");
        assert!(collides(&world, blocked), "the low ceiling should block walking");
        let stepped = step(&world, blocked, 1.0, -1.0).expect("should step down");
        assert_eq!(stepped.y, 1.0);
        assert!(stepped.x - HALF_WIDTH >= 3.0, "body fully past the ledge");
    }

    #[test]
    fn does_not_step_up_without_room_on_the_step() {
        // Only 2 tiles of room on top of the step.
        let stepped = step_right(&[
            "...#####",
            "........",
            "........",
            "...#####",
            "########",
        ]);
        assert!(stepped.is_none());
    }

    #[test]
    fn measures_the_drop_off_an_edge() {
        // A ledge of height 4 at columns 0-2, with a 3-tile step at column 5 beyond it.
        let world = world::World::from_ascii(&[
            "###.....",
            "###..###",
            "###..###",
            "###..###",
            "########",
        ]);
        let right = |x| drop_below(&world, Vec2::new(x, 5.0), 1.0);
        assert_eq!(right(2.5), 0.0, "standing on the ledge");
        assert_eq!(right(2.75), 0.0, "front quarter just reaching the edge");
        assert_eq!(right(2.85), 4.0, "front quarter past the edge");
        assert_eq!(right(3.5), 4.0, "over the gap");
        assert_eq!(right(5.5), 1.0, "over the lower step");
        // Walking left off the ledge at column 5 into the gap.
        assert_eq!(drop_below(&world, Vec2::new(5.25, 4.0), -1.0), 0.0);
        assert_eq!(drop_below(&world, Vec2::new(5.15, 4.0), -1.0), 3.0);
    }

    /// A ledge (columns 0-2, feet at y = 5), a one-tile step down (column 3),
    /// then a 4-tile cliff (columns 4+).
    fn step_then_cliff() -> world::World {
        world::World::from_ascii(&["###.....", "####....", "####....", "####....", "####...."])
    }

    #[test]
    fn stops_at_a_cliff_while_dropping_down_a_step() {
        let world = step_then_cliff();
        let walk = |from: f32, to: f32, y: f32| {
            may_walk(&world, Vec2::new(from, y), Vec2::new(to, y), 1.0, false)
        };
        assert!(walk(2.9, 3.0, 5.0), "off the ledge onto the step");
        assert!(walk(3.7, 3.75, 4.5), "along the step while dropping onto it");
        assert!(!walk(3.75, 3.85, 4.5), "not past the cliff edge while still dropping");
        assert!(!walk(3.75, 3.85, 4.0), "not past the cliff edge after landing");
        assert!(may_walk(&world, Vec2::new(3.85, 4.0), Vec2::new(3.85, 4.0), 1.0, true), "S jumps");
    }

    #[test]
    fn can_back_away_from_an_overhang() {
        // Body mostly over the cliff, held only by the step's corner.
        let world = step_then_cliff();
        let from = Vec2::new(4.3, 4.0);
        assert!(may_walk(&world, from, from - Vec2::new(0.1, 0.0), -1.0, false));
    }

    #[test]
    fn does_not_climb_without_room_on_top() {
        let mut level = cliff(2);
        level[4] = "########".to_string(); // ceiling two tiles above the cliff top
        assert!(climb_right(&rows(&level)).is_none());
    }
}
