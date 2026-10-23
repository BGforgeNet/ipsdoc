//! Lip-sync generation: a sound, and optionally its line's phonemes, to a lip file.
//!
//! With text, the line's phrases are matched to the stretches of speech found in the sound, words get time by the
//! typical length of their phonemes, and word boundaries move to nearby dips in loudness. Without text, shapes come
//! from the sound alone. Either way, neighbours showing the same head frame merge, shapes are held a minimum time,
//! changes are capped per second, and pauses show the silent mouth.

use crate::g2p::{self, Token};
use crate::lip::{Lip, Marker};
use crate::pcm::Audio;
use crate::speech::{self, SpeechParams};

/// The head frame the engine shows for each phoneme code.
pub const HEAD_FRAME: [u8; 42] = [
    0, 3, 1, 1, 3, 1, 1, 1, 7, 8, 7, 3, 1, 8, 1, 7, 7, 6, 6, 2, 2, 2, 2, 4, 4, 5, 5, 2, 2, 2, 2, 2, 6, 2, 2, 5, 8, 2,
    2, 2, 2, 8,
];

/// Tuning (times in seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Speech detection.
    pub speech: SpeechParams,
    /// A word boundary moves to a dip this close.
    pub snap: f32,
    /// The shortest a shape is held ...
    pub hold: f32,
    /// ... except closed lips (m, p, b), held at least this long.
    pub closed_hold: f32,
    /// At most this many shape changes per second.
    pub max_rate: f32,
    /// Pauses inside a phrase longer than this show the silent mouth.
    pub silence: f32,
    /// Shapes start this much before their sound, as the original lip files' do.
    pub lead: f32,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            speech: SpeechParams::default(),
            snap: 0.04,
            hold: 0.07,
            closed_hold: 0.04,
            max_rate: 8.0,
            silence: 0.25,
            lead: 0.06,
        }
    }
}

/// The lip file for `audio`, a sound named `name`, from its line's `tokens` when there are any.
pub fn generate(name: &str, audio: &Audio, tokens: Option<&[Token]>, p: &Params) -> Lip {
    let a = speech::analyse(audio, &p.speech);
    let total = audio.frames() as f64 * f64::from(speech::FRAMES_PER_SECOND) / f64::from(audio.rate.max(1));
    let phrases = tokens.map(phrases).unwrap_or_default();
    let mut shapes = vec![Shape { start: 0.0, code: 0 }];
    if phrases.is_empty() {
        audio_shapes(&a, &mut shapes);
    } else {
        text_shapes(&a, &phrases, p, &mut shapes);
    }
    let mut shapes = clean(shapes, total, &a, p);
    let lead = f64::from(p.lead) * f64::from(speech::FRAMES_PER_SECOND);
    for s in shapes.iter_mut().skip(1) {
        s.start = (s.start - lead).max(0.0);
    }
    // A shape moved to the very start replaces the opening silence.
    while shapes.len() > 1 && shapes[1].start <= 0.0 {
        shapes.remove(0);
    }
    to_lip(name, audio, &shapes)
}

/// A shape from `start` (in 10 ms frames) to the next shape's start.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    start: f64,
    code: u8,
}

impl Shape {
    fn frame(self) -> u8 {
        HEAD_FRAME[usize::from(self.code.min(41))]
    }
}

/// Closed lips: m, p, b.
const CLOSED: u8 = 6;
/// The shape for a vocal noise: an open mouth (A as in father).
const NOISE_CODE: u8 = 6;

/// A placeable unit: phonemes with their relative lengths.
#[derive(Debug, Clone)]
struct Word(Vec<(u8, f64)>);

impl Word {
    fn weight(&self) -> f64 {
        self.0.iter().map(|&(_, w)| w).sum()
    }
}

