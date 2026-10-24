# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A 2D side-view colony/simulation game (inspired by Oxygen Not Included and Factorio) built with Rust + Bevy 0.20. The game design lives in `info/concept.md`; read it before making design-level changes. Its key constraints: everything is visible (no hidden layers like gas or temperature), a small family of characters with one directly controlled, a focus on animals/plants over buildings, small levels (about 2x3 screens), and flowing liquids on a halftile grid.

Remote: `https://github.com/illich/critteri.git`, branch `main`.

## Commands

- `cargo run`: build and launch. The window is borderless fullscreen; Esc quits, WASD/arrows scroll.
- `cargo build`: the first build compiles Bevy and takes several minutes; later builds are incremental.
- `cargo test`: run tests (world generation tests live in `src/world.rs`). Run one test with `cargo test <name>`.
- `cargo clippy`: lint.

The dev profile sets `opt-level = 1` for this crate and `3` for dependencies; Bevy is too slow in unoptimized debug builds.

## Architecture

- `src/world.rs`: engine-independent world data (the tile grid as a flat `Vec`, indexed `y * WIDTH + x`, with `y = 0` as the bottom row). **Keep Bevy out of this module**: the simulation (tiles, halftiles, liquids) is meant to move into its own crate later and be tested headless. Grid cells should stay plain arrays, not ECS entities.
- `asset/biomes.ron`: biome definitions (background texture plus the materials it may contain). The material list is derived from it, and only textures it names are loaded. The generator splits the level into 15-20 Voronoi sections with a random biome each; solid tiles only use their section's materials.
- `asset/oni/`: placeholder textures extracted from the user's Oxygen Not Included install (gitignored, never publish). The extraction scripts and their Python venv are in `.tools/` (also gitignored).
- `src/main.rs`: the Bevy app, which handles rendering, input and the camera. The camera uses a fixed horizontal scaling so exactly `VISIBLE_TILES_X` (60) tiles span the screen width at any resolution. World units are `TILE_SIZE` (32) per tile.
- Tiles currently render as one sprite each, which is fine at the current size (180x68). When halftiles and liquids arrive (about 50k frequently changing cells), switch grid rendering to a texture or tilemap.
- Characters, critters and plants are intended to be ECS entities.

## Bevy version notes

Bevy 0.20 APIs differ from older tutorials. Examples in use: `Camera2d`, `Projection::Orthographic(OrthographicProjection { .. })` with `bevy::camera::ScalingMode`, `Sprite::from_color`, `Single<..>` queries, `MessageWriter<AppExit>` (not `EventWriter`), and `bevy::window::WindowMode` (not in the prelude). When unsure, check the crate source in `~/.cargo/registry/src/*/bevy_*-0.20*`.

## Environment

The desktop sets `SSH_ASKPASS=/usr/bin/ksshaskpass` with `SSH_ASKPASS_REQUIRE=prefer`, which intercepts git credential prompts. Credentials are stored via `credential.helper store`, so `git push` works non-interactively. If authentication ever needs re-entering, run git with `env -u SSH_ASKPASS -u SSH_ASKPASS_REQUIRE`.

Never write outside the project directory (/a/work/colony) and its subdirs.