//! Wild plants: ONI species growing on the ground of their biome. How far a
//! plant has grown shows in its look (ONI's growth stages); grown plants sway,
//! and clicking one with the right mouse button harvests it into the inventory
//! (one unit of its produce), after which it grows again. A plant dies when its floor is dug out or its tile filled.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::cursor::HoveredTile;
use crate::inventory::{Inventory, BITS_PER_TILE};
use crate::player::TILES_PER_SHEET_PX;
use crate::sheet::{load_clip, Clip};
use crate::{world, Level, TILE_SIZE};

/// How many plants grow in the level, and the least distance between two, in tiles.
const PLANT_COUNT: usize = 45;
const SPACING: i32 = 3;
/// Time for a plant to grow from a sprout until it is ripe, in seconds (min..max).
const GROW_TIME: (f32, f32) = (90.0, 240.0);
/// Depth: in front of the tiles and the tile highlight, behind critters and the player.
const PLANT_Z: f32 = 0.6;

pub struct PlantPlugin;

impl Plugin for PlantPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_plants).add_systems(
            Update,
            (harvest_plants, grow_plants, animate_plants).chain().after(crate::mining::dig),
        );
    }
}

/// A plant species' animations, and the empty tiles it needs above its own.
struct Species {
    /// Growth stages, one frame each.
    grow: Clip,
    /// Ripening, played once when fully grown.
    ripen: Clip,
    grown: Clip,
    harvest: Clip,
    headroom: i32,
}

/// Every plant species, indexed like [`world::Biomes::plants`].
#[derive(Resource)]
struct PlantGraphics {
    species: Vec<Species>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    /// Growth from 0 (a sprout) to 1 (ripe).
    Growing(f32),
    /// Playing the ripening or harvest animation for this long.
    Ripening(f32),
    Grown,
    Harvested(f32),
}

#[derive(Component)]
pub struct Plant {
    species: usize,
    /// The tile it grows in, standing on the solid tile below.
    tile: IVec2,
    grow_time: f32,
    stage: Stage,
}

impl Plant {
    /// Advance by `dt` seconds; ripening and harvesting last `ripen` and `harvest` seconds.
    fn grow(&mut self, dt: f32, ripen: f32, harvest: f32) {
        self.stage = match self.stage {
            Stage::Growing(growth) if growth + dt / self.grow_time >= 1.0 => Stage::Ripening(0.0),
            Stage::Growing(growth) => Stage::Growing(growth + dt / self.grow_time),
            Stage::Ripening(t) if t + dt >= ripen => Stage::Grown,
            Stage::Ripening(t) => Stage::Ripening(t + dt),
            Stage::Grown => Stage::Grown,
            Stage::Harvested(t) if t + dt >= harvest => Stage::Growing(0.0),
            Stage::Harvested(t) => Stage::Harvested(t + dt),
        };
    }

    /// Whether the plant still has its floor and its tile is free.
    fn rooted(&self, world: &world::World) -> bool {
        world.is_solid(self.tile.x, self.tile.y - 1) && !world.is_solid(self.tile.x, self.tile.y)
    }
}

/// Animation state: when a grown plant's sway started, so neighbours don't sway in step.
#[derive(Component)]
struct PlantAnimation {
    elapsed: f32,
}

/// Tiles where a species with `headroom` could grow in `world`: empty, with no
/// building, on solid ground, and with that many empty tiles above.
fn can_grow(world: &world::World, x: i32, y: i32, headroom: i32) -> bool {
    world.is_solid(x, y - 1)
        && !world.is_ladder(x, y)
        && (y..=y + headroom).all(|ty| ty < world::HEIGHT as i32 && !world.is_solid(x, ty))
}

fn spawn_plants(
    mut commands: Commands,
    level: Res<Level>,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let Level { world, biomes } = &*level;
    let species: Vec<Species> = biomes
        .plants
        .iter()
        .map(|plant| {
            let folder = format!("plants/{}", plant.name);
            let mut load = |anim: &str| load_clip(&folder, anim, &assets, &mut layouts);
            let grown = load("idle_full");
            // The grown plant may poke a little into the rock above it.
            let headroom = (grown.rise * TILES_PER_SHEET_PX).floor() as i32;
            Species {
                grow: load("grow"),
                ripen: load("grow_pst"),
                harvest: load("harvest"),
                grown,
                headroom,
            }
        })
        .collect();

    let mut rng = world::XorShift::new(11);
    let mut candidates: Vec<IVec2> = (1..world::HEIGHT as i32)
        .flat_map(|y| (0..world::WIDTH as i32).map(move |x| IVec2::new(x, y)))
        .filter(|tile| can_grow(world, tile.x, tile.y, 1))
        .collect();
    let mut planted: Vec<IVec2> = Vec::new();
    while planted.len() < PLANT_COUNT && !candidates.is_empty() {
        let tile = candidates.swap_remove(rng.below(candidates.len()));
        let choices = &biomes.biomes[world.biome(tile.x as usize, tile.y as usize) as usize].plants;
        if choices.is_empty() || planted.iter().any(|p| (*p - tile).abs().max_element() < SPACING) {
            continue;
        }
        let kind = choices[rng.below(choices.len())];
        if !can_grow(world, tile.x, tile.y, species[kind].headroom) {
            continue;
        }
        planted.push(tile);
        let grow_time = GROW_TIME.0 + (GROW_TIME.1 - GROW_TIME.0) * rng.next_f32();
        // Some start ripe, the rest part way grown.
        let growth = rng.next_f32() * 1.5;
        let clip = &species[kind].grown;
        let mut sprite = Sprite::from_atlas_image(
            clip.image.clone(),
            TextureAtlas { layout: clip.layout.clone(), index: 0 },
        );
        sprite.flip_x = rng.below(2) == 0;
        commands.spawn((
            Plant {
                species: kind,
                tile,
                grow_time,
                stage: if growth >= 1.0 { Stage::Grown } else { Stage::Growing(growth) },
            },
            PlantAnimation { elapsed: rng.next_f32() * clip.duration() },
            sprite,
            clip.anchor,
            Transform::from_translation(
                (Vec2::new(tile.x as f32 + 0.5, tile.y as f32) * TILE_SIZE).extend(PLANT_Z),
            )
            .with_scale(Vec3::new(TILE_SIZE * TILES_PER_SHEET_PX, TILE_SIZE * TILES_PER_SHEET_PX, 1.0)),
        ));
    }
    commands.insert_resource(PlantGraphics { species });
}

