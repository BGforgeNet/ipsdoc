//! Places a line's sounds on its speech: a forced alignment over the whole line. Each frame is scored against each
//! sound's typical features ([`features`]: Gaussians fitted per sound), each sound's length against its typical length
//! (log-normal), with an optional pause between words, a cheaper one at a phrase break, and optional speech the text
//! lacks before and after it. Without text, [`recognise`] picks the likeliest run of any sounds the same way, each
//! sound following the one before as often as speech has it.

mod model;

use crate::g2p::{self, Token};
use crate::speech::{Analysis, CEPSTRA};
use model::{DUR, MEAN, NEXT, VAR};

/// Features per frame: [`LOUDNESS`] of them from the loudness, then the cepstrum and its slope.
pub const FEATURES: usize = LOUDNESS + 2 * CEPSTRA;
/// The features that come from the loudness.
pub const LOUDNESS: usize = 5;
/// Model classes: the 39 ARPAbet sounds in [`g2p::Arpabet`] order, then a vocal noise, then silence.
pub const CLASSES: usize = 41;
/// The class of a vocal noise.
pub const NOISE: usize = 39;
/// The class of silence.
pub const SILENCE: usize = 40;
/// The code shown for a vocal noise: an open mouth (A as in father).
const NOISE_CODE: u8 = 6;
/// A pause between words costs this much against the frames' fit, so the alignment pauses only where the sound does ...
const PAUSE_COST: f64 = 10.0;
/// ... and at a phrase break this much: a comma or a full stop is often a pause.
const BREAK_PAUSE_COST: f64 = 2.0;
/// The longest pause inside a line (frames).
const MAX_PAUSE: usize = 200;
/// Speech the text lacks before or after it costs this much against the frames' fit. Without it, a line opening with
/// 3.5 s of speech its text leaves out showed the mouth shut through it; allowed at phrase breaks too, it took breaths
/// for speech and opened the mouth through more of the silence (against forced alignments, 4.0% to 7.0%).
const UNSCRIPTED_COST: f64 = 120.0;
/// Without text, each stretch [`recognise`] places (a sound or a pause) costs this much against the frames' fit, so it
/// changes sound only where the frames ask for it ...
const STRETCH_COST: f64 = 5.0;
/// ... and closed lips this much more. Without it, a tenth of the speech that is not m, b or p showed closed lips;
/// with it, 7% (of vowels 2.6%, Interplay's own 5.2%), with half of the m, b and p still closed (forced alignments).
const CLOSED_COST: f64 = 3.0;
/// Without text, how much a sound's length straying from its typical length costs.
const RECOGNISE_DURATION_WEIGHT: f64 = 1.0;
/// The shortest a sound lasts (frames). The aligner the model was fitted to gives 30 ms at least; 20 ms leaves room
/// where a fast line packs its sounds.
const MIN_SOUND: usize = 2;

/// Per frame of `a`: loudness against the line's 95th percentile, loudness against the loudest frame within 150 ms,
/// zero crossings, the low band's share, the loudness slope (dB per two frames); then the cepstrum less its mean over
/// the line's speech (taking out the voice's and the recording's own colour), and its slope over 50 ms.
pub fn features(a: &Analysis) -> Vec<[f32; FEATURES]> {
    let n = a.frames.len();
    let db: Vec<f32> = a.frames.iter().map(|f| f.db).collect();
    let mut sorted = db.clone();
    sorted.sort_by(f32::total_cmp);
    let p95 = sorted
        .get(((n.max(1) - 1) as f32 * 0.95) as usize)
        .copied()
        .unwrap_or(0.0);
    let speech: Vec<usize> = a.segments.iter().flat_map(|s| s.start..s.end).collect();
    let speech = if speech.is_empty() { (0..n).collect() } else { speech };
    let mut mean = [0.0f32; CEPSTRA];
    for &i in &speech {
        for (m, c) in mean.iter_mut().zip(a.frames[i].cep) {
            *m += c / speech.len() as f32;
        }
    }
    let cep = |i: usize, d: isize| a.frames[i.saturating_add_signed(d).min(n - 1)].cep;
    (0..n)
        .map(|i| {
            let peak = db[i.saturating_sub(15)..(i + 16).min(n)]
                .iter()
                .copied()
                .fold(f32::MIN, f32::max);
            let slope = db[(i + 1).min(n - 1)] - db[i.saturating_sub(1)];
            let mut x = [0.0; FEATURES];
            x[..LOUDNESS].copy_from_slice(&[
                (db[i] - p95).clamp(-60.0, 10.0),
                (db[i] - peak).clamp(-60.0, 0.0),
                a.frames[i].zcr,
                a.frames[i].low,
                slope.clamp(-30.0, 30.0),
            ]);
            for k in 0..CEPSTRA {
                x[LOUDNESS + k] = a.frames[i].cep[k] - mean[k];
                x[LOUDNESS + CEPSTRA + k] =
                    (cep(i, 1)[k] - cep(i, -1)[k] + 2.0 * (cep(i, 2)[k] - cep(i, -2)[k])) / 10.0;
            }
            x
        })
        .collect()
}

