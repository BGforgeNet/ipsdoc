//! Every reader of files from game installs and mods, fed damaged copies of valid files and random bytes: each
//! returns, as a value or an error, and none panics. Seeded, so a failure reproduces.

use ipsdoc::input::{ReadOptions, read};
use ipsdoc::lip::{Lip, Marker};
use ipsdoc::output::{OutputFormat, WriteOptions, write};
use ipsdoc::pcm::Audio;
use ipsdoc::{acm, msg, repair, wavc};

/// xorshift64*, so the inputs are reproducible without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A damaged copy of `valid`: a few bytes overwritten, half of them in the first 64, where the headers that size
/// everything else sit; then perhaps cut short or extended with noise.
fn damaged(rng: &mut Rng, valid: &[u8]) -> Vec<u8> {
    let mut b = valid.to_vec();
    for _ in 0..1 + rng.below(4) {
        let span = if rng.below(2) == 0 { b.len().min(64) } else { b.len() };
        let at = rng.below(span);
        b[at] = rng.next() as u8;
    }
    match rng.below(3) {
        0 => b.truncate(rng.below(b.len() + 1)),
        1 => b.extend((0..rng.below(64)).map(|_| rng.next() as u8)),
        _ => {}
    }
    b
}

fn noise(rng: &mut Rng) -> Vec<u8> {
    (0..rng.below(256)).map(|_| rng.next() as u8).collect()
}

/// A twentieth of a second of a stereo tone at 22050 Hz: short, so each of the many reads is quick.
fn sound() -> Audio {
    let samples = (0..2206)
        .map(|i| (4000.0 * ((i / 2) as f64 * 0.05 * (1 + i % 2) as f64).sin()) as i16)
        .collect();
    Audio::from_i16(22050, 2, samples)
}

/// Every reader, on `bytes`; the results are dropped, a panic fails the test.
fn read_all(bytes: &[u8]) {
    let _ = acm::decode(bytes);
    let _ = wavc::Header::parse(bytes);
    let _ = wavc::acm_of(bytes).map(acm::decode);
    let _ = read(bytes, &ReadOptions::default());
    let _ = repair::fixes(bytes);
    if let Ok(l) = Lip::parse(bytes) {
        let _ = l.validate(Some("x"));
    }
    if let Ok(entries) = msg::parse(bytes) {
        let _ = msg::find(&entries, "x");
    }
}

#[test]
fn no_reader_panics_on_damaged_or_random_input() {
    let audio = sound();
    let opts = WriteOptions::default();
    let acm_file = write(&audio, OutputFormat::Acm, &opts).unwrap();
    let seeds = [
        acm_file.clone(),
        wavc::wrap(&acm_file).unwrap(),
        write(&audio, OutputFormat::Riff, &opts).unwrap(),
        write(&audio, OutputFormat::Ogg, &opts).unwrap(),
        Lip::new(
            "x",
            vec![0, 1],
            vec![
                Marker { kind: 1, position: 0 },
                Marker { kind: 1, position: 400 },
                Marker { kind: 1, position: 800 },
            ],
        )
        .to_bytes(),
        b"{100}{x}{Hello there.}\n{101}{}{No sound.}\n".to_vec(),
    ];
    // IPSDOC_MALFORMED_ROUNDS sets a longer sweep.
    let rounds = std::env::var("IPSDOC_MALFORMED_ROUNDS").map_or(300, |r| r.parse().unwrap());
    let mut rng = Rng(0x1950_ACDC);
    for seed in &seeds {
        read_all(seed);
        for _ in 0..rounds {
            read_all(&damaged(&mut rng, seed));
        }
    }
    for _ in 0..rounds {
        read_all(&noise(&mut rng));
    }
}
