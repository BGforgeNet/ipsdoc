//! Lossless ACM encoding: `levels = 0`, so the engines skip the synthesis transform and each block's coefficients
//! are its samples.
//!
//! Per block of [`ROWS`] samples: `val` is the samples' greatest common divisor, the stored indices are the samples
//! divided by it, the filler is the cheapest that can code them (by exact bit count), and `pwr` is the smallest that
//! puts every index inside the block's table. Output is about 0.88 of 16-bit PCM for the games' and mods' RIFF
//! files, and 0.78 for their decoded ACMs (about 3.4 times the lossy originals).

use super::bitwriter::BitWriter;
pub use super::header::Header;

/// Rows per block. Smaller blocks fit `val` and the filler more tightly but rebuild the dequantisation table more
/// often; a cost model over 663 RIFF files from the games and mods gave 0.880 of PCM size here against 0.872 at the
/// optimum (64), for a quarter of the table work.
pub const ROWS: u16 = 256;

/// Why samples cannot be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// The header's sample count is 32-bit.
    #[error("{samples} samples, more than an ACM header can count")]
    TooLong {
        /// Samples given.
        samples: usize,
    },
}

/// Where codes go: the output, or a counter that prices them.
trait Sink {
    fn put(&mut self, value: u32, bits: u32);
}

impl Sink for BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        BitWriter::put(self, value, bits);
    }
}

struct Count(u64);

impl Sink for Count {
    fn put(&mut self, _value: u32, bits: u32) {
        self.0 += u64::from(bits);
    }
}

/// Fillers other than linear, with the largest index magnitude each can code.
const SMALL_FILLERS: [(u32, i32); 11] = [
    (17, 1),
    (18, 1),
    (19, 1),
    (20, 2),
    (21, 2),
    (22, 2),
    (23, 3),
    (24, 3),
    (26, 4),
    (27, 4),
    (29, 5),
];

