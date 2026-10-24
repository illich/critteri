mod player;
mod world;

use bevy::asset::RenderAssetUsages;
use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::window::WindowMode;
use image::imageops::FilterType;

/// Biome definitions used by the level generator.
const BIOMES_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/asset/biomes.ron");
/// Placeholder textures extracted from Oxygen Not Included (not in git).
const TILE_TEXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/asset/oni/tiles");
/// Texture resolution per tile after resampling; enough for 60 tiles across a 4K screen.
const TEXTURE_PX_PER_TILE: u32 = 64;
/// How many tiles one repeat of a material / background texture spans.
const MATERIAL_TEXTURE_TILES: u32 = 4;
const BACKGROUND_TEXTURE_TILES: u32 = 8;

/// Size of one tile in world units.
const TILE_SIZE: f32 = 32.0;
/// How many tiles fit across the screen, regardless of window size.
const VISIBLE_TILES_X: f32 = 60.0;

/// The generated level and the biomes it was built from.
#[derive(Resource)]
struct Level {
    world: world::World,
    biomes: world::Biomes,
}

fn main() {
    let source = std::fs::read_to_string(BIOMES_FILE)
        .unwrap_or_else(|e| panic!("cannot read {BIOMES_FILE}: {e}"));
    let biomes = world::Biomes::from_ron(&source)
        .unwrap_or_else(|e| panic!("invalid {BIOMES_FILE}: {e}"));
    let world = world::World::generate(42, &biomes);

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Colony".into(),
                        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: "asset".into(),
                    ..default()
                }),
        )
        .add_plugins(player::PlayerPlugin)
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Level { world, biomes })
        .add_systems(Startup, (spawn_camera, spawn_tiles))
        .add_systems(Update, (follow_player.after(player::PlayerSystems), quit_on_esc))
        .run();
}

fn quit_on_esc(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
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
        // `follow_player` moves it to the player.
        Transform::default(),
    ));
}

/// Load a seamless texture and resample it so it spans `tiles_across` tiles
/// at [`TEXTURE_PX_PER_TILE`]. The source textures are far larger than they
/// appear on screen, and without mipmaps they would shimmer while scrolling.
fn load_tiling_texture(path: &str, tiles_across: u32, images: &mut Assets<Image>) -> Handle<Image> {
    let source = image::open(path).unwrap_or_else(|e| panic!("cannot load {path}: {e}"));
    let size = tiles_across * TEXTURE_PX_PER_TILE;
    let resized = source.resize_exact(size, size, FilterType::Lanczos3);
    images.add(Image::from_dynamic(resized, true, RenderAssetUsages::RENDER_WORLD))
}

fn spawn_tiles(mut commands: Commands, level: Res<Level>, mut images: ResMut<Assets<Image>>) {
    let Level { world, biomes } = &*level;

    let materials: Vec<_> = biomes
        .materials
        .iter()
        .map(|name| {
            let path = format!("{TILE_TEXTURE_DIR}/materials/{name}.png");
            load_tiling_texture(&path, MATERIAL_TEXTURE_TILES, &mut images)
        })
        .collect();
    // One background per biome, indexed like `biomes.biomes`.
    let backgrounds: Vec<_> = biomes
        .biomes
        .iter()
        .map(|biome| {
            let path = format!("{TILE_TEXTURE_DIR}/backgrounds/{}.png", biome.background);
            load_tiling_texture(&path, BACKGROUND_TEXTURE_TILES, &mut images)
        })
        .collect();

    for y in 0..world::HEIGHT {
        for x in 0..world::WIDTH {
            let (texture, tiles_across) = match world.tile(x, y) {
                world::Tile::Solid { material } => {
                    (&materials[material as usize], MATERIAL_TEXTURE_TILES)
                }
                world::Tile::Empty => (
                    &backgrounds[world.biome(x, y) as usize],
                    BACKGROUND_TEXTURE_TILES,
                ),
            };
            // Each tile shows its own piece of the texture, so neighbouring
            // tiles of the same material join seamlessly. Image rows run
            // top-down while world y runs bottom-up.
            let px = TEXTURE_PX_PER_TILE as f32;
            let u = (x as u32 % tiles_across) as f32 * px;
            let v = (tiles_across - 1 - y as u32 % tiles_across) as f32 * px;
            commands.spawn((
                Sprite {
                    image: texture.clone(),
                    rect: Some(Rect::new(u, v, u + px, v + px)),
                    custom_size: Some(Vec2::splat(TILE_SIZE)),
                    ..default()
                },
                Transform::from_xyz(
                    (x as f32 + 0.5) * TILE_SIZE,
                    (y as f32 + 0.5) * TILE_SIZE,
                    0.0,
                ),
            ));
        }
    }
}

fn follow_player(
    player: Single<&player::Player>,
    camera: Single<(&mut Transform, &Projection), With<Camera2d>>,
) {
    let (mut camera, projection) = camera.into_inner();
    // Centre on the middle of the body rather than the feet.
    let target = player.focus + Vec2::Y * player::HEIGHT_TILES / 2.0 * TILE_SIZE;

    // Keep the view inside the level; the visible area depends on the window's aspect ratio.
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let half_view = ortho.area.half_size();
    let level = Vec2::new(world::WIDTH as f32, world::HEIGHT as f32) * TILE_SIZE;
    let max = (level - half_view).max(half_view);
    camera.translation.x = target.x.clamp(half_view.x, max.x);
    camera.translation.y = target.y.clamp(half_view.y, max.y);
}
