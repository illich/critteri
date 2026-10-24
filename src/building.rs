//! Building in empty tiles: holding R places a ladder on the hovered tile.
//! Buildings are removed by right-clicking them or filling their tile (see `mining`).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use image::imageops::FilterType;

use crate::cursor::HoveredTile;
use crate::{tile_center, world, Level, TEXTURE_PX_PER_TILE, TILE_SIZE, TILE_TEXTURE_DIR};

/// Size of the ladder sprite in tiles. ONI's ladder is a little larger than a
/// cell, so ladders stacked in a column join up.
const LADDER_SIZE: Vec2 = Vec2::new(1.2, 1.18);

#[derive(Resource)]
pub struct BuildingGraphics {
    ladder: Handle<Image>,
}

/// The sprite of each building, indexed like the world grid, `y * WIDTH + x`.
#[derive(Resource)]
pub struct BuildingSprites(Vec<Option<Entity>>);

impl BuildingSprites {
    /// Despawn the sprite of the building that was in tile `(x, y)`, if any.
    pub fn remove(&mut self, commands: &mut Commands, x: usize, y: usize) {
        if let Some(entity) = self.0[y * world::WIDTH + x].take() {
            commands.entity(entity).despawn();
        }
    }
}

pub fn setup_buildings(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let path = format!("{TILE_TEXTURE_DIR}/buildings/ladder.png");
    let source = image::open(&path).unwrap_or_else(|e| panic!("cannot load {path}: {e}"));
    // Resample to the tiles' texture resolution, as for the terrain textures.
    let size = (LADDER_SIZE * TEXTURE_PX_PER_TILE as f32).round();
    let resized = source.resize_exact(size.x as u32, size.y as u32, FilterType::Lanczos3);
    let ladder = images.add(Image::from_dynamic(resized, true, RenderAssetUsages::RENDER_WORLD));
    commands.insert_resource(BuildingGraphics { ladder });
    commands.insert_resource(BuildingSprites(vec![None; world::WIDTH * world::HEIGHT]));
}

pub fn place_ladder(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    hovered: Res<HoveredTile>,
    mut level: ResMut<Level>,
    graphics: Res<BuildingGraphics>,
    mut sprites: ResMut<BuildingSprites>,
) {
    let Some(tile) = hovered.0.filter(|_| keys.pressed(KeyCode::KeyR)) else {
        return;
    };
    let (x, y) = (tile.x as usize, tile.y as usize);
    if level.world.build(x, y, world::Building::Ladder) {
        let sprite = commands.spawn((
            Sprite {
                image: graphics.ladder.clone(),
                custom_size: Some(LADDER_SIZE * TILE_SIZE),
                ..default()
            },
            // Above the tile it stands in, below digging cracks and the hover highlight.
            Transform::from_translation(tile_center(x, y, 0.1)),
        ));
        sprites.0[y * world::WIDTH + x] = Some(sprite.id());
    }
}
