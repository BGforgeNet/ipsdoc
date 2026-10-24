//! `fixtures <dir>`: writes the WAV files the cross-tool comparison feeds other encoders, deterministically.

use std::f64::consts::TAU;
use std::process::ExitCode;

use ipsdoc::output::{OutputFormat, WriteOptions, write};
use ipsdoc::pcm::Audio;

/// xorshift64*, so the fixtures are the same on every runner.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Name, rate, channels, and the sample at (frame, channel).
type Fixture = (&'static str, u32, u16, Box<dyn Fn(usize, usize) -> f64>);

fn fixtures() -> Vec<Fixture> {
    let mut rng = Rng(17);
    let noise: Vec<f64> = (0..44100 * 3).map(|_| rng.next() * 2.0 - 1.0).collect();
    let noise2 = noise.clone();
    vec![
        // Speech-like: a pitched, amplitude-modulated tone with pauses.
        (
            "speech-mono-22050",
            22050,
            1,
            Box::new(|n, _| {
                let t = n as f64 / 22050.0;
                let gate = if (t * 2.0).fract() < 0.7 { 1.0 } else { 0.0 };
                gate * 0.3 * (TAU * 140.0 * t).sin() * (1.0 + (TAU * 3.0 * t).sin()) / 2.0
            }),
        ),
        // Music-like: two different chords, one per channel.
        (
            "music-stereo-44100",
            44100,
            2,
            Box::new(|n, c| {
                let t = n as f64 / 44100.0;
                let f = if c == 0 {
                    [220.0, 277.2, 329.6]
                } else {
                    [196.0, 246.9, 293.7]
                };
                f.iter().map(|f| 0.2 * (TAU * f * t).sin()).sum()
            }),
        ),
        // A loud effect near full scale.
        (
            "loud-mono-22050",
            22050,
            1,
            Box::new(move |n, _| 0.97 * noise[n % noise.len()]),
        ),
        // Quiet stereo noise, a few steps wide.
        (
            "quiet-stereo-22050",
            22050,
            2,
            Box::new(move |n, c| 0.0005 * noise2[(2 * n + c) % noise2.len()]),
        ),
    ]
}

#[expect(
    clippy::print_stderr,
    reason = "a harness the cross-tool scripts run, never piped: its only output is an error, on stderr"
)]
fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: fixtures <dir>");
        return ExitCode::from(2);
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("fixtures: {dir}: {e}");
        return ExitCode::FAILURE;
    }
    for (name, rate, channels, f) in fixtures() {
        let frames = rate as usize * 2;
        let samples = (0..frames * usize::from(channels))
            .map(|i| (f(i / usize::from(channels), i % usize::from(channels)) * 32767.0).round() as i16)
            .collect();
        let audio = Audio::from_i16(rate, channels, samples);
        let path = std::path::Path::new(&dir).join(format!("{name}.wav"));
        let written = write(&audio, OutputFormat::Riff, &WriteOptions::default())
            .map_err(|e| e.to_string())
            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()));
        if let Err(e) = written {
            eprintln!("fixtures: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
