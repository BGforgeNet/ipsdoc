//! The ACM bit reader: least significant bit first, zeros past the end, as in the engines.

#[derive(Debug, Clone)]
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// Bits consumed.
    pos: u64,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    /// The next `bits` bits (at most 24), zeros where the input has ended.
    pub(crate) fn read(&mut self, bits: u32) -> u32 {
        debug_assert!(bits <= 24, "{bits} bits do not fit one 32-bit window");
        let start = (self.pos / 8) as usize;
        let mut window = [0u8; 4];
        if let Some(available) = self.data.get(start..) {
            let n = available.len().min(4);
            window[..n].copy_from_slice(&available[..n]);
        }
        let value = (u32::from_le_bytes(window) >> (self.pos % 8)) & ((1 << bits) - 1);
        self.pos += u64::from(bits);
        value
    }

    /// Bits consumed so far.
    pub(crate) fn position(&self) -> u64 {
        self.pos
    }

    /// Bits consumed beyond the input.
    pub(crate) fn overrun(&self) -> u64 {
        self.pos.saturating_sub(self.data.len() as u64 * 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_least_significant_bits_first() {
        // 0b1011_0110, 0b0000_0011
        let mut r = BitReader::new(&[0xB6, 0x03]);
        assert_eq!(r.read(3), 0b110);
        assert_eq!(r.read(4), 0b0110);
        // The top bit of the first byte, then the low bits of the second.
        assert_eq!(r.read(3), 0b111);
        assert_eq!(r.position(), 10);
    }

    #[test]
    fn reads_sixteen_bits_across_three_bytes() {
        let mut r = BitReader::new(&[0xFF, 0x34, 0x12]);
        r.read(4);
        assert_eq!(r.read(16), 0x234F);
    }

    #[test]
    fn reads_zeros_past_the_end_and_counts_them() {
        let mut r = BitReader::new(&[0xFF]);
        assert_eq!(r.read(5), 0b11111);
        assert_eq!(r.overrun(), 0);
        assert_eq!(r.read(7), 0b111);
        assert_eq!(r.overrun(), 4);
        assert_eq!(r.read(16), 0);
        assert_eq!(r.overrun(), 20);
    }

    #[test]
    fn exactly_at_the_end_is_not_past_it() {
        let mut r = BitReader::new(&[0xAB]);
        r.read(8);
        assert_eq!(r.overrun(), 0);
    }

    #[test]
    fn empty_input_reads_zeros() {
        let mut r = BitReader::new(&[]);
        assert_eq!(r.read(16), 0);
        assert_eq!(r.overrun(), 16);
    }
}
