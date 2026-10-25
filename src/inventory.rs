//! The player's inventory: how much of each material has been dug out, shown
//! as a row of ONI's material icons on a black panel in the lower right corner
//! of the screen.
//!
//! Quantities count in bits, 8 to a tile (see `info/concept.md`). An icon shows
//! the whole tiles as a number 1-9 and the remaining 0-7 bits as small squares
//! in a 2x4 grid filled from the lower left. A slot holds at most 10 tiles; a
//! full slot shows no number or bits, only a white outline around the icon.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use image::RgbaImage;

use crate::Level;

/// ONI's material icons, `<material>.png` (placeholder art, not in git).
const ICON_DIR: &str = "oni/icons";

pub const BITS_PER_TILE: u32 = 8;
/// Most a slot can hold: 10 tiles.
pub const SLOT_CAPACITY: u32 = 10 * BITS_PER_TILE;

/// Size of a material icon, its gap to the next, and the margin to the screen edge, in logical pixels.
const ICON_SIZE: f32 = 48.0;
const ICON_GAP: f32 = 8.0;
const MARGIN: f32 = 12.0;
/// The black panel behind the icons: its padding around them and corner radius.
const PANEL_PADDING: f32 = 8.0;
const PANEL_RADIUS: f32 = 6.0;
/// Width of the white outline following the icon's shape in a full slot, in logical pixels.
const FULL_OUTLINE: f32 = 2.0;
/// Icon pixels at least this opaque count as its shape; ONI's drop shadow under
/// each icon is fainter and gets no outline.
const SHAPE_ALPHA: u8 = 128;
const TILES_FONT_SIZE: f32 = 28.0;
/// Metrics of Bevy's default font (Fira Mono) as fractions of the font size:
/// the height of a digit, and how far the baseline sits above the bottom of
/// the text's line (line height 1.2).
const DIGIT_HEIGHT: f32 = 0.70;
const BASELINE: f32 = 0.26;
/// The bit grid's columns (filled from the lower left) and the gap between squares;
/// the full grid is as tall as a digit and stands on the same baseline.
const BIT_COLUMNS: u32 = 2;
const BIT_GAP: f32 = 1.0;
const BIT_COLOR: Color = Color::WHITE;

/// Amounts held, always counted in bits; whole tiles only appear when shown.
#[derive(Resource)]
pub struct Inventory {
    /// Bits of each material, indexed like [`crate::world::Biomes::materials`].
    bits: Vec<u32>,
}

/// An amount as shown: whole tiles and the bits left over, or a full slot.
#[derive(Debug, PartialEq)]
enum Quantity {
    Partial { tiles: u32, bits: u32 },
    Full,
}

impl Quantity {
    fn of(bits: u32) -> Self {
        if bits >= SLOT_CAPACITY {
            Quantity::Full
        } else {
            Quantity::Partial { tiles: bits / BITS_PER_TILE, bits: bits % BITS_PER_TILE }
        }
    }
}

impl Inventory {
    pub fn new(materials: usize) -> Self {
        Self { bits: vec![0; materials] }
    }

    /// Add `bits` of `material`, up to a full slot; anything beyond is lost.
    pub fn add(&mut self, material: u8, bits: u32) {
        let held = &mut self.bits[material as usize];
        *held = (*held + bits).min(SLOT_CAPACITY);
    }

    /// The materials held, with their bits, in material order.
    fn held(&self) -> impl Iterator<Item = (u8, u32)> + '_ {
        (0..)
            .zip(self.bits.iter().copied())
            .filter(|&(_, bits)| bits > 0)
    }
}

/// The panel holding the row of material icons.
#[derive(Component)]
pub struct InventoryBar;

/// A material's icon, and the same with a white outline for a full slot, with
/// their sizes on screen (fitted into a slot, keeping their shape).
struct MaterialIcon {
    plain: Handle<Image>,
    plain_size: Vec2,
    full: Handle<Image>,
    full_size: Vec2,
}

/// Icons indexed like [`crate::world::Biomes::materials`].
#[derive(Resource)]
pub struct MaterialIcons(Vec<MaterialIcon>);

