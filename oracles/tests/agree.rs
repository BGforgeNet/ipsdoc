//! The reference decoders against hand-built ACM files: lossless (`levels = 0`) files decode to their input exactly,
//! and transformed (`levels > 0`) files decode identically in every decoder.
#![expect(
    clippy::unwrap_used,
    reason = "fixture helpers outside #[test] fns: a panic is the failure report"
)]

use oracles::Oracle;

/// A minimal ACM writer, independent of the crate's encoder by design. It stores the given values as the block's
/// coefficients: `val = 1`, each column coded with the narrowest linear filler (or the zero filler), `pwr` covering
/// the widest column. At `levels = 0` the coefficients are the samples.
mod writer {
    struct Bits {
        out: Vec<u8>,
        acc: u64,
        n: u32,
    }

    impl Bits {
        fn put(&mut self, value: u32, bits: u32) {
            self.acc |= u64::from(value & ((1u32 << bits) - 1)) << self.n;
            self.n += bits;
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.n > 0 {
                self.out.push(self.acc as u8);
            }
            self.out
        }
    }

    fn width(column: &[i16]) -> u32 {
        (3..=16u32)
            .find(|&ind| {
                column
                    .iter()
                    .all(|&v| (-(1i32 << (ind - 1))..(1i32 << (ind - 1))).contains(&i32::from(v)))
            })
            .unwrap()
    }

    pub fn encode(values: &[i16], channels: u16, rate: u16, levels: u16, rows: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0x97, 0x28, 0x03, 0x01]);
        out.extend_from_slice(&u32::try_from(values.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rows << 4 | levels).to_le_bytes());
        let mut bits = Bits { out, acc: 0, n: 0 };
        let (cols, rows) = (1usize << levels, usize::from(rows));
        for chunk in values.chunks(rows * cols) {
            let mut block = chunk.to_vec();
            block.resize(rows * cols, 0);
            let columns: Vec<Vec<i16>> = (0..cols)
                .map(|c| (0..rows).map(|r| block[r * cols + c]).collect())
                .collect();
            let zero = |column: &[i16]| column.iter().all(|&v| v == 0);
            let pwr = columns
                .iter()
                .filter(|c| !zero(c))
                .map(|c| width(c) - 1)
                .max()
                .unwrap_or(0);
            bits.put(pwr, 4);
            bits.put(1, 16);
            for column in &columns {
                if zero(column) {
                    bits.put(0, 5);
                    continue;
                }
                let ind = width(column);
                bits.put(ind, 5);
                for &v in column {
                    bits.put((i32::from(v) + (1i32 << (ind - 1))) as u32, ind);
                }
            }
        }
        bits.finish()
    }
}

/// xorshift64*, so the fixtures are reproducible without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

fn full_scale_noise(len: usize, seed: u64) -> Vec<i16> {
    let mut rng = Rng(seed);
    let mut s: Vec<i16> = (0..len).map(|_| rng.next() as i16).collect();
    s[0] = i16::MIN;
    s[1] = i16::MAX;
    s
}

/// Quiet, speech-like: a slow random walk within a few hundred LSB, with silent stretches.
fn quiet_speech(len: usize, seed: u64) -> Vec<i16> {
    let mut rng = Rng(seed);
    let mut v = 0i32;
    (0..len)
        .map(|n| {
            if (n / 3000) % 4 == 3 {
                return 0;
            }
            v = (v + (rng.next() % 61) as i32 - 30).clamp(-400, 400);
            v as i16
        })
        .collect()
}

/// libacm rejects `levels = 0` outright (it requires a non-empty inter-block state), so it is no oracle for lossless
/// files; `libacm_rejects_level_0` pins that.
const LEVEL_0_ORACLES: [Oracle; 3] = [Oracle::Fallout2Ce, Oracle::GemRb, Oracle::Infinitier];

