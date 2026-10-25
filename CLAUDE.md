# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A 2D side-view colony/simulation game (inspired by Oxygen Not Included and Factorio) built with Rust + Bevy 0.20. The game design lives in `info/concept.md`; read it before making design-level changes. Its key constraints: everything is visible (no hidden layers like gas or temperature), a small family of characters with one directly controlled, a focus on animals/plants over buildings, small levels (about 2x3 screens), and flowing liquids on a halftile grid.

Remote: `https://github.com/illich/critteri.git`, branch `main`.

## Commands

- `cargo run`: build and launch (borderless fullscreen).
- `cargo build`: the first build compiles Bevy and takes several minutes; later builds are incremental.
- `cargo test`: run all tests; `cargo test <name>` runs matching tests. Tests live next to the code in `src/world.rs` and `src/player.rs`, and use `World::from_ascii` to build small hand-drawn levels (`#` solid, first row is the top).
- `cargo clippy --all-targets`: lint (kept warning-free; Bevy systems with many parameters use `#[expect(clippy::too_many_arguments, ...)]`).

The dev profile sets `opt-level = 1` for this crate and `3` for dependencies; Bevy is too slow in unoptimized debug builds.

Controls: A/D or arrows walk (hold S/down to walk off drops over 3 tiles); W/S climb ladders; holding the right mouse button digs the hovered solid tile or removes a building; holding F fills the hovered empty tile; holding R places a ladder; Esc quits. The camera follows the player.

## Architecture

- `src/world.rs`: engine-independent world data. **Keep Bevy out of this module**: the simulation (tiles, halftiles, liquids) is meant to move into its own crate later and be tested headless. Grids are flat `Vec`s indexed `y * WIDTH + x` with `y = 0` the bottom row (not ECS entities): terrain (`Tile::Empty` / `Tile::Solid { material }`), a biome per tile, and a building layer (`Building::Ladder`) that only empty tiles can hold. `is_solid` blocks movement; `is_floor(x, y)` says whether a body standing at level `y` is held up (solid below, or a ladder below or in the tile itself, as in ONI) and is what all ground checks use. `World::fill` picks a random solid neighbour's material (else one of the tile's biome's) and replaces any building.
- `asset/biomes.ron`: biome definitions (background texture plus the materials it may contain). The material list is derived from it, and only textures it names are loaded. The generator splits the level into 15-20 Voronoi sections with a random biome each; solid tiles only use their section's materials.
- `src/main.rs`: the Bevy app: generates the `Level` resource (world + biomes) before startup, spawns tiles and the camera, and orders the per-frame systems: `PlayerSystems` first, then `follow_player` → `highlight_hovered_tile` → `dig` → `fill` → `place_ladder`. The camera uses fixed horizontal scaling so exactly `VISIBLE_TILES_X` (60) tiles span the screen; world units are `TILE_SIZE` (32) per tile. Tiles render as one sprite each; `TileGraphics` keeps every tile's entity so a changed tile is redrawn with `TileGraphics::sprite` (switch to a texture or tilemap once halftiles and liquids arrive).
- `src/player.rs` (`PlayerPlugin`): the player duplicant, a 0.8x2.7-tile collision box. Its `Motion` is idle/walking, `StepAcross`, a `Leap` (climb or jump), or `Ladder`. Movement rules are small pure functions, each tested:
  - `step`: instant 1-tile steps up or down needing only 3 free tiles at the destination; if the body must move fully into the next column, `StepAcross` walks it there at the current speed.
  - `plan_climb`: animated 2-3 tile climbs (the cliff height is its highest solid tile in reach, so overhangs count).
  - `plan_jump`: automatic jumps over 1-2 column gaps (each dropping 2+ tiles, 3 tiles of headroom from the take-off level) onto ground at the same level or one tile up or down.
  - `may_walk` / `drop_below`: stop at drops over 3 tiles (measured 3/4 along the body, also mid-fall) unless S is held, but never block backing away. `flat_ahead`: start running (double speed) after half a tile of walking with 3 tiles of flat open ground ahead; running lasts while the key is held.
  - `ladder_step` / `hanging_on_ladder`: ladder climbing one whole level at a time, gravity off; S on the lowest ladder tile lets go. `ladder_reach`: W from the ground also grabs a ladder whose bottom is 1-2 tiles above the feet.
  - Leap animations carry the dupe a fixed distance inside the sprite (`Leap::shift`), so the sprite is offset during the leap to end exactly at the target; the camera follows `Player::focus`, not the sprite. `Player::covers_tile` (body plus any leap/ladder path) keeps F from filling tiles the player needs.
  - Sprite scale comes from `SHEET_HEIGHT_PX` and `SHEET_CELL_PX` (one ONI cell is 100 px in the sheets, 1.75 of our tiles); looping animations are paced from movement speed so feet don't slide.
- `src/cursor.rs`: tracks the tile under the mouse (`HoveredTile`) and highlights it when solid or built on. `src/mining.rs`: right button digs a solid tile in 0.2 s with 6 crack stages generated at startup from ONI's `crackAlpha` mask (its alpha holds cracks by damage level), or removes a building at once; F fills. `src/building.rs`: ladder graphics, R to place, and `BuildingSprites` (each building's sprite entity, despawned on removal).
- Characters, critters and plants are intended to be ECS entities.

## Placeholder assets

`asset/oni/` holds graphics extracted from the user's Oxygen Not Included install (`/m/steam/steamapps/common/OxygenNotIncluded`). It is gitignored and must never be published. The tools are in `.tools/` (also gitignored): a Python venv with UnityPy and dnfile, and scripts in `.tools/scripts/`:
- `export_tiles.py <ONI data dir> asset/oni/tiles`: material and biome background textures, the crack mask, the ladder.
- `kanim.py`: reader for Klei kanim build/anim files.
- `render_dupe.py`: renders the player (ONI's Catalina, slightly smiling resting mouth) from kanim into sprite sheets in `asset/oni/player/{loco,pole,swim,idle}`, each with an `animations.ron` index (frame size, grid, fps, and the feet `origin`). Options: `--personality`, `--mouth`, `--resting-mouth`. Delete the junk animation named `" crap"` from the swim set after re-rendering.

## Bevy version notes

Bevy 0.20 APIs differ from older tutorials. Examples in use: `Camera2d`, `Projection::Orthographic(OrthographicProjection { .. })` with `bevy::camera::ScalingMode`, `Sprite::from_color`, `Single<..>` queries, `MessageWriter<AppExit>` (not `EventWriter`), `bevy::sprite::Anchor` as a component, and `bevy::window::WindowMode` (not in the prelude). When unsure, check the crate source in `~/.cargo/registry/src/*/bevy_*-0.20*`.

## Environment

- Visual checks: run the game in the background and capture the screen with `spectacle -b -n -f -o <file>` (writing into the project, e.g. `.tools/out/`). Sending keys with `xdotool` triggers a KDE "Remote Control" permission prompt, so don't; ask the user to try input-driven behaviour.
- The desktop sets `SSH_ASKPASS=/usr/bin/ksshaskpass` with `SSH_ASKPASS_REQUIRE=prefer`, which intercepts git credential prompts. Credentials are stored via `credential.helper store`, so `git push` works non-interactively. If authentication ever needs re-entering, run git with `env -u SSH_ASKPASS -u SSH_ASKPASS_REQUIRE`.
- Never write outside the project directory (/a/work/colony) and its subdirs.
