//! ACM decoding, sample for sample as the engines decode (the Fallout 2 engine and GemRB), except that a sample
//! beyond 16 bits is clamped by default where the engines wrap it ([`Overflow`]). Where they would read undefined
//! data (an invalid filler, a code outside a filler's table, a dequantisation entry the block never set), decoding
//! fails with the position instead. A header whose sample count no real stream's input could carry
//! ([`MAX_SAMPLES_PER_BIT`]) is refused before anything is decoded.

use super::bitreader::BitReader;
pub use super::header::{Header, HeaderError};

/// A decoded stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The stream's header, as stored.
    pub header: Header,
    /// Interleaved samples, exactly `header.samples` of them.
    pub samples: Vec<i16>,
    /// What the bitstream used.
    pub stats: Stats,
}

/// Bitstream statistics of a decoded stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    /// Columns coded with each filler index (for filler 31, blocks).
    pub fillers: [u64; 32],
    /// Bits the final block reads past the end of the input, as zeros.
    pub overrun_bits: u64,
    /// Samples beyond 16 bits, clamped or wrapped as the [`Overflow`] mode says.
    pub overflowed: u64,
}

/// Why a stream cannot be decoded. `block` counts from 0; `bit` is the offset from the start of the input of the
/// column's filler index.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The header is unreadable.
    #[error("bad header")]
    Header(#[from] HeaderError),
    /// Filler indices 1, 2, 25, 28 and 30 are undefined.
    #[error("block {block}, bit {bit}: invalid filler index {index}")]
    InvalidFiller {
        /// Block number.
        block: u64,
        /// Bit offset.
        bit: u64,
        /// The filler index.
        index: u32,
    },
    /// A value index outside the `-(1 << pwr) .. (1 << pwr)` range the block's table defines.
    #[error("block {block}, bit {bit}: filler {filler} reads table entry {index}, outside pwr {pwr}")]
    OutsideTable {
        /// Block number.
        block: u64,
        /// Bit offset.
        bit: u64,
        /// The filler index.
        filler: u32,
        /// The table entry read.
        index: i32,
        /// The block's table size exponent.
        pwr: u32,
    },
    /// A packed-group code beyond the filler's table (27 for filler 19, 125 for 22, 121 for 29).
    #[error("block {block}, bit {bit}: filler {filler} has undefined code {code}")]
    InvalidCode {
        /// Block number.
        block: u64,
        /// Bit offset.
        bit: u64,
        /// The filler index.
        filler: u32,
        /// The code.
        code: u32,
    },
    /// A block larger than [`MAX_BLOCK_LEN`]: checked before decoding, as the block buffer is sized from the header
    /// alone (the input may end inside the block, which then reads as zeros).
    #[error("blocks of {len} samples; at most {max} are read", max = MAX_BLOCK_LEN)]
    BlockTooLarge {
        /// Samples per block, as the header states.
        len: usize,
    },
    /// The header's sample count needs more blocks than the input could hold, each block taking at least 20 bits
    /// plus 5 per column: checked before decoding.
    #[error("the header needs {blocks} blocks; the input holds at most {fit}")]
    TooManyBlocks {
        /// Blocks the header needs.
        blocks: u64,
        /// Blocks that could begin within the input.
        fit: u64,
    },
    /// The header's sample count is more than [`MAX_SAMPLES_PER_BIT`] times the input's bits: checked before
    /// decoding, so a corrupt header cannot drive a huge sample buffer.
    #[error("the header claims {samples} samples; {bits} bits of input hold at most {}", bits * MAX_SAMPLES_PER_BIT)]
    TooManySamples {
        /// Samples the header claims.
        samples: u64,
        /// Bits of input after the header.
        bits: u64,
    },
    /// The input ends before a block the header's sample count needs begins.
    #[error("stream ends before block {block} of {blocks}")]
    Truncated {
        /// The first missing block.
        block: u64,
        /// Blocks the header needs.
        blocks: u64,
    },
}

/// What to do with a sample beyond 16 bits, which lossy encoders produce now and then on loud material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overflow {
    /// Clip to the 16-bit range: a slightly flattened peak.
    #[default]
    Clamp,
    /// Keep the low 16 bits, as the engines do: the sample jumps to the far end of the range, an audible click.
    Wrap,
}