/// Every decoder's output beside `expected`: ours and each of `oracles`. Returns one line per decoder that fails,
/// differs in length, or differs in a sample (the first one). libacm and `infinitier` emit whole frames only, so on
/// a stereo header with an odd sample count they end one sample early; the engines and ours do not.
fn disagreements(acm: &[u8], oracles: &[Oracle], expected: &[i16]) -> Vec<String> {
    let channels = usize::from(u16::from_le_bytes([acm[8], acm[9]])).max(1);
    let whole_frames = expected.len() - expected.len() % channels;
    let ours = ipsdoc::acm::decode(acm)
        .map(|d| d.samples)
        .map_err(|e| format!("ipsdoc: {e}"));
    let theirs = oracles.iter().map(|&o| {
        let len = if matches!(o, Oracle::Libacm | Oracle::Infinitier) {
            whole_frames
        } else {
            expected.len()
        };
        (format!("{o:?}"), len, o.decode(acm).map_err(|e| e.to_string()))
    });
    std::iter::once(("ipsdoc".to_owned(), expected.len(), ours))
        .chain(theirs)
        .filter_map(|(name, len, result)| match result {
            Err(e) => Some(e),
            Ok(d) if d.len() != len => Some(format!("{name}: {} of {len} samples", d.len())),
            Ok(d) => d
                .iter()
                .zip(expected)
                .position(|(a, b)| a != b)
                .map(|i| format!("{name}: differs at sample {i}: {} != {}", d[i], expected[i])),
        })
        .collect()
}

fn check_lossless(name: &str, samples: &[i16], channels: u16, rate: u16) {
    let acm = writer::encode(samples, channels, rate, 0, 256);
    let failures = disagreements(&acm, &LEVEL_0_ORACLES, samples);
    assert!(failures.is_empty(), "{name}:\n{}", failures.join("\n"));
}

#[test]
fn full_scale_noise_mono_22050() {
    check_lossless("noise mono", &full_scale_noise(22050, 1), 1, 22050);
}

#[test]
fn full_scale_noise_stereo_44100() {
    check_lossless("noise stereo", &full_scale_noise(2 * 44100, 2), 2, 44100);
}

#[test]
fn quiet_speech_mono_22050() {
    check_lossless("speech mono", &quiet_speech(3 * 22050, 3), 1, 22050);
}

#[test]
fn quiet_speech_stereo_44100_partial_final_block() {
    check_lossless("speech stereo", &quiet_speech(2 * 44100 + 2 * 77, 4), 2, 44100);
}

#[test]
fn libacm_rejects_level_0() {
    let acm = writer::encode(&quiet_speech(22050, 5), 1, 22050, 0, 256);
    let err = Oracle::Libacm.decode(&acm).unwrap_err();
    // -3: libacm's "not an ACM stream".
    assert!(err.message.ends_with("-: rejected, error -3"), "{}", err.message);
}

/// Coefficients small enough that no decoder's output wraps, where 16-bit and wider arithmetic would differ.
fn coefficients(len: usize, seed: u64) -> Vec<i16> {
    let mut rng = Rng(seed);
    (0..len).map(|_| (rng.next() % 2001) as i16 - 1000).collect()
}

/// Compares every decoder against the Fallout 2 engine's output, which must itself be complete.
fn check_against_engine(name: &str, acm: &[u8], oracles: &[Oracle], samples: usize) -> Vec<String> {
    let reference = Oracle::Fallout2Ce.decode(acm).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(reference.len(), samples, "{name}: Fallout2Ce sample count");
    disagreements(acm, oracles, &reference)
        .into_iter()
        .map(|f| format!("{name}: {f}"))
        .collect()
}

