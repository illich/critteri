//! Tracks the tile under the mouse cursor and highlights it when it is solid.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{world, Level, TILE_SIZE};

/// Colour laid over the hovered tile.
const HIGHLIGHT: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);

#[derive(Component)]
pub struct TileHighlight;

/// The level tile under the mouse cursor, if any.
#[derive(Resource, Default)]
pub struct HoveredTile(pub Option<UVec2>);

/// The highlight entity, kept apart from the camera so both transforms can be borrowed.
type HighlightOnly = (With<TileHighlight>, Without<Camera2d>);

pub fn spawn_tile_highlight(mut commands: Commands) {
    commands.spawn((
        TileHighlight,
        Sprite::from_color(HIGHLIGHT, Vec2::splat(TILE_SIZE)),
        // Above the tiles, below the player.
        Transform::from_xyz(0.0, 0.0, 0.5),
        Visibility::Hidden,
    ));
}

/// Run after the camera has moved this frame, so the highlight doesn't lag behind.
pub fn highlight_hovered_tile(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<Camera2d>>,
    highlight: Single<(&mut Transform, &mut Visibility), HighlightOnly>,
    mut hovered: ResMut<HoveredTile>,
    level: Res<Level>,
) {
    let (camera, camera_transform) = camera.into_inner();
    let (mut transform, mut visibility) = highlight.into_inner();

    // The camera has no parent, so its global transform is its local one; using it
    // directly avoids waiting for this frame's transform propagation.
    let tile = window
        .cursor_position()
        .and_then(|cursor| {
            camera
                .viewport_to_world_2d(&GlobalTransform::from(*camera_transform), cursor)
                .ok()
        })
        .map(|point| (point / TILE_SIZE).floor())
        .filter(|tile| {
            (0.0..world::WIDTH as f32).contains(&tile.x)
                && (0.0..world::HEIGHT as f32).contains(&tile.y)
        });

    hovered.0 = tile.map(|tile| tile.as_uvec2());
    match tile.filter(|tile| level.world.is_solid(tile.x as i32, tile.y as i32)) {
        Some(tile) => {
            transform.translation = ((tile + 0.5) * TILE_SIZE).extend(transform.translation.z);
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}
