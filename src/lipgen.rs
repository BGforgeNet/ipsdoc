//! Lip-sync generation: a sound, and optionally its line's phonemes, to a lip file.
//!
//! With text, the line's sounds are placed on its speech by a forced alignment ([`crate::lipalign`]). Without text,
//! the same model picks the likeliest sounds from the sound alone. Either way, neighbours showing the same head frame
//! merge, shapes are held a minimum time, changes are capped per second, and pauses show the silent mouth.

use crate::g2p::Token;
use crate::lip::{self, CLOSED, HEAD_FRAME, Lip, Marker};
use crate::lipalign;
use crate::pcm::Audio;
use crate::speech::{self, SpeechParams};

/// Tuning (times in seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Speech detection.
    pub speech: SpeechParams,
    /// In placing the text's sounds, how much a sound's length straying from its typical length costs against its
    /// frames' fit.
    pub duration_weight: f32,
    /// The shortest a shape is held ...
    pub hold: f32,
    /// ... except closed lips (m, p, b), held at least this long.
    pub closed_hold: f32,
    /// At most this many shape changes per second.
    pub max_rate: f32,
    /// With text, pauses inside the line longer than this show the silent mouth.
    pub silence: f32,
    /// Shapes start this much before their sound, as the original lip files' do.
    pub lead: f32,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            speech: SpeechParams::default(),
            // Against forced alignments of Fallout 2's lines (held-out half of the speakers), the frames' fit at half
            // this weight (the costs in `lipalign` doubled to match) placed f and v better than at the same weight
            // (95.9% to 93.2%), the rest alike.
            duration_weight: 16.0,
            // Shorter than a 40 ms shape is a flicker; at 50 ms the sound-only shapes, with fewer closures, moved less
            // than Interplay's (5.7 changes a second to its 6.8, tuning half), at 40 ms 6.5.
            hold: 0.04,
            closed_hold: 0.04,
            // A cap of 8 cut placed sounds: f and v from 88.0% to 84.0% (tuning half); the sound alone is the same at
            // either.
            max_rate: 12.0,
            // A shorter pause keeps its shape: at 0.05 s the mouth shut more often in speech, at 0.2 s it missed more
            // of the pauses.
            silence: 0.1,
            lead: 0.03,
        }
    }
}

/// The lip file for `audio`, a sound named `name`, from its line's `tokens` when there are any.
pub fn generate(name: &str, audio: &Audio, tokens: Option<&[Token]>, p: &Params) -> Lip {
    let a = speech::analyse(audio, &p.speech);
    let total = audio.frames() as f64 * f64::from(speech::FRAMES_PER_SECOND) / f64::from(audio.rate);
    let placed = tokens
        .filter(|t| t.iter().any(|x| !matches!(x, Token::Break)))
        .map(|t| lipalign::align(t, &a, f64::from(p.duration_weight)))
        .filter(|v| !v.is_empty());
    // No text, or more sounds than the speech has frames for: the sounds heard.
    let placed = placed.unwrap_or_else(|| lipalign::recognise(&a, 0..a.frames.len()));
    let mut shapes = vec![Shape { start: 0.0, code: 0 }];
    placed_shapes(&placed, &a, p, &mut shapes);
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
        HEAD_FRAME[usize::from(self.code.min(lip::PHONEME_COUNT - 1))]
    }
}

