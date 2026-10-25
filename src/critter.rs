//! Critters living in the level: ONI Hatches that wander along the ground,
//! stop now and then to look around or groom, get excited when the player comes
//! close, and fall when their floor is dug out.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::player::{Player, PlayerSystems, CELL_TILES, TILES_PER_SHEET_PX};
use crate::sheet::{load_clip, Clip};
use crate::{world, Level, TILE_SIZE};

/// How many Hatches live in the level.
const HATCH_COUNT: usize = 15;
/// Height of a Hatch's body, in tiles: it needs two free tiles to fit.
const BODY_HEIGHT: f32 = 1.4;
/// How far ahead of its centre a Hatch checks for walls and edges, in tiles.
const REACH: f32 = 0.45;
/// Walking speed in tiles per second.
const WALK_SPEED: f32 = 1.8;
/// Gravity in tiles per second squared, and the terminal falling speed.
const GRAVITY: f32 = 40.0;
const MAX_FALL_SPEED: f32 = 20.0;
/// How long a Hatch walks, and rests, before deciding again (seconds, min..max).
const WALK_TIME: (f32, f32) = (2.0, 7.0);
const REST_TIME: (f32, f32) = (1.5, 4.0);
/// Share of rests spent grooming rather than looking around.
const GROOM_SHARE: f32 = 0.35;
/// How close (horizontally, and vertically) the player must be for a resting Hatch to notice them.
const NOTICE_DISTANCE: Vec2 = Vec2::new(3.5, 2.0);
/// Depth: just behind the player.
const CRITTER_Z: f32 = 0.9;

pub struct CritterPlugin;

impl Plugin for CritterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hatches)
            .add_systems(Update, (move_critters, animate_critters).chain().after(PlayerSystems));
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Activity {
    Walk,
    Idle,
    Groom,
}

#[derive(Component)]
pub struct Critter {
    /// Bottom centre of the body, in tiles.
    position: Vec2,
    fall_speed: f32,
    facing_left: bool,
    activity: Activity,
    /// Resting with the player close by.
    excited: bool,
    /// Time left in the current activity.
    remaining: f32,
    rng: world::XorShift,
}

impl Critter {
    /// Whether tile `(x, y)` is inside the body.
    pub fn covers_tile(&self, x: i32, y: i32) -> bool {
        let y0 = self.position.y.floor() as i32;
        let y1 = (self.position.y + BODY_HEIGHT).ceil() as i32;
        x == self.position.x.floor() as i32 && (y0..y1).contains(&y)
    }

    fn falling(&self) -> bool {
        self.fall_speed > 0.0
    }

    fn random_time(&mut self, (min, max): (f32, f32)) -> f32 {
        min + (max - min) * self.rng.next_f32()
    }

    /// Advance by `dt` seconds: climb out if buried, fall without a floor,
    /// otherwise walk or rest.
    fn update(&mut self, world: &world::World, dt: f32) {
        if !fits(world, self.position) {
            if let Some(free) = unbury(world, self.position) {
                self.position = free;
            }
            return;
        }
        // Fall when the floor is gone (e.g. dug out from under it).
        if self.falling() || !supported(world, self.position) {
            self.fall_speed = (self.fall_speed + GRAVITY * dt).min(MAX_FALL_SPEED);
            let (to, landed) = fall(world, self.position, self.fall_speed * dt);
            self.position = to;
            if landed {
                self.fall_speed = 0.0;
            }
            return;
        }

        self.remaining -= dt;
        if self.remaining <= 0.0 {
            self.decide();
        }
        if self.activity == Activity::Walk {
            let dx = if self.facing_left { -WALK_SPEED * dt } else { WALK_SPEED * dt };
            match walk(world, self.position, dx) {
                Some(to) => self.position = to,
                None => self.facing_left = !self.facing_left,
            }
        }
    }

    /// A resting Hatch with the player (feet at `player`) close by turns to them, excited.
    fn notice(&mut self, player: Vec2) {
        let offset = player - self.position;
        self.excited = self.activity != Activity::Walk
            && !self.falling()
            && offset.abs().cmple(NOTICE_DISTANCE).all();
        if self.excited && offset.x.abs() > 0.3 {
            self.facing_left = offset.x < 0.0;
        }
    }

