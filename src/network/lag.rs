/// Seeded xorshift64 simulation only. Per-connection FIFO delivery.
pub struct Lag {
    enabled: bool,
    state: u64,
}
impl Lag {
    pub fn new(enabled: bool, seed: u64) -> Self {
        Self {
            enabled,
            state: seed.max(1),
        }
    }
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
    /// None means drop; Some is the asynchronous delay in milliseconds.
    pub fn schedule(&mut self) -> Option<u64> {
        if !self.enabled {
            return Some(0);
        }
        let drop = self.next().is_multiple_of(10);
        let delay = 20 + self.next() % 231;
        if drop { None } else { Some(delay) }
    }
}
