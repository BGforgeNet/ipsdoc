//! The reference decoders against hand-built ACM files: lossless (`levels = 0`) files decode to their input exactly,
//! and transformed (`levels > 0`) files decode identically in every decoder.
#![expect(clippy::unwrap_used, reason = "fixture helpers outside #[test] fns: a panic is the failure report")]

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

fn check_lossless(name: &str, samples: &[i16], channels: u16, rate: u16) {
    let acm = writer::encode(samples, channels, rate, 0, 256);
    let failures: Vec<String> = LEVEL_0_ORACLES
        .into_iter()
        .filter_map(|oracle| match oracle.decode(&acm) {
            Err(e) => Some(e.to_string()),
            Ok(d) if d.len() != samples.len() => Some(format!("{oracle:?}: {} of {} samples", d.len(), samples.len())),
            Ok(d) => d
                .iter()
                .zip(samples)
                .position(|(a, b)| a != b)
                .map(|i| format!("{oracle:?}: differs at sample {i}: {} != {}", d[i], samples[i])),
        })
        .collect();
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
    // ACM_ERR_NOT_ACM
    assert_eq!(err.message, "error code -3");
}

/// Coefficients small enough that no decoder's output wraps, where 16-bit and wider arithmetic would differ.
fn coefficients(len: usize, seed: u64) -> Vec<i16> {
    let mut rng = Rng(seed);
    (0..len).map(|_| (rng.next() % 2001) as i16 - 1000).collect()
}

#[test]
fn transformed_files_decode_identically_in_every_decoder() {
    let mut failures = Vec::new();
    for (levels, rows, channels) in [(1u16, 64u16, 1), (4, 32, 2), (7, 16, 2), (9, 8, 1)] {
        let len = (1 << levels) * usize::from(rows) * 5 + 2 * 51;
        let acm = writer::encode(&coefficients(len, levels.into()), channels, 22050, levels, rows);
        let decoded: Vec<_> = Oracle::ALL.into_iter().map(|o| (o, o.decode(&acm))).collect();
        let (first, reference) = (decoded[0].0, decoded[0].1.as_ref().unwrap());
        assert_eq!(reference.len(), len, "levels {levels}: {first:?} sample count");
        for (oracle, result) in &decoded[1..] {
            match result {
                Err(e) => failures.push(format!("levels {levels}: {e}")),
                Ok(d) if d != reference => failures.push(format!("levels {levels}: {oracle:?} differs from {first:?}")),
                Ok(_) => {}
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
