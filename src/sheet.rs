//! Sprite sheets rendered from ONI's kanim animations (placeholder art, not in
//! git), each folder indexed by an `animations.ron`.

use bevy::prelude::*;
use bevy::sprite::Anchor;
use serde::Deserialize;

/// Folder holding the sheet folders, relative to the asset root.
const ONI_DIR: &str = "oni";

#[derive(Deserialize)]
struct SheetIndex {
    animations: Vec<SheetInfo>,
}

#[derive(Deserialize)]
struct SheetInfo {
    name: String,
    file: String,
    frame_size: (u32, u32),
    /// Pixel within a frame where the anchor (a duplicant's feet) is, from the frame's top-left.
    origin: (u32, u32),
    columns: u32,
    rows: u32,
    frames: usize,
    fps: f32,
    /// Pixel of a held tool's muzzle in each frame, for duplicants rendered with a tool.
    #[serde(default)]
    tips: Vec<(f32, f32)>,
}

pub struct Clip {
    pub image: Handle<Image>,
    pub layout: Handle<TextureAtlasLayout>,
    pub anchor: Anchor,
    pub frames: usize,
    pub fps: f32,
    /// How far a frame reaches right of the origin, in sheet pixels.
    pub reach: f32,
    /// Each frame's tool muzzle relative to the origin, in sheet pixels (y up);
    /// empty without a tool.
    pub tips: Vec<Vec2>,
}

impl Clip {
    pub fn duration(&self) -> f32 {
        self.frames as f32 / self.fps
    }
}

/// Load the animation `name` from sheet folder `folder` (under `asset/oni`).
pub fn load_clip(
    folder: &str,
    name: &str,
    assets: &AssetServer,
    layouts: &mut Assets<TextureAtlasLayout>,
) -> Clip {
    let index_path =
        format!("{}/asset/{ONI_DIR}/{folder}/animations.ron", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&index_path)
        .unwrap_or_else(|e| panic!("cannot read {index_path}: {e}"));
    let index: SheetIndex =
        ron::from_str(&source).unwrap_or_else(|e| panic!("invalid {index_path}: {e}"));
    let info = index
        .animations
        .into_iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("no animation {name} in {index_path}"));

    let (w, h) = info.frame_size;
    let (ox, oy) = info.origin;
    Clip {
        image: assets.load(format!("{ONI_DIR}/{folder}/{}", info.file)),
        layout: layouts.add(TextureAtlasLayout::from_grid(
            UVec2::new(w, h),
            info.columns,
            info.rows,
            None,
            None,
        )),
        // Sheet origin (y down from the top-left) as a sprite anchor (y up from the centre).
        anchor: Anchor(Vec2::new(
            ox as f32 / w as f32 - 0.5,
            0.5 - oy as f32 / h as f32,
        )),
        frames: info.frames,
        fps: info.fps,
        reach: (w - ox) as f32,
        tips: info
            .tips
            .iter()
            .map(|&(x, y)| Vec2::new(x - ox as f32, oy as f32 - y))
            .collect(),
    }
}
