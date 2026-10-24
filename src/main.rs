mod cursor;
mod mining;
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
        .init_resource::<cursor::HoveredTile>()
        .add_systems(
            Startup,
            (spawn_camera, spawn_tiles, cursor::spawn_tile_highlight, mining::setup_digging),
        )
        .add_systems(
            Update,
            (
                (follow_player, cursor::highlight_hovered_tile, mining::dig)
                    .chain()
                    .after(player::PlayerSystems),
                quit_on_esc,
            ),
        )
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

/// Textures for drawing tiles, and the sprite entity of every tile.
#[derive(Resource)]
struct TileGraphics {
    /// Indexed like [`world::Biomes::materials`].
    materials: Vec<Handle<Image>>,
    /// One background per biome, indexed like [`world::Biomes::biomes`].
    backgrounds: Vec<Handle<Image>>,
    /// Indexed like the world grid, `y * WIDTH + x`.
    sprites: Vec<Entity>,
}

impl TileGraphics {
    /// The sprite showing tile `(x, y)` as it currently is in `world`.
    fn sprite(&self, world: &world::World, x: usize, y: usize) -> Sprite {
        let (texture, tiles_across) = match world.tile(x, y) {
            world::Tile::Solid { material } => {
                (&self.materials[material as usize], MATERIAL_TEXTURE_TILES)
            }
            world::Tile::Empty => (
                &self.backgrounds[world.biome(x, y) as usize],
                BACKGROUND_TEXTURE_TILES,
            ),
        };
        Sprite {
            image: texture.clone(),
            rect: Some(texture_rect(x, y, tiles_across)),
            custom_size: Some(Vec2::splat(TILE_SIZE)),
            ..default()
        }
    }
}

/// The piece of a seamless texture spanning `tiles_across` tiles that tile
/// `(x, y)` shows, so neighbouring tiles join up. Image rows run top-down
/// while world y runs bottom-up.
fn texture_rect(x: usize, y: usize, tiles_across: u32) -> Rect {
    let px = TEXTURE_PX_PER_TILE as f32;
    let u = (x as u32 % tiles_across) as f32 * px;
    let v = (tiles_across - 1 - y as u32 % tiles_across) as f32 * px;
    Rect::new(u, v, u + px, v + px)
}

/// World-space centre of tile `(x, y)` at depth `z`.
fn tile_center(x: usize, y: usize, z: f32) -> Vec3 {
    Vec3::new((x as f32 + 0.5) * TILE_SIZE, (y as f32 + 0.5) * TILE_SIZE, z)
}

fn spawn_tiles(mut commands: Commands, level: Res<Level>, mut images: ResMut<Assets<Image>>) {
    let Level { world, biomes } = &*level;

    let mut graphics = TileGraphics {
        materials: biomes
            .materials
            .iter()
            .map(|name| {
                let path = format!("{TILE_TEXTURE_DIR}/materials/{name}.png");
                load_tiling_texture(&path, MATERIAL_TEXTURE_TILES, &mut images)
            })
            .collect(),
        backgrounds: biomes
            .biomes
            .iter()
            .map(|biome| {
                let path = format!("{TILE_TEXTURE_DIR}/backgrounds/{}.png", biome.background);
                load_tiling_texture(&path, BACKGROUND_TEXTURE_TILES, &mut images)
            })
            .collect(),
        sprites: Vec::with_capacity(world::WIDTH * world::HEIGHT),
    };
    for y in 0..world::HEIGHT {
        for x in 0..world::WIDTH {
            let sprite = graphics.sprite(world, x, y);
            let entity = commands
                .spawn((sprite, Transform::from_translation(tile_center(x, y, 0.0))))
                .id();
            graphics.sprites.push(entity);
        }
    }
    commands.insert_resource(graphics);
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
