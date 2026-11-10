//! A small seeded random source (SplitMix64). The layout needs a few random numbers for starting
//! places and for pushing apart nodes that sit on the same spot; taking them from a seed makes the
//! same graph and seed give the same picture on every machine.

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)`, from the top 24 bits so every value is exact in `f32`.
    pub(crate) fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A point inside a disc of the given radius, evenly spread over its area.
    pub(crate) fn in_disc(&mut self, radius: f32) -> (f32, f32) {
        let angle = self.unit() * std::f32::consts::TAU;
        let r = radius * self.unit().sqrt();
        (r * angle.cos(), r * angle.sin())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!(a.next_u64(), b.next_u64());
        assert_eq!(a.unit(), b.unit());
    }

    #[test]
    fn unit_numbers_stay_below_one() {
        let mut rng = Rng::new(1);
        assert!((0..10_000).all(|_| (0.0..1.0).contains(&rng.unit())));
    }
}