/// Encodes interleaved 16-bit samples losslessly.
///
/// # Errors
///
/// When there are more samples than a 32-bit count holds.
pub fn encode(samples: &[i16], channels: u16, rate: u16) -> Result<Vec<u8>, EncodeError> {
    let count = u32::try_from(samples.len()).map_err(|_| EncodeError::TooLong { samples: samples.len() })?;
    let header = Header {
        samples: count,
        channels,
        rate,
        levels: 0,
        rows: ROWS,
    };
    let mut w = BitWriter::default();
    let mut indices = vec![0i32; usize::from(ROWS)];
    for block in samples.chunks(usize::from(ROWS)) {
        let val = block.iter().fold(0u32, |g, &s| gcd(g, s.unsigned_abs().into())).max(1);
        indices.fill(0);
        for (i, &s) in indices.iter_mut().zip(block) {
            *i = i32::from(s) / val as i32;
        }
        let (lo, hi) = indices.iter().fold((0, 0), |(lo, hi), &i| (i.min(lo), i.max(hi)));
        let filler = cheapest(&indices, lo, hi);
        let pwr = (0..=15u32)
            .find(|&p| -(1i32 << p) <= lo && hi < 1i32 << p)
            .unwrap_or(15);
        w.put(if filler == 0 { 0 } else { pwr }, 4);
        w.put(val, 16);
        w.put(filler, 5);
        emit(&mut w, filler, &indices);
    }
    let mut out = header.to_bytes().to_vec();
    out.extend(w.finish());
    Ok(out)
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The filler coding `indices` in the fewest bits; the first such in index order on a tie.
fn cheapest(indices: &[i32], lo: i32, hi: i32) -> u32 {
    if lo == 0 && hi == 0 {
        return 0;
    }
    // The narrowest linear filler that fits; a wider one only costs more.
    let linear = (3..=16u32)
        .find(|&ind| -(1i32 << (ind - 1)) <= lo && hi < 1i32 << (ind - 1))
        .unwrap_or(16);
    let mut best = (u64::from(linear) * indices.len() as u64, linear);
    for (filler, magnitude) in SMALL_FILLERS {
        if -magnitude <= lo && hi <= magnitude {
            let mut c = Count(0);
            emit(&mut c, filler, indices);
            if c.0 < best.0 || (c.0 == best.0 && filler < best.1) {
                best = (c.0, filler);
            }
        }
    }
    best.1
}

/// Writes `indices` in filler `filler`'s codes, the decoder's mirror (bits are least significant first).
fn emit(w: &mut impl Sink, filler: u32, indices: &[i32]) {
    let rows = indices.len();
    let mut row = 0;
    while row < rows {
        let i = indices[row];
        match filler {
            0 => row = rows,
            ind @ 3..=16 => {
                w.put((i + (1 << (ind - 1))) as u32, ind);
                row += 1;
            }
            19 | 22 | 29 => {
                let (bits, base, count) = match filler {
                    19 => (5, 3, 3),
                    22 => (7, 5, 3),
                    _ => (7, 11, 2),
                };
                // Digits least significant first, each the index plus half the base; past the last row, index 0.
                let mut code = 0;
                for k in (0..count).rev() {
                    let digit = indices.get(row + k).map_or(base / 2, |&v| v + base / 2);
                    code = code * base + digit;
                }
                w.put(code as u32, bits);
                row += count;
            }
            // k13, k24, k35, k45: bit 0 clear codes two zeros (one at the last row), then bit 1 clear one zero.
            17 | 20 | 23 | 26 => {
                if i == 0 && indices.get(row + 1).is_none_or(|&n| n == 0) {
                    w.put(0, 1);
                    row += 2;
                    continue;
                }
                if i == 0 {
                    w.put(0b01, 2);
                } else {
                    w.put(0b11, 2);
                    put_nonzero(w, filler, i);
                }
                row += 1;
            }
            // k12, k23, k34, k44: bit 0 clear codes one zero.
            _ => {
                if i == 0 {
                    w.put(0, 1);
                } else {
                    w.put(1, 1);
                    put_nonzero(w, filler, i);
                }
                row += 1;
            }
        }
    }
}

/// The rest of a non-zero k-filler code, after its prefix.
fn put_nonzero(w: &mut impl Sink, filler: u32, i: i32) {
    match filler {
        17 | 18 => w.put(u32::from(i > 0), 1),
        // Low bit, then high: -2, -1, +1, +2 are (0, 0), (1, 0), (0, 1), (1, 1).
        20 | 21 => {
            let (low, high) = match i {
                -2 => (0, 0),
                -1 => (1, 0),
                1 => (0, 1),
                _ => (1, 1),
            };
            w.put(low | high << 1, 2);
        }
        // A clear bit then the sign codes +-1; a set bit then two bits code -3, -2, +2, +3.
        23 | 24 => match i {
            -1 | 1 => w.put(u32::from(i > 0) << 1, 2),
            _ => w.put(
                1 | ([-3, -2, 2, 3].iter().position(|&v| v == i).unwrap_or(0) as u32) << 1,
                3,
            ),
        },
        // 26, 27: three bits code -4 ..= -1, +1 ..= +4.
        _ => w.put(
            [-4, -3, -2, -1, 1, 2, 3, 4].iter().position(|&v| v == i).unwrap_or(0) as u32,
            3,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acm::decode;

    /// xorshift64*, for reproducible inputs.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
    }

    #[track_caller]
    fn round_trip(samples: &[i16], channels: u16, rate: u16) -> decode::Decoded {
        let acm = encode(samples, channels, rate).unwrap();
        let d = decode::decode(&acm).unwrap();
        assert_eq!(d.header.channels, channels);
        assert_eq!(d.header.rate, rate);
        assert_eq!(d.header.samples as usize, samples.len());
        assert_eq!((d.header.levels, d.header.rows), (0, ROWS));
        if let Some(i) = d.samples.iter().zip(samples).position(|(a, b)| a != b) {
            panic!("sample {i}: {} != {}", d.samples[i], samples[i]);
        }
        assert_eq!(d.samples.len(), samples.len());
        assert_eq!(d.stats.overrun_bits, 0);
        d
    }

    #[test]
    fn full_scale_noise_round_trips() {
        let mut rng = Rng(1);
        let mut s: Vec<i16> = (0..50_000).map(|_| rng.next() as i16).collect();
        s[7] = i16::MIN;
        s[8] = i16::MAX;
        round_trip(&s, 2, 44100);
    }

    #[test]
    fn quiet_material_uses_the_small_fillers() {
        let mut rng = Rng(2);
        // Each block draws from a narrow range: zero, within one, within two (where the packed triplets, 430 and 602
        // bits a block, beat a 3-bit linear code's 768).
        let s: Vec<i16> = (0..256 * 30)
            .map(|n| {
                let range = (n / 256 % 3) as u64;
                (rng.next() % (2 * range + 1)) as i16 - range as i16
            })
            .collect();
        let d = round_trip(&s, 1, 22050);
        // Every block but the silent ones (range 0) takes a filler for small values, never a linear one.
        let linear: u64 = d.stats.fillers[3..17].iter().sum();
        assert_eq!(linear, 0, "{:?}", d.stats.fillers);
    }

    #[test]
    fn a_common_factor_becomes_the_block_scale() {
        let mut rng = Rng(3);
        let s: Vec<i16> = (0..2000).map(|_| ((rng.next() % 61) as i16 - 30) * 1000).collect();
        let d = round_trip(&s, 1, 22050);
        // Indices of at most 30 need 6-bit linear codes, not the 16 the raw values would.
        assert_eq!(d.stats.fillers[16], 0);
    }

    #[test]
    fn extremes_alone_round_trip() {
        round_trip(&[i16::MIN; 300], 1, 22050);
        round_trip(&[i16::MAX; 300], 1, 22050);
        round_trip(&[i16::MIN, i16::MAX, 0, -1, 1], 1, 8000);
    }

    #[test]
    fn silence_costs_25_bits_a_block() {
        let acm = encode(&[0; 256 * 4], 1, 22050).unwrap();
        // 4 blocks of pwr (4 bits), val (16) and the zero filler's index (5): 100 bits, 13 bytes.
        assert_eq!(acm.len(), Header::LEN + 13);
        round_trip(&[0; 256 * 4], 1, 22050);
    }

    #[test]
    fn odd_lengths_and_partial_blocks_round_trip() {
        let mut rng = Rng(4);
        for len in [0, 1, 2, 255, 256, 257, 1001] {
            let s: Vec<i16> = (0..len).map(|_| (rng.next() % 2001) as i16 - 1000).collect();
            round_trip(&s, 2, 22050);
        }
    }

    #[test]
    fn every_filler_writes_what_the_decoder_reads() {
        let mut rng = Rng(5);
        let fillers = (3..=16u32)
            .map(|ind| (ind, (1i32 << (ind - 1)) - 1))
            .chain(SMALL_FILLERS);
        for (filler, magnitude) in fillers {
            for _ in 0..20 {
                // Indices within the filler's domain, with runs of zeros (pairs, singles, one at the last row).
                let indices: Vec<i32> = (0..usize::from(ROWS))
                    .map(|_| match rng.next() % 3 {
                        0 => 0,
                        _ => (rng.next() % (2 * magnitude as u64 + 1)) as i32 - magnitude,
                    })
                    .collect();
                let mut w = BitWriter::default();
                w.put(15, 4);
                w.put(1, 16);
                w.put(filler, 5);
                emit(&mut w, filler, &indices);
                let mut acm = Header {
                    samples: u32::from(ROWS),
                    channels: 1,
                    rate: 22050,
                    levels: 0,
                    rows: ROWS,
                }
                .to_bytes()
                .to_vec();
                acm.extend(w.finish());
                let d = decode::decode(&acm).unwrap_or_else(|e| panic!("filler {filler}: {e}"));
                let got: Vec<i32> = d.samples.iter().map(|&s| i32::from(s)).collect();
                assert_eq!(got, indices, "filler {filler}");
                // The price the encoder compares is the length it writes.
                let (mut c, mut written) = (Count(0), BitWriter::default());
                emit(&mut c, filler, &indices);
                emit(&mut written, filler, &indices);
                assert_eq!(c.0, written.bits(), "filler {filler}");
            }
        }
    }

    #[test]
    fn each_block_takes_its_cheapest_filler() {
        // One block of `1, 0, 0, 0` repeated. k12 (18): 192 zeros at 1 bit, 64 ones at 2: 320 bits. The runners-up,
        // k13 and k23, take 384; t15 430; linear 768.
        let s: Vec<i16> = (0..256).map(|n| i16::from(n % 4 == 0)).collect();
        let d = round_trip(&s, 1, 22050);
        assert_eq!(d.stats.fillers[18], 1, "{:?}", d.stats.fillers);
        assert_eq!(
            encode(&s, 1, 22050).unwrap().len(),
            Header::LEN + (25 + 320_usize).div_ceil(8)
        );
    }
}
