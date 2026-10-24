//! Engine-independent world data. Keep Bevy out of this module so the
//! simulation can later move into its own crate and be tested headless.

/// Level size in tiles: 3 screens wide, 2 screens tall (60 x ~34 tiles per screen).
pub const WIDTH: usize = 180;
pub const HEIGHT: usize = 68;

#[derive(Clone, Copy, Debug)]
pub struct Tile {
    /// Placeholder until real materials exist: an sRGB color.
    pub color: [f32; 3],
}

pub struct World {
    tiles: Vec<Tile>,
}

impl World {
    pub fn random(seed: u64) -> Self {
        let mut rng = XorShift(seed.max(1));
        let tiles = (0..WIDTH * HEIGHT)
            .map(|_| Tile {
                color: [rng.next_f32(), rng.next_f32(), rng.next_f32()],
            })
            .collect();
        Self { tiles }
    }

    /// `y = 0` is the bottom row.
    pub fn tile(&self, x: usize, y: usize) -> &Tile {
        &self.tiles[y * WIDTH + x]
    }
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
}