pub fn spawn_inventory_bar(
    mut commands: Commands,
    level: Res<Level>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut add = |image: RgbaImage| {
        images.add(Image::from_dynamic(image.into(), true, RenderAssetUsages::RENDER_WORLD))
    };
    let icons = level
        .biomes
        .materials
        .iter()
        .map(|name| {
            let path = format!("{}/asset/{ICON_DIR}/{name}.png", env!("CARGO_MANIFEST_DIR"));
            let icon = image::open(&path)
                .unwrap_or_else(|e| panic!("cannot load {path}: {e}"))
                .to_rgba8();
            let scale = ICON_SIZE / icon.width().max(icon.height()) as f32;
            let full = outlined(&icon, FULL_OUTLINE / scale);
            let size = |image: &RgbaImage| Vec2::new(image.width() as f32, image.height() as f32) * scale;
            MaterialIcon {
                plain_size: size(&icon),
                full_size: size(&full),
                plain: add(icon),
                full: add(full),
            }
        })
        .collect();
    commands.insert_resource(MaterialIcons(icons));
    commands.spawn((
        InventoryBar,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            right: Val::Px(MARGIN),
            bottom: Val::Px(MARGIN),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(ICON_GAP),
            padding: UiRect::all(Val::Px(PANEL_PADDING)),
            border_radius: BorderRadius::all(Val::Px(PANEL_RADIUS)),
            ..default()
        },
        BackgroundColor(Color::BLACK),
    ));
}

