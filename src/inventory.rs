//! The player's inventory: units of each material dug out, shown as a row of
//! material icons with their amounts in the lower right corner of the screen.

use bevy::prelude::*;

use crate::{TileGraphics, TEXTURE_PX_PER_TILE};

/// Units of material gained by digging out one tile.
pub const UNITS_PER_TILE: u32 = 4;
/// Size of a material icon, its gap to the next, and the margin to the screen edge, in logical pixels.
const ICON_SIZE: f32 = 48.0;
const ICON_GAP: f32 = 8.0;
const MARGIN: f32 = 12.0;
const AMOUNT_FONT_SIZE: f32 = 16.0;

/// Amount held of each material, indexed like [`crate::world::Biomes::materials`].
#[derive(Resource)]
pub struct Inventory {
    amounts: Vec<u32>,
}

impl Inventory {
    pub fn new(materials: usize) -> Self {
        Self { amounts: vec![0; materials] }
    }

    pub fn add(&mut self, material: u8, amount: u32) {
        self.amounts[material as usize] += amount;
    }

    /// The materials held, with their amounts, in material order.
    pub fn held(&self) -> impl Iterator<Item = (u8, u32)> + '_ {
        (0..)
            .zip(self.amounts.iter().copied())
            .filter(|&(_, amount)| amount > 0)
    }
}

/// The row of material icons.
#[derive(Component)]
pub struct InventoryBar;

pub fn spawn_inventory_bar(mut commands: Commands) {
    commands.spawn((
        InventoryBar,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(MARGIN),
            bottom: Val::Px(MARGIN),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(ICON_GAP),
            ..default()
        },
    ));
}

/// Rebuild the icon row whenever the inventory changes.
pub fn show_inventory(
    mut commands: Commands,
    inventory: Res<Inventory>,
    graphics: Res<TileGraphics>,
    bar: Single<Entity, With<InventoryBar>>,
) {
    if !inventory.is_changed() {
        return;
    }
    let px = TEXTURE_PX_PER_TILE as f32;
    commands.entity(*bar).despawn_children().with_children(|bar| {
        for (material, amount) in inventory.held() {
            bar.spawn((
                Node { width: Val::Px(ICON_SIZE), height: Val::Px(ICON_SIZE), ..default() },
                ImageNode {
                    image: graphics.materials[material as usize].clone(),
                    // One tile's worth of the material texture.
                    rect: Some(Rect::new(0.0, 0.0, px, px)),
                    ..default()
                },
                children![(
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(3.0),
                        bottom: Val::Px(1.0),
                        ..default()
                    },
                    Text::new(amount.to_string()),
                    TextFont::from_font_size(AMOUNT_FONT_SIZE),
                    TextColor(Color::WHITE),
                    TextShadow { offset: Vec2::splat(1.5), color: Color::BLACK },
                )],
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_materials_held() {
        let mut inventory = Inventory::new(4);
        assert_eq!(inventory.held().count(), 0);
        inventory.add(2, UNITS_PER_TILE);
        inventory.add(0, 1);
        inventory.add(2, UNITS_PER_TILE);
        assert_eq!(inventory.held().collect::<Vec<_>>(), [(0, 1), (2, 8)]);
    }
}