#[test]
fn transformed_files_decode_identically_in_every_decoder() {
    let mut failures = Vec::new();
    for (levels, rows, channels) in [(1u16, 64u16, 1), (4, 32, 2), (7, 16, 2), (9, 8, 1)] {
        let len = (1 << levels) * usize::from(rows) * 5 + 2 * 51;
        let acm = writer::encode(&coefficients(len, levels.into()), channels, 22050, levels, rows);
        failures.extend(check_against_engine(
            &format!("levels {levels}"),
            &acm,
            &Oracle::ALL,
            len,
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Random but valid bitstreams using every filler the engines define except 31, which only the Fallout 2 engine's
/// decoder knows. The writer follows each filler's code tree only far enough to emit complete codes; what the codes
/// mean is left to the decoders, which must then agree.
mod random {
    use super::Rng;

    /// Per k-filler: for each prefix bit in turn, the code length and rows coded if that bit is 0; then the length
    /// when every prefix bit is 1 (one row).
    fn k_tree(filler: u32) -> (&'static [(u32, usize)], u32) {
        match filler {
            17 => (&[(1, 2), (2, 1)], 3),
            18 => (&[(1, 1)], 2),
            20 => (&[(1, 2), (2, 1)], 4),
            21 => (&[(1, 1)], 3),
            23 => (&[(1, 2), (2, 1), (4, 1)], 5),
            24 => (&[(1, 1), (3, 1)], 4),
            26 => (&[(1, 2), (2, 1)], 5),
            27 => (&[(1, 1)], 4),
            _ => panic!("{filler} is not a k-filler"),
        }
    }

    const FILLERS: [u32; 26] = [
        0, 3, 4, 5, 6, 7, 8, 9, 10, 17, 18, 19, 20, 21, 22, 23, 24, 26, 27, 29, 3, 4, 5, 17, 23, 29,
    ];

    struct Bits(Vec<u8>, u64, u32);

    impl Bits {
        fn put(&mut self, value: u64, bits: u32) {
            for i in 0..bits {
                self.1 |= ((value >> i) & 1) << self.2;
                self.2 += 1;
                if self.2 == 8 {
                    self.0.push(self.1 as u8);
                    (self.1, self.2) = (0, 0);
                }
            }
        }
    }

    /// A stream of `blocks` full blocks plus `extra` samples. `val` stays small and linear fillers narrow so that
    /// no table entry or output wraps, where decoders with 16-bit and wider arithmetic would legitimately differ.
    pub fn stream(
        rng: &mut Rng,
        levels: u16,
        rows: u16,
        channels: u16,
        blocks: usize,
        extra: usize,
    ) -> (Vec<u8>, usize) {
        let (cols, rows_n) = (1usize << levels, usize::from(rows));
        let samples = blocks * cols * rows_n + extra;
        let mut out = vec![0x97, 0x28, 0x03, 0x01];
        out.extend_from_slice(&u32::try_from(samples).unwrap().to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&22050u16.to_le_bytes());
        out.extend_from_slice(&(rows << 4 | levels).to_le_bytes());
        let mut bits = Bits(out, 0, 0);
        for _ in 0..samples.div_ceil(cols * rows_n) {
            // pwr 9 covers every index the fillers below can emit (linear fillers up to 10 bits).
            bits.put(9, 4);
            bits.put(1 + rng.next() % 4, 16);
            for _ in 0..cols {
                let filler = FILLERS[(rng.next() % FILLERS.len() as u64) as usize];
                bits.put(filler.into(), 5);
                let mut row = 0;
                while row < rows_n {
                    match filler {
                        0 => row = rows_n,
                        3..=16 => {
                            bits.put(rng.next(), filler);
                            row += 1;
                        }
                        19 | 22 | 29 => {
                            let (width, codes, per) = match filler {
                                19 => (5, 27, 3),
                                22 => (7, 125, 3),
                                _ => (7, 121, 2),
                            };
                            bits.put(rng.next() % codes, width);
                            row += per;
                        }
                        k => {
                            let (prefix, full) = k_tree(k);
                            let mut done = false;
                            for (i, &(len, n)) in prefix.iter().enumerate() {
                                let bit = rng.next() & 1;
                                bits.put(bit, 1);
                                if bit == 0 {
                                    // Prefix bits 0..=i are written (ones, then this zero); the rest is random.
                                    bits.put(rng.next(), len - (i as u32 + 1));
                                    row += n;
                                    done = true;
                                    break;
                                }
                            }
                            if !done {
                                bits.put(rng.next(), full - prefix.len() as u32);
                                row += 1;
                            }
                        }
                    }
                }
            }
        }
        if bits.2 > 0 {
            bits.0.push(bits.1 as u8);
        }
        (bits.0, samples)
    }
}

#[test]
fn random_valid_streams_decode_identically_in_every_decoder() {
    let mut rng = Rng(0x00AC_0FFE);
    let mut failures = Vec::new();
    for n in 0..120 {
        let levels = 1 + (rng.next() % 9) as u16;
        let rows = [2u16, 4, 6, 16, 30, 64][(rng.next() % 6) as usize];
        let channels = 1 + (rng.next() % 2) as u16;
        let (blocks, extra) = (1 + (rng.next() % 4) as usize, (rng.next() % 300) as usize);
        let (acm, samples) = random::stream(&mut rng, levels, rows, channels, blocks, extra);
        failures.extend(check_against_engine(
            &format!("stream {n} (levels {levels}, rows {rows})"),
            &acm,
            &Oracle::ALL,
            samples,
        ));
    }
    for n in 0..40 {
        let rows = [1u16, 2, 7, 64, 256][(rng.next() % 5) as usize];
        let (blocks, extra) = (1 + (rng.next() % 6) as usize, (rng.next() % 50) as usize);
        let (acm, samples) = random::stream(&mut rng, 0, rows, 1, blocks, extra);
        failures.extend(check_against_engine(
            &format!("level-0 stream {n} (rows {rows})"),
            &acm,
            &LEVEL_0_ORACLES,
            samples,
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