    /// Pick what to do next: alternate walking (in a random direction) and resting.
    fn decide(&mut self) {
        if self.activity == Activity::Walk {
            self.activity =
                if self.rng.next_f32() < GROOM_SHARE { Activity::Groom } else { Activity::Idle };
            self.remaining = self.random_time(REST_TIME);
        } else {
            self.activity = Activity::Walk;
            self.facing_left = self.rng.below(2) == 0;
            self.remaining = self.random_time(WALK_TIME);
        }
    }
}

/// Whether a body standing at `feet` is free of solid tiles. A Hatch occupies
/// the column under its centre; [`REACH`] keeps its snout out of walls.
fn fits(world: &world::World, feet: Vec2) -> bool {
    let x = feet.x.floor() as i32;
    (feet.y.floor() as i32..(feet.y + BODY_HEIGHT).ceil() as i32).all(|y| !world.is_solid(x, y))
}

/// Whether a body at `feet` (at a whole level) stands on something.
fn supported(world: &world::World, feet: Vec2) -> bool {
    feet.y.fract() == 0.0 && world.is_floor(feet.x.floor() as i32, feet.y as i32)
}

/// Where walking `dx` tiles from `feet` (at a whole level) leads: along the
/// floor, towards a step down (gravity drops the body once its centre is over
/// it), or hopping one tile up a step into the next column. `None` means a wall
/// or a deeper drop ahead, and the Hatch should turn around.
fn walk(world: &world::World, feet: Vec2, dx: f32) -> Option<Vec2> {
    let ahead = feet + Vec2::new(dx, 0.0);
    let level = feet.y as i32;
    let lead = Vec2::new(ahead.x + REACH * dx.signum(), feet.y);
    let column = lead.x.floor() as i32;
    if fits(world, lead) {
        (world.is_floor(column, level) || world.is_floor(column, level - 1)).then_some(ahead)
    } else {
        let edge = if dx > 0.0 { column as f32 + 0.05 } else { column as f32 + 0.95 };
        let up = Vec2::new(edge, feet.y + 1.0);
        (fits(world, up) && world.is_floor(column, level + 1) && fits(world, feet + Vec2::Y))
            .then_some(up)
    }
}

/// Move a falling body at `feet` down by `distance`, landing on the first
/// level with a floor. Returns the new position and whether it landed.
fn fall(world: &world::World, feet: Vec2, distance: f32) -> (Vec2, bool) {
    let x = feet.x.floor() as i32;
    let to = feet.y - distance;
    let landing = (to.ceil() as i32..=feet.y.floor() as i32)
        .rev()
        .find(|&level| world.is_floor(x, level));
    match landing {
        Some(level) => (Vec2::new(feet.x, level as f32), true),
        None => (Vec2::new(feet.x, to), false),
    }
}

/// A spot to free a body buried at `feet` (a tile was filled into it): the
/// nearest place above where it fits.
fn unbury(world: &world::World, feet: Vec2) -> Option<Vec2> {
    (1..=3)
        .map(|up| Vec2::new(feet.x, feet.y.floor() + up as f32))
        .find(|&to| fits(world, to))
}

struct HatchClips {
    idle: Clip,
    walk: Clip,
    groom: Clip,
    fall: Clip,
    excited: Clip,
}

#[derive(Resource)]
struct CritterGraphics {
    hatch: HatchClips,
}

#[derive(Clone, Copy, PartialEq)]
enum ClipKind {
    Idle,
    Walk,
    Groom,
    Fall,
    Excited,
}

impl HatchClips {
    fn get(&self, kind: ClipKind) -> &Clip {
        match kind {
            ClipKind::Idle => &self.idle,
            ClipKind::Walk => &self.walk,
            ClipKind::Groom => &self.groom,
            ClipKind::Fall => &self.fall,
            ClipKind::Excited => &self.excited,
        }
    }
}

#[derive(Component)]
struct CritterAnimation {
    showing: ClipKind,
    elapsed: f32,
}

