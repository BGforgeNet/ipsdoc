//! Places a line's sounds on its speech: a forced alignment over the whole line. Each frame is scored against each
//! sound's typical features ([`features`]: Gaussians fitted per sound), each sound's length against its typical length
//! (log-normal), with an optional pause between words, a cheaper one at a phrase break, and optional speech the text
//! lacks before and after it.

use crate::g2p::{self, Token};
use crate::lipmodel::{DUR, MEAN, VAR};
use crate::speech::Analysis;

/// Features per frame.
pub const FEATURES: usize = 5;
/// Model classes: the 39 ARPAbet sounds in [`g2p::Arpabet`] order, then a vocal noise, then silence.
pub const CLASSES: usize = 41;
/// The class of a vocal noise.
pub const NOISE: usize = 39;
/// The class of silence.
pub const SILENCE: usize = 40;
/// The code shown for a vocal noise: an open mouth (A as in father).
const NOISE_CODE: u8 = 6;
/// A pause between words costs this much against the frames' fit, so the alignment pauses only where the sound does ...
const PAUSE_COST: f64 = 5.0;
/// ... and at a phrase break this much: a comma or a full stop is often a pause.
const BREAK_PAUSE_COST: f64 = 1.0;
/// The longest pause inside a line (frames).
const MAX_PAUSE: usize = 200;
/// Speech the text lacks before or after it costs this much against the frames' fit. Without it, a line opening with
/// 3.5 s of speech its text leaves out showed the mouth shut through it; allowed at phrase breaks too, it took breaths
/// for speech and opened the mouth through more of the silence (against forced alignments, 4.0% to 7.0%).
const UNSCRIPTED_COST: f64 = 60.0;
/// The shortest a sound lasts (frames). The aligner the model was fitted to gives 30 ms at least; 20 ms leaves room
/// where a fast line packs its sounds.
const MIN_SOUND: usize = 2;

/// Per frame of `a`: loudness against the line's 95th percentile, loudness against the loudest frame within 150 ms,
/// zero crossings, the low band's share, and the loudness slope (dB per two frames).
pub fn features(a: &Analysis) -> Vec<[f32; FEATURES]> {
    let n = a.frames.len();
    let db: Vec<f32> = a.frames.iter().map(|f| f.db).collect();
    let mut sorted = db.clone();
    sorted.sort_by(f32::total_cmp);
    let p95 = sorted
        .get(((n.max(1) - 1) as f32 * 0.95) as usize)
        .copied()
        .unwrap_or(0.0);
    (0..n)
        .map(|i| {
            let peak = db[i.saturating_sub(15)..(i + 16).min(n)]
                .iter()
                .copied()
                .fold(f32::MIN, f32::max);
            let slope = db[(i + 1).min(n - 1)] - db[i.saturating_sub(1)];
            [
                (db[i] - p95).clamp(-60.0, 10.0),
                (db[i] - peak).clamp(-60.0, 0.0),
                a.frames[i].zcr,
                a.frames[i].low,
                slope.clamp(-30.0, 30.0),
            ]
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
    // Per class used, the negative log-likelihood of frames 0..i, summed.
    let mut fit: Vec<Option<Vec<f64>>> = vec![None; CLASSES];
    for u in &units {
        let c = class(u);
        if fit[c].is_none() {
            let mut sums = vec![0.0; n + 1];
            for (i, f) in x.iter().enumerate() {
                let nll: f64 = (0..FEATURES)
                    .map(|k| {
                        let (d, v) = (f64::from(f[k] - MEAN[c][k]), f64::from(VAR[c][k]));
                        0.5 * (d * d / v + v.ln())
                    })
                    .sum();
                sums[i + 1] = sums[i] + nll;
            }
            fit[c] = Some(sums);
        }
    }
    // cost[u][j]: the best for units 0..u over frames 0..j; from[u][j]: where unit u-1 starts (j when skipped).
    let k = units.len();
    let mut cost = vec![vec![f64::INFINITY; n + 1]; k + 1];
    let mut from = vec![vec![usize::MAX; n + 1]; k + 1];
    cost[0][0] = 0.0;
    for (ui, u) in units.iter().enumerate() {
        let sums = fit[class(u)].as_deref().unwrap_or_default();
        let (mu, sd) = DUR[class(u)];
        let (shortest, longest, extra, skip) = match *u {
            Unit::Pause { edge: true, .. } => (1, n, 0.0, true),
            Unit::Pause { cost, .. } => (1, MAX_PAUSE, cost, true),
            Unit::Unscripted => (1, n, UNSCRIPTED_COST, true),
            Unit::Sound { .. } => (
                MIN_SOUND,
                (f64::from(mu + 3.0 * sd).exp().ceil() as usize).clamp(4, 80),
                0.0,
                false,
            ),
        };
        let length = |d: usize| match u {
            Unit::Pause { .. } | Unit::Unscripted => 0.0,
            Unit::Sound { .. } => {
                let z = ((d as f64).ln() - f64::from(mu)) / f64::from(sd);
                weight * 0.5 * z * z
            }
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
    use crate::speech::{Frame, Segment};

    /// Silence, a vowel (loud, few zero crossings, mostly low), a hiss (quieter, many crossings, little low energy).
    const SILENT: Frame = Frame {
        db: -70.0,
        zcr: 0.17,
        low: 0.5,
    };
    const VOWEL: Frame = Frame {
        db: -20.0,
        zcr: 0.12,
        low: 0.6,
    };
    const HISS: Frame = Frame {
        db: -28.0,
        zcr: 0.5,
        low: 0.1,
    };

    /// Frames from `(frame, count)` runs.
    fn analysis(runs: &[(Frame, usize)]) -> Analysis {
        let frames: Vec<Frame> = runs.iter().flat_map(|&(f, n)| std::iter::repeat_n(f, n)).collect();
        Analysis {
            segments: vec![Segment {
                start: 0,
                end: frames.len(),
            }],
            frames,
            dips: Vec::new(),
        }
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
        let a = analysis(&[(SILENT, 30), (VOWEL, 12), (HISS, 15), (SILENT, 30)]);
        let ah = g2p::code(g2p::Phone {
            sound: g2p::Arpabet::Ah,
            stress: Some(1),
        });
        let s = g2p::code(g2p::Phone {
            sound: g2p::Arpabet::S,
            stress: None,
        });
        let got = spans(&align(&tokens("us"), &a, 8.0));
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
        let word = [(VOWEL, 12), (HISS, 15)];
        let a = analysis(&[&word[..], &[(SILENT, 30)], &word[..]].concat());
        let placed = spans(&align(&tokens("us, us"), &a, 8.0));
        assert!(
            placed.iter().any(|p| near(&[*p], &[(What::Pause, 27, 57)])),
            "{placed:?}"
        );
        // Two seconds of speech the text leaves out, then "us".
        let a = analysis(&[
            (SILENT, 20),
            (VOWEL, 200),
            (SILENT, 30),
            (VOWEL, 12),
            (HISS, 15),
            (SILENT, 20),
        ]);
        let placed = spans(&align(&tokens("us"), &a, 8.0));
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
        let a = analysis(&[(VOWEL, 5)]);
        assert_eq!(align(&tokens("antidisestablishmentarianism"), &a, 8.0), []);
    }

    #[test]
    fn classes_are_named() {
        assert_eq!(
            (class_name(0), class_name(NOISE), class_name(SILENCE)),
            ("AA", "noise", "silence")
        );
    }
}
