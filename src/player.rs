//! The player-controlled duplicant: walking, climbing, jumping, ladders, gravity and sprite animation.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::sheet::{load_clip, Clip};
use crate::{world, Level, TILE_SIZE};

/// Visible height of the standing duplicant in tiles.
pub const HEIGHT_TILES: f32 = 2.7;
/// Height of a standing duplicant in the rendered sheets, in pixels; sets the
/// sprite scale. Measured on a short-haired dupe; hair makes some a little taller.
const SHEET_HEIGHT_PX: f32 = 154.0;
/// Size of one ONI cell in the rendered sheets, in pixels. Animations that
/// move the duplicant (walk cycle, climbs, jumps) move it in whole cells.
const SHEET_CELL_PX: f32 = 100.0;
/// Our tiles per sheet pixel, from scaling the duplicant to [`HEIGHT_TILES`].
/// Effects rendered at the same kanim scale use it too.
pub const TILES_PER_SHEET_PX: f32 = HEIGHT_TILES / SHEET_HEIGHT_PX;
/// One ONI cell in our tiles.
const CELL_TILES: f32 = SHEET_CELL_PX * TILES_PER_SHEET_PX;
/// Half the width of the collision box, in tiles. Its height is [`HEIGHT_TILES`].
const HALF_WIDTH: f32 = 0.4;
/// Walking speed in tiles per second.
const WALK_SPEED: f32 = 4.0;
/// Running speed. The player starts running after walking [`RUN_AFTER`] tiles
/// when the ground ahead stays flat and open for [`RUN_CLEARANCE`] tiles, and
/// keeps running for as long as the direction key stays held.
const RUN_SPEED: f32 = 2.0 * WALK_SPEED;
const RUN_AFTER: f32 = 0.5;
const RUN_CLEARANCE: f32 = 3.0;
/// Highest drop, in tiles, the player walks off without holding down (S).
const MAX_SAFE_DROP: f32 = 3.0;
/// Speed on ladders in tiles per second.
const LADDER_SPEED: f32 = 3.0;
/// Height above the feet, in tiles, from which the dig pose is chosen by the
/// direction to the dug tile (about where the tool is held).
const AIM_HEIGHT: f32 = 1.1;
/// Where the tool is assumed to be, relative to the feet and facing right, while
/// digging without a dig pose (walking, falling, climbing).
const UNPOSED_TOOL: Vec2 = Vec2::new(0.35, 1.2);
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

#[derive(Clone, Copy, PartialEq)]
enum ClipKind {
    Idle,
    Walk,
    /// Up one ONI cell, used for 2-tile cliffs.
    ClimbLow,
    /// Up two ONI cells, used for 3-tile cliffs.
    ClimbHigh,
    /// Across two ONI cells, used to jump 1- and 2-tile gaps; level, up a cell, or down a cell.
    Jump,
    JumpUp,
    JumpDown,
    LadderUp,
    LadderDown,
    /// Holding on to a ladder without climbing.
    LadderIdle,
    /// Aiming the dig tool, standing or on a ladder; first raising it, then looping.
    Dig { aim: Aim, ladder: bool, looping: bool },
}

/// Directions of the dig poses.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Aim {
    Forward,
    Up,
    Down,
    DiagonalUp,
    DiagonalDown,
}

impl Aim {
    const ALL: [Aim; 5] = [Aim::Forward, Aim::Up, Aim::Down, Aim::DiagonalUp, Aim::DiagonalDown];

    /// The pose closest to direction `to` (from the tool, facing right or left).
    fn towards(to: Vec2) -> Aim {
        let angle = to.y.atan2(to.x.abs()).to_degrees();
        match angle {
            a if a > 67.5 => Aim::Up,
            a if a > 22.5 => Aim::DiagonalUp,
            a if a >= -22.5 => Aim::Forward,
            a if a >= -67.5 => Aim::DiagonalDown,
            _ => Aim::Down,
        }
    }