fn spawn_hatches(
    mut commands: Commands,
    level: Res<Level>,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let mut load = |name: &str| load_clip("critters/hatch", name, &assets, &mut layouts);
    let hatch = HatchClips {
        idle: load("idle_loop"),
        walk: load("floor_floor_1_0_loop"),
        groom: load("grooming_loop"),
        fall: load("fall"),
        excited: load("excited_loop"),
    };

    let mut rng = world::XorShift::new(7);
    // The player starts near the middle.
    let player_x = world::WIDTH / 2;
    for i in 0..HATCH_COUNT {
        // Spread over the level, away from the player's start.
        let mut near = (i * world::WIDTH + world::WIDTH / 2) / HATCH_COUNT;
        if near.abs_diff(player_x) < 8 {
            near = (near + 6) % world::WIDTH;
        }
        let Some((x, y)) = level.world.find_standing_spot(near, 2) else {
            continue;
        };
        let clip = &hatch.idle;
        commands.spawn((
            Critter {
                position: Vec2::new(x as f32 + 0.5, y as f32),
                fall_speed: 0.0,
                facing_left: rng.below(2) == 0,
                activity: Activity::Idle,
                excited: false,
                remaining: 0.5 + rng.next_f32() * REST_TIME.1,
                rng: world::XorShift::new(rng.below(1 << 30) as u64 + 1),
            },
            CritterAnimation { showing: ClipKind::Idle, elapsed: rng.next_f32() * clip.duration() },
            Sprite::from_atlas_image(
                clip.image.clone(),
                TextureAtlas { layout: clip.layout.clone(), index: 0 },
            ),
            clip.anchor,
            Transform::default(),
        ));
    }
    commands.insert_resource(CritterGraphics { hatch });
}

fn move_critters(
    time: Res<Time>,
    level: Res<Level>,
    player: Single<&Player>,
    mut critters: Query<&mut Critter>,
) {
    let player_at = player.focus / TILE_SIZE;
    for mut critter in &mut critters {
        critter.update(&level.world, time.delta_secs());
        critter.notice(player_at);
    }
}

