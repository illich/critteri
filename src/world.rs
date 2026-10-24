//! Engine-independent world data. Keep Bevy out of this module so the
//! simulation can later move into its own crate and be tested headless.

use serde::Deserialize;

/// Level size in tiles: 3 screens wide, 2 screens tall (60 x ~34 tiles per screen).
pub const WIDTH: usize = 180;
pub const HEIGHT: usize = 68;

/// Range of biome sections the level is split into.
const SECTIONS: std::ops::RangeInclusive<usize> = 15..=20;
/// Number of material regions scattered over the level.
const REGIONS: usize = 160;
/// Share of regions that are empty (caves) instead of solid.
const EMPTY_SHARE: f32 = 0.35;

/// Biome definitions as written in the data file.
#[derive(Deserialize)]
struct BiomeFile {
    biomes: Vec<BiomeDef>,
}

#[derive(Deserialize)]
struct BiomeDef {
    name: String,
    background: String,
    materials: Vec<String>,
}

pub struct Biome {
    /// Name of the background texture shown behind empty tiles.
    pub background: String,
    /// Indices into [`Biomes::materials`].
    pub materials: Vec<u8>,
}

/// All biomes, plus the materials they use (each listed once).
pub struct Biomes {
    pub materials: Vec<String>,
    pub biomes: Vec<Biome>,
}

impl Biomes {
    pub fn from_ron(source: &str) -> Result<Self, String> {
        let file: BiomeFile = ron::from_str(source).map_err(|e| e.to_string())?;
        if file.biomes.is_empty() {
            return Err("no biomes defined".into());
        }

        let mut materials: Vec<String> = Vec::new();
        let mut biomes = Vec::with_capacity(file.biomes.len());
        for def in file.biomes {
            if def.materials.is_empty() {
                return Err(format!("biome {} has no materials", def.name));
            }
            let ids = def
                .materials
                .into_iter()
                .map(|name| match materials.iter().position(|m| *m == name) {
                    Some(id) => id as u8,
                    None => {
                        materials.push(name);
                        (materials.len() - 1) as u8
                    }
                })
                .collect();
            biomes.push(Biome { background: def.background, materials: ids });
        }
        Ok(Self { materials, biomes })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Tile {
    Empty,
    /// Index into [`Biomes::materials`].
    Solid { material: u8 },
}

pub struct World {
    tiles: Vec<Tile>,
    /// Index into [`Biomes::biomes`] for every tile.
    biomes: Vec<u8>,
    /// Randomness for changes made while playing, such as the material of filled tiles.
    rng: XorShift,
}

impl World {
    /// Placeholder generator: the level is split into Voronoi sections, each
    /// a random biome. A finer Voronoi of regions decides caves and materials;
    /// a region crossing a section border takes that section's materials.
    pub fn generate(seed: u64, biomes: &Biomes) -> Self {
        let mut rng = XorShift(seed.max(1));

        let section_count = SECTIONS.start() + rng.below(SECTIONS.end() - SECTIONS.start() + 1);
        let sections: Vec<_> = (0..section_count)
            .map(|_| (rng.point(), rng.below(biomes.biomes.len()) as u8))
            .collect();
        // Each region holds a random pick, mapped to a material of whichever biome it lies in.
        let regions: Vec<_> = (0..REGIONS)
            .map(|_| {
                let pick = (rng.next_f32() >= EMPTY_SHARE).then(|| rng.next_u64());
                (rng.point(), pick)
            })
            .collect();

        let mut tiles = Vec::with_capacity(WIDTH * HEIGHT);
        let mut tile_biomes = Vec::with_capacity(WIDTH * HEIGHT);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let biome = nearest(&sections, x, y);
                let materials = &biomes.biomes[biome as usize].materials;
                tiles.push(match nearest(&regions, x, y) {
                    None => Tile::Empty,
                    Some(pick) => Tile::Solid {
                        material: materials[(pick % materials.len() as u64) as usize],
                    },
                });
                tile_biomes.push(biome);
            }
        }
        Self { tiles, biomes: tile_biomes, rng }
    }

    /// A level drawn in ASCII (`#` solid, anything else empty), placed in the
    /// bottom-left corner of an otherwise empty level. The first row is the top.
    #[cfg(test)]
    pub fn from_ascii(rows: &[&str]) -> Self {
        let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
        for (i, row) in rows.iter().enumerate() {
            let y = rows.len() - 1 - i;
            for (x, c) in row.chars().enumerate() {
                if c == '#' {
                    tiles[y * WIDTH + x] = Tile::Solid { material: 0 };
                }
            }
        }
        Self { tiles, biomes: vec![0; WIDTH * HEIGHT], rng: XorShift(1) }
    }

    /// `y = 0` is the bottom row.
    pub fn tile(&self, x: usize, y: usize) -> Tile {
        self.tiles[y * WIDTH + x]
    }

    pub fn biome(&self, x: usize, y: usize) -> u8 {
        self.biomes[y * WIDTH + x]
    }

    /// Dig out a solid tile, leaving it empty. Returns whether there was anything to dig.
    pub fn dig(&mut self, x: usize, y: usize) -> bool {
        let tile = &mut self.tiles[y * WIDTH + x];
        let was_solid = matches!(tile, Tile::Solid { .. });
        *tile = Tile::Empty;
        was_solid
    }