/// Rebuild the icon row whenever the inventory changes; the panel only shows
/// while something is held.
pub fn show_inventory(
    mut commands: Commands,
    inventory: Res<Inventory>,
    icons: Res<MaterialIcons>,
    bar: Single<(Entity, &mut Node), With<InventoryBar>>,
) {
    if !inventory.is_changed() {
        return;
    }
    let (bar, mut node) = bar.into_inner();
    node.display = if inventory.held().next().is_some() { Display::Flex } else { Display::None };
    commands.entity(bar).despawn_children().with_children(|bar| {
        for (material, bits) in inventory.held() {
            let quantity = Quantity::of(bits);
            let icon = &icons.0[material as usize];
            let (image, size) = match quantity {
                Quantity::Full => (&icon.full, icon.full_size),
                Quantity::Partial { .. } => (&icon.plain, icon.plain_size),
            };
            // Every slot has room for the outline, so icons stay put when one fills up.
            let slot_size = Val::Px(ICON_SIZE + 2.0 * FULL_OUTLINE);
            bar.spawn(Node {
                width: slot_size,
                height: slot_size,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|slot| {
                slot.spawn((
                    Node { width: Val::Px(size.x), height: Val::Px(size.y), ..default() },
                    ImageNode::new(image.clone()),
                ));
                if let Quantity::Partial { tiles, bits } = quantity {
                    spawn_amount(slot, tiles, bits);
                }
            });
        }
    });
}

/// `icon` with a white outline `width` pixels wide around its shape (its pixels
/// at least [`SHAPE_ALPHA`] opaque, drawn over the outline; the fainter shadow
/// goes under it), on a canvas grown to fit the outline.
fn outlined(icon: &RgbaImage, width: f32) -> RgbaImage {
    let margin = width.ceil() as i32 + 1;
    let (w, h) = (icon.width() as i32, icon.height() as i32);
    let in_shape = |x: i32, y: i32| {
        (0..w).contains(&x) && (0..h).contains(&y) && icon.get_pixel(x as u32, y as u32)[3] >= SHAPE_ALPHA
    };
    let mut out = RgbaImage::new((w + 2 * margin) as u32, (h + 2 * margin) as u32);
    for (ox, oy, pixel) in out.enumerate_pixels_mut() {
        let (x, y) = (ox as i32 - margin, oy as i32 - margin);
        // Distance to the nearest shape pixel, antialiased over the outline's outer pixel.
        let nearest = (-margin..=margin)
            .flat_map(|dy| (-margin..=margin).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| in_shape(x + dx, y + dy))
            .map(|(dx, dy)| ((dx * dx + dy * dy) as f32).sqrt())
            .fold(f32::INFINITY, f32::min);
        let coverage = (width + 0.5 - nearest).clamp(0.0, 1.0);
        let outline = image::Rgba([255, 255, 255, (coverage * 255.0) as u8]);
        // The icon's shape on top of the outline, its faint shadow below it.
        let icon_pixel = if (0..w).contains(&x) && (0..h).contains(&y) {
            *icon.get_pixel(x as u32, y as u32)
        } else {
            image::Rgba([0, 0, 0, 0])
        };
        *pixel = if icon_pixel[3] >= SHAPE_ALPHA {
            over(icon_pixel, outline)
        } else {
            over(outline, icon_pixel)
        };
    }
    out
}

/// Straight-alpha "source over" compositing of `top` onto `bottom`.
fn over(top: image::Rgba<u8>, bottom: image::Rgba<u8>) -> image::Rgba<u8> {
    let (ta, ba) = (top[3] as f32 / 255.0, bottom[3] as f32 / 255.0);
    let a = ta + ba * (1.0 - ta);
    if a == 0.0 {
        return image::Rgba([0, 0, 0, 0]);
    }
    let channel =
        |i: usize| ((top[i] as f32 * ta + bottom[i] as f32 * ba * (1.0 - ta)) / a).round() as u8;
    image::Rgba([channel(0), channel(1), channel(2), (a * 255.0).round() as u8])
}

/// The tile count (if any) and the bit squares, in the icon's lower right corner.
fn spawn_amount(icon: &mut ChildSpawnerCommands, tiles: u32, bits: u32) {
    icon.spawn(Node {
        position_type: PositionType::Absolute,
        right: Val::Px(3.0),
        // The text line reaches below the baseline; let that part hang over the edge.
        bottom: Val::Px(3.0 - BASELINE * TILES_FONT_SIZE),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::FlexEnd,
        column_gap: Val::Px(3.0),
        ..default()
    })
    .with_children(|amount| {
        if tiles > 0 {
            amount.spawn((
                Text::new(tiles.to_string()),
                TextFont::from_font_size(TILES_FONT_SIZE),
                TextColor(Color::WHITE),
                TextShadow { offset: Vec2::splat(1.5), color: Color::BLACK },
            ));
        }
        let rows = BITS_PER_TILE / BIT_COLUMNS;
        let height = DIGIT_HEIGHT * TILES_FONT_SIZE;
        let size = (height - (rows - 1) as f32 * BIT_GAP) / rows as f32;
        let step = size + BIT_GAP;
        amount
            .spawn(Node {
                width: Val::Px(BIT_COLUMNS as f32 * step - BIT_GAP),
                height: Val::Px(height),
                margin: UiRect::bottom(Val::Px(BASELINE * TILES_FONT_SIZE)),
                ..default()
            })
            .with_children(|grid| {
                for i in 0..bits {
                    let (column, row) = (i % BIT_COLUMNS, i / BIT_COLUMNS);
                    grid.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(column as f32 * step),
                            bottom: Val::Px(row as f32 * step),
                            width: Val::Px(size),
                            height: Val::Px(size),
                            ..default()
                        },
                        BackgroundColor(BIT_COLOR),
                        BoxShadow::new(Color::BLACK, Val::Px(1.0), Val::Px(1.0), Val::ZERO, Val::Px(1.0)),
                    ));
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_tiles_and_bits_up_to_a_full_slot() {
        assert_eq!(Quantity::of(5), Quantity::Partial { tiles: 0, bits: 5 });
        assert_eq!(Quantity::of(8), Quantity::Partial { tiles: 1, bits: 0 });
        assert_eq!(Quantity::of(79), Quantity::Partial { tiles: 9, bits: 7 });
        assert_eq!(Quantity::of(80), Quantity::Full);
    }

    #[test]
    fn outlines_the_shape_but_not_the_faint_shadow() {
        // A solid pixel, and a faint shadow pixel well apart from it.
        let mut icon = RgbaImage::new(9, 1);
        icon.put_pixel(1, 0, image::Rgba([10, 20, 30, 255]));
        icon.put_pixel(8, 0, image::Rgba([0, 0, 0, 77]));
        let out = outlined(&icon, 2.0);
        let margin = (out.width() - icon.width()) as i32 / 2;
        let pixel = |x: i32, y: i32| *out.get_pixel((x + margin) as u32, (y + margin) as u32);
        let alpha = |x: i32, y: i32| pixel(x, y)[3];
        assert_eq!(pixel(1, 0), image::Rgba([10, 20, 30, 255]), "the icon on top");
        assert_eq!(alpha(2, 0), 255, "outline to the right");
        assert_eq!(alpha(1, -1), 255, "and above");
        assert_eq!(alpha(3, 0), 127, "its antialiased edge, 2 px out");
        assert_eq!(alpha(4, 0), 0, "nothing beyond");
        assert_eq!(alpha(7, 0), 0, "no outline around the shadow");
        assert_eq!(alpha(8, 0), 77, "the shadow is kept");
    }

    #[test]
    fn holds_at_most_ten_tiles_per_material() {
        let mut inventory = Inventory::new(4);
        assert_eq!(inventory.held().count(), 0);
        inventory.add(2, BITS_PER_TILE);
        inventory.add(0, 1);
        inventory.add(2, 4);
        assert_eq!(inventory.held().collect::<Vec<_>>(), [(0, 1), (2, 12)]);
        for _ in 0..10 {
            inventory.add(2, BITS_PER_TILE);
        }
        assert_eq!(inventory.held().last(), Some((2, SLOT_CAPACITY)));
    }
}
