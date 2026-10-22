//! The ACM bit writer, the reader's mirror: least significant bit first.

#[derive(Debug, Clone, Default)]
pub(crate) struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    /// Bits held in `acc`, always below 8 between calls.
    pending: u32,
}

impl BitWriter {
    /// Appends the low `bits` bits (at most 32) of `value`.
    pub(crate) fn put(&mut self, value: u32, bits: u32) {
        debug_assert!(bits <= 32);
        let masked = u64::from(value) & ((1u64 << bits) - 1);
        self.acc |= masked << self.pending;
        self.pending += bits;
        while self.pending >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.pending -= 8;
        }
    }

    /// Bits written so far.
    pub(crate) fn bits(&self) -> u64 {
        self.out.len() as u64 * 8 + u64::from(self.pending)
    }

    /// The bytes, the last one padded with zero bits.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        if self.pending > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acm::bitreader::BitReader;

    #[test]
    fn writes_least_significant_bits_first() {
        let mut w = BitWriter::default();
        w.put(0b110, 3);
        w.put(0b0110, 4);
        w.put(0b111, 3);
        assert_eq!(w.bits(), 10);
        assert_eq!(w.finish(), [0xB6, 0x03]);
    }

    #[test]
    fn keeps_only_the_low_bits_of_a_value() {
        let mut w = BitWriter::default();
        w.put(0xFFFF_FFF5, 4);
        assert_eq!(w.finish(), [0x05]);
    }

    #[test]
    fn writes_nothing_for_nothing() {
        assert_eq!(BitWriter::default().finish(), Vec::<u8>::new());
    }

    #[test]
    fn reads_back_what_it_writes() {
        let fields: Vec<(u32, u32)> = (1..=24)
            .map(|bits| ((0x00A5_5A3C >> (24 - bits)) & ((1 << bits) - 1), bits))
            .collect();
        let mut w = BitWriter::default();
        for &(v, bits) in &fields {
            w.put(v, bits);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        for &(v, bits) in &fields {
            assert_eq!(r.read(bits), v, "{bits}-bit field");
        }
        assert_eq!(r.overrun(), 0);
    }
}
