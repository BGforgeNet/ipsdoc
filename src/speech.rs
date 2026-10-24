//! Where a speech sound is speaking: loudness per 10 ms frame against the line's own noise floor, the stretches of
//! speech, and the dips inside them, plus per-frame features for choosing mouth shapes.

use crate::pcm::Audio;

/// Frames per second.
pub const FRAMES_PER_SECOND: u32 = 100;

/// Tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechParams {
    /// Gaps shorter than this (seconds) inside speech are bridged.
    pub bridge: f32,
    /// Speech shorter than this (seconds) is dropped.
    pub min_speech: f32,
    /// Speech starts this far (as a share of the line's range from floor to peak) above the floor ...
    pub on: f32,
    /// ... and ends below this share.
    pub off: f32,
    /// A dip is at least this many dB below the loudest frame within 50 ms on each side.
    pub dip_depth: f32,
}

impl Default for SpeechParams {
    fn default() -> SpeechParams {
        SpeechParams {
            bridge: 0.1,
            min_speech: 0.03,
            on: 0.35,
            off: 0.25,
            dip_depth: 6.0,
        }
    }
}

/// One 10 ms frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Loudness, dB below full scale (smoothed over 30 ms).
    pub db: f32,
    /// Zero crossings per sample: high for hiss (s, f, sh), low for vowels.
    pub zcr: f32,
    /// Share of the energy below about 1 kHz.
    pub low: f32,
}

/// Frames `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// First frame.
    pub start: usize,
    /// One past the last frame.
    pub end: usize,
}

/// The analysis of one sound.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Per frame.
    pub frames: Vec<Frame>,
    /// Stretches of speech, in order.
    pub segments: Vec<Segment>,
    /// Frames, inside speech, where the loudness dips.
    pub dips: Vec<usize>,
}

/// The audio frame where 10 ms frame `f` starts: `round(f * rate / 100)`, exact over any length (220.5 audio frames
/// per frame at 22050 Hz would drift by 22 ms over ten seconds if rounded once).
fn frame_start(f: usize, rate: u32) -> usize {
    let per = u64::from(FRAMES_PER_SECOND);
    ((f as u64 * u64::from(rate) + per / 2) / per) as usize
}

/// Below this peak loudness (dB) a sound holds no speech at all.
const SILENT_PEAK: f32 = -60.0;
/// The floor is never closer than this (dB) under the peak, so a line with no pause still has a range.
const MIN_RANGE: f32 = 30.0;
/// The low band's corner (Hz).
const LOW_HZ: f32 = 1000.0;

/// Analyses `audio`.
pub fn analyse(audio: &Audio, p: &SpeechParams) -> Analysis {
    let channels = usize::from(audio.channels);
    let samples = audio.to_f32();
    let mono: Vec<f32> = samples
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect();
    let rate = audio.rate;
    let per = FRAMES_PER_SECOND as usize;
    let count = (mono.len() * per).div_ceil(rate as usize);
    let alpha = 1.0 - (-std::f32::consts::TAU * LOW_HZ / rate as f32).exp();
    let mut low_state = 0.0f32;
    let mut energy = Vec::with_capacity(count);
    let mut frames = Vec::with_capacity(count);
    for f in 0..count {
        let block = &mono[frame_start(f, rate).min(mono.len())..frame_start(f + 1, rate).min(mono.len())];
        let (mut e, mut low, mut crossings) = (0.0f32, 0.0f32, 0usize);
        for (i, &x) in block.iter().enumerate() {
            e += x * x;
            low_state += alpha * (x - low_state);
            low += low_state * low_state;
            if i > 0 && (x >= 0.0) != (block[i - 1] >= 0.0) {
                crossings += 1;
            }
        }
        let n = block.len().max(1) as f32;
        energy.push(e / n);
        frames.push(Frame {
            db: 0.0,
            zcr: crossings as f32 / n,
            low: if e > 0.0 { (low / e).min(1.0) } else { 0.0 },
        });
    }
    let db = |e: f32| 10.0 * e.max(1e-18).log10();
    let raw: Vec<f32> = energy.iter().map(|&e| db(e)).collect();
    for (f, frame) in frames.iter_mut().enumerate() {
        let window = &energy[f.saturating_sub(1)..(f + 2).min(energy.len())];
        frame.db = db(window.iter().sum::<f32>() / window.len() as f32);
    }
    let segments = segments(&raw, p);
    let dips = dips(&frames, &segments, p);
    Analysis { frames, segments, dips }
}