/// The line split at its breaks, empty phrases dropped.
fn phrases(tokens: &[Token]) -> Vec<Vec<Word>> {
    let mut out = vec![Vec::new()];
    for t in tokens {
        match t {
            Token::Word(phones) => {
                if let Some(last) = out.last_mut() {
                    last.push(Word(
                        phones
                            .iter()
                            .map(|&ph| {
                                let c = g2p::code(ph);
                                (c, f64::from(CODE_MS[usize::from(c)]))
                            })
                            .collect(),
                    ));
                }
            }
            Token::Noise => {
                if let Some(last) = out.last_mut() {
                    last.push(Word(vec![(NOISE_CODE, NOISE_MS)]));
                }
            }
            Token::Break => out.push(Vec::new()),
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

/// Each code's typical length (ms): the median length of its shapes in Fallout 2's 1029 original lip files (shapes
/// over 500 ms, held through a pause, left out). Nearly even: the original tool spaced shapes between anchors.
const CODE_MS: [u16; 42] = [
    100, 112, 105, 124, 115, 113, 116, 111, 141, 143, 106, 111, 111, 96, 136, 138, 158, 102, 99, 91, 93, 99, 95, 108,
    106, 113, 97, 111, 104, 117, 103, 95, 105, 97, 98, 101, 104, 88, 91, 110, 103, 123,
];
/// A vocal noise (a cough, a laugh) takes about two shapes' time.
const NOISE_MS: f64 = 220.0;

/// Splits `n` items into `groups` contiguous non-empty groups (`n >= groups >= 1`) so that each group's share of the
/// items' total size is as close as can be to the share of its counterpart; returns each group's end.
fn partition(sizes: &[f64], targets: &[f64]) -> Vec<usize> {
    let (n, g) = (sizes.len(), targets.len());
    let (total, target_total) = (
        sizes.iter().sum::<f64>().max(1e-9),
        targets.iter().sum::<f64>().max(1e-9),
    );
    let mut prefix = vec![0.0; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + sizes[i] / total;
    }
    // cost[k][j]: best for the first k groups covering the first j items.
    let mut cost = vec![vec![f64::INFINITY; n + 1]; g + 1];
    let mut from = vec![vec![0; n + 1]; g + 1];
    cost[0][0] = 0.0;
    for k in 1..=g {
        let want = targets[k - 1] / target_total;
        for j in k..=n - (g - k) {
            for i in (k - 1)..j {
                let c = cost[k - 1][i] + (prefix[j] - prefix[i] - want).powi(2);
                if c < cost[k][j] {
                    cost[k][j] = c;
                    from[k][j] = i;
                }
            }
        }
    }
    let mut ends = vec![n; g];
    let mut j = n;
    for k in (1..=g).rev() {
        ends[k - 1] = j;
        j = from[k][j];
    }
    ends
}

/// Phrases laid over speech: phrases matched to stretches of speech, words to time by weight, word starts moved to
/// dips; silent mouth between matched groups and over long pauses inside one.
fn text_shapes(a: &speech::Analysis, phrases: &[Vec<Word>], p: &Params, out: &mut Vec<Shape>) {
    let segs: Vec<(f64, f64)> = a.segments.iter().map(|s| (s.start as f64, s.end as f64)).collect();
    if segs.is_empty() {
        return;
    }
    let pw: Vec<f64> = phrases.iter().map(|ph| ph.iter().map(Word::weight).sum()).collect();
    let sd: Vec<f64> = segs.iter().map(|&(s, e)| e - s).collect();
    // Pairs of (phrase range, segment range).
    let mut pairs = Vec::new();
    if phrases.len() <= segs.len() {
        let mut s0 = 0;
        for (k, end) in partition(&sd, &pw).into_iter().enumerate() {
            pairs.push((k..k + 1, s0..end));
            s0 = end;
        }
    } else {
        let mut p0 = 0;
        for (k, end) in partition(&pw, &sd).into_iter().enumerate() {
            pairs.push((p0..end, k..k + 1));
            p0 = end;
        }
    }
    let silence = f64::from(p.silence) * f64::from(speech::FRAMES_PER_SECOND);
    let snap = f64::from(p.snap) * f64::from(speech::FRAMES_PER_SECOND);
    for (prange, srange) in pairs {
        let group = &segs[srange];
        let words: Vec<&Word> = phrases[prange].iter().flatten().collect();
        let total_w: f64 = words.iter().map(|w| w.weight()).sum::<f64>().max(1e-9);
        let speech_len: f64 = group.iter().map(|&(s, e)| e - s).sum();
        // Time at a share of the group's speech, skipping its pauses.
        let at = |share: f64| -> f64 {
            let mut left = share.clamp(0.0, 1.0) * speech_len;
            for &(s, e) in group {
                if left <= e - s {
                    return s + left;
                }
                left -= e - s;
            }
            group.last().map_or(0.0, |&(_, e)| e)
        };
        let mut bounds = Vec::with_capacity(words.len() + 1);
        let mut cum = 0.0;
        bounds.push(at(0.0));
        for w in &words {
            cum += w.weight();
            bounds.push(at(cum / total_w));
        }
        let last = bounds.len() - 1;
        for b in &mut bounds[1..last] {
            if let Some(&d) = a
                .dips
                .iter()
                .filter(|&&d| (d as f64 - *b).abs() <= snap)
                .min_by(|&&x, &&y| (x as f64 - *b).abs().total_cmp(&(y as f64 - *b).abs()))
            {
                *b = d as f64;
            }
        }
        for (i, w) in words.iter().enumerate() {
            let (start, end) = (bounds[i], bounds[i + 1].max(bounds[i]));
            let ww = w.weight().max(1e-9);
            let mut t = start;
            for &(code, weight) in &w.0 {
                out.push(Shape { start: t, code });
                t += (end - start) * weight / ww;
            }
        }
        // Long pauses inside the group, and the group's end.
        for pair in group.windows(2) {
            let (gap_start, gap_end) = (pair[0].1, pair[1].0);
            if gap_end - gap_start > silence {
                let resume = out.iter().rev().find(|s| s.start <= gap_end).map_or(0, |s| s.code);
                out.push(Shape {
                    start: gap_start,
                    code: 0,
                });
                out.push(Shape {
                    start: gap_end,
                    code: resume,
                });
            }
        }
        if let Some(&(_, end)) = group.last() {
            out.push(Shape { start: end, code: 0 });
        }
    }
}

/// Shapes from the sound alone, frame by frame inside speech: closed lips at dips, a hiss shape for noisy frames,
/// and openness by loudness under the stretch's peak.
fn audio_shapes(a: &speech::Analysis, out: &mut Vec<Shape>) {
    for s in &a.segments {
        let peak = a.frames[s.start..s.end].iter().map(|f| f.db).fold(f32::MIN, f32::max);
        for f in s.start..s.end {
            let fr = a.frames[f];
            let code = if a.dips.contains(&f) {
                32
            } else if fr.zcr > 0.15 && fr.low < 0.5 {
                27
            } else if fr.db > peak - 6.0 {
                6
            } else if fr.db > peak - 14.0 {
                4
            } else {
                13
            };
            if out.last().is_none_or(|l| l.code != code) {
                out.push(Shape { start: f as f64, code });
            }
        }
        out.push(Shape {
            start: s.end as f64,
            code: 0,
        });
    }
}

/// Sorted, merged by head frame, held long enough, and capped in rate.
fn clean(mut shapes: Vec<Shape>, total: f64, a: &speech::Analysis, p: &Params) -> Vec<Shape> {
    let fps = f64::from(speech::FRAMES_PER_SECOND);
    shapes.retain(|s| s.start < total);
    shapes.sort_by(|x, y| x.start.total_cmp(&y.start));
    // Of shapes starting together, the last placed wins.
    let mut v: Vec<Shape> = Vec::with_capacity(shapes.len());
    for s in shapes {
        match v.last_mut() {
            Some(l) if (l.start - s.start).abs() < 1e-9 => *l = s,
            _ => v.push(s),
        }
    }
    if v.first().is_none_or(|s| s.start > 0.0) {
        v.insert(0, Shape { start: 0.0, code: 0 });
    }
    merge_same(&mut v);
    let len = |v: &[Shape], i: usize| v.get(i + 1).map_or(total, |n| n.start) - v[i].start;
    let (hold, closed_hold) = (f64::from(p.hold) * fps, f64::from(p.closed_hold) * fps);
    // Shapes inside the line that are too short go to their longer neighbour; closed lips first borrow time.
    loop {
        let short = (1..v.len().saturating_sub(1))
            .filter(|&i| {
                let least = if v[i].frame() == CLOSED { closed_hold } else { hold };
                len(&v, i) < least - 1e-9
            })
            .min_by(|&x, &y| len(&v, x).total_cmp(&len(&v, y)));
        let Some(i) = short else { break };
        if v[i].frame() == CLOSED {
            let need = closed_hold - len(&v, i);
            if len(&v, i + 1) - need >= hold && i + 2 <= v.len() {
                v[i + 1].start += need;
                continue;
            }
            if len(&v, i - 1) - need >= hold {
                v[i].start -= need;
                continue;
            }
        }
        remove(&mut v, i, total);
    }
    let speech_secs: f64 = a.segments.iter().map(|s| (s.end - s.start) as f64).sum::<f64>() / fps;
    let allowed = (f64::from(p.max_rate) * speech_secs).ceil() as usize + 2;
    while changes(&v) > allowed && v.len() > 2 {
        let i = (1..v.len() - 1)
            .filter(|&i| v[i].code != 0)
            .min_by(|&x, &y| len(&v, x).total_cmp(&len(&v, y)));
        let Some(i) = i else { break };
        remove(&mut v, i, total);
    }
    v
}

fn changes(v: &[Shape]) -> usize {
    v.windows(2).filter(|w| w[0].frame() != w[1].frame()).count()
}

/// Neighbours showing the same head frame become one.
fn merge_same(v: &mut Vec<Shape>) {
    v.dedup_by(|next, prev| next.frame() == prev.frame());
}

/// Drops shape `i`, giving its time to its longer neighbour, then merges what now touches.
fn remove(v: &mut Vec<Shape>, i: usize, total: f64) {
    let len = |v: &[Shape], i: usize| v.get(i + 1).map_or(total, |n| n.start) - v[i].start;
    if i + 1 < v.len() && len(v, i + 1) > len(v, i - 1) {
        v[i + 1].start = v[i].start;
    }
    v.remove(i);
    merge_same(v);
}

/// The shapes as a lip file: positions 2 x the interleaved sample index, the last at the sound's end; at least
/// six markers.
fn to_lip(name: &str, audio: &Audio, shapes: &[Shape]) -> Lip {
    let channels = i64::from(audio.channels.max(1));
    let last = 2 * audio.len() as i64;
    let position = |t: f64| -> i32 {
        let frame = (t * f64::from(audio.rate) / f64::from(speech::FRAMES_PER_SECOND)).round() as i64;
        (2 * channels * frame).clamp(0, last) as i32
    };
    let mut phonemes: Vec<u8> = shapes.iter().map(|s| s.code).collect();
    let mut markers: Vec<Marker> = shapes
        .iter()
        .map(|s| Marker {
            kind: 1,
            position: position(s.start),
        })
        .collect();
    while markers.len() + 1 < 6 {
        phonemes.push(0);
        markers.push(Marker {
            kind: 1,
            position: last as i32,
        });
    }
    markers.push(Marker {
        kind: 1,
        position: last as i32,
    });
    Lip::new(name, phonemes, markers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g2p::tokens;

    const RATE: u32 = 22050;

    /// `(seconds, amplitude)` pieces of a voiced tone, or silence at amplitude 0.
    fn sound(pieces: &[(f32, f32)]) -> Audio {
        let mut out: Vec<i16> = Vec::new();
        for &(secs, amp) in pieces {
            for _ in 0..(secs * RATE as f32).round() as usize {
                let t = out.len() as f32 / RATE as f32;
                let v = (t * 150.0 * std::f32::consts::TAU).sin() + 0.4 * (t * 450.0 * std::f32::consts::TAU).sin();
                out.push((amp * v) as i16);
            }
        }
        Audio::from_i16(RATE, 1, out)
    }

    /// The head frame shown at `secs`.
    fn frame_at(lip: &Lip, secs: f32) -> u8 {
        let pos = 2 * (secs * RATE as f32) as i32;
        let i = lip.markers.iter().rposition(|m| m.position <= pos).unwrap_or(0);
        HEAD_FRAME[usize::from(lip.phonemes.get(i).copied().unwrap_or(0).min(41))]
    }

    /// Each shape's head frame and length in seconds.
    fn shapes(lip: &Lip) -> Vec<(u8, f32)> {
        lip.phonemes
            .iter()
            .zip(lip.markers.windows(2))
            .map(|(&c, w)| {
                (
                    HEAD_FRAME[usize::from(c)],
                    (w[1].position - w[0].position) as f32 / 2.0 / RATE as f32,
                )
            })
            .collect()
    }

    fn check_valid(lip: &Lip, audio: &Audio) {
        assert_eq!(lip.validate(Some("test")), vec![], "{lip:?}");
        assert_eq!(lip.last(), 2 * audio.len() as i32);
    }

    const LINE: [(f32, f32); 3] = [(0.3, 0.0), (0.6, 9000.0), (0.3, 0.0)];

    #[test]
    fn text_shows_closed_lips_and_holds_shapes() {
        let audio = sound(&LINE);
        let t = tokens("map be");
        let lip = generate("test", &audio, Some(&t), &Params::default());
        check_valid(&lip, &audio);
        let s = shapes(&lip);
        let inner = &s[1..s.len() - 1];
        assert!(inner.iter().any(|&(f, _)| f == 6), "m, p or b closes the lips: {s:?}");
        for &(f, len) in inner {
            let least = if f == 6 { 0.04 } else { 0.07 };
            assert!(len >= least - 0.011, "{f} held {len}: {s:?}");
        }
        assert_eq!(frame_at(&lip, 0.1), 0);
        assert_eq!(frame_at(&lip, 1.1), 0);
    }

    #[test]
    fn caps_the_change_rate() {
        let audio = sound(&LINE);
        let t = tokens("a b c d e f g h i j k l m n o p");
        let lip = generate("test", &audio, Some(&t), &Params::default());
        check_valid(&lip, &audio);
        let changes = shapes(&lip).windows(2).filter(|w| w[0].0 != w[1].0).count();
        assert!(changes as f32 <= 8.0 * 1.2 + 2.0, "{changes} changes over 1.2 s");
    }

    #[test]
    fn audio_only_opens_on_speech_and_closes_in_silence() {
        let audio = sound(&LINE);
        let lip = generate("test", &audio, None, &Params::default());
        check_valid(&lip, &audio);
        assert_eq!(frame_at(&lip, 0.1), 0);
        assert_ne!(frame_at(&lip, 0.6), 0);
        assert_eq!(frame_at(&lip, 1.1), 0);
    }

    #[test]
    fn a_short_line_is_padded_to_six_markers() {
        let audio = sound(&[(0.05, 0.0), (0.1, 9000.0), (0.05, 0.0)]);
        let t = tokens("oh");
        let lip = generate("test", &audio, Some(&t), &Params::default());
        check_valid(&lip, &audio);
        assert!(lip.markers.len() >= 6);
    }

    #[test]
    fn a_break_over_a_pause_is_silent_and_one_in_speech_is_not() {
        let audio = sound(&[(0.2, 0.0), (0.5, 9000.0), (0.2, 0.0), (0.5, 9000.0), (0.2, 0.0)]);
        let lip = generate("test", &audio, Some(&tokens("one two, three four")), &Params::default());
        check_valid(&lip, &audio);
        assert_eq!(frame_at(&lip, 0.8), 0, "the pause holds the comma");
        let joined = sound(&[(0.2, 0.0), (1.2, 9000.0), (0.2, 0.0)]);
        let lip = generate(
            "test",
            &joined,
            Some(&tokens("one two, three four")),
            &Params::default(),
        );
        check_valid(&lip, &joined);
        for t in [0.3, 0.5, 0.7, 0.9, 1.1, 1.3] {
            assert_ne!(frame_at(&lip, t), 0, "speech at {t}");
        }
    }

    #[test]
    fn stereo_positions_count_both_channels() {
        let m = sound(&LINE);
        let st = Audio::from_i16(RATE, 2, m.to_i16().iter().flat_map(|&s| [s, s]).collect());
        let lip = generate("test", &st, Some(&tokens("map be")), &Params::default());
        assert_eq!(lip.last(), 2 * st.len() as i32);
        assert_eq!(lip.validate(None), vec![]);
    }

    #[test]
    fn names_the_sound() {
        let audio = sound(&LINE);
        let lip = generate("cas0103", &audio, None, &Params::default());
        assert_eq!(lip.name(), "CAS0103");
    }
}
