//! How well a lip file follows its sound: whether the mouth opens while the line is loud, at what offset it follows
//! the loudness best, and, given the line's text, whether the lips close for its m, b and p.
//!
//! The limits come from Fallout 2's 994 voiced lines whose lip file ends with its sound, against their own lip files
//! (Interplay's), ours from the sound alone, wav2lip's, another line's lip file stretched to fit, and the original
//! moved 200 ms late; the text from the Unofficial Patch's English dialog.

use std::fmt;

use crate::g2p::{Arpabet, Token};
use crate::lip::{Lip, PHONEME_COUNT, Severity};
use crate::lipgen::HEAD_FRAME;
use crate::pcm::Audio;
use crate::speech::{self, SpeechParams, frame_start};

/// Loud frames: within this many dB of the line's 95th-percentile loudness.
const LOUD_DB: f32 = 15.0;
/// A mouth shut through more of the loud frames than this is an error: wav2lip's files are (96% of them), none of
/// Interplay's (with valid codes) or ours is.
const SHUT_LIMIT: f64 = 0.5;
/// Offsets up to this many 10 ms frames go unseen (see [`crate::lip::Fit`]), so alignment is the best within them.
const UNSEEN_FRAMES: i32 = 5;
/// From this many frames an offset is seen; offsets are looked for up to [`MAX_FRAMES`].
const SEEN_FRAMES: i32 = 15;
/// The longest offset looked for: 300 ms.
const MAX_FRAMES: i32 = 30;
/// A line this long or longer (seconds) has enough speech to show an offset: about a quarter of Interplay's shorter
/// lines align as well somewhere off by chance.
const OFFSET_MIN_SECONDS: f64 = 2.0;
/// An offset is reported when the mouth aligns this well there ...
const OFFSET_ALIGNMENT: f64 = 0.7;
/// ... and this much better than within the unseen offsets. It flags 4% of Interplay's lines (some of them truly
/// 150 to 300 ms off), 0.7% of ours, and half of the originals moved 200 ms late.
const OFFSET_GAIN: f64 = 0.15;
/// A directory of at least this many lip files is graded as a set, where chance evens out ...
const SET_MIN: usize = 15;
/// ... and warned about below this median alignment: Fallout 2's 11 speech directories have 0.64 to 0.82, speakers'
/// sets of other lines' lip files 0.53 to 0.59 ...
const SET_ALIGNMENT: f64 = 0.6;
/// ... or above this median share of the loud speech shown shut: Fallout 2's directories have 0.06 to 0.25, ours
/// 0.06 to 0.08 per speaker, wav2lip's 0.77 to 0.92 ...
const SET_SHUT: f64 = 0.35;
/// ... or with a median of closures per m, b and p outside these: Fallout 2's directories have 0.75 to 1.43,
/// wav2lip's 0 (it never closes the lips).
const SET_CLOSURES: (f64, f64) = (0.5, 1.6);
/// Interplay's lip files close the lips once per m, b or p of the text (the middle half of them exactly once); a
/// lip file closing them at a ratio outside these ...
const CLOSURES: (f64, f64) = (0.67, 1.5);
/// ... with under this share of the m, b and p met by a closure nearby, fits another line: Interplay's 4%, another
/// line's lip file stretched to fit 46%, wav2lip's all.
const PLACED: f64 = 0.25;
/// A closure meets an m, b or p within this many frames of where the sound falls with the line's phones spread
/// evenly over its speech.
const NEAR_FRAMES: f64 = 20.0;

/// How a lip file's closed lips fit its line's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFit {
    /// The text's m, b and p.
    pub bilabials: usize,
    /// The lip file's closed-lips shapes in or beside speech.
    pub closures: usize,
    /// The m, b and p met by a closure nearby.
    pub placed: usize,
}

impl TextFit {
    /// Closures per m, b and p; `None` for a text without them.
    pub fn ratio(&self) -> Option<f64> {
        (self.bilabials > 0).then(|| self.closures as f64 / self.bilabials as f64)
    }
}

/// How a lip file follows its sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    /// Share of the loud frames that show a shut mouth (rest, or closed lips); `None` for a sound with no speech.
    pub shut_while_loud: Option<f64>,
    /// Chance that a frame of speech showing an open mouth is louder than one showing it shut: 0.5 is chance, 1 a
    /// mouth that opens exactly with the loudness. The best within the offsets no player sees. `None` when the mouth
    /// is never open, or never shut, in speech.
    pub alignment: Option<f64>,
    /// The best alignment 150 to 300 ms off, and that offset in milliseconds (positive: the mouth runs late).
    pub offset: Option<(f64, f64)>,
    /// The sound's length in seconds.
    pub seconds: f64,
    /// How the closed lips fit the line's text, when it was given.
    pub text: Option<TextFit>,
}