/// The largest block decoded, in samples. The games' and mods' ACMs (some 30 thousand streams) use at most 4096
/// (levels 8, rows 16) and the lossless encoder 256; 16 times the largest bounds a hostile header's block buffer at
/// 256 KiB.
pub const MAX_BLOCK_LEN: usize = 1 << 16;

/// The most samples decoded per bit of input. The block minimum alone allows 655 (levels 4, rows 4095), while the
/// lossless encoder's silent blocks hold 10.24 (256 samples in 25 bits) and the games' and mods' ACMs at most 3.15;
/// 16 bounds a hostile header's sample buffer at 256 bytes per input byte.
pub const MAX_SAMPLES_PER_BIT: u64 = 16;

/// Decodes a bare ACM stream (header first), clamping overflowing samples.
///
/// # Errors
///
/// When the header is unreadable, the bitstream holds undefined codes, or it ends before its final block.
pub fn decode(acm: &[u8]) -> Result<Decoded, DecodeError> {
    decode_with(acm, Overflow::default())
}

/// Decodes a bare ACM stream (header first), treating overflowing samples as `overflow` says.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_with(acm: &[u8], overflow: Overflow) -> Result<Decoded, DecodeError> {
    let header = Header::parse(acm)?;
    let body = &acm[Header::LEN..];
    let total_bits = body.len() as u64 * 8;
    let mut r = BitReader::new(body);
    let mut stats = Stats::default();
    let (levels, cols, rows) = (u32::from(header.levels), header.cols(), usize::from(header.rows));
    let block_len = header.block_len();
    if block_len > MAX_BLOCK_LEN {
        return Err(DecodeError::BlockTooLarge { len: block_len });
    }
    let total = header.samples as usize;
    let blocks = total.div_ceil(block_len) as u64;
    let fit = total_bits.div_ceil(20 + 5 * cols as u64);
    if blocks > fit {
        return Err(DecodeError::TooManyBlocks { blocks, fit });
    }
    if u64::from(header.samples) > total_bits * MAX_SAMPLES_PER_BIT {
        return Err(DecodeError::TooManySamples {
            samples: header.samples.into(),
            bits: total_bits,
        });
    }
    let mut samples = Vec::with_capacity(total);
    let mut block = vec![0i32; block_len];
    let mut state = vec![0i32; 2 * (cols - 1)];
    for b in 0..blocks {
        if r.position() >= total_bits {
            return Err(DecodeError::Truncated { block: b, blocks });
        }
        let raw = read_block(&mut r, &mut block, levels, cols, rows, &mut stats).map_err(|e| e.at(b))?;
        if !raw {
            transform(&mut block, levels, rows, &mut state);
        }
        let take = block_len.min(total - samples.len());
        for &v in &block[..take] {
            let v = v >> levels;
            let fits = i16::try_from(v);
            if fits.is_err() {
                stats.overflowed += 1;
            }
            samples.push(match (fits, overflow) {
                (Ok(s), _) => s,
                (Err(_), Overflow::Clamp) => v.clamp(i16::MIN.into(), i16::MAX.into()) as i16,
                (Err(_), Overflow::Wrap) => v as i16,
            });
        }
    }
    stats.overrun_bits = r.overrun();
    Ok(Decoded { header, samples, stats })
}

/// A column error before the block number is known.
enum ColumnError {
    InvalidFiller {
        bit: u64,
        index: u32,
    },
    OutsideTable {
        bit: u64,
        filler: u32,
        index: i32,
        pwr: u32,
    },
    InvalidCode {
        bit: u64,
        filler: u32,
        code: u32,
    },
}

impl ColumnError {
    fn at(self, block: u64) -> DecodeError {
        let bit = |b: u64| b + Header::LEN as u64 * 8;
        match self {
            ColumnError::InvalidFiller { bit: b, index } => DecodeError::InvalidFiller {
                block,
                bit: bit(b),
                index,
            },
            ColumnError::OutsideTable {
                bit: b,
                filler,
                index,
                pwr,
            } => DecodeError::OutsideTable {
                block,
                bit: bit(b),
                filler,
                index,
                pwr,
            },
            ColumnError::InvalidCode { bit: b, filler, code } => DecodeError::InvalidCode {
                block,
                bit: bit(b),
                filler,
                code,
            },
        }
    }
}

