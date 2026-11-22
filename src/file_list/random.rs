pub(crate) struct SimpleRng(u64);

impl SimpleRng {
    pub(crate) fn new() -> Self {
        let mut buf = [0u8; 8];
        let seed = if getrandom::fill(&mut buf).is_ok() {
            u64::from_ne_bytes(buf)
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
        };
        Self(seed | 1)
    }

    pub(crate) fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(crate) fn next_usize(&mut self, bound: usize) -> usize {
        let s = bound as u64;
        let mut m = self.step() as u128 * s as u128;
        let mut l = m as u64;
        if l < s {
            let t = s.wrapping_neg() % s;
            while l < t {
                m = self.step() as u128 * s as u128;
                l = m as u64;
            }
        }
        (m >> 64) as usize
    }
}