/// A way a lip file fails its sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QualityProblem {
    /// The mouth stays shut through this share of the loud speech.
    Shut {
        /// The share.
        share: f64,
    },
    /// The mouth follows the sound clearly better this many milliseconds off (positive: it runs late).
    Offset {
        /// Milliseconds.
        ms: f64,
    },
    /// The lips close neither as often as the text has m, b and p, nor where they fall.
    Closures(TextFit),
}

impl QualityProblem {
    /// How much it matters.
    pub fn severity(&self) -> Severity {
        match self {
            QualityProblem::Shut { .. } => Severity::Error,
            QualityProblem::Offset { .. } | QualityProblem::Closures(_) => Severity::Warning,
        }
    }
}

impl fmt::Display for QualityProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QualityProblem::Shut { share } => {
                write!(f, "the mouth is shut through {:.0}% of the loud speech", 100.0 * share)
            }
            QualityProblem::Offset { ms } => {
                let side = if *ms < 0.0 { "early" } else { "late" };
                write!(
                    f,
                    "the mouth runs about {:.0} ms {side}: it follows the sound best there",
                    ms.abs()
                )
            }
            QualityProblem::Closures(t) => write!(
                f,
                "the lips close {} times for the line's {} m, b and p, {} of them nearby: made for another line?",
                t.closures, t.bilabials, t.placed
            ),
        }
    }
}

impl Quality {
    /// What is wrong with it.
    pub fn problems(&self) -> Vec<QualityProblem> {
        let mut out = Vec::new();
        if let Some(share) = self.shut_while_loud
            && share > SHUT_LIMIT
        {
            out.push(QualityProblem::Shut { share });
        }
        if let (Some(near), Some((far, ms))) = (self.alignment, self.offset)
            && self.seconds >= OFFSET_MIN_SECONDS
            && far >= OFFSET_ALIGNMENT
            && far - near >= OFFSET_GAIN
        {
            out.push(QualityProblem::Offset { ms });
        }
        if let Some(t) = self.text
            && let Some(r) = t.ratio()
            && !(CLOSURES.0..=CLOSURES.1).contains(&r)
            && (t.placed as f64) < PLACED * t.bilabials as f64
        {
            out.push(QualityProblem::Closures(t));
        }
        out
    }
}

/// A directory's lip files, graded together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetGrade {
    /// Lip files judged: those whose sound holds speech.
    pub files: usize,
    /// Their median [`Quality::alignment`], when enough of them have one (a mouth that never opens has none).
    pub alignment: Option<f64>,
    /// Their median [`Quality::shut_while_loud`].
    pub shut: f64,
    /// Their median [`TextFit::ratio`], when enough of them have text with m, b or p.
    pub closures: Option<f64>,
}

/// A way a set of lip files fails its sounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetProblem {
    /// The mouths follow their sounds little better than chance: median alignment this.
    Unaligned(f64),
    /// The mouths stay shut through this median share of the loud speech.
    Shut(f64),
    /// The lips close this many times per m, b and p of the text (median).
    Closures(f64),
}

impl fmt::Display for SetProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetProblem::Unaligned(m) => write!(
                f,
                "its lip files follow their sounds poorly: median alignment {m:.2}, under {SET_ALIGNMENT}"
            ),
            SetProblem::Shut(m) => write!(
                f,
                "its lip files keep the mouth shut through {:.0}% of the loud speech (median), over {:.0}%",
                100.0 * m,
                100.0 * SET_SHUT
            ),
            SetProblem::Closures(m) => write!(
                f,
                "its lip files close the lips {m:.2} times per m, b and p of their lines (median), outside {} to {}",
                SET_CLOSURES.0, SET_CLOSURES.1
            ),
        }
    }
}

