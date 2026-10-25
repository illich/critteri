//! Changing the terrain by hand: digging tiles out by holding the right mouse
//! button (shown as spreading cracks; buildings are removed at once) and filling
//! empty tiles by holding F (replacing any building). While digging, the
//! player aims a beam at the tile, as with ONI's multitool.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use image::imageops::FilterType;
use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::building::BuildingSprites;
use crate::critter::Critter;
use crate::cursor::HoveredTile;
use crate::inventory::{Inventory, BITS_PER_TILE};
use crate::player::{Player, TILES_PER_SHEET_PX};
use crate::sheet::{load_clip, Clip};
use crate::{
    texture_rect, tile_center, world, Level, TileGraphics, TEXTURE_PX_PER_TILE, TILE_SIZE,
    TILE_TEXTURE_DIR,
};

/// Time to dig out one tile, in seconds.
const DIG_TIME: f32 = 0.2;
/// Number of crack stages shown while digging.
const CRACK_STAGES: usize = 6;
/// How many tiles one repeat of the crack texture spans.
const CRACK_TEXTURE_TILES: u32 = 4;
/// Opacity of the drawn cracks.
const CRACK_OPACITY: f32 = 0.8;
/// Depth of the dig beam and its glow: in front of the player and the tile highlight.
const BEAM_Z: f32 = 2.0;

#[derive(Resource)]
pub struct Digging {
    /// Tile being dug, and for how long.
    tile: Option<UVec2>,
    elapsed: f32,
    /// Crack overlay for each stage, from first cracks to fully cracked.
    stages: Vec<Handle<Image>>,
    /// The beam (pointing right from its origin, stretched to reach the tile),
    /// and how long it has been animating.
    beam: Clip,
    beam_time: f32,
}

#[derive(Component)]
pub struct CrackOverlay;

#[derive(Component)]
pub struct DigBeam;

#[derive(Component)]
pub struct DigGlow;

pub fn setup_digging(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let beam = load_clip("effects/laser", "laser", &assets, &mut layouts);
    let glow = load_clip("effects/laser", "glow", &assets, &mut layouts);
    let sprite = |clip: &Clip| {
        (
            Sprite::from_atlas_image(
                clip.image.clone(),
                TextureAtlas { layout: clip.layout.clone(), index: 0 },
            ),
            clip.anchor,
            Transform::default(),
            Visibility::Hidden,
        )
    };
    commands.spawn((DigBeam, sprite(&beam)));
    commands.spawn((DigGlow, sprite(&glow)));
    commands.insert_resource(Digging {
        tile: None,
        elapsed: 0.0,
        stages: load_crack_stages(&mut images),
        beam,
        beam_time: 0.0,
    });
    commands.spawn((
        CrackOverlay,
        Sprite::default(),
        // Above the tiles, below the hover highlight.
        Transform::from_xyz(0.0, 0.0, 0.25),
        Visibility::Hidden,
    ));
}

/// Crack overlays for each digging stage. The crack texture's alpha holds
/// cracks at rising damage levels; each stage shows those above a falling
/// threshold, so the strongest cracks appear first and the rest spread from them.
fn load_crack_stages(images: &mut Assets<Image>) -> Vec<Handle<Image>> {
    let path = format!("{TILE_TEXTURE_DIR}/effects/crack.png");
    let damage = image::open(&path)
        .unwrap_or_else(|e| panic!("cannot load {path}: {e}"))
        .to_rgba8();
    let size = CRACK_TEXTURE_TILES * TEXTURE_PX_PER_TILE;

    (0..CRACK_STAGES)
        .map(|stage| {
            let threshold = 255.0 * (1.0 - (stage + 1) as f32 / CRACK_STAGES as f32);
            let mask = GrayImage::from_fn(damage.width(), damage.height(), |x, y| {
                Luma([if damage.get_pixel(x, y)[3] as f32 > threshold { 255 } else { 0 }])
            });
            // Threshold at full resolution, then shrink, so crack edges stay smooth.
            let mask = image::imageops::resize(&mask, size, size, FilterType::Triangle);
            let cracks = RgbaImage::from_fn(size, size, |x, y| {
                Rgba([0, 0, 0, (mask.get_pixel(x, y)[0] as f32 * CRACK_OPACITY) as u8])
            });
            images.add(Image::from_dynamic(cracks.into(), true, RenderAssetUsages::RENDER_WORLD))
        })
        .collect()
}