    /// Fill an empty tile with the material of one of its solid neighbours (left,
    /// right, below, above) at random, or if it has none, one of its biome's.
    /// Returns whether the tile was empty.
    pub fn fill(&mut self, x: usize, y: usize, biomes: &Biomes) -> bool {
        if matches!(self.tile(x, y), Tile::Solid { .. }) {
            return false;
        }
        let (xi, yi) = (x as i32, y as i32);
        let neighbours: Vec<u8> = [(xi - 1, yi), (xi + 1, yi), (xi, yi - 1), (xi, yi + 1)]
            .into_iter()
            .filter(|&(nx, ny)| nx >= 0 && ny >= 0 && nx < WIDTH as i32 && ny < HEIGHT as i32)
            .filter_map(|(nx, ny)| match self.tile(nx as usize, ny as usize) {
                Tile::Solid { material } => Some(material),
                Tile::Empty => None,
            })
            .collect();
        let choices = if neighbours.is_empty() {
            &biomes.biomes[self.biome(x, y) as usize].materials
        } else {
            &neighbours
        };
        let material = choices[self.rng.below(choices.len())];
        self.tiles[y * WIDTH + x] = Tile::Solid { material };
        true
    }

    /// Whether the tile blocks movement. Everything outside the level does.
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
            return true;
        }
        matches!(self.tile(x as usize, y as usize), Tile::Solid { .. })
    }

    /// The lowest empty tile standing on a solid tile of the level (not its
    /// bottom edge) with `headroom` empty tiles above it (counting itself), in
    /// the column closest to `near_x` that has one.
    pub fn find_standing_spot(&self, near_x: usize, headroom: usize) -> Option<(usize, usize)> {
        let mut columns: Vec<usize> = (0..WIDTH).collect();
        columns.sort_by_key(|x| x.abs_diff(near_x));
        columns.into_iter().find_map(|x| {
            let xi = x as i32;
            (1..HEIGHT)
                .find(|&y| {
                    let yi = y as i32;
                    self.is_solid(xi, yi - 1)
                        && (yi..yi + headroom as i32).all(|ty| !self.is_solid(xi, ty))
                })
                .map(|y| (x, y))
        })
    }
}

/// Value of the site closest to `(x, y)`.
fn nearest<T: Copy>(sites: &[((usize, usize), T)], x: usize, y: usize) -> T {
    sites
        .iter()
        .min_by_key(|((sx, sy), _)| sx.abs_diff(x).pow(2) + sy.abs_diff(y).pow(2))
        .expect("at least one site")
        .1
}

/// Tiny deterministic PRNG, good enough for placeholder content.
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn point(&mut self) -> (usize, usize) {
        (self.below(WIDTH), self.below(HEIGHT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn biomes() -> Biomes {
        Biomes::from_ron(include_str!("../asset/biomes.ron")).unwrap()
    }

    #[test]
    fn biome_file_parses() {
        let biomes = biomes();
        assert_eq!(biomes.biomes.len(), 4);
        assert!(biomes.biomes.iter().all(|b| !b.materials.is_empty()));
    }

    #[test]
    fn solid_tiles_use_their_biome_materials() {
        let biomes = biomes();
        for seed in 1..20 {
            let world = World::generate(seed, &biomes);
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    if let Tile::Solid { material } = world.tile(x, y) {
                        let biome = &biomes.biomes[world.biome(x, y) as usize];
                        assert!(biome.materials.contains(&material), "seed {seed} at ({x}, {y})");
                    }
                }
            }
        }
    }

    #[test]
    fn standing_spot_has_floor_and_headroom() {
        let world = World::generate(42, &biomes());
        let (x, y) = world.find_standing_spot(WIDTH / 2, 3).expect("a spot");
        let (x, y) = (x as i32, y as i32);
        assert!(world.is_solid(x, y - 1));
        assert!((y..y + 3).all(|ty| !world.is_solid(x, ty)));
    }

    #[test]
    fn digging_empties_solid_tiles() {
        let mut world = World::from_ascii(&["#."]);
        assert!(world.dig(0, 0));
        assert!(!world.is_solid(0, 0));
        assert!(!world.dig(1, 0), "nothing to dig in an empty tile");
    }

    #[test]
    fn filling_uses_neighbouring_materials() {
        let biomes = biomes();
        let mut world = World::from_ascii(&["...", "#.#", "..."]);
        // Make the two neighbours different materials.
        world.tiles[WIDTH] = Tile::Solid { material: 3 };
        world.tiles[WIDTH + 2] = Tile::Solid { material: 5 };
        let mut seen = Vec::new();
        for _ in 0..20 {
            world.tiles[WIDTH + 1] = Tile::Empty;
            assert!(world.fill(1, 1, &biomes));
            let Tile::Solid { material } = world.tile(1, 1) else { panic!("should be solid") };
            assert!(material == 3 || material == 5);
            seen.push(material);
        }
        assert!(seen.contains(&3) && seen.contains(&5), "picks among neighbours at random");
        assert!(!world.fill(1, 1, &biomes), "already solid");
    }

    #[test]
    fn filling_without_neighbours_uses_biome_materials() {
        let biomes = biomes();
        let mut world = World::from_ascii(&["..."]);
        assert!(world.fill(10, 10, &biomes));
        let Tile::Solid { material } = world.tile(10, 10) else { panic!("should be solid") };
        assert!(biomes.biomes[0].materials.contains(&material));
    }

    #[test]
    fn rejects_biome_without_materials() {
        let source = r#"(biomes: [(name: "Void", background: "ocean", materials: [])])"#;
        let Err(error) = Biomes::from_ron(source) else { panic!("accepted empty biome") };
        assert!(error.contains("Void"));
    }
}