fn animate_critters(
    time: Res<Time>,
    graphics: Res<CritterGraphics>,
    mut critters: Query<(&Critter, &mut CritterAnimation, &mut Sprite, &mut Anchor, &mut Transform)>,
) {
    let scale = TILE_SIZE * TILES_PER_SHEET_PX;
    for (critter, mut anim, mut sprite, mut anchor, mut transform) in &mut critters {
        let kind = match critter.activity {
            _ if critter.falling() => ClipKind::Fall,
            _ if critter.excited => ClipKind::Excited,
            Activity::Walk => ClipKind::Walk,
            Activity::Idle => ClipKind::Idle,
            Activity::Groom => ClipKind::Groom,
        };
        let clips = &graphics.hatch;
        if anim.showing != kind {
            anim.showing = kind;
            anim.elapsed = 0.0;
            let clip = clips.get(kind);
            sprite.image = clip.image.clone();
            sprite.texture_atlas = Some(TextureAtlas { layout: clip.layout.clone(), index: 0 });
            *anchor = clip.anchor;
        }
        let clip = clips.get(kind);
        // The walk cycle covers one cell; pace it so the feet don't slide.
        let fps = match kind {
            ClipKind::Walk => clip.frames as f32 * WALK_SPEED / CELL_TILES,
            _ => clip.fps,
        };
        anim.elapsed = (anim.elapsed + time.delta_secs()) % (clip.frames as f32 / fps);
        if let Some(atlas) = &mut sprite.texture_atlas {
            atlas.index = (anim.elapsed * fps) as usize % clip.frames;
        }

        transform.translation = (critter.position * TILE_SIZE).extend(CRITTER_Z);
        // The sheets face right; mirroring through the scale keeps the feet in place.
        transform.scale =
            Vec3::new(if critter.facing_left { -scale } else { scale }, scale, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Hatch at x = 1.5 on level `level` walking a little to the right.
    fn walk_right(rows: &[&str], level: f32) -> Option<Vec2> {
        let world = world::World::from_ascii(rows);
        walk(&world, Vec2::new(1.5, level), 0.1)
    }

    #[test]
    fn walks_along_the_floor_and_up_and_down_steps() {
        let ahead = Vec2::new(1.6, 1.0);
        assert_eq!(walk_right(&["....", "....", "....", "####"], 1.0), Some(ahead));
        let up = &["....", "....", "....", "..##", "####"];
        assert_eq!(walk_right(up, 1.0), Some(Vec2::new(2.05, 2.0)), "hops up a step");
        let down = &["....", "....", "....", "##..", "####"];
        assert_eq!(walk_right(down, 2.0), Some(Vec2::new(1.6, 2.0)), "walks on towards a step down");
    }

    #[test]
    fn turns_at_walls_and_drops() {
        assert_eq!(walk_right(&["....", "....", "..#.", "..#.", "####"], 1.0), None, "wall");
        let drop = &["....", "....", "....", "##..", "#...", "#..."];
        assert_eq!(walk_right(drop, 3.0), None, "drop");
        let low = &["....", "..#.", "....", "..##", "####"];
        assert_eq!(walk_right(low, 1.0), None, "no headroom on the step");
    }

    #[test]
    fn falls_onto_the_next_floor() {
        let world = world::World::from_ascii(&["....", "....", "....", "####"]);
        let (to, landed) = fall(&world, Vec2::new(1.5, 2.5), 1.6);
        assert!(landed);
        assert_eq!(to, Vec2::new(1.5, 1.0));
        let (to, landed) = fall(&world, Vec2::new(1.5, 2.5), 0.2);
        assert!(!landed);
        assert_eq!(to.y, 2.3);
    }

    #[test]
    fn wanders_the_generated_level_without_getting_stuck_in_rock() {
        let biomes =
            world::Biomes::from_ron(include_str!("../asset/biomes.ron")).expect("biomes parse");
        let world = world::World::generate(42, &biomes);
        for i in 0..8 {
            let (x, y) = world.find_standing_spot(i * world::WIDTH / 8, 2).unwrap();
            let start = Vec2::new(x as f32 + 0.5, y as f32);
            let mut hatch = Critter {
                position: start,
                fall_speed: 0.0,
                facing_left: false,
                activity: Activity::Idle,
                excited: false,
                remaining: 0.0,
                rng: world::XorShift::new(i as u64 + 1),
            };
            let mut travelled = 0.0;
            let mut falling_for = 0;
            for _ in 0..60 * 60 {
                let before = hatch.position;
                hatch.update(&world, 1.0 / 60.0);
                travelled += (hatch.position.x - before.x).abs();
                assert!(fits(&world, hatch.position), "hatch {i} in rock at {}", hatch.position);
                falling_for = if hatch.falling() { falling_for + 1 } else { 0 };
                assert!(falling_for < 60, "hatch {i} falling for a second at {}", hatch.position);
            }
            assert!(travelled > 5.0, "hatch {i} barely moved ({travelled} tiles) from {start}");
        }
    }

    #[test]
    fn resting_hatches_turn_to_a_nearby_player() {
        let mut hatch = Critter {
            position: Vec2::new(5.5, 1.0),
            fall_speed: 0.0,
            facing_left: false,
            activity: Activity::Idle,
            excited: false,
            remaining: 1.0,
            rng: world::XorShift::new(1),
        };
        hatch.notice(Vec2::new(3.0, 1.0));
        assert!(hatch.excited && hatch.facing_left);
        hatch.notice(Vec2::new(12.0, 1.0));
        assert!(!hatch.excited, "too far");
        hatch.activity = Activity::Walk;
        hatch.notice(Vec2::new(6.5, 1.0));
        assert!(!hatch.excited, "busy walking");
    }

    #[test]
    fn climbs_out_when_buried() {
        let world = world::World::from_ascii(&["....", "....", ".#..", "####"]);
        assert!(!fits(&world, Vec2::new(1.5, 1.0)));
        assert_eq!(unbury(&world, Vec2::new(1.5, 1.0)), Some(Vec2::new(1.5, 2.0)));
    }
}