#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub fn dig(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    hovered: Res<HoveredTile>,
    mut digging: ResMut<Digging>,
    mut level: ResMut<Level>,
    graphics: Res<TileGraphics>,
    mut tile_sprites: Query<&mut Sprite, Without<CrackOverlay>>,
    overlay: Single<(&mut Sprite, &mut Transform, &mut Visibility), With<CrackOverlay>>,
    mut building_sprites: ResMut<BuildingSprites>,
    mut inventory: ResMut<Inventory>,
) {
    let (mut overlay_sprite, mut overlay_transform, mut overlay_visibility) = overlay.into_inner();

    // A building in the hovered tile is removed at once.
    if let Some(tile) = hovered.0.filter(|_| buttons.pressed(MouseButton::Right)) {
        let (x, y) = (tile.x as usize, tile.y as usize);
        if level.world.remove_building(x, y) {
            building_sprites.remove(&mut commands, x, y);
        }
    }

    let target = hovered.0.filter(|tile| {
        buttons.pressed(MouseButton::Right) && level.world.is_solid(tile.x as i32, tile.y as i32)
    });
    let Some(tile) = target else {
        digging.tile = None;
        *overlay_visibility = Visibility::Hidden;
        return;
    };
    // Moving to another tile starts over.
    if digging.tile != Some(tile) {
        digging.tile = Some(tile);
        digging.elapsed = 0.0;
    }
    digging.elapsed += time.delta_secs();

    let (x, y) = (tile.x as usize, tile.y as usize);
    if digging.elapsed >= DIG_TIME {
        if let Some(material) = level.world.dig(x, y) {
            inventory.add(material, BITS_PER_TILE);
        }
        redraw_tile(&graphics, &level.world, &mut tile_sprites, x, y);
        digging.tile = None;
        *overlay_visibility = Visibility::Hidden;
        return;
    }

    let stage = ((digging.elapsed / DIG_TIME * CRACK_STAGES as f32) as usize).min(CRACK_STAGES - 1);
    *overlay_sprite = Sprite {
        image: digging.stages[stage].clone(),
        rect: Some(texture_rect(x, y, CRACK_TEXTURE_TILES)),
        custom_size: Some(Vec2::splat(TILE_SIZE)),
        ..default()
    };
    overlay_transform.translation = tile_center(x, y, overlay_transform.translation.z);
    *overlay_visibility = Visibility::Visible;
}

type BeamParts<'a> = (&'a mut Sprite, &'a mut Transform, &'a mut Visibility);
type GlowParts<'a> = (&'a mut Transform, &'a mut Visibility);

/// Tell the player which tile is being dug, and draw the beam from the tool to it.
pub fn show_dig_beam(
    time: Res<Time>,
    mut digging: ResMut<Digging>,
    mut player: Single<&mut Player>,
    beam: Single<BeamParts, (With<DigBeam>, Without<DigGlow>)>,
    glow: Single<GlowParts, (With<DigGlow>, Without<DigBeam>)>,
) {
    let (mut beam_sprite, mut beam_transform, mut beam_visibility) = beam.into_inner();
    let (mut glow_transform, mut glow_visibility) = glow.into_inner();

    let target = digging.tile.map(|tile| tile.as_vec2() + 0.5);
    player.dig_target = target;
    // The player aims at a new target from the next frame on, and only then
    // knows where its tool is.
    let (Some(target), Some(tip)) = (target, player.tool_tip) else {
        digging.beam_time = 0.0;
        *beam_visibility = Visibility::Hidden;
        *glow_visibility = Visibility::Hidden;
        return;
    };
    digging.beam_time += time.delta_secs();

    let scale = TILE_SIZE * TILES_PER_SHEET_PX;
    let to = target * TILE_SIZE - tip;
    *beam_transform = Transform {
        translation: tip.extend(BEAM_Z),
        rotation: Quat::from_rotation_z(to.to_angle()),
        // Stretched along its length so it ends at the tile's centre.
        scale: Vec3::new(to.length() / digging.beam.reach, scale, 1.0),
    };
    let frame = (digging.beam_time * digging.beam.fps) as usize % digging.beam.frames;
    if let Some(atlas) = &mut beam_sprite.texture_atlas {
        atlas.index = frame;
    }
    *beam_visibility = Visibility::Visible;

    *glow_transform = Transform::from_translation(tip.extend(BEAM_Z + 0.01))
        .with_scale(Vec3::new(scale, scale, 1.0));
    *glow_visibility = Visibility::Visible;
}

/// Update tile `(x, y)`'s sprite after the tile changed in `world`.
fn redraw_tile(
    graphics: &TileGraphics,
    world: &world::World,
    tile_sprites: &mut Query<&mut Sprite, Without<CrackOverlay>>,
    x: usize,
    y: usize,
) {
    if let Ok(mut sprite) = tile_sprites.get_mut(graphics.sprites[y * world::WIDTH + x]) {
        *sprite = graphics.sprite(world, x, y);
    }
}

/// While F is held, fill the hovered empty tile, unless the player or a critter is in the way.
#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
pub fn fill(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    hovered: Res<HoveredTile>,
    player: Single<&Player>,
    critters: Query<&Critter>,
    mut level: ResMut<Level>,
    graphics: Res<TileGraphics>,
    mut tile_sprites: Query<&mut Sprite, Without<CrackOverlay>>,
    mut building_sprites: ResMut<BuildingSprites>,
) {
    let Some(tile) = hovered.0.filter(|_| keys.pressed(KeyCode::KeyF)) else {
        return;
    };
    let (tx, ty) = (tile.x as i32, tile.y as i32);
    if player.covers_tile(tx, ty) || critters.iter().any(|critter| critter.covers_tile(tx, ty)) {
        return;
    }
    let (x, y) = (tile.x as usize, tile.y as usize);
    let level = &mut *level;
    if level.world.fill(x, y, &level.biomes) {
        building_sprites.remove(&mut commands, x, y);
        redraw_tile(&graphics, &level.world, &mut tile_sprites, x, y);
    }
}