    /// Names of the standing and ladder pose animations (raising the tool, then looping).
    fn animations(self) -> [[&'static str; 2]; 2] {
        match self {
            Aim::Forward => [["dig_fwd_pre", "dig_fwd_loop"], ["ladder_dig_pre", "ladder_dig_loop"]],
            Aim::Up => [["dig_up_pre", "dig_up_loop"], ["ladder_dig_up_pre", "ladder_dig_up_loop"]],
            Aim::Down => [["dig_dn_pre", "dig_dn_loop"], ["ladder_dig_dn_pre", "ladder_dig_dn_loop"]],
            Aim::DiagonalUp => [
                ["dig_diag_up_pre", "dig_diag_up_loop"],
                ["ladder_dig_diag_up_pre", "ladder_dig_loop_diag_up"],
            ],
            Aim::DiagonalDown => [
                ["dig_diag_dn_pre", "dig_diag_dn_loop"],
                ["ladder_dig_diag_dn_pre", "ladder_dig_loop_diag_dn"],
            ],
        }
    }
}

struct Clips {
    idle: Clip,
    walk: Clip,
    climb_low: Clip,
    climb_high: Clip,
    jump: Clip,
    jump_up: Clip,
    jump_down: Clip,
    ladder_up: Clip,
    ladder_down: Clip,
    ladder_idle: Clip,
    /// Dig poses by [`Aim::ALL`] index, then standing/ladder, then raising/looping.
    dig: Vec<[[Clip; 2]; 2]>,
}

impl Clips {
    fn get(&self, kind: ClipKind) -> &Clip {
        match kind {
            ClipKind::Idle => &self.idle,
            ClipKind::Walk => &self.walk,
            ClipKind::ClimbLow => &self.climb_low,
            ClipKind::ClimbHigh => &self.climb_high,
            ClipKind::Jump => &self.jump,
            ClipKind::JumpUp => &self.jump_up,
            ClipKind::JumpDown => &self.jump_down,
            ClipKind::LadderUp => &self.ladder_up,
            ClipKind::LadderDown => &self.ladder_down,
            ClipKind::LadderIdle => &self.ladder_idle,
            ClipKind::Dig { aim, ladder, looping } => {
                let aim = Aim::ALL.iter().position(|&a| a == aim).unwrap();
                &self.dig[aim][ladder as usize][looping as usize]
            }
        }
    }
}

/// A climb or jump in progress. Their animations carry the duplicant a fixed
/// distance (`shift`) within the sprite; the sprite itself is moved so that
/// the animation ends exactly at `to`.
#[derive(Clone, Copy)]
struct Leap {
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
    Leap(Leap),
    /// On a ladder, centred in its column, climbing to the whole level `target`
    /// (or resting there once reached). Gravity doesn't apply.
    Ladder { target: f32 },
}

#[derive(Component)]
pub struct Player {
    /// Bottom centre of the body, in tiles.
    position: Vec2,
    fall_speed: f32,
    facing_left: bool,
    motion: Motion,
    /// Distance walked without stopping, turning or leaving the ground, in tiles.
    walked: f32,
    running: bool,
    /// Current walking or running speed; sets the pace of the walk cycle.
    speed: f32,
    /// Where the camera should look, in world units; follows the body smoothly while leaping.
    pub focus: Vec2,
    /// Centre of the tile being dug, in tiles; set by digging.
    pub dig_target: Option<Vec2>,
    /// Muzzle of the dig tool while digging, in world units.
    pub tool_tip: Option<Vec2>,
}

impl Player {
    /// Whether tile `(x, y)` is inside the body, or inside the area it is moving
    /// through during a climb, jump or step.
    pub fn covers_tile(&self, x: i32, y: i32) -> bool {
        let (end_a, end_b) = match self.motion {
            Motion::Leap(leap) => (leap.from, leap.to),
            Motion::StepAcross { to } => (self.position, to),
            Motion::Ladder { target } => (self.position, Vec2::new(self.position.x, target)),
            Motion::Idle | Motion::Walk => (self.position, self.position),
        };
        let (low, high) = (end_a.min(end_b).min(self.position), end_a.max(end_b).max(self.position));
        let x0 = (low.x - HALF_WIDTH).floor() as i32;
        let x1 = (high.x + HALF_WIDTH).ceil() as i32;
        let y0 = low.y.floor() as i32;
        let y1 = (high.y + HEIGHT_TILES).ceil() as i32;
        (x0..x1).contains(&x) && (y0..y1).contains(&y)
    }
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
    let mut load = |folder: &str, name: &str| {
        load_clip(&format!("player/{folder}"), name, &assets, &mut layouts)
    };
    let clips = Clips {
        idle: load("idle", "idle_default"),
        walk: load("loco", "floor_floor_1_0_loop"),
        climb_low: load("loco", "floor_floor_1_1"),
        climb_high: load("loco", "floor_floor_1_2"),
        jump: load("loco", "floor_floor_2_0"),
        jump_up: load("loco", "floor_floor_2_1"),
        jump_down: load("loco", "floor_floor_2_-1"),
        ladder_up: load("loco", "ladder_ladder_0_1_loop"),
        ladder_down: load("loco", "ladder_ladder_0_-1_loop"),
        ladder_idle: load("idle", "ladder_idle"),
        dig: Aim::ALL
            .iter()
            .map(|aim| aim.animations().map(|names| names.map(|name| load("dig", name))))
            .collect(),
    };