fn percentile(values: &[f32], q: f32) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    v.get(((v.len() as f32 - 1.0) * q).round() as usize)
        .copied()
        .unwrap_or(f32::NEG_INFINITY)
}

/// Speech by hysteresis between two thresholds over the line's own floor and peak, short gaps bridged, short bursts
/// dropped.
fn segments(db: &[f32], p: &SpeechParams) -> Vec<Segment> {
    let peak = percentile(db, 0.95);
    if peak < SILENT_PEAK {
        return Vec::new();
    }
    let floor = percentile(db, 0.1).min(peak - MIN_RANGE);
    let (on, off) = (floor + p.on * (peak - floor), floor + p.off * (peak - floor));
    let mut runs: Vec<Segment> = Vec::new();
    let mut start = None;
    for (f, &d) in db.iter().enumerate() {
        match start {
            None if d >= on => start = Some(f),
            Some(s) if d < off => {
                runs.push(Segment { start: s, end: f });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push(Segment {
            start: s,
            end: db.len(),
        });
    }
    let frames = |secs: f32| (secs * FRAMES_PER_SECOND as f32).round() as usize;
    let mut out: Vec<Segment> = Vec::new();
    for r in runs {
        match out.last_mut() {
            Some(last) if r.start - last.end < frames(p.bridge) => last.end = r.end,
            _ => out.push(r),
        }
    }
    out.retain(|s| s.end - s.start >= frames(p.min_speech).max(1));
    out
}

/// Inside each segment, the middle of each run of frames that lie `dip_depth` under the loudest frame within 50 ms
/// on both sides.
fn dips(frames: &[Frame], segments: &[Segment], p: &SpeechParams) -> Vec<usize> {
    const REACH: usize = 5;
    let mut out = Vec::new();
    for s in segments {
        let mut run: Option<(usize, usize)> = None;
        for f in s.start..s.end {
            let d = frames[f].db;
            let left = frames[f.saturating_sub(REACH).max(s.start)..f]
                .iter()
                .map(|x| x.db)
                .fold(f32::MIN, f32::max);
            let right = frames[f + 1..(f + 1 + REACH).min(s.end)]
                .iter()
                .map(|x| x.db)
                .fold(f32::MIN, f32::max);
            let dip = left - d >= p.dip_depth && right - d >= p.dip_depth;
            run = match (run, dip) {
                (None, true) => Some((f, f)),
                (Some((a, _)), true) => Some((a, f)),
                (Some((a, b)), false) => {
                    out.push((a + b) / 2);
                    None
                }
                (None, false) => None,
            };
        }
        if let Some((a, b)) = run {
            out.push((a + b) / 2);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 22050;

    /// `(seconds, amplitude)` pieces: a 180 Hz tone with harmonics at that amplitude, or silence at 0; with noise at
    /// `noise` amplitude throughout.
    fn signal(pieces: &[(f32, f32)], noise: f32) -> Vec<i16> {
        let mut out = Vec::new();
        let mut seed: u32 = 1;
        for &(secs, amp) in pieces {
            for _ in 0..(secs * RATE as f32).round() as usize {
                let t = out.len() as f32 / RATE as f32;
                let tone = (t * 180.0 * std::f32::consts::TAU).sin() + 0.5 * (t * 540.0 * std::f32::consts::TAU).sin();
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let n = ((seed >> 16) as f32 / 32768.0 - 1.0) * noise;
                out.push((amp * tone + n).clamp(-32767.0, 32767.0) as i16);
            }
        }
        out
    }

    fn mono(pieces: &[(f32, f32)], noise: f32) -> Audio {
        Audio::from_i16(RATE, 1, signal(pieces, noise))
    }

    /// Segment bounds in seconds.
    fn bounds(a: &Analysis) -> Vec<(f32, f32)> {
        a.segments
            .iter()
            .map(|s| (s.start as f32 / 100.0, s.end as f32 / 100.0))
            .collect()
    }

    fn near(got: &[(f32, f32)], want: &[(f32, f32)]) {
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g.0 - w.0).abs() <= 0.011 && (g.1 - w.1).abs() <= 0.011,
                "{got:?} vs {want:?}"
            );
        }
    }

    const BURSTS: [(f32, f32); 5] = [(0.3, 0.0), (0.5, 8000.0), (0.06, 0.0), (0.4, 8000.0), (0.15, 0.0)];

    #[test]
    fn finds_speech_and_bridges_short_gaps() {
        let a = analyse(&mono(&BURSTS, 0.0), &SpeechParams::default());
        near(&bounds(&a), &[(0.3, 1.26)]);
        let tight = SpeechParams {
            bridge: 0.05,
            ..SpeechParams::default()
        };
        let a = analyse(&mono(&BURSTS, 0.0), &tight);
        near(&bounds(&a), &[(0.3, 0.8), (0.86, 1.26)]);
    }

    #[test]
    fn keeps_a_long_gap() {
        let pieces = [(0.2, 0.0), (0.4, 8000.0), (0.15, 0.0), (0.4, 8000.0), (0.2, 0.0)];
        let a = analyse(&mono(&pieces, 0.0), &SpeechParams::default());
        near(&bounds(&a), &[(0.2, 0.6), (0.75, 1.15)]);
    }

    #[test]
    fn adapts_to_the_noise_floor_and_level() {
        // Noise 40 dB under the louder burst; the second burst 6 dB quieter.
        let pieces = [(0.2, 0.0), (0.4, 8000.0), (0.15, 0.0), (0.4, 4000.0), (0.2, 0.0)];
        let a = analyse(&mono(&pieces, 80.0), &SpeechParams::default());
        near(&bounds(&a), &[(0.2, 0.6), (0.75, 1.15)]);
    }

    #[test]
    fn finds_a_dip_inside_speech() {
        let pieces = [(0.2, 0.0), (0.3, 8000.0), (0.04, 1000.0), (0.3, 8000.0), (0.2, 0.0)];
        let a = analyse(&mono(&pieces, 0.0), &SpeechParams::default());
        near(&bounds(&a), &[(0.2, 0.84)]);
        assert_eq!(a.dips.len(), 1, "{:?}", a.dips);
        assert!((a.dips[0] as f32 / 100.0 - 0.52).abs() <= 0.021, "{:?}", a.dips);
    }

    #[test]
    fn stereo_reads_as_its_mix() {
        let m = signal(&BURSTS, 0.0);
        let st: Vec<i16> = m.iter().flat_map(|&s| [s, s]).collect();
        let (am, ast) = (
            analyse(&Audio::from_i16(RATE, 1, m), &SpeechParams::default()),
            analyse(&Audio::from_i16(RATE, 2, st), &SpeechParams::default()),
        );
        assert_eq!(am.segments, ast.segments);
        assert_eq!(am.dips, ast.dips);
    }

    #[test]
    fn hiss_has_more_zero_crossings_than_a_vowel() {
        let mut seed: u32 = 7;
        let hiss: Vec<i16> = (0..RATE / 2)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((seed >> 16) as i32 - 32768) as i16 / 4
            })
            .collect();
        let tone = signal(&[(0.5, 8000.0)], 0.0);
        let p = SpeechParams::default();
        let h = analyse(&Audio::from_i16(RATE, 1, hiss), &p);
        let t = analyse(&Audio::from_i16(RATE, 1, tone), &p);
        let (hz, tz) = (h.frames[20].zcr, t.frames[20].zcr);
        assert!(hz > 5.0 * tz, "{hz} vs {tz}");
        assert!(t.frames[20].low > 0.9 && h.frames[20].low < 0.5);
    }

    #[test]
    fn silence_has_no_speech() {
        let a = analyse(&mono(&[(1.0, 0.0)], 0.0), &SpeechParams::default());
        assert!(a.segments.is_empty());
        assert_eq!(a.frames.len(), 100);
    }
}
