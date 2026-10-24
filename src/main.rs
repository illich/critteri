mod world;

use bevy::camera::ScalingMode;
use bevy::prelude::*;

/// Size of one tile in world units.
const TILE_SIZE: f32 = 32.0;
/// How many tiles fit across the screen, regardless of window size.
const VISIBLE_TILES_X: f32 = 60.0;
/// Camera scroll speed in tiles per second.
const SCROLL_SPEED: f32 = 30.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Colony".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, (spawn_camera, spawn_tiles))
        .add_systems(Update, scroll_camera)
        .run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedHorizontal {
                viewport_width: VISIBLE_TILES_X * TILE_SIZE,
            },
            ..OrthographicProjection::default_2d()
        }),
        // Start in the bottom-left corner of the level.
        Transform::from_xyz(
            VISIBLE_TILES_X * TILE_SIZE / 2.0,
            VISIBLE_TILES_X * TILE_SIZE * 9.0 / 32.0,
            0.0,
        ),
    ));
}

fn spawn_tiles(mut commands: Commands) {
    let world = world::World::random(42);
    for y in 0..world::HEIGHT {
        for x in 0..world::WIDTH {
            let [r, g, b] = world.tile(x, y).color;
            commands.spawn((
                Sprite::from_color(Color::srgb(r, g, b), Vec2::splat(TILE_SIZE)),
                Transform::from_xyz(
                    (x as f32 + 0.5) * TILE_SIZE,
                    (y as f32 + 0.5) * TILE_SIZE,
                    0.0,
                ),
            ));
        }
    }
}

fn scroll_camera(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera: Single<&mut Transform, With<Camera2d>>,
) {
    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir.x += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir.y -= 1.0;
    }

    let delta = dir.normalize_or_zero() * SCROLL_SPEED * TILE_SIZE * time.delta_secs();
    camera.translation += delta.extend(0.0);

    // Keep the camera center inside the level.
    let max = Vec2::new(world::WIDTH as f32, world::HEIGHT as f32) * TILE_SIZE;
    camera.translation.x = camera.translation.x.clamp(0.0, max.x);
    camera.translation.y = camera.translation.y.clamp(0.0, max.y);
}