    commands.spawn((
        Player {
            position: Vec2::new(x as f32 + 0.5, y as f32),
            fall_speed: 0.0,
            facing_left: false,
            motion: Motion::Idle,
            walked: 0.0,
            running: false,
            speed: WALK_SPEED,
            focus: Vec2::ZERO,
            dig_target: None,
            tool_tip: None,
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

/// The next level to climb to from whole level `level` on a ladder in `column`:
/// up while the feet are in a ladder tile, down while there is a ladder below
/// them, as long as the body fits there.
fn ladder_step(world: &world::World, column: i32, level: i32, up: bool, down: bool) -> Option<f32> {
    let fits = |level: i32| !collides(world, Vec2::new(column as f32 + 0.5, level as f32));
    if up && world.is_ladder(column, level) && fits(level + 1) {
        Some((level + 1) as f32)
    } else if down && world.is_ladder(column, level - 1) && fits(level - 1) {
        Some((level - 1) as f32)
    } else {
        None
    }
}

/// The level to reach up to from the ground at whole level `level` for a ladder
/// in `column` whose bottom is 1 or 2 tiles above the feet: the bottom ladder
/// tile's level, as long as the body fits there.
fn ladder_reach(world: &world::World, column: i32, level: i32) -> Option<f32> {
    (level + 1..=level + 2)
        .find(|&y| world.is_ladder(column, y))
        .filter(|&y| !collides(world, Vec2::new(column as f32 + 0.5, y as f32)))
        .map(|y| y as f32)
}

/// Whether resting at whole level `level` in `column` means hanging on a ladder,
/// rather than standing on the ground or on top of the ladder.
fn hanging_on_ladder(world: &world::World, column: i32, level: i32) -> bool {
    world.is_ladder(column, level) && !world.is_solid(column, level - 1)
}

/// The highest level the body at `x`, moving down from `from_y` to `to_y`, lands
/// on: one with a floor under any column the body covers (see [`world::World::is_floor`]).
fn landing(world: &world::World, x: f32, from_y: f32, to_y: f32) -> Option<f32> {
    let x0 = (x - HALF_WIDTH).floor() as i32;
    let x1 = (x + HALF_WIDTH).ceil() as i32;
    (to_y.floor() as i32 + 1..=from_y.floor() as i32)
        .rev()
        .find(|&level| (x0..x1).any(|c| world.is_floor(c, level)))
        .map(|level| level as f32)
}

/// How far below `feet` the ground is when walking in direction `dir`, in tiles.
/// Measured three quarters of the way along the body, so the player stops at a
/// cliff edge with only its leading quarter over it. The level's bottom edge
/// counts as ground.
fn drop_below(world: &world::World, feet: Vec2, dir: f32) -> f32 {
    let x = (feet.x + dir * HALF_WIDTH / 2.0).floor() as i32;
    let y = feet.y.floor() as i32;
    let landing = (0..=y).rev().find(|&ty| world.is_floor(x, ty)).unwrap_or(0);
    feet.y - landing as f32
}

/// Whether the ground ahead of the body standing at `feet` stays flat and open
/// for `distance` tiles in direction `dir`: solid underfoot at the same level, with
/// 3 tiles of headroom.
fn flat_ahead(world: &world::World, feet: Vec2, dir: f32, distance: f32) -> bool {
    let y = feet.y.round() as i32;
    let step = dir as i32;
    let front = (feet.x + dir * HALF_WIDTH).floor() as i32;
    let end = (feet.x + dir * (HALF_WIDTH + distance)).floor() as i32;
    (0..=(end - front) * step).all(|i| {
        let c = front + step * i;
        world.is_floor(c, y) && (y..y + 3).all(|ty| !world.is_solid(c, ty))
    })
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
fn plan_climb(world: &world::World, from: Vec2, blocked: Vec2, dir: f32) -> Option<Leap> {
    let wall_x = (blocked.x + dir * HALF_WIDTH).floor() as i32;
    let y = from.y.round() as i32;
    // The top of the wall ahead, from its highest solid tile within reach: the tiles
    // below it may be empty, as under an overhang. A solid tile 4 up is too high.
    let height = (0..4)
        .rev()
        .find(|&k| world.is_solid(wall_x, y + k))
        .map_or(0, |k| k as usize + 1);
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
    Some(Leap {
        clip,
        from,
        to,
        shift: Vec2::new(dir, cells_up) * CELL_TILES,
        elapsed: 0.0,
    })
}

/// A jump over a gap of 1 or 2 columns, triggered when walking from `from` to
/// `to` (direction `dir`) would bring the front of the body over the gap. Each gap
/// column needs 3 tiles of headroom at the take-off level and must drop at least
/// 2 tiles below it (a 1-tile step down is walked). The column after the gap must
/// be ground at the same level or one tile up or down, with 3 tiles of headroom
/// above that ground.
fn plan_jump(world: &world::World, from: Vec2, to: Vec2, dir: f32) -> Option<Leap> {
    let y = from.y.round() as i32;
    let step = dir as i32;
    // 3 tiles of headroom in column `c` from level `level` up.
    let open = |c: i32, level: i32| (level..level + 3).all(|ty| !world.is_solid(c, ty));
    let is_gap = |c: i32| open(c, y) && !world.is_floor(c, y) && !world.is_floor(c, y - 1);
    let is_ground = |c: i32, level: i32| open(c, level) && world.is_floor(c, level);

    // The column under the front quarter of the body, as for cliff edges.
    let first_gap = (to.x + dir * HALF_WIDTH / 2.0).floor() as i32;
    let (gap, rise) = (1..=2)
        .flat_map(|gap| [0, 1, -1].map(move |rise| (gap, rise)))
        .find(|&(gap, rise)| {
            (0..gap).all(|i| is_gap(first_gap + step * i))
                && is_ground(first_gap + step * gap, y + rise)
        })?;
    let landing = first_gap + step * gap;
    let level = y + rise;

    // Land where the animation naturally ends if the ground carries on that far,
    // otherwise in the middle of the first column past the gap.
    let ground_up_to = |x: f32| {
        let last = (x + dir * HALF_WIDTH).floor() as i32;
        let columns = (last - landing) * step;
        columns >= 0 && (0..=columns).all(|i| is_ground(landing + step * i, level))
    };
    let natural = from.x + dir * 2.0 * CELL_TILES;
    let centre = landing as f32 + 0.5;
    let x = [natural, centre].into_iter().find(|&x| ground_up_to(x))?;
    let clip = match rise {
        1 => ClipKind::JumpUp,
        -1 => ClipKind::JumpDown,
        _ => ClipKind::Jump,
    };
    Some(Leap {
        clip,
        from,
        to: Vec2::new(x, level as f32),
        shift: Vec2::new(dir * 2.0, rise as f32) * CELL_TILES,
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

    let up = keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]);
    let down = keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]);
    let sideways = keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft, KeyCode::KeyD, KeyCode::ArrowRight]);

    // On a ladder: finish climbing to the current level, then climb on, hang
    // there, or get off (walking sideways, or standing on ground or the top).
    if let Motion::Ladder { target } = player.motion {
        let remaining = target - player.position.y;
        let advance = LADDER_SPEED * dt;
        if remaining.abs() > advance {
            player.position.y += advance * remaining.signum();
            return;
        }
        player.position.y = target;
        let (column, level) = (player.position.x.floor() as i32, target as i32);
        if !sideways {
            if let Some(next) = ladder_step(world, column, level, up, down) {
                player.motion = Motion::Ladder { target: next };
                return;
            }
            if hanging_on_ladder(world, column, level) {
                if down && !world.is_ladder(column, level - 1) {
                    // Let go at the bottom end: drop just below the ladder's bottom
                    // edge so it no longer holds the body, and fall.
                    player.position.y -= 0.001;
                } else {
                    return;
                }
            }
        }
        player.motion = Motion::Idle;
    }

    // Steps across, climbs and jumps run to completion without input or gravity.
    if let Motion::StepAcross { to } = player.motion {
        let remaining = to.x - player.position.x;
        let advance = player.speed * dt;
        if remaining.abs() > advance {
            player.position.x += advance * remaining.signum();
        } else {
            player.position = to;
            player.motion = Motion::Walk;
        }
        return;
    }
    if let Motion::Leap(mut climb) = player.motion {
        climb.elapsed += dt;
        if climb.elapsed < anim.clips.get(climb.clip).duration() {
            player.motion = Motion::Leap(climb);
            return;
        }
        player.position = climb.to;
        player.motion = Motion::Idle;
    }

    player.fall_speed = (player.fall_speed + GRAVITY * dt).min(MAX_FALL_SPEED);
    let below = player.position - Vec2::new(0.0, player.fall_speed * dt);
    let on_ground = match landing(world, player.position.x, player.position.y, below.y) {
        Some(level) => {
            player.position.y = level;
            player.fall_speed = 0.0;
            true
        }
        None => {
            player.position = below;
            false
        }
    };

    let mut dir = 0.0;
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir += 1.0;
    }