/// The name of model class `c`: its ARPAbet symbol, `noise` or `silence`.
pub fn class_name(c: usize) -> &'static str {
    match c {
        NOISE => "noise",
        SILENCE => "silence",
        _ => g2p::SYMBOLS.get(c).map_or("?", |s| s.0),
    }
}

/// What [`align`] placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// One of the text's sounds, as its phoneme code.
    Sound(u8),
    /// A pause.
    Pause,
    /// Speech the text lacks (an ad-lib, a hum, a laugh not written down).
    Unscripted,
}

/// A stretch placed by [`align`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// First frame.
    pub start: usize,
    /// Length, in frames.
    pub len: usize,
    /// What it is.
    pub what: What,
}

#[derive(Debug, Clone, Copy)]
enum Unit {
    Sound {
        class: usize,
        code: u8,
    },
    /// A pause costing `cost`, which the alignment may skip; at the line's edges any length, free.
    Pause {
        cost: f64,
        edge: bool,
    },
    /// Speech the text lacks, which the alignment may skip.
    Unscripted,
}

/// `tokens` placed over `a`'s frames, `weight` the cost of a sound's length straying from its typical length against
/// its frames' fit; empty when the line's sounds cannot fit its frames.
pub fn align(tokens: &[Token], a: &Analysis, weight: f64) -> Vec<Placed> {
    let pause = |cost| Unit::Pause { cost, edge: false };
    let mut units = vec![
        Unit::Pause { cost: 0.0, edge: true },
        Unit::Unscripted,
        pause(PAUSE_COST),
    ];
    let mut started = false;
    for t in tokens {
        let sounds: Vec<Unit> = match t {
            Token::Word(p) | Token::Drawn(p) => p
                .iter()
                .map(|&p| Unit::Sound {
                    class: p.sound as usize,
                    code: g2p::code(p),
                })
                .collect(),
            Token::Noise => vec![Unit::Sound {
                class: NOISE,
                code: NOISE_CODE,
            }],
            Token::Break if !started => continue,
            Token::Break => {
                match units.last_mut() {
                    Some(Unit::Pause { cost, .. }) => *cost = BREAK_PAUSE_COST,
                    _ => units.push(pause(BREAK_PAUSE_COST)),
                }
                continue;
            }
        };
        if !matches!(units.last(), Some(Unit::Pause { .. })) {
            units.push(pause(PAUSE_COST));
        }
        units.extend(sounds);
        started = true;
    }
    if !matches!(units.last(), Some(Unit::Pause { .. })) {
        units.push(pause(PAUSE_COST));
    }
    units.push(Unit::Unscripted);
    units.push(Unit::Pause { cost: 0.0, edge: true });
    let x = features(a);
    let n = x.len();
    let mut fit: Vec<Option<Vec<f64>>> = vec![None; CLASSES];
    for u in &units {
        let c = class(u);
        if fit[c].is_none() {
            fit[c] = Some(misfit(&x, c));
        }
    }
    // cost[u][j]: the best for units 0..u over frames 0..j; from[u][j]: where unit u-1 starts (j when skipped).
    let k = units.len();
    let mut cost = vec![vec![f64::INFINITY; n + 1]; k + 1];
    let mut from = vec![vec![usize::MAX; n + 1]; k + 1];
    cost[0][0] = 0.0;
    for (ui, u) in units.iter().enumerate() {
        let sums = fit[class(u)].as_deref().unwrap_or_default();
        let (shortest, longest, extra, skip) = match *u {
            Unit::Pause { edge: true, .. } => (1, n, 0.0, true),
            Unit::Pause { cost, .. } => (1, MAX_PAUSE, cost, true),
            Unit::Unscripted => (1, n, UNSCRIPTED_COST, true),
            Unit::Sound { class, .. } => (MIN_SOUND, longest(class), 0.0, false),
        };
        let length = |d: usize| match u {
            Unit::Pause { .. } | Unit::Unscripted => 0.0,
            Unit::Sound { class, .. } => length_cost(*class, d, weight),
        };
        for j in 0..=n {
            let (mut best, mut arg) = (f64::INFINITY, usize::MAX);
            if skip {
                (best, arg) = (cost[ui][j], j);
            }
            if j >= shortest {
                for i in j.saturating_sub(longest)..=j - shortest {
                    let c = cost[ui][i] + (sums[j] - sums[i]) + length(j - i) + extra;
                    if c < best {
                        (best, arg) = (c, i);
                    }
                }
            }
            cost[ui + 1][j] = best;
            from[ui + 1][j] = arg;
        }
    }
    let mut out = Vec::new();
    let mut j = n;
    for ui in (0..k).rev() {
        let i = from[ui + 1][j];
        if i == usize::MAX {
            return Vec::new();
        }
        if i < j {
            out.push(Placed {
                start: i,
                len: j - i,
                what: match units[ui] {
                    Unit::Sound { code, .. } => What::Sound(code),
                    Unit::Pause { .. } => What::Pause,
                    Unit::Unscripted => What::Unscripted,
                },
            });
        }
        j = i;
    }
    out.reverse();
    out
}