/// Reads one block's coefficients into `block` (row-major, `cols` wide). Returns true for a raw (filler 31) block,
/// which bypasses the transform.
fn read_block(
    r: &mut BitReader<'_>,
    block: &mut [i32],
    levels: u32,
    cols: usize,
    rows: usize,
    stats: &mut Stats,
) -> Result<bool, ColumnError> {
    let pwr = r.read(4);
    let val = r.read(16) as i32;
    for c in 0..cols {
        let bit = r.position();
        let filler = r.read(5);
        stats.fillers[filler as usize] += 1;
        if filler == 31 {
            // An extension of the Fallout 2 engine's decoder: the whole block as raw 16-bit samples, pre-scaled
            // by `levels` so the output shift restores them; the remaining columns' indices are not stored.
            for v in block.iter_mut() {
                *v = i32::from(r.read(16) as u16 as i16) << levels;
            }
            return Ok(true);
        }
        let mut column = Column {
            r: &mut *r,
            pwr,
            val,
            filler,
            bit,
            block: &mut *block,
            cols,
            c,
            rows,
        };
        column.fill()?;
    }
    Ok(false)
}

struct Column<'r, 'a, 'b> {
    r: &'r mut BitReader<'a>,
    pwr: u32,
    val: i32,
    filler: u32,
    bit: u64,
    block: &'b mut [i32],
    cols: usize,
    c: usize,
    rows: usize,
}

impl Column<'_, '_, '_> {
    /// Entry `index` of the block's dequantisation table, `index * val` kept to 16 bits as in the engines.
    fn table(&self, index: i32) -> Result<i32, ColumnError> {
        let n = 1i32 << self.pwr;
        if !(-n..n).contains(&index) {
            return Err(ColumnError::OutsideTable {
                bit: self.bit,
                filler: self.filler,
                index,
                pwr: self.pwr,
            });
        }
        Ok(i32::from(index.wrapping_mul(self.val) as i16))
    }

    fn set(&mut self, row: usize, value: i32) {
        self.block[row * self.cols + self.c] = value;
    }

    fn bit(&mut self) -> bool {
        self.r.read(1) == 1
    }

    /// Fills the column's rows; a zero pair at the last row writes one zero, a group stops at the last row.
    fn fill(&mut self) -> Result<(), ColumnError> {
        let mut row = 0;
        while row < self.rows {
            let index = match self.filler {
                0 => 0,
                ind @ 3..=16 => self.r.read(ind) as i32 - (1 << (ind - 1)),
                // k13, k24, k35, k45: bit 0 clear codes two zeros, then bit 1 clear one zero.
                17 | 20 | 23 | 26 => {
                    if !self.bit() {
                        self.set(row, 0);
                        row += 1;
                        if row < self.rows {
                            self.set(row, 0);
                            row += 1;
                        }
                        continue;
                    }
                    if !self.bit() { 0 } else { self.nonzero() }
                }
                // k12, k23, k34, k44: bit 0 clear codes one zero.
                18 | 21 | 24 | 27 => {
                    if !self.bit() {
                        0
                    } else {
                        self.nonzero()
                    }
                }
                19 => {
                    self.group(&mut row, 5, 3, 3)?;
                    continue;
                }
                22 => {
                    self.group(&mut row, 7, 5, 3)?;
                    continue;
                }
                29 => {
                    self.group(&mut row, 7, 11, 2)?;
                    continue;
                }
                index => return Err(ColumnError::InvalidFiller { bit: self.bit, index }),
            };
            // A zero is written as such, never looked up: the engines do the same.
            let v = if index == 0 { 0 } else { self.table(index)? };
            self.set(row, v);
            row += 1;
        }
        Ok(())
    }

    /// The rest of a non-zero k-filler code, by the filler's largest magnitude.
    fn nonzero(&mut self) -> i32 {
        match self.filler {
            17 | 18 => {
                if self.bit() {
                    1
                } else {
                    -1
                }
            }
            20 | 21 => {
                let low = self.bit();
                match (self.bit(), low) {
                    (true, true) => 2,
                    (true, false) => 1,
                    (false, true) => -1,
                    (false, false) => -2,
                }
            }
            23 | 24 => {
                if !self.bit() {
                    if self.bit() { 1 } else { -1 }
                } else {
                    [-3, -2, 2, 3][self.r.read(2) as usize]
                }
            }
            // 26, 27
            _ => [-4, -3, -2, -1, 1, 2, 3, 4][self.r.read(3) as usize],
        }
    }

    /// A packed group: one `bits`-bit code holding `count` base-`base` digits, least significant first, each the
    /// table index `digit - base / 2`.
    fn group(&mut self, row: &mut usize, bits: u32, base: u32, count: u32) -> Result<(), ColumnError> {
        let code = self.r.read(bits);
        if code >= base.pow(count) {
            return Err(ColumnError::InvalidCode {
                bit: self.bit,
                filler: self.filler,
                code,
            });
        }
        let mut rest = code;
        for _ in 0..count {
            if *row == self.rows {
                break;
            }
            let v = self.table((rest % base) as i32 - (base / 2) as i32)?;
            self.set(*row, v);
            *row += 1;
            rest /= base;
        }
        Ok(())
    }
}