/// The placed sounds as shapes: each sound from its start; a pause inside the line shown as the silent mouth only when
/// longer than `p.silence` (a shorter one keeps the shape before it), the edges' always; speech the text lacks shaped
/// from the sounds heard there.
fn placed_shapes(placed: &[lipalign::Placed], a: &speech::Analysis, p: &Params, out: &mut Vec<Shape>) {
    let silence = f64::from(p.silence) * f64::from(speech::FRAMES_PER_SECOND);
    for x in placed {
        let code = match x.what {
            lipalign::What::Sound(code) => code,
            // Showing only a pause's quiet part (outside the stretches of speech) left the mouth open through more of
            // the silence (forced alignments: 1.7% to 15.2%), as those stretches take in breaths.
            lipalign::What::Pause => {
                let inside = x.start > 0 && x.start + x.len < a.frames.len();
                if inside && (x.len as f64) < silence {
                    continue;
                }
                0
            }
            lipalign::What::Unscripted => {
                placed_shapes(&lipalign::recognise(a, x.start..x.start + x.len), a, p, out);
                continue;
            }
        };
        out.push(Shape {
            start: x.start as f64,
            code,
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
            if len(&v, i + 1) - need >= hold {
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
        // Closed lips are the shortest shapes by design, and the one a viewer misses on m, b and p: others go first.
        let i = (1..v.len() - 1)
            .filter(|&i| v[i].code != 0 && v[i].frame() != CLOSED)
            .min_by(|&x, &y| len(&v, x).total_cmp(&len(&v, y)))
            .or_else(|| {
                (1..v.len() - 1)
                    .filter(|&i| v[i].code != 0)
                    .min_by(|&x, &y| len(&v, x).total_cmp(&len(&v, y)))
            });
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
    let channels = i64::from(audio.channels);
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

    /// `(seconds, amplitude)` pieces of a held vowel, or silence at amplitude 0.
    fn sound(pieces: &[(f32, f32)]) -> Audio {
        let amps: Vec<f32> = pieces
            .iter()
            .flat_map(|&(secs, amp)| std::iter::repeat_n(amp, (secs * RATE as f32).round() as usize))
            .collect();
        let voice = crate::testaudio::vowel(amps.len(), RATE);
        Audio::from_i16(RATE, 1, voice.iter().zip(&amps).map(|(v, a)| (v * a) as i16).collect())
    }

    /// The head frame shown at `secs`.
    fn frame_at(lip: &Lip, secs: f32) -> u8 {
        let pos = 2 * (secs * RATE as f32) as i32;
        let i = lip.markers.iter().rposition(|m| m.position <= pos).unwrap_or(0);
        HEAD_FRAME[usize::from(lip.phonemes.get(i).copied().unwrap_or(0).min(lip::PHONEME_COUNT - 1))]
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
        let p = Params::default();
        for &(f, len) in inner {
            let least = if f == 6 { p.closed_hold } else { p.hold };
            assert!(len >= least - 0.011, "{f} held {len}: {s:?}");
        }
        assert_eq!(frame_at(&lip, 0.1), 0);
        assert_eq!(frame_at(&lip, 1.1), 0);
    }

    #[test]
    fn consonants_follow_the_loudness_dips() {
        // "pa pa" over loud-quiet-loud with the quiet part early: the second P belongs in the dip, not halfway.
        let audio = sound(&[(0.2, 0.0), (0.25, 9000.0), (0.08, 700.0), (0.6, 9000.0), (0.2, 0.0)]);
        let lip = generate("test", &audio, Some(&tokens("pa pa")), &Params::default());
        check_valid(&lip, &audio);
        let lead = f64::from(Params::default().lead);
        let closed: Vec<f64> = lip
            .phonemes
            .iter()
            .zip(&lip.markers)
            .filter(|&(&c, _)| HEAD_FRAME[usize::from(c)] == CLOSED)
            .map(|(_, m)| f64::from(m.position) / 2.0 / f64::from(RATE) + lead)
            .collect();
        assert_eq!(closed.len(), 2, "{lip:?}");
        assert!(
            (closed[1] - 0.45).abs() <= 0.05,
            "second P at {} s, the dip is 0.45 to 0.53 s",
            closed[1]
        );
    }

    #[test]
    fn after_each_long_pause_the_mouth_resumes_with_the_phoneme_placed_there() {
        // One phrase over three bursts with long pauses between them.
        let audio = sound(&[
            (0.2, 0.0),
            (0.4, 9000.0),
            (0.4, 0.0),
            (0.4, 9000.0),
            (0.4, 0.0),
            (0.4, 9000.0),
            (0.2, 0.0),
        ]);
        let p = Params {
            lead: 0.0,
            ..Params::default()
        };
        let t = tokens("fee foo thaw mama lily oak");
        let lip = generate("test", &audio, Some(&t), &p);
        check_valid(&lip, &audio);
        // The third burst (from 1.8 s) starts with a phoneme placed there, never the AW of "thaw" (code 7), which
        // belongs before the second pause.
        let pos = 2 * (1.83 * RATE as f32) as i32;
        let i = lip.markers.iter().rposition(|m| m.position <= pos).unwrap_or(0);
        let code = lip.phonemes[i];
        assert!(
            code != 0 && code != 7,
            "code {code} at the third burst's start: {lip:?}"
        );
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
    fn the_rate_cap_takes_closed_lips_last() {
        // Two seconds of speech: shapes 70 ms long, but the closed lips 40 ms, the shortest; at most 2 changes a
        // second, so the cap must drop most of them.
        let frame = speech::Frame {
            db: -20.0,
            zcr: 0.1,
            low: 0.5,
            cep: [0.0; speech::CEPSTRA],
        };
        let a = speech::Analysis {
            frames: vec![frame; 200],
            segments: vec![speech::Segment { start: 0, end: 200 }],
        };
        let mut shapes = vec![Shape { start: 0.0, code: 0 }];
        let mut at = 10.0;
        for k in 0..20 {
            let code = if k == 10 {
                32
            } else if k % 2 == 0 {
                1
            } else {
                13
            };
            shapes.push(Shape { start: at, code });
            at += if code == 32 { 4.0 } else { 7.0 };
        }
        shapes.push(Shape { start: at, code: 0 });
        let p = Params {
            max_rate: 2.0,
            ..Params::default()
        };
        let kept = clean(shapes, 200.0, &a, &p);
        assert!(changes(&kept) <= 6, "{kept:?}");
        assert!(kept.iter().any(|s| s.frame() == CLOSED), "{kept:?}");
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
        // Syllables at a speaking rate: 150 ms of vowel, then a 40 ms dip where a consonant falls. Speech held much
        // longer than the text's sounds last has the model's silence, the broadest of its classes, take the rest.
        let syllable = [(0.15, 9000.0), (0.04, 700.0)];
        let silent = [(0.2, 0.0)];
        let audio = sound(
            &[
                &silent[..],
                &syllable,
                &syllable,
                &silent,
                &syllable,
                &syllable,
                &silent,
            ]
            .concat(),
        );
        let lip = generate("test", &audio, Some(&tokens("one two, three four")), &Params::default());
        check_valid(&lip, &audio);
        assert_eq!(frame_at(&lip, 0.68), 0, "the pause holds the comma");
        let joined = sound(&[&silent[..], &syllable.repeat(4), &silent].concat());
        let lip = generate(
            "test",
            &joined,
            Some(&tokens("one two, three four")),
            &Params::default(),
        );
        check_valid(&lip, &joined);
        // The middle of each vowel.
        for t in [0.27, 0.46, 0.65, 0.84] {
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