    // Get on a ladder in the column under the centre of the body (W up, S down),
    // reaching up to one whose bottom is up to 2 tiles above the feet.
    if on_ground && dir == 0.0 {
        let column = player.position.x.floor() as i32;
        let level = player.position.y.round() as i32;
        let centred = Vec2::new(column as f32 + 0.5, level as f32);
        if let Some(next) = ladder_step(world, column, level, up, down)
            .or_else(|| ladder_reach(world, column, level).filter(|_| up))
            && !collides(world, centred)
        {
            player.position = centred;
            player.motion = Motion::Ladder { target: next };
            player.walked = 0.0;
            player.running = false;
            return;
        }
    }

    player.motion = Motion::Idle;
    // Only an uninterrupted walk keeps counting towards running.
    let walked = std::mem::take(&mut player.walked);
    if dir == 0.0 {
        player.running = false;
        player.speed = WALK_SPEED;
    } else {
        let turning = player.facing_left != (dir < 0.0);
        player.facing_left = dir < 0.0;
        let starts_running = on_ground
            && walked >= RUN_AFTER
            && flat_ahead(world, player.position, dir, RUN_CLEARANCE);
        player.running = !turning && (player.running || starts_running);
        let speed = if player.running { RUN_SPEED } else { WALK_SPEED };
        player.speed = speed;
        let target = player.position + Vec2::new(dir * speed * dt, 0.0);
        let wants_down = keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]);
        if !collides(world, target) {
            if on_ground
                && !wants_down
                && let Some(jump) = plan_jump(world, player.position, target, dir)
            {
                player.motion = Motion::Leap(jump);
            } else if may_walk(world, player.position, target, dir, wants_down) {
                player.position = target;
                player.motion = Motion::Walk;
                if on_ground && !turning {
                    player.walked = walked + speed * dt;
                }
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
            player.motion = Motion::Leap(climb);
        }
    }
}