/// The sounds heard over `a`'s frames in `span`, without text: the likeliest run of any sounds and pauses, each sound
/// near its typical length and following the one before as often as speech has it.
pub fn recognise(a: &Analysis, span: std::ops::Range<usize>) -> Vec<Placed> {
    let x = features(a);
    let x = &x[span.start.min(x.len())..span.end.min(x.len())];
    let n = x.len();
    // A vocal noise is a sound only the text can name.
    let classes: Vec<usize> = (0..CLASSES).filter(|&c| c != NOISE).collect();
    let fit: Vec<Vec<f64>> = (0..CLASSES)
        .map(|c| if c == NOISE { Vec::new() } else { misfit(x, c) })
        .collect();
    let closed = [g2p::Arpabet::B, g2p::Arpabet::M, g2p::Arpabet::P].map(|s| s as usize);
    // best[j][c]: the cheapest run over frames 0..j whose last stretch, of class c, ends at j; back[j][c]: where that
    // stretch starts and the class before it. enter[i][c]: the cheapest run that a stretch of class c can start at i.
    let mut best = vec![[f64::INFINITY; CLASSES]; n + 1];
    let mut back = vec![[(0, SILENCE); CLASSES]; n + 1];
    let mut enter = vec![[(f64::INFINITY, SILENCE); CLASSES]; n + 1];
    for &c in &classes {
        enter[0][c] = (f64::from(NEXT[SILENCE][c]), SILENCE);
    }
    for j in 1..=n {
        for &c in &classes {
            let (shortest, longest, extra) = if c == SILENCE {
                (1, MAX_PAUSE, STRETCH_COST)
            } else {
                let extra = if closed.contains(&c) { CLOSED_COST } else { 0.0 };
                (MIN_SOUND, longest(c), STRETCH_COST + extra)
            };
            for i in j.saturating_sub(longest)..=j.saturating_sub(shortest) {
                let (from, _) = enter[i][c];
                let length = if c == SILENCE {
                    0.0
                } else {
                    length_cost(c, j - i, RECOGNISE_DURATION_WEIGHT)
                };
                let v = from + (fit[c][j] - fit[c][i]) + length + extra;
                if v < best[j][c] {
                    best[j][c] = v;
                    back[j][c] = (i, enter[i][c].1);
                }
            }
        }
        for &c in &classes {
            enter[j][c] = classes
                .iter()
                .map(|&b| (best[j][b] + f64::from(NEXT[b][c]), b))
                .min_by(|p, q| p.0.total_cmp(&q.0))
                .unwrap_or((f64::INFINITY, SILENCE));
        }
    }
    let mut out = Vec::new();
    let (mut j, mut c) = (n, SILENCE);
    if let Some(&last) = classes.iter().min_by(|&&p, &&q| best[n][p].total_cmp(&best[n][q])) {
        c = last;
    }
    while j > 0 && best[j][c].is_finite() {
        let (i, before) = back[j][c];
        let what = if c == SILENCE {
            What::Pause
        } else {
            What::Sound(code_of(c))
        };
        match out.last_mut() {
            // The same shape twice in a row is one stretch.
            Some(Placed { start, len, what: w }) if *w == what => {
                *start = span.start + i;
                *len += j - i;
            }
            _ => out.push(Placed {
                start: span.start + i,
                len: j - i,
                what,
            }),
        }
        (j, c) = (i, before);
    }
    out.reverse();
    out
}