/// The synthesis transform, `levels` stages from the widest subband down, then `+1` on the first stage's first
/// column. `state` holds two values per column per stage across blocks. The engines keep the first stage's state in
/// 16 bits; it holds that stage's inputs, which are table entries and already 16-bit, so 32 bits here is the same.
fn transform(block: &mut [i32], levels: u32, rows: usize, state: &mut [i32]) {
    if levels == 0 {
        return;
    }
    let (mut width, mut height) = (1usize << (levels - 1), 2 * rows);
    let mut offset = 0;
    let mut first = true;
    while width > 0 {
        stage(block, width, height, &mut state[offset..offset + 2 * width]);
        if first {
            for k in 0..height {
                block[k * width] = block[k * width].wrapping_add(1);
            }
            first = false;
        }
        offset += 2 * width;
        width /= 2;
        height *= 2;
    }
}

/// One stage over the block seen as `height` rows of `width` columns: per column, each pair of rows (x0, x1) with
/// the previous pair (d0, d1) becomes (d0 + 2 d1 + x0, 2 x0 - d1 - x1).
fn stage(block: &mut [i32], width: usize, height: usize, state: &mut [i32]) {
    for c in 0..width {
        let (mut d0, mut d1) = (state[2 * c], state[2 * c + 1]);
        for p in (0..height).step_by(2) {
            let (i0, i1) = (p * width + c, (p + 1) * width + c);
            let (x0, x1) = (block[i0], block[i1]);
            block[i0] = d0.wrapping_add(d1.wrapping_mul(2)).wrapping_add(x0);
            block[i1] = x0.wrapping_mul(2).wrapping_sub(d1).wrapping_sub(x1);
            (d0, d1) = (x0, x1);
        }
        state[2 * c] = d0;
        state[2 * c + 1] = d1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acm::bitwriter::BitWriter;

    /// A mono 22050 Hz stream: the header, then whatever `body` writes.
    fn stream(levels: u8, rows: u16, samples: u32, body: impl FnOnce(&mut BitWriter)) -> Vec<u8> {
        let mut out = Header {
            samples,
            channels: 1,
            rate: 22050,
            levels,
            rows,
        }
        .to_bytes()
        .to_vec();
        let mut w = BitWriter::default();
        body(&mut w);
        out.extend(w.finish());
        out
    }

    fn block_start(w: &mut BitWriter, pwr: u32, val: u32) {
        w.put(pwr, 4);
        w.put(val, 16);
    }

    /// One level-0 block of `rows` values coded with filler `filler`, whose codes `codes` writes.
    fn one_column(rows: u16, pwr: u32, val: u32, filler: u32, codes: &[(u32, u32)]) -> Vec<i16> {
        let acm = stream(0, rows, rows.into(), |w| {
            block_start(w, pwr, val);
            w.put(filler, 5);
            for &(code, bits) in codes {
                w.put(code, bits);
            }
        });
        decode(&acm).unwrap().samples
    }

    #[test]
    fn linear_filler_scales_by_val() {
        // 4-bit codes are the index plus 8.
        let codes = [(0, 4), (15, 4), (8, 4), (7, 4)];
        assert_eq!(one_column(4, 3, 3, 4, &codes), [-24, 21, 0, -3]);
    }

    #[test]
    fn zero_filler_reads_no_codes() {
        let acm = stream(0, 3, 6, |w| {
            block_start(w, 0, 1);
            w.put(0, 5);
            block_start(w, 2, 1);
            w.put(4, 5);
            for code in [9, 10, 11] {
                w.put(code, 4);
            }
        });
        assert_eq!(decode(&acm).unwrap().samples, [0, 0, 0, 1, 2, 3]);
    }

    #[test]
    fn k13_codes_zero_pairs_zeros_and_ones() {
        // 0: two zeros; 01: zero; 111: +1; 011: -1; a final 0 with one row left: one zero.
        let codes = [(0, 1), (0b01, 2), (0b111, 3), (0b011, 3), (0, 1)];
        assert_eq!(one_column(6, 1, 5, 17, &codes), [0, 0, 0, 5, -5, 0]);
    }

    #[test]
    fn k12_codes_zeros_and_ones() {
        assert_eq!(one_column(3, 1, 5, 18, &[(0, 1), (0b11, 2), (0b01, 2)]), [0, 5, -5]);
    }

    #[test]
    fn t15_codes_base_3_triplets_and_drops_the_rest_of_the_last() {
        // 5 = 2 + 3 * 1: +1, 0, -1. Then 0 with one row left: -1.
        assert_eq!(one_column(4, 1, 5, 19, &[(5, 5), (0, 5)]), [5, 0, -5, -5]);
    }

    #[test]
    fn k24_codes_up_to_two() {
        let codes = [(0, 1), (0b01, 2), (0b1111, 4), (0b1011, 4), (0b0111, 4), (0b0011, 4)];
        assert_eq!(one_column(7, 2, 1, 20, &codes), [0, 0, 0, 2, 1, -1, -2]);
    }

    #[test]
    fn k23_codes_up_to_two() {
        let codes = [(0, 1), (0b111, 3), (0b101, 3), (0b011, 3), (0b001, 3)];
        assert_eq!(one_column(5, 2, 1, 21, &codes), [0, 2, 1, -1, -2]);
    }

    #[test]
    fn t27_codes_base_5_triplets() {
        // 54 = 4 + 5 * 0 + 25 * 2: +2, -2, 0.
        assert_eq!(one_column(3, 2, 1, 22, &[(54, 7)]), [2, -2, 0]);
    }

    #[test]
    fn k35_codes_up_to_three() {
        let codes = [
            (0, 1),
            (0b01, 2),
            (0b1011, 4),
            (0b0011, 4),
            (0b00111, 5),
            (0b01111, 5),
            (0b10111, 5),
            (0b11111, 5),
        ];
        assert_eq!(one_column(9, 2, 1, 23, &codes), [0, 0, 0, 1, -1, -3, -2, 2, 3]);
    }

    #[test]
    fn k34_codes_up_to_three() {
        let codes = [
            (0, 1),
            (0b101, 3),
            (0b001, 3),
            (0b0011, 4),
            (0b0111, 4),
            (0b1011, 4),
            (0b1111, 4),
        ];
        assert_eq!(one_column(7, 2, 1, 24, &codes), [0, 1, -1, -3, -2, 2, 3]);
    }

    #[test]
    fn k45_codes_up_to_four() {
        let codes = [
            (0, 1),
            (0b01, 2),
            (0b00011, 5),
            (0b01111, 5),
            (0b10011, 5),
            (0b11111, 5),
        ];
        assert_eq!(one_column(7, 3, 1, 26, &codes), [0, 0, 0, -4, -1, 1, 4]);
    }

    #[test]
    fn k44_codes_up_to_four() {
        assert_eq!(one_column(3, 3, 1, 27, &[(0, 1), (0b0001, 4), (0b1111, 4)]), [0, -4, 4]);
    }

    #[test]
    fn t37_codes_base_11_pairs() {
        // 10 = 10 + 11 * 0: +5, -5.
        assert_eq!(one_column(2, 3, 1, 29, &[(10, 7)]), [5, -5]);
    }

    #[test]
    fn rejects_invalid_fillers_with_their_position() {
        for index in [1, 2, 25, 28, 30] {
            let acm = stream(0, 1, 1, |w| {
                block_start(w, 0, 1);
                w.put(index, 5);
            });
            assert_eq!(
                decode(&acm).unwrap_err(),
                DecodeError::InvalidFiller {
                    block: 0,
                    bit: 112 + 20,
                    index
                }
            );
        }
    }

    #[test]
    fn rejects_a_value_outside_the_blocks_table() {
        // pwr 0 defines only the entries 0 and -1; +1 is undefined in the engines.
        let acm = stream(0, 1, 1, |w| {
            block_start(w, 0, 1);
            w.put(18, 5);
            w.put(0b11, 2);
        });
        assert_eq!(
            decode(&acm).unwrap_err(),
            DecodeError::OutsideTable {
                block: 0,
                bit: 112 + 20,
                filler: 18,
                index: 1,
                pwr: 0
            }
        );
    }

    #[test]
    fn rejects_an_undefined_triplet_code() {
        let acm = stream(0, 3, 3, |w| {
            block_start(w, 1, 1);
            w.put(19, 5);
            w.put(27, 5);
        });
        assert_eq!(
            decode(&acm).unwrap_err(),
            DecodeError::InvalidCode {
                block: 0,
                bit: 112 + 20,
                filler: 19,
                code: 27
            }
        );
    }

    /// One row, level 0: pwr 6, val 1, linear filler 7, one 7-bit code; 32 bits.
    fn byte_aligned_block(w: &mut BitWriter, code: u32) {
        block_start(w, 6, 1);
        w.put(7, 5);
        w.put(code, 7);
    }

    #[test]
    fn rejects_a_stream_ending_before_a_block_starts() {
        let acm = stream(0, 1, 2, |w| byte_aligned_block(w, 65));
        assert_eq!(
            decode(&acm).unwrap_err(),
            DecodeError::Truncated { block: 1, blocks: 2 }
        );
    }

    #[test]
    fn reads_zeros_where_the_final_block_runs_past_the_end() {
        let mut acm = stream(0, 1, 1, |w| byte_aligned_block(w, 65));
        acm.pop();
        let d = decode(&acm).unwrap();
        // The lost code reads as 0, index -64.
        assert_eq!(d.samples, [-64]);
        assert_eq!(d.stats.overrun_bits, 8);
    }

    #[test]
    fn stops_at_the_header_sample_count() {
        let acm = stream(0, 4, 3, |w| {
            block_start(w, 3, 1);
            w.put(4, 5);
            for code in [9, 10, 11, 12] {
                w.put(code, 4);
            }
        });
        assert_eq!(decode(&acm).unwrap().samples, [1, 2, 3]);
    }

    #[test]
    fn transform_carries_state_across_blocks() {
        // levels 1, one row: blocks [10, 4] and [6, 2]. Derived by hand from the synthesis step
        // (y0 = d0 + 2 d1 + x0, y1 = 2 x0 - d1 - x1, then +1 and >> 1).
        let acm = stream(1, 1, 4, |w| {
            for [a, b] in [[10, 4], [6, 2]] {
                block_start(w, 4, 1);
                for v in [a, b] {
                    w.put(5, 5);
                    w.put(v + 16, 5);
                }
            }
        });
        assert_eq!(decode(&acm).unwrap().samples, [5, 8, 12, 3]);
    }

    #[test]
    fn filler_31_stores_raw_samples_and_skips_the_transform() {
        let acm = stream(1, 1, 4, |w| {
            block_start(w, 0, 1);
            w.put(31, 5);
            w.put(5, 16);
            w.put(0xFFFE, 16);
            // A zero block: the transform state must still be zero, so it decodes to zeros.
            block_start(w, 0, 1);
            w.put(0, 5);
            w.put(0, 5);
        });
        let d = decode(&acm).unwrap();
        assert_eq!(d.samples, [5, -2, 0, 0]);
        assert_eq!((d.stats.fillers[31], d.stats.fillers[0]), (1, 2));
    }

    fn overflow_stream() -> Vec<u8> {
        // levels 1, one row: [32767, -32768] synthesises to (32768, 98303), >> 1: 16384 and 49151.
        stream(1, 1, 2, |w| {
            block_start(w, 15, 1);
            for v in [32767, -32768i32] {
                w.put(16, 5);
                w.put((v + 32768) as u32, 16);
            }
        })
    }

    #[test]
    fn clamps_an_overflowing_sample_by_default_and_counts_it() {
        let d = decode(&overflow_stream()).unwrap();
        assert_eq!(d.samples, [16384, 32767]);
        assert_eq!(d.stats.overflowed, 1);
    }

    #[test]
    fn wraps_an_overflowing_sample_as_the_engines_do() {
        let d = decode_with(&overflow_stream(), Overflow::Wrap).unwrap();
        // 49151 - 65536
        assert_eq!(d.samples, [16384, -16385]);
        assert_eq!(d.stats.overflowed, 1);
    }

    #[test]
    fn clamps_below_as_well() {
        // [-32768, 32767]: (-32767, -98302), >> 1: -16384 and -49151.
        let acm = stream(1, 1, 2, |w| {
            block_start(w, 15, 1);
            for v in [-32768, 32767i32] {
                w.put(16, 5);
                w.put((v + 32768) as u32, 16);
            }
        });
        let d = decode(&acm).unwrap();
        assert_eq!(d.samples, [-16384, -32768]);
        assert_eq!(d.stats.overflowed, 1);
    }

    fn header_over(levels: u8, rows: u16, samples: u32, body: usize) -> Vec<u8> {
        let mut acm = Header {
            samples,
            channels: 2,
            rate: 22050,
            levels,
            rows,
        }
        .to_bytes()
        .to_vec();
        acm.resize(Header::LEN + body, 0);
        acm
    }

    #[test]
    fn rejects_a_header_the_input_cannot_hold_before_allocating() {
        // Four bytes of data under a header claiming 2^32 - 1 samples in blocks of 256 x 16.
        assert_eq!(
            decode(&header_over(8, 16, u32::MAX, 4)).unwrap_err(),
            DecodeError::TooManyBlocks {
                blocks: 1_048_576,
                fit: 1
            }
        );
    }

    #[test]
    fn rejects_more_samples_than_the_input_could_encode_before_allocating() {
        // 820 KB of data under 2^32 - 1 samples in blocks of 16 x 4095: enough bits for every block's minimum, and
        // an 8 GiB buffer if allocated.
        assert_eq!(
            decode(&header_over(4, 4095, u32::MAX, 820_000)).unwrap_err(),
            DecodeError::TooManySamples {
                samples: u64::from(u32::MAX),
                bits: 6_560_000
            }
        );
        // Silence through the lossless encoder, the densest stream it writes, still decodes.
        let silence = vec![0i16; 256 * 64];
        let acm = crate::acm::encode(&silence, 1, 22050).unwrap();
        assert_eq!(decode(&acm).unwrap().samples, silence);
    }

    #[test]
    fn rejects_a_block_beyond_the_largest_before_allocating() {
        // One byte of data under a single block of 32768 x 4095: a 512 MiB buffer if allocated.
        assert_eq!(
            decode(&header_over(15, 4095, 1, 1)).unwrap_err(),
            DecodeError::BlockTooLarge { len: 134_184_960 }
        );
        assert_eq!(
            decode(&header_over(12, 17, 1, 1)).unwrap_err(),
            DecodeError::BlockTooLarge { len: 69_632 }
        );
        // The largest block allowed decodes; the input ends inside it and the rest reads as zeros.
        assert_eq!(decode(&header_over(12, 16, 1, 1)).unwrap().samples, [0]);
    }

    #[test]
    fn passes_header_errors_through() {
        assert_eq!(
            decode(&[0x97]).unwrap_err(),
            DecodeError::Header(HeaderError::TooShort { len: 1 })
        );
    }
}