/// Grades a directory's lip files, when there are enough of them for chance to even out.
pub fn grade(files: &[Quality]) -> Option<SetGrade> {
    let alignments: Vec<f64> = files.iter().filter_map(|q| q.alignment).collect();
    let shut: Vec<f64> = files.iter().filter_map(|q| q.shut_while_loud).collect();
    let ratios: Vec<f64> = files.iter().filter_map(|q| q.text?.ratio()).collect();
    (shut.len() >= SET_MIN).then(|| SetGrade {
        files: shut.len(),
        alignment: (alignments.len() >= SET_MIN).then(|| median(&alignments)),
        shut: median(&shut),
        closures: (ratios.len() >= SET_MIN).then(|| median(&ratios)),
    })
}

impl SetGrade {
    /// What is wrong with the set.
    pub fn problems(&self) -> Vec<SetProblem> {
        let mut out = Vec::new();
        if let Some(a) = self.alignment
            && a < SET_ALIGNMENT
        {
            out.push(SetProblem::Unaligned(a));
        }
        if self.shut > SET_SHUT {
            out.push(SetProblem::Shut(self.shut));
        }
        if let Some(c) = self.closures
            && !(SET_CLOSURES.0..=SET_CLOSURES.1).contains(&c)
        {
            out.push(SetProblem::Closures(c));
        }
        out
    }
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => f64::NAN,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// The head frame `l` shows in each of `frames` 10 ms frames of `audio`.
pub fn head_frames(l: &Lip, audio: &Audio, frames: usize) -> Vec<u8> {
    let channels = usize::from(audio.channels);
    let mut out = Vec::with_capacity(frames);
    let mut m = 0;
    for f in 0..frames {
        let position = i64::try_from(2 * frame_start(f, audio.rate) * channels).unwrap_or(i64::MAX);
        while m + 1 < l.markers.len() && i64::from(l.markers[m + 1].position) <= position {
            m += 1;
        }
        let code = l.phonemes.get(m).copied().unwrap_or(0);
        out.push(if code < PHONEME_COUNT {
            HEAD_FRAME[usize::from(code)]
        } else {
            0
        });
    }
    out
}

/// Head frames 0 (rest) and 6 (closed lips: m, p, b) show a shut mouth.
fn open(frame: u8) -> bool {
    frame != 0 && frame != 6
}

/// Judges `l` against `audio`, and against the line's `text` when given. Not at all when `l` holds codes the game
/// misreads: its shapes are unknown.
pub fn quality(l: &Lip, audio: &Audio, text: Option<&[Token]>) -> Quality {
    let analysis = speech::analyse(audio, &SpeechParams::default());
    let db: Vec<f32> = analysis.frames.iter().map(|f| f.db).collect();
    let n = db.len();
    let seconds = audio.frames() as f64 / f64::from(audio.rate);
    if analysis.segments.is_empty() || l.phonemes.iter().any(|&c| c >= PHONEME_COUNT) {
        return Quality {
            shut_while_loud: None,
            alignment: None,
            offset: None,
            seconds,
            text: None,
        };
    }
    let mut speaking = vec![false; n];
    for s in &analysis.segments {
        speaking[s.start.min(n)..s.end.min(n)].fill(true);
    }
    let shown = head_frames(l, audio, n);
    let mut sorted = db.clone();
    sorted.sort_by(f32::total_cmp);
    let p95 = sorted[((n - 1) as f64 * 0.95).round() as usize];
    let loud: Vec<usize> = (0..n).filter(|&i| db[i] >= p95 - LOUD_DB).collect();
    let shut = loud.iter().filter(|&&i| !open(shown[i])).count();
    let (mut near, mut far) = (None::<f64>, None::<(f64, f64)>);
    for lag in -MAX_FRAMES..=MAX_FRAMES {
        let Some(a) = alignment(&shown, &db, &speaking, lag) else {
            continue;
        };
        if lag.abs() <= UNSEEN_FRAMES {
            near = Some(near.map_or(a, |b| b.max(a)));
        }
        if lag.abs() >= SEEN_FRAMES && far.is_none_or(|(b, _)| a > b) {
            far = Some((a, f64::from(lag * 10)));
        }
    }
    let span = (
        analysis.segments.first().map_or(0, |s| s.start),
        analysis.segments.last().map_or(0, |s| s.end.min(n)),
    );
    Quality {
        shut_while_loud: (!loud.is_empty()).then(|| shut as f64 / loud.len() as f64),
        alignment: near,
        offset: far,
        seconds,
        text: text.map(|t| text_fit(&shown, &speaking, span, t)),
    }
}

/// Closed-lips shapes (head frame 6) in or within 50 ms of speech, against the text's m, b and p, each placed where
/// it falls with the line's phones spread evenly over `span`, the frames from the first speech to the last.
fn text_fit(shown: &[u8], speaking: &[bool], span: (usize, usize), text: &[Token]) -> TextFit {
    let n = shown.len();
    let mut closures = Vec::new();
    let mut start = 0;
    for i in 1..=n {
        if i == n || shown[i] != shown[start] {
            if shown[start] == 6 && speaking[start.saturating_sub(5)..(i + 5).min(n)].contains(&true) {
                closures.push((start + i) as f64 / 2.0);
            }
            start = i;
        }
    }
    let phones: Vec<bool> = text
        .iter()
        .filter_map(|t| if let Token::Word(p) = t { Some(p) } else { None })
        .flatten()
        .map(|p| matches!(p.sound, Arpabet::M | Arpabet::B | Arpabet::P))
        .collect();
    let (from, to) = (span.0 as f64, span.1 as f64);
    let each = (to - from) / phones.len().max(1) as f64;
    let placed = phones
        .iter()
        .enumerate()
        .filter(|&(i, &b)| {
            b && closures
                .iter()
                .any(|&c| (c - (from + (i as f64 + 0.5) * each)).abs() <= NEAR_FRAMES)
        })
        .count();
    TextFit {
        bilabials: phones.iter().filter(|&&b| b).count(),
        closures: closures.len(),
        placed,
    }
}

/// The chance that a speech frame showing an open mouth `lag` frames later is louder than one showing it shut
/// (Mann-Whitney, ties counted half); `None` without both.
fn alignment(shown: &[u8], db: &[f32], speaking: &[bool], lag: i32) -> Option<f64> {
    let (mut opened, mut shut) = (Vec::new(), Vec::new());
    for (i, &d) in db.iter().enumerate() {
        let Some(j) = i.checked_add_signed(lag as isize).filter(|&j| j < shown.len()) else {
            continue;
        };
        if speaking[i] {
            if open(shown[j]) { opened.push(d) } else { shut.push(d) }
        }
    }
    if opened.is_empty() || shut.is_empty() {
        return None;
    }
    shut.sort_by(f32::total_cmp);
    let wins: f64 = opened
        .iter()
        .map(|&x| {
            let below = shut.partition_point(|&y| y < x);
            let level = shut.partition_point(|&y| y <= x);
            below as f64 + 0.5 * (level - below) as f64
        })
        .sum();
    Some(wins / (opened.len() as f64 * shut.len() as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lip::Marker;
    use crate::lipgen::{Params, generate};

    /// Three seconds at 22050 Hz: bursts of a tone 80 to 250 ms long, 40 to 150 ms apart, after a quarter second of
    /// silence; irregular, so no offset lines the bursts up again.
    fn speech() -> Audio {
        let rate = 22050;
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |lo: usize, hi: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed % (hi - lo) as u64) as usize
        };
        let mut samples = vec![0i16; rate / 4];
        while samples.len() < 3 * rate - rate / 2 {
            let on = next(80, 250) * rate / 1000;
            let start = samples.len();
            samples.extend((0..on).map(|i| (12000.0 * ((start + i) as f64 * 0.04).sin()) as i16));
            samples.extend(std::iter::repeat_n(0, next(40, 150) * rate / 1000));
        }
        samples.resize(3 * rate, 0);
        Audio::from_i16(22050, 1, samples)
    }

    fn whole(code: u8, audio: &Audio) -> Lip {
        let last = 2 * audio.len() as i32;
        Lip::new(
            "x",
            vec![code],
            vec![
                Marker { kind: 1, position: 0 },
                Marker {
                    kind: 1,
                    position: last,
                },
            ],
        )
    }

    #[test]
    fn a_lip_file_made_from_its_sound_passes() {
        let audio = speech();
        let q = quality(&generate("x", &audio, None, &Params::default()), &audio, None);
        assert!(q.shut_while_loud.unwrap() < 0.2, "{q:?}");
        assert!(q.alignment.unwrap() > 0.8, "{q:?}");
        assert_eq!(q.problems(), []);
    }

    #[test]
    fn a_mouth_shut_through_the_speech_is_an_error() {
        let audio = speech();
        let q = quality(&whole(0, &audio), &audio, None);
        assert_eq!(q.problems(), [QualityProblem::Shut { share: 1.0 }]);
        assert_eq!(q.problems()[0].severity(), Severity::Error);
    }

    #[test]
    fn a_mouth_running_late_is_a_warning_naming_the_offset() {
        let audio = speech();
        let mut late = generate("x", &audio, None, &Params::default());
        let (by, last) = (2 * 22050 / 4, late.last());
        for m in &mut late.markers {
            m.position = (m.position + by).min(last);
        }
        let q = quality(&late, &audio, None);
        let [QualityProblem::Offset { ms }] = q.problems()[..] else {
            panic!("{q:?}")
        };
        assert!((200.0..=300.0).contains(&ms), "{ms}");
        assert_eq!(q.problems()[0].severity(), Severity::Warning);
        // On a line too short to tell, the same measures report nothing.
        let at = |seconds| Quality { seconds, ..q };
        assert_eq!(at(OFFSET_MIN_SECONDS).problems(), q.problems());
        assert_eq!(at(OFFSET_MIN_SECONDS - 0.1).problems(), []);
    }

    #[test]
    fn codes_the_game_misreads_are_not_judged() {
        let audio = speech();
        let q = quality(&whole(PHONEME_COUNT, &audio), &audio, None);
        assert_eq!((q.shut_while_loud, q.alignment, q.problems()), (None, None, vec![]));
    }

    #[test]
    fn closed_lips_are_counted_against_the_texts_m_b_and_p() {
        let audio = speech();
        // "Maybe, Bob": M EY B IY, B AA B: four, and a mouth that never closes for them.
        let words = crate::g2p::tokens("Maybe, Bob.");
        let q = quality(&whole(1, &audio), &audio, Some(&words));
        let t = TextFit {
            bilabials: 4,
            closures: 0,
            placed: 0,
        };
        assert_eq!(q.text, Some(t));
        assert!(q.problems().contains(&QualityProblem::Closures(t)));
        assert_eq!(QualityProblem::Closures(t).severity(), Severity::Warning);
        // One closure for each (the ratio fits), or the right count misplaced, or none to make: no warning.
        let with = |text| Quality { text: Some(text), ..q };
        let fits = |t: TextFit| {
            !with(t)
                .problems()
                .iter()
                .any(|p| matches!(p, QualityProblem::Closures(_)))
        };
        assert!(fits(TextFit { closures: 4, ..t }));
        assert!(fits(TextFit {
            closures: 9,
            placed: 1,
            ..t
        }));
        assert!(!fits(TextFit {
            closures: 9,
            placed: 0,
            ..t
        }));
        assert!(fits(TextFit { bilabials: 0, ..t }));
        assert_eq!(quality(&whole(1, &audio), &audio, None).text, None);
    }

    #[test]
    fn a_set_is_graded_by_its_medians_once_it_is_large_enough() {
        let file = |alignment, shut| Quality {
            shut_while_loud: Some(shut),
            alignment: Some(alignment),
            offset: None,
            seconds: 3.0,
            text: None,
        };
        assert_eq!(grade(&vec![file(0.5, 0.9); SET_MIN - 1]), None);
        let good = grade(&vec![file(0.75, 0.1); SET_MIN]).unwrap();
        assert_eq!((good.files, good.problems()), (SET_MIN, vec![]));
        let poor = grade(&vec![file(0.55, 0.4); SET_MIN]).unwrap();
        assert_eq!(poor.problems(), [SetProblem::Unaligned(0.55), SetProblem::Shut(0.4)]);
        // Mouths that never open have no alignment, and are graded all the same.
        let shut = Quality {
            alignment: None,
            ..file(0.0, 1.0)
        };
        let g = grade(&vec![shut; SET_MIN]).unwrap();
        assert_eq!((g.alignment, g.problems()), (None, vec![SetProblem::Shut(1.0)]));
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), 2.5);
        // With text: the median of closures per m, b and p.
        let closing = |closures| Quality {
            text: Some(TextFit {
                bilabials: 4,
                closures,
                placed: 4,
            }),
            ..file(0.75, 0.1)
        };
        let g = grade(&vec![closing(4); SET_MIN]).unwrap();
        assert_eq!((g.closures, g.problems()), (Some(1.0), vec![]));
        let g = grade(&vec![closing(8); SET_MIN]).unwrap();
        assert_eq!(g.problems(), [SetProblem::Closures(2.0)]);
        assert_eq!(
            grade(&vec![closing(0); SET_MIN]).unwrap().problems(),
            [SetProblem::Closures(0.0)]
        );
    }
}