/// The phoneme code of a sound class.
fn code_of(c: usize) -> u8 {
    if c == NOISE {
        return NOISE_CODE;
    }
    let sound = g2p::SYMBOLS[c].1;
    g2p::code(g2p::Phone {
        sound,
        stress: sound.is_vowel().then_some(1),
    })
}

/// Per frame `i`, the summed misfit (negative log-likelihood) of frames `0..i` to class `c`.
fn misfit(x: &[[f32; FEATURES]], c: usize) -> Vec<f64> {
    let mut sums = vec![0.0; x.len() + 1];
    for (i, f) in x.iter().enumerate() {
        let nll: f64 = (0..FEATURES)
            .map(|k| {
                let (d, v) = (f64::from(f[k] - MEAN[c][k]), f64::from(VAR[c][k]));
                0.5 * (d * d / v + v.ln())
            })
            .sum();
        sums[i + 1] = sums[i] + nll;
    }
    sums
}

/// The longest a sound of class `c` may last (frames): three spreads over its typical length.
fn longest(c: usize) -> usize {
    let (mu, sd) = DUR[c];
    (f64::from(mu + 3.0 * sd).exp().ceil() as usize).clamp(4, 80)
}

/// The cost of a sound of class `c` lasting `d` frames, against its typical length.
fn length_cost(c: usize, d: usize, weight: f64) -> f64 {
    let (mu, sd) = DUR[c];
    let z = ((d as f64).ln() - f64::from(mu)) / f64::from(sd);
    weight * 0.5 * z * z
}

