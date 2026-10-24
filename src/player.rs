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

    // A climb runs to completion without input or gravity.
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
        if !collides(world, target) {
            player.position = target;
            player.motion = Motion::Walk;
        } else if on_ground
            && !collides(world, player.position + Vec2::Y)
            && !collides(world, target + Vec2::Y)
        {
            // Step up onto a one-tile ledge.
            player.position = target + Vec2::Y;
            player.motion = Motion::Walk;
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
        Motion::Walk => (ClipKind::Walk, player.position, player.position, None),
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

    #[test]
    fn does_not_climb_without_room_on_top() {
        let mut level = cliff(2);
        level[4] = "########".to_string(); // ceiling two tiles above the cliff top
        assert!(climb_right(&rows(&level)).is_none());
    }
}
