# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A 2D side-view colony/simulation game (inspired by Oxygen Not Included and Factorio) built with Rust + Bevy 0.20. The game design lives in `info/concept.md`; read it before making design-level changes. Its key constraints: everything is visible (no hidden layers like gas or temperature), a small family of characters with one directly controlled, a focus on animals/plants over buildings, small levels (about 2x3 screens), and flowing liquids on a halftile grid.

Remote: `https://github.com/illich/critteri.git`, branch `main`.

## Commands

- `cargo run`: build and launch. The window is borderless fullscreen; Esc quits, A/D (or arrows) walk the player (hold S/down to walk off cliffs higher than 3 tiles), holding the right mouse button digs the hovered tile, holding F fills the hovered empty tile, and the camera follows the player.
- `cargo build`: the first build compiles Bevy and takes several minutes; later builds are incremental.
- `cargo test`: run tests (world generation in `src/world.rs`, climbing in `src/player.rs`; `World::from_ascii` builds small hand-drawn test levels). Run one test with `cargo test <name>`.
- `cargo clippy`: lint.

The dev profile sets `opt-level = 1` for this crate and `3` for dependencies; Bevy is too slow in unoptimized debug builds.

## Architecture

- `src/world.rs`: engine-independent world data (the tile grid as a flat `Vec`, indexed `y * WIDTH + x`, with `y = 0` as the bottom row). **Keep Bevy out of this module**: the simulation (tiles, halftiles, liquids) is meant to move into its own crate later and be tested headless. Grid cells should stay plain arrays, not ECS entities.
- `asset/biomes.ron`: biome definitions (background texture plus the materials it may contain). The material list is derived from it, and only textures it names are loaded. The generator splits the level into 15-20 Voronoi sections with a random biome each; solid tiles only use their section's materials.
- `asset/oni/`: placeholder textures extracted from the user's Oxygen Not Included install (gitignored, never publish). The extraction scripts and their Python venv are in `.tools/` (also gitignored).
- `src/player.rs`: the player duplicant (`PlayerPlugin`): a 0.8x2.7-tile collision box with gravity and walking. Movement rules (each a small pure function with tests on `World::from_ascii` levels): `step` does instant 1-tile steps up or down needing only 3 free tiles at the destination (if the body must move fully into the next column, `Motion::StepAcross` walks it there at normal speed); `plan_climb` does animated 2-3 tile climbs (the cliff height is its highest solid tile within reach, so overhangs count); `plan_jump` does automatic jumps over 1-2 column gaps (each dropping 2+ tiles, 3 tiles of headroom from the take-off level) onto ground at the same level or one tile up or down (3 tiles of headroom above it). Climbs and jumps share `Leap`; after walking half a tile with `flat_ahead` (3 tiles of flat open ground) the player runs at double speed until the key is released or it turns; `may_walk` stops the player at drops over 3 tiles (measured 3/4 along the body, also while falling) unless S is held, but never blocks backing away. Climb animations carry the dupe a fixed distance inside the sprite (`Climb::shift`), so the sprite is offset during the climb to end exactly on the cliff top; the camera follows `Player::focus`, not the sprite. Sprite scale comes from `SHEET_HEIGHT_PX`/`SHEET_CELL_PX` (one ONI cell is 100 px in the sheets).
- `asset/oni/player/`: the dupe (ONI's Catalina, with a slightly smiling resting mouth) pre-rendered from ONI kanim files into sprite sheets, one folder per animation group with an `animations.ron` index (frame size, grid, fps, and the feet `origin`). Regenerate with `.tools/scripts/render_dupe.py` (`--personality`, `--mouth`, `--resting-mouth`).
- `src/cursor.rs`: tracks the tile under the mouse (`HoveredTile`) and highlights it when solid. `src/mining.rs`: holding the right button digs the hovered solid tile in 0.2 s, showing 6 crack stages generated at startup from ONI's `crackAlpha` mask (its alpha holds cracks by damage level), then swaps the tile's sprite via `TileGraphics`. Holding F fills the hovered empty tile (`World::fill`: a random solid neighbour's material, else one of its biome's), except tiles the player covers (`Player::covers_tile`, including a leap's path).
- `src/main.rs`: the Bevy app, which handles rendering, input and the camera. The camera uses a fixed horizontal scaling so exactly `VISIBLE_TILES_X` (60) tiles span the screen width at any resolution. World units are `TILE_SIZE` (32) per tile.
- Tiles currently render as one sprite each (`TileGraphics` keeps every tile's entity so a changed tile can be redrawn with `TileGraphics::sprite`), which is fine at the current size (180x68). When halftiles and liquids arrive (about 50k frequently changing cells), switch grid rendering to a texture or tilemap.
- Characters, critters and plants are intended to be ECS entities.

## Bevy version notes

Bevy 0.20 APIs differ from older tutorials. Examples in use: `Camera2d`, `Projection::Orthographic(OrthographicProjection { .. })` with `bevy::camera::ScalingMode`, `Sprite::from_color`, `Single<..>` queries, `MessageWriter<AppExit>` (not `EventWriter`), and `bevy::window::WindowMode` (not in the prelude). When unsure, check the crate source in `~/.cargo/registry/src/*/bevy_*-0.20*`.

## Environment

The desktop sets `SSH_ASKPASS=/usr/bin/ksshaskpass` with `SSH_ASKPASS_REQUIRE=prefer`, which intercepts git credential prompts. Credentials are stored via `credential.helper store`, so `git push` works non-interactively. If authentication ever needs re-entering, run git with `env -u SSH_ASKPASS -u SSH_ASKPASS_REQUIRE`.

Never write outside the project directory (/a/work/colony) and its subdirs.