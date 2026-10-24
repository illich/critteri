//! Digging out tiles by holding the right mouse button, shown as spreading cracks.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use image::imageops::FilterType;
use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::cursor::HoveredTile;
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

#[derive(Resource)]
pub struct Digging {
    /// Tile being dug, and for how long.
    tile: Option<UVec2>,
    elapsed: f32,
    /// Crack overlay for each stage, from first cracks to fully cracked.
    stages: Vec<Handle<Image>>,
}

#[derive(Component)]
pub struct CrackOverlay;

pub fn setup_digging(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(Digging {
        tile: None,
        elapsed: 0.0,
        stages: load_crack_stages(&mut images),
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
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    hovered: Res<HoveredTile>,
    mut digging: ResMut<Digging>,
    mut level: ResMut<Level>,
    graphics: Res<TileGraphics>,
    mut tile_sprites: Query<&mut Sprite, Without<CrackOverlay>>,
    overlay: Single<(&mut Sprite, &mut Transform, &mut Visibility), With<CrackOverlay>>,
) {
    let (mut overlay_sprite, mut overlay_transform, mut overlay_visibility) = overlay.into_inner();

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
        level.world.dig(x, y);
        if let Ok(mut sprite) = tile_sprites.get_mut(graphics.sprites[y * world::WIDTH + x]) {
            *sprite = graphics.sprite(&level.world, x, y);
        }
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