/// Right-clicking a ripe plant (its tile or the tiles above it that it covers) harvests it.
fn harvest_plants(
    buttons: Res<ButtonInput<MouseButton>>,
    hovered: Res<HoveredTile>,
    graphics: Res<PlantGraphics>,
    mut inventory: ResMut<Inventory>,
    mut plants: Query<&mut Plant>,
) {
    let Some(tile) = hovered.0.filter(|_| buttons.just_pressed(MouseButton::Right)) else {
        return;
    };
    let tile = tile.as_ivec2();
    for mut plant in &mut plants {
        let height = graphics.species[plant.species].headroom.max(1);
        if plant.stage == Stage::Grown
            && tile.x == plant.tile.x
            && (plant.tile.y..plant.tile.y + height).contains(&tile.y)
        {
            plant.stage = Stage::Harvested(0.0);
            let produce = inventory.produce(plant.species);
            inventory.add(produce, BITS_PER_TILE);
        }
    }
}

fn grow_plants(
    mut commands: Commands,
    time: Res<Time>,
    level: Res<Level>,
    graphics: Res<PlantGraphics>,
    mut plants: Query<(Entity, &mut Plant)>,
) {
    for (entity, mut plant) in &mut plants {
        if !plant.rooted(&level.world) {
            commands.entity(entity).despawn();
            continue;
        }
        let species = &graphics.species[plant.species];
        plant.grow(time.delta_secs(), species.ripen.duration(), species.harvest.duration());
    }
}

fn animate_plants(
    time: Res<Time>,
    graphics: Res<PlantGraphics>,
    mut plants: Query<(&Plant, &mut PlantAnimation, &mut Sprite, &mut Anchor)>,
) {
    for (plant, mut anim, mut sprite, mut anchor) in &mut plants {
        let species = &graphics.species[plant.species];
        let (clip, frame) = match plant.stage {
            Stage::Growing(growth) => {
                (&species.grow, (growth * species.grow.frames as f32) as usize)
            }
            Stage::Ripening(t) => (&species.ripen, (t * species.ripen.fps) as usize),
            Stage::Harvested(t) => (&species.harvest, (t * species.harvest.fps) as usize),
            Stage::Grown => {
                anim.elapsed = (anim.elapsed + time.delta_secs()) % species.grown.duration();
                (&species.grown, (anim.elapsed * species.grown.fps) as usize)
            }
        };
        if sprite.image != clip.image {
            sprite.image = clip.image.clone();
            sprite.texture_atlas = Some(TextureAtlas { layout: clip.layout.clone(), index: 0 });
            *anchor = clip.anchor;
        }
        if let Some(atlas) = &mut sprite.texture_atlas {
            atlas.index = frame.min(clip.frames - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plant(stage: Stage) -> Plant {
        Plant { species: 0, tile: IVec2::new(1, 1), grow_time: 10.0, stage }
    }

    #[test]
    fn grows_ripens_and_regrows_after_harvest() {
        let mut p = plant(Stage::Growing(0.0));
        p.grow(5.0, 1.0, 2.0);
        assert_eq!(p.stage, Stage::Growing(0.5));
        p.grow(5.0, 1.0, 2.0);
        assert_eq!(p.stage, Stage::Ripening(0.0));
        p.grow(1.0, 1.0, 2.0);
        assert_eq!(p.stage, Stage::Grown);
        p.grow(100.0, 1.0, 2.0);
        assert_eq!(p.stage, Stage::Grown, "stays ripe until harvested");
        p.stage = Stage::Harvested(0.0);
        p.grow(2.0, 1.0, 2.0);
        assert_eq!(p.stage, Stage::Growing(0.0));
    }

    #[test]
    fn needs_its_floor_and_free_tile() {
        let mut world = world::World::from_ascii(&["...", "...", "###"]);
        let p = plant(Stage::Grown);
        assert!(p.rooted(&world));
        world.dig(1, 0);
        assert!(!p.rooted(&world), "floor dug out");
        assert!(can_grow(&world::World::from_ascii(&["...", "...", "###"]), 1, 1, 1));
        assert!(!can_grow(&world::World::from_ascii(&[".#.", "...", "###"]), 1, 1, 1), "no headroom");
    }
}