fn class(u: &Unit) -> usize {
    match *u {
        Unit::Sound { class, .. } => class,
        Unit::Pause { .. } => SILENCE,
        Unit::Unscripted => NOISE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g2p::tokens;
    use crate::pcm::Audio;
    use crate::speech::{SpeechParams, analyse};
    use crate::testaudio::{hiss, vowel};

    #[derive(Clone, Copy)]
    enum Piece {
        Silent,
        Vowel,
        Hiss,
    }
    use Piece::*;

    const RATE: u32 = 22050;

    /// The analysis of `(piece, frames)` runs: silence, a held "ah", a hiss 8 dB under it.
    fn analysis(runs: &[(Piece, usize)]) -> Analysis {
        let per = RATE as usize / 100;
        let len = runs.iter().map(|r| r.1).sum::<usize>() * per;
        let (v, h) = (vowel(len, RATE), hiss(len));
        let mut samples = Vec::with_capacity(len);
        for &(piece, frames) in runs {
            for _ in 0..frames * per {
                let i = samples.len();
                samples.push(match piece {
                    Silent => 0,
                    Vowel => (10000.0 * v[i]) as i16,
                    Hiss => (4000.0 * h[i]) as i16,
                });
            }
        }
        analyse(&Audio::from_i16(RATE, 1, samples), &SpeechParams::default())
    }

    /// Each placed stretch as (what, first frame, last frame + 1).
    fn spans(placed: &[Placed]) -> Vec<(What, usize, usize)> {
        placed.iter().map(|p| (p.what, p.start, p.start + p.len)).collect()
    }

    /// `got` is `want` to a frame either way: the slope and the loudness against the nearby peak straddle each edge.
    fn near(got: &[(What, usize, usize)], want: &[(What, usize, usize)]) -> bool {
        got.len() == want.len()
            && got
                .iter()
                .zip(want)
                .all(|(g, w)| g.0 == w.0 && g.1.abs_diff(w.1) <= 1 && g.2.abs_diff(w.2) <= 1)
    }

    #[test]
    fn sounds_go_where_the_frames_sound_like_them() {
        // "us": AH over the vowel, S over the hiss, silence either side.
        let a = analysis(&[(Silent, 30), (Vowel, 12), (Hiss, 15), (Silent, 30)]);
        let (ah, s) = (code_of(g2p::Arpabet::Ah as usize), code_of(g2p::Arpabet::S as usize));
        let got = spans(&align(&tokens("us"), &a, 16.0));
        let want = [
            (What::Pause, 0, 30),
            (What::Sound(ah), 30, 42),
            (What::Sound(s), 42, 57),
            (What::Pause, 57, 87),
        ];
        assert!(near(&got, &want), "{got:?}");
    }

    #[test]
    fn a_silent_gap_at_a_comma_is_a_pause_and_speech_before_the_text_is_unscripted() {
        // "us, us" over two vowel-hiss stretches 300 ms apart.
        let word = [(Vowel, 12), (Hiss, 15)];
        let a = analysis(&[&word[..], &[(Silent, 30)], &word[..]].concat());
        let placed = spans(&align(&tokens("us, us"), &a, 16.0));
        assert!(
            placed.iter().any(|p| near(&[*p], &[(What::Pause, 27, 57)])),
            "{placed:?}"
        );
        // Two seconds of speech the text leaves out (ten syllables), then "us".
        let syllables = [(Vowel, 14), (Hiss, 6)].repeat(10);
        let a = analysis(
            &[
                &[(Silent, 20)],
                &syllables[..],
                &[(Silent, 30)],
                &word[..],
                &[(Silent, 20)],
            ]
            .concat(),
        );
        let placed = spans(&align(&tokens("us"), &a, 16.0));
        assert!(
            placed.iter().any(|p| near(&[*p], &[(What::Unscripted, 20, 220)])),
            "{placed:?}"
        );
        assert!(
            placed.iter().any(|p| near(&[*p], &[(What::Pause, 220, 250)])),
            "the word follows a pause: {placed:?}"
        );
    }

    #[test]
    fn more_sounds_than_frames_place_nothing() {
        let a = analysis(&[(Vowel, 5)]);
        assert_eq!(align(&tokens("antidisestablishmentarianism"), &a, 16.0), []);
    }

    #[test]
    fn without_text_the_sounds_heard_are_placed() {
        // A held "ah", then a hiss: an open vowel, then s, silence either side.
        let a = analysis(&[(Silent, 30), (Vowel, 25), (Hiss, 15), (Silent, 30)]);
        let got = recognise(&a, 0..a.frames.len());
        let at = |f: usize| {
            got.iter()
                .find(|p| (p.start..p.start + p.len).contains(&f))
                .map(|p| p.what)
        };
        let s = code_of(g2p::Arpabet::S as usize);
        assert_eq!(
            (at(10), at(62), at(90)),
            (Some(What::Pause), Some(What::Sound(s)), Some(What::Pause)),
            "{got:?}"
        );
        let Some(What::Sound(vowel)) = at(42) else {
            panic!("{got:?}")
        };
        // Head frames 0 and 6 are the resting and the closed mouth: any other is open.
        assert!(
            ![0, crate::lip::CLOSED].contains(&crate::lip::head_frame(vowel)),
            "{got:?}"
        );
        // Over part of the line, only that part.
        let part = recognise(&a, 50..a.frames.len());
        assert!(part.first().is_some_and(|p| p.start == 50), "{part:?}");
    }

    #[test]
    fn classes_are_named() {
        assert_eq!(
            (class_name(0), class_name(NOISE), class_name(SILENCE)),
            ("AA", "noise", "silence")
        );
    }
}