fn animate_player(
    time: Res<Time>,
    player: Single<(&mut Player, &mut Animation, &mut Sprite, &mut Anchor, &mut Transform)>,
) {
    let (mut player, mut anim, mut sprite, mut anchor, mut transform) = player.into_inner();

    let (mut kind, sprite_at, body_at, climb_progress) = match player.motion {
        Motion::Idle => (ClipKind::Idle, player.position, player.position, None),
        Motion::Walk | Motion::StepAcross { .. } => {
            (ClipKind::Walk, player.position, player.position, None)
        }
        Motion::Ladder { target } => {
            let kind = if target > player.position.y {
                ClipKind::LadderUp
            } else if target < player.position.y {
                ClipKind::LadderDown
            } else {
                ClipKind::LadderIdle
            };
            (kind, player.position, player.position, None)
        }
        Motion::Leap(c) => {
            let t = (c.elapsed / anim.clips.get(c.clip).duration()).min(1.0);
            let body = c.from + (c.to - c.from) * t;
            (c.clip, body - c.shift * t, body, Some(t))
        }
    };
    player.focus = body_at * TILE_SIZE;

    // Digging while standing or resting on a ladder: face the tile and aim the
    // tool at it, raising the tool first unless already aiming.
    if let Some(target) = player.dig_target
        && matches!(kind, ClipKind::Idle | ClipKind::LadderIdle)
    {
        let to = target - (player.position + Vec2::new(0.0, AIM_HEIGHT));
        if to.x.abs() > 0.05 {
            player.facing_left = to.x < 0.0;
        }
        let aim = Aim::towards(to);
        let ladder = kind == ClipKind::LadderIdle;
        // Once raised, the tool turns straight to a new aim.
        let raised = anim.elapsed + time.delta_secs() >= anim.clips.get(anim.showing).duration();
        let looping = match anim.showing {
            ClipKind::Dig { aim: shown, ladder: on_ladder, looping } if on_ladder == ladder => {
                looping || (shown == aim && raised)
            }
            _ => false,
        };
        kind = ClipKind::Dig { aim, ladder, looping };
    }

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
    let (frames, clip_fps) = (clip.frames, clip.fps);
    let index = match climb_progress {
        Some(t) => ((t * frames as f32) as usize).min(frames - 1),
        None => {
            // The walk cycle covers one cell; play it at the speed that keeps the feet planted.
            let fps = match kind {
                ClipKind::Walk => frames as f32 * player.speed / CELL_TILES,
                // Like the walk cycle, a climbing cycle covers one cell.
                ClipKind::LadderUp | ClipKind::LadderDown => {
                    frames as f32 * LADDER_SPEED / CELL_TILES
                }
                _ => clip_fps,
            };
            anim.elapsed = (anim.elapsed + time.delta_secs()) % (frames as f32 / fps);
            (anim.elapsed * fps) as usize % frames
        }
    };
    if let Some(atlas) = &mut sprite.texture_atlas {
        atlas.index = index;
    }

    // The tool's muzzle: from the dig pose's frame, or a guess near the hands.
    let mirror = Vec2::new(transform.scale.x.signum(), 1.0);
    player.tool_tip = player.dig_target.map(|_| match anim.clips.get(kind).tips.get(index) {
        Some(&tip) => transform.translation.truncate() + tip * mirror * scale,
        None => (body_at + UNPOSED_TOOL * mirror) * TILE_SIZE,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk right from x = 1.5 on the floor (row y = 0) into the cliff at x = 3.
    fn climb_right(rows: &[&str]) -> Option<Leap> {
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

    /// Walking right on ground whose surface is at y = 3 (columns 0-2), with the
    /// front quarter of the body just reaching column 3.
    fn jump_right(rows: &[&str]) -> Option<Leap> {
        let world = world::World::from_ascii(rows);
        let from = Vec2::new(2.75, 3.0);
        assert!(!collides(&world, from), "the start should fit");
        plan_jump(&world, from, from + Vec2::new(0.1, 0.0), 1.0)
    }

    /// Open space above ground at y = 0-2, with `ground` as each ground row.
    fn ground(ground: &str) -> Vec<&str> {
        vec!["........", "........", "........", ground, ground, ground]
    }

    #[test]
    fn jumps_one_and_two_tile_gaps() {
        let jump = jump_right(&ground("###.####")).expect("should jump a 1-tile gap");
        assert!(jump.clip == ClipKind::Jump);
        assert_eq!(jump.to.y, 3.0);
        assert!(jump.to.x - HALF_WIDTH >= 4.0, "lands past the gap");

        let jump = jump_right(&ground("###..###")).expect("should jump a 2-tile gap");
        assert!(jump.to.x - HALF_WIDTH >= 5.0, "lands past the gap");
    }

    #[test]
    fn lands_where_the_animation_ends_if_the_ground_continues() {
        let jump = jump_right(&ground("###.####")).unwrap();
        assert_eq!(jump.to.x, 2.75 + 2.0 * CELL_TILES);
        // Another hole further on: land on the first column past the gap instead.
        let jump = jump_right(&ground("###.##.#")).unwrap();
        assert_eq!(jump.to.x, 4.5);
    }

    #[test]
    fn does_not_jump_a_step_down_a_wide_gap_or_without_room() {
        let mut step_down = ground("###.####");
        step_down[4] = "########";
        assert!(jump_right(&step_down).is_none(), "a 1-tile step down is walked");
        assert!(jump_right(&ground("###...##")).is_none(), "a 3-tile gap is too wide");
        let mut low = ground("###.####");
        low[0] = "....#...";
        assert!(jump_right(&low).is_none(), "only 2 tiles of headroom past the gap");
        let mut two_up = ground("###.####");
        two_up[1] = "....####";
        two_up[2] = "....####";
        assert!(jump_right(&two_up).is_none(), "the far side is 2 tiles higher");
    }

    #[test]
    fn jumps_one_tile_up_or_down() {
        let mut up = vec!["........"];
        up.extend(ground("###.####"));
        up[3] = "....####";
        let jump = jump_right(&up).expect("should jump up");
        assert!(jump.clip == ClipKind::JumpUp);
        assert_eq!(jump.to.y, 4.0);

        let mut down = ground("###.####");
        down[3] = "###.....";
        let jump = jump_right(&down).expect("should jump down");
        assert!(jump.clip == ClipKind::JumpDown);
        assert_eq!(jump.to.y, 2.0);
    }

    #[test]
    fn jumping_up_needs_room_above_the_landing() {
        // Enough room at the take-off level over the gap, but not above the higher landing.
        let mut up = vec!["....#..."];
        up.extend(ground("###.####"));
        up[3] = "....####";
        assert!(jump_right(&up).is_none());
    }

    #[test]
    fn covers_the_body_and_the_path_of_a_leap() {
        let mut player = Player {
            position: Vec2::new(2.5, 1.0),
            fall_speed: 0.0,
            facing_left: false,
            motion: Motion::Idle,
            walked: 0.0,
            running: false,
            speed: WALK_SPEED,
            focus: Vec2::ZERO,
            dig_target: None,
            tool_tip: None,
        };
        // The body spans x 2.1-2.9 and y 1-3.7.
        assert!(player.covers_tile(2, 1) && player.covers_tile(2, 3));
        assert!(!player.covers_tile(2, 4) && !player.covers_tile(3, 1) && !player.covers_tile(2, 0));

        player.motion = Motion::Leap(Leap {
            clip: ClipKind::Jump,
            from: player.position,
            to: Vec2::new(5.5, 2.0),
            shift: Vec2::ZERO,
            elapsed: 0.0,
        });
        assert!(player.covers_tile(4, 2), "a tile on the way");
        assert!(player.covers_tile(5, 4), "the top of the body at the landing");
    }

    #[test]
    fn climbs_onto_overhangs() {
        // A single tile 1 or 2 tiles above the floor, with empty tiles under it.
        for (gap, clip) in [(1, ClipKind::ClimbLow), (2, ClipKind::ClimbHigh)] {
            let mut level: Vec<String> = (0..6).map(|_| "........".to_string()).collect();
            level.push("...#####".to_string());
            level.extend((0..gap).map(|_| "........".to_string()));
            level.push("########".to_string());
            let leap = climb_right(&rows(&level)).expect("should climb the overhang");
            assert!(leap.clip == clip);
            assert_eq!(leap.to.y, 2.0 + gap as f32);
        }
    }

    #[test]
    fn runs_only_on_flat_open_ground() {
        let flat = |rows: &[&str]| {
            let world = world::World::from_ascii(rows);
            flat_ahead(&world, Vec2::new(1.5, 1.0), 1.0, RUN_CLEARANCE)
        };
        assert!(flat(&["........", "........", "........", "........", "########"]));
        assert!(!flat(&["........", "........", "........", "....#...", "########"]), "a step");
        assert!(!flat(&["........", "........", "........", "........", "####.###"]), "a hole");
        assert!(!flat(&["........", "....#...", "........", "........", "########"]), "low ceiling");
        // Further than 3 tiles ahead of the body's front doesn't matter.
        assert!(flat(&["........", "........", "........", "......#.", "########"]));
    }

    #[test]
    fn lands_on_ladders() {
        let mut world = world::World::from_ascii(&["........", "........", "........", "........", "........"]);
        world.build(3, 2, world::Building::Ladder);
        // Falling onto the top of the ladder.
        assert_eq!(landing(&world, 3.5, 3.2, 2.9), Some(3.0));
        // Falling into the ladder from beside: its bottom edge holds the body.
        assert_eq!(landing(&world, 3.5, 2.1, 1.8), Some(2.0));
        // Missing it entirely, down to the floor at the level's bottom edge.
        assert_eq!(landing(&world, 6.5, 3.2, 2.9), None);
        assert_eq!(landing(&world, 6.5, 0.2, -0.1), Some(0.0));
    }

    #[test]
    fn walks_onto_ladder_tops_without_stopping() {
        // A 4-tile cliff bridged by a ladder top at the cliff's level.
        let mut world = world::World::from_ascii(&["###.....", "###.....", "###.....", "###....."]);
        world.build(3, 3, world::Building::Ladder);
        assert_eq!(drop_below(&world, Vec2::new(2.95, 4.0), 1.0), 0.0);
        assert!(flat_ahead(&world, Vec2::new(1.5, 4.0), 1.0, 1.0));
    }

    #[test]
    fn climbs_ladders_up_and_down() {
        // A ladder in column 3 from the floor (y = 0) up to y = 3; a ceiling at y = 7.
        let mut world = world::World::from_ascii(&[
            "########", "........", "........", "........", "........", "........", "........", "........",
        ]);
        for y in 0..4 {
            world.build(3, y, world::Building::Ladder);
        }
        assert_eq!(ladder_step(&world, 3, 0, true, false), Some(1.0), "up from the bottom");
        assert_eq!(ladder_step(&world, 3, 3, true, false), Some(4.0), "up onto the top");
        assert_eq!(ladder_step(&world, 3, 4, true, false), None, "nothing more to climb");
        assert_eq!(ladder_step(&world, 3, 4, false, true), Some(3.0), "down from the top");
        assert_eq!(ladder_step(&world, 3, 0, false, true), None, "already at the bottom");
        assert_eq!(ladder_step(&world, 2, 0, true, false), None, "no ladder in this column");

        assert!(hanging_on_ladder(&world, 3, 2));
        assert!(!hanging_on_ladder(&world, 3, 4), "standing on top");
        assert!(!hanging_on_ladder(&world, 3, 0), "standing on the floor");
    }

    #[test]
    fn letting_go_at_the_bottom_end_falls() {
        // A ladder hanging in mid-air at y = 3-4 in column 3.
        let mut world = world::World::from_ascii(&["........", "........", "........", "........", "........"]);
        world.build(3, 3, world::Building::Ladder);
        world.build(3, 4, world::Building::Ladder);
        assert!(hanging_on_ladder(&world, 3, 3));
        assert_eq!(ladder_step(&world, 3, 3, false, true), None, "no more ladder below");
        // Just below the bottom edge, nothing holds the body until the floor.
        assert_eq!(landing(&world, 3.5, 2.999, 2.9), None);
        assert_eq!(landing(&world, 3.5, 3.0, 2.9), Some(3.0), "but resting on it does");
    }

    #[test]
    fn aims_at_the_nearest_dig_pose() {
        assert_eq!(Aim::towards(Vec2::new(3.0, 0.5)), Aim::Forward);
        assert_eq!(Aim::towards(Vec2::new(-3.0, -0.5)), Aim::Forward, "facing left");
        assert_eq!(Aim::towards(Vec2::new(1.0, 1.0)), Aim::DiagonalUp);
        assert_eq!(Aim::towards(Vec2::new(-1.0, -1.0)), Aim::DiagonalDown);
        assert_eq!(Aim::towards(Vec2::new(0.0, 2.0)), Aim::Up);
        assert_eq!(Aim::towards(Vec2::new(0.3, -2.0)), Aim::Down);
    }

    #[test]
    fn reaches_up_to_ladders_above_the_ground() {
        // Ladders in columns 2, 4 and 6 whose bottoms are 1, 2 and 3 tiles above the floor.
        let mut world = world::World::from_ascii(&["........"; 9].into_iter().chain(["########"]).collect::<Vec<_>>());
        for (column, bottom) in [(2, 2), (4, 3), (6, 4)] {
            for y in bottom..bottom + 3 {
                world.build(column, y, world::Building::Ladder);
            }
        }
        assert_eq!(ladder_reach(&world, 2, 1), Some(2.0), "1 tile up");
        assert_eq!(ladder_reach(&world, 4, 1), Some(3.0), "2 tiles up");
        assert_eq!(ladder_reach(&world, 6, 1), None, "3 tiles up is out of reach");
        assert_eq!(ladder_reach(&world, 3, 1), None, "no ladder in this column");
        assert!(hanging_on_ladder(&world, 4, 3), "hangs once there");
    }

    #[test]
    fn does_not_climb_a_ladder_into_a_ceiling() {
        let mut world = world::World::from_ascii(&["....", "...#", "....", "....", "...."]);
        world.build(3, 0, world::Building::Ladder);
        world.build(3, 1, world::Building::Ladder);
        // From y = 1 the body would need y 2-4, but y = 3 is solid.
        assert_eq!(ladder_step(&world, 3, 1, true, false), None);
    }

    #[test]
    fn does_not_climb_without_room_on_top() {
        let mut level = cliff(2);
        level[4] = "########".to_string(); // ceiling two tiles above the cliff top
        assert!(climb_right(&rows(&level)).is_none());
    }
}
