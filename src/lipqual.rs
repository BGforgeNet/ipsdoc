//! How well a lip file follows its sound: whether the mouth opens while the line is loud, at what offset it follows
//! the loudness best, and, given the line's text, whether the lips close for its m, b and p.
//!
//! The limits come from Fallout 2's 993 voiced lines whose lip file ends with its sound, against their own lip files
//! (Interplay's), ours, wav2lip's, and spoiled copies of the originals; the text from the Unofficial Patch's English
//! dialog. `oracles/tests/lipqual.rs` measures every figure quoted here.

use std::fmt;

use crate::g2p::{Arpabet, Token};
use crate::lip::{CLOSED, HEAD_FRAME, Lip, PHONEME_COUNT, Severity};
use crate::pcm::Audio;
use crate::speech::{self, SpeechParams, frame_start};

/// Loud frames: within this many dB of the line's 95th-percentile loudness.
const LOUD_DB: f32 = 15.0;
/// A mouth shut through more of the loud frames than this is an error: wav2lip's files are (96% of them), none of
/// Interplay's (with valid codes) or ours is.
const SHUT_LIMIT: f64 = 0.5;
/// Offsets no player sees go unjudged, and alignment is the best within them: a mouth up to this many 10 ms frames
/// late (sound ahead of the picture shows from about 45 ms, ITU-R BT.1359) ...
const LATE_UNSEEN_FRAMES: i32 = 5;
/// ... or this many early (sound behind the picture shows from about 125 ms). The engine's own lead, its mouth
/// following the mixer at least one 23 ms buffer ahead of the speakers, is not counted: late errs strict.
const EARLY_UNSEEN_FRAMES: i32 = 12;
/// The longest offset looked for under the plain limits: 300 ms.
const OFFSET_FRAMES: i32 = 30;
/// The longest far offset looked for: 600 ms. Looking to 1 s under the plain limits flagged 11% of Interplay's files,
/// half of them chance alignments past 600 ms; a file that far off shows as open while quiet, its directory unaligned.
const MAX_FRAMES: i32 = 60;
/// Frames this close to speech are not quiet: a shape is held into a pause, and opens ahead of the voice.
const QUIET_MARGIN: usize = 15;
/// A sound with fewer quiet frames than this has no quiet to judge.
const QUIET_MIN: usize = 20;
/// A mouth open through more of the quiet frames than this is a warning: on 5% of Interplay's files with quiet to
/// judge, none of ours or wav2lip's, three quarters of the originals moved 500 ms late or shuffled, and every file
/// holding one open shape.
const OPEN_QUIET_LIMIT: f64 = 0.5;
/// Speech shorter than this (frames) has too few shapes for a rate.
const MOVEMENT_MIN_FRAMES: usize = 100;
/// Shape changes per second of speech outside these are a warning, a mouth that hardly moves or one that jitters:
/// Interplay's files range 1.9 (a drawn-out "Riiiight...", the one warned about) to 14.7, wav2lip's 0.5 to 38.
const MOVEMENT: (f64, f64) = (2.0, 20.0);
/// One shape held through more than this of unbroken speech (seconds) is a warning, a mouth frozen while the voice
/// goes on: on 3.7% of Interplay's files (most of them drawn-out words, "Ahhh", "Hmmm"; their median 0.33), 11% of
/// ours from the text, 26% from the sound alone, and 69% of wav2lip's.
const HOLD_LIMIT: f64 = 0.65;
/// Head frames 0 to 8 ranked by the median loudness rank of the speech frames showing them in Interplay's files
/// (rest 0.14, then frame 4, closed lips 0.43, 2, 5, 3, 8, 1, and 7 at 0.71): how wide each opens, as voiced.
const OPENNESS: [u8; 9] = [0, 7, 3, 5, 1, 4, 2, 8, 6];
/// A line this long or longer (seconds) has enough speech to show an offset: about a quarter of Interplay's shorter
/// lines align as well somewhere off by chance.
const OFFSET_MIN_SECONDS: f64 = 2.0;
/// An offset is reported when the mouth aligns this well there ...
const OFFSET_ALIGNMENT: f64 = 0.7;
/// ... and this much better than within the unseen offsets, and better than chance ([`CHANCE_COPIES`]). It flags
/// 0.4% of Interplay's lines, 0.5% of ours from the text and 0.1% from the sound; 19% of the originals moved 100 ms
/// late, 53% moved 200 ms late. Interplay's lines 100 ms early get 2.5%.
const OFFSET_GAIN: f64 = 0.08;
/// A far offset needs a line this long (seconds) ...
const FAR_OFFSET_MIN_SECONDS: f64 = 3.0;
/// ... and this gain, as more offsets are tried there: it flags none of Interplay's lines, and 52% of the originals
/// moved 500 ms late.
const FAR_OFFSET_GAIN: f64 = 0.25;
/// An offset is reported only when it aligns better than each of this many copies of the line's lip file, its shapes
/// in speech shuffled, manage at their best offset in the same band: chance under 1 in 20.
const CHANCE_COPIES: usize = 19;
/// A directory of at least this many lip files is graded as a set, where chance evens out ...
const SET_MIN: usize = 15;
/// ... and warned about below this median alignment: Fallout 2's 11 speech directories have 0.67 to 0.96, ours 0.89
/// to 0.99; speakers' sets of other lines' lip files 0.52 to 0.75, so width catches those alignment misses ...
const SET_ALIGNMENT: f64 = 0.6;
/// ... or above this median share of the loud speech shown shut: Fallout 2's directories have 0.06 to 0.25, ours
/// 0.01 to 0.06 per speaker, wav2lip's 0.78 to 0.90 ...
const SET_SHUT: f64 = 0.35;
/// ... or with a median of closures per m, b and p outside these: Fallout 2's directories have 0.75 to 1.50,
/// wav2lip's 0 (it never closes the lips) ...
const SET_CLOSURES: (f64, f64) = (0.5, 1.6);
/// ... or below this median openness: Fallout 2's directories have 0.23 to 0.51, ours 0.39 to 0.68, wav2lip's 0.21
/// to 0.31, other lines' lip files and the originals moved 200 ms or more or shuffled 0.13 at most ...
const SET_OPENNESS: f64 = 0.16;
/// ... or above this median share of the quiet shown open: Fallout 2's directories have 0 to 0.06, other lines' lip
/// files and the originals moved 500 ms late or shuffled 0.42 and more ...
const SET_OPEN_QUIET: f64 = 0.25;
/// ... or with a median of shape changes a second of speech outside these: Fallout 2's directories have 6.1 to 9.1,
/// wav2lip's 2.0 to 15.5.
const SET_MOVEMENT: (f64, f64) = (4.0, 12.0);
/// Interplay's lip files close the lips once per m, b or p of the text (the middle half of them exactly once); a
/// lip file closing them at a ratio outside these ...
const CLOSURES: (f64, f64) = (0.67, 1.5);
/// ... with under this share of the m, b and p met by a closure nearby, misses the words: Interplay's 3%, ours from
/// the text 1%, from the sound alone 26% (it cannot know them), another line's lip file stretched to fit 41%,
/// wav2lip's all.
const PLACED: f64 = 0.25;
/// A closure meets an m, b or p within this many frames of where the sound falls with the line's phones spread
/// evenly over its speech.
const NEAR_FRAMES: f64 = 20.0;

/// How a lip file's closed lips fit its line's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFit {
    /// The text's m, b and p, a run of them counted once (see `text_fit`).
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

impl fmt::Display for TextFit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bilabials == 0 {
            return write!(f, "the line has no m, b or p to check the closed lips against");
        }
        write!(
            f,
            "the lips close {} times for the line's {} m, b and p, {} of them nearby",
            self.closures, self.bilabials, self.placed
        )
    }
}

/// The best alignment within a band of offsets a player sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offset {
    /// The alignment there.
    pub alignment: f64,
    /// The offset, in milliseconds (positive: the mouth runs late).
    pub ms: f64,
    /// The best that copies of the lip file, its shapes in speech shuffled, reach in the band; `None` unless the
    /// offset would be reported otherwise, as each copy costs a sweep of the band.
    pub chance: Option<f64>,
}

/// How a lip file follows its sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    /// Share of the loud frames that show a shut mouth (rest, or closed lips); `None` for a sound with no speech.
    pub shut_while_loud: Option<f64>,
    /// The part of [`Quality::shut_while_loud`] showing closed lips, as an m, b or p does.
    pub closed_while_loud: Option<f64>,
    /// Chance that a frame of speech showing an open mouth is louder than one showing it at rest: 0.5 is chance, 1 a
    /// mouth that opens exactly with the loudness. Closed lips count on neither side, as an m is voiced. The best
    /// within the offsets no player sees. `None` when the mouth is never open, or never at rest, in speech.
    pub alignment: Option<f64>,
    /// Rank correlation of how wide the mouth opens (head frames ranked by the loudness they show in Interplay's
    /// files) with the loudness, over speech: 0 is chance. The best within the offsets no player sees. `None` when the
    /// mouth shows one width all through the speech.
    pub openness: Option<f64>,
    /// The best alignment 60 to 300 ms late or 130 to 300 ms early; `None` for a line too short to show one.
    pub offset: Option<Offset>,
    /// The same 310 to 600 ms off, where a line must be longer still.
    pub far_offset: Option<Offset>,
    /// Share of the quiet frames, away from speech, that show an open mouth; `None` for a sound with little quiet.
    pub open_while_quiet: Option<f64>,
    /// Shape changes per second of speech; `None` for a sound with little speech.
    pub movement: Option<f64>,
    /// The longest one shape is held through unbroken speech, in seconds; `None` for a sound with no speech.
    pub longest_hold: Option<f64>,
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
    /// The mouth stays open through this share of the quiet.
    OpenWhileQuiet {
        /// The share.
        share: f64,
    },
    /// The mouth changes shape this many times a second of speech: too seldom, or too often.
    Movement {
        /// Changes a second.
        rate: f64,
    },
    /// The mouth holds one shape through this many seconds of unbroken speech.
    Held {
        /// Seconds.
        seconds: f64,
    },
}

impl QualityProblem {
    /// How much it matters.
    pub fn severity(&self) -> Severity {
        match self {
            QualityProblem::Shut { .. } => Severity::Error,
            QualityProblem::Offset { .. }
            | QualityProblem::Closures(_)
            | QualityProblem::OpenWhileQuiet { .. }
            | QualityProblem::Movement { .. }
            | QualityProblem::Held { .. } => Severity::Warning,
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
            QualityProblem::Closures(t) => t.fmt(f),
            QualityProblem::OpenWhileQuiet { share } => {
                write!(f, "the mouth is open through {:.0}% of the quiet", 100.0 * share)
            }
            QualityProblem::Movement { rate } => {
                let how = if *rate < MOVEMENT.0 { "hardly moves" } else { "jitters" };
                write!(f, "the mouth {how}: {rate:.1} shape changes a second of speech")
            }
            QualityProblem::Held { seconds } => {
                write!(
                    f,
                    "the mouth holds one shape through {:.0} ms of unbroken speech",
                    1000.0 * seconds
                )
            }
        }
    }
}

/// Whether an offset aligning `offset` would be reported against `near`, the alignment where no offset shows, before
/// chance is weighed.
fn stands_out(near: Option<f64>, offset: f64, gain: f64) -> bool {
    near.is_some_and(|near| offset >= OFFSET_ALIGNMENT && offset - near >= gain)
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
        let found = |offset: Option<Offset>, gain| {
            offset
                .filter(|o| stands_out(self.alignment, o.alignment, gain) && o.chance.is_some_and(|c| o.alignment > c))
                .map(|o| o.ms)
        };
        if let Some(ms) = found(self.offset, OFFSET_GAIN).or_else(|| found(self.far_offset, FAR_OFFSET_GAIN)) {
            out.push(QualityProblem::Offset { ms });
        }
        if let Some(t) = self.text
            && let Some(r) = t.ratio()
            && !(CLOSURES.0..=CLOSURES.1).contains(&r)
            && (t.placed as f64) < PLACED * t.bilabials as f64
        {
            out.push(QualityProblem::Closures(t));
        }
        if let Some(share) = self.open_while_quiet
            && share > OPEN_QUIET_LIMIT
        {
            out.push(QualityProblem::OpenWhileQuiet { share });
        }
        if let Some(rate) = self.movement
            && !(MOVEMENT.0..=MOVEMENT.1).contains(&rate)
        {
            out.push(QualityProblem::Movement { rate });
        }
        if let Some(seconds) = self.longest_hold
            && seconds > HOLD_LIMIT
        {
            out.push(QualityProblem::Held { seconds });
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
    /// Their median [`Quality::closed_while_loud`].
    pub closed: f64,
    /// Their median [`TextFit::ratio`], when enough of them have text with m, b or p.
    pub closures: Option<f64>,
    /// Their median [`Quality::openness`], when enough of them have one.
    pub openness: Option<f64>,
    /// Their median [`Quality::open_while_quiet`], when enough of them have one.
    pub open_while_quiet: Option<f64>,
    /// Their median [`Quality::movement`], when enough of them have one.
    pub movement: Option<f64>,
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
    /// How wide the mouths open follows the loudness little better than chance: median rank correlation this.
    Openness(f64),
    /// The mouths stay open through this median share of the quiet.
    OpenWhileQuiet(f64),
    /// The mouths change shape this many times a second of speech (median).
    Movement(f64),
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
            SetProblem::Openness(m) => write!(
                f,
                "how wide its lip files open the mouth follows their sounds poorly: median rank correlation {m:.2}, \
                 under {SET_OPENNESS}"
            ),
            SetProblem::OpenWhileQuiet(m) => write!(
                f,
                "its lip files keep the mouth open through {:.0}% of the quiet (median), over {:.0}%",
                100.0 * m,
                100.0 * SET_OPEN_QUIET
            ),
            SetProblem::Movement(m) => write!(
                f,
                "its lip files change the mouth's shape {m:.1} times a second of speech (median), outside {} to {}",
                SET_MOVEMENT.0, SET_MOVEMENT.1
            ),
        }
    }
}

/// Grades a directory's lip files, when there are enough of them for chance to even out.
pub fn grade(files: &[Quality]) -> Option<SetGrade> {
    let shut: Vec<f64> = files.iter().filter_map(|q| q.shut_while_loud).collect();
    let closed: Vec<f64> = files.iter().filter_map(|q| q.closed_while_loud).collect();
    // The median of a figure, when enough of the files have it.
    let of = |figure: &dyn Fn(&Quality) -> Option<f64>| {
        let v: Vec<f64> = files.iter().filter_map(figure).collect();
        (v.len() >= SET_MIN).then(|| median(&v))
    };
    (shut.len() >= SET_MIN).then(|| SetGrade {
        files: shut.len(),
        alignment: of(&|q| q.alignment),
        shut: median(&shut),
        closed: median(&closed),
        closures: of(&|q| q.text?.ratio()),
        openness: of(&|q| q.openness),
        open_while_quiet: of(&|q| q.open_while_quiet),
        movement: of(&|q| q.movement),
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
        if let Some(o) = self.openness
            && o < SET_OPENNESS
        {
            out.push(SetProblem::Openness(o));
        }
        if let Some(o) = self.open_while_quiet
            && o > SET_OPEN_QUIET
        {
            out.push(SetProblem::OpenWhileQuiet(o));
        }
        if let Some(m) = self.movement
            && !(SET_MOVEMENT.0..=SET_MOVEMENT.1).contains(&m)
        {
            out.push(SetProblem::Movement(m));
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

/// Head frames 0 (rest) and [`CLOSED`] show a shut mouth.
fn open(frame: u8) -> bool {
    frame != 0 && frame != CLOSED
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
            closed_while_loud: None,
            alignment: None,
            openness: None,
            offset: None,
            far_offset: None,
            open_while_quiet: None,
            movement: None,
            longest_hold: None,
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
    let closed = loud.iter().filter(|&&i| shown[i] == CLOSED).count();
    let openness = (-EARLY_UNSEEN_FRAMES..=LATE_UNSEEN_FRAMES)
        .filter_map(|lag| width_correlation(&shown, &db, &speaking, lag))
        .reduce(f64::max);
    // The best alignment of `shown` over the offsets of a band, and that offset.
    let best = |shown: &[u8], band: Band| {
        (-MAX_FRAMES..=MAX_FRAMES)
            .filter(|&lag| band_of(lag) == band)
            .filter_map(|lag| Some((alignment(shown, &db, &speaking, lag)?, lag)))
            .reduce(|b, x| if x.0 > b.0 { x } else { b })
    };
    let near = best(&shown, Band::Unseen).map(|b| b.0);
    let offset = |band: Band, gain: f64, min_seconds: f64| {
        let (a, lag) = best(&shown, band).filter(|_| seconds >= min_seconds)?;
        let chance = stands_out(near, a, gain).then(|| {
            let mut seed = SEED;
            (0..CHANCE_COPIES)
                .filter_map(|_| best(&shuffled(&shown, &speaking, &mut seed), band).map(|b| b.0))
                .fold(f64::NEG_INFINITY, f64::max)
        });
        Some(Offset {
            alignment: a,
            ms: f64::from(lag * 10),
            chance,
        })
    };
    let (mut hold, mut longest) = (0usize, 0usize);
    for i in 0..n {
        hold = match speaking[i] {
            false => 0,
            true if i > 0 && speaking[i - 1] && shown[i] == shown[i - 1] => hold + 1,
            true => 1,
        };
        longest = longest.max(hold);
    }
    let span = (
        analysis.segments.first().map_or(0, |s| s.start),
        analysis.segments.last().map_or(0, |s| s.end.min(n)),
    );
    let quiet = quiet_frames(&speaking);
    let open_quiet = quiet.iter().filter(|&&i| open(shown[i])).count();
    let speech_frames = speaking.iter().filter(|&&s| s).count();
    let changes = (1..n).filter(|&i| speaking[i] && shown[i] != shown[i - 1]).count();
    Quality {
        shut_while_loud: (!loud.is_empty()).then(|| shut as f64 / loud.len() as f64),
        closed_while_loud: (!loud.is_empty()).then(|| closed as f64 / loud.len() as f64),
        alignment: near,
        openness,
        offset: offset(Band::Near, OFFSET_GAIN, OFFSET_MIN_SECONDS),
        far_offset: offset(Band::Far, FAR_OFFSET_GAIN, FAR_OFFSET_MIN_SECONDS),
        open_while_quiet: (quiet.len() >= QUIET_MIN).then(|| open_quiet as f64 / quiet.len() as f64),
        movement: (speech_frames >= MOVEMENT_MIN_FRAMES)
            .then(|| changes as f64 * f64::from(speech::FRAMES_PER_SECOND) / speech_frames as f64),
        longest_hold: Some(longest as f64 / f64::from(speech::FRAMES_PER_SECOND)),
        text: text.map(|t| text_fit(&shown, &speaking, span, t)),
    }
}

/// Where an offset falls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Band {
    /// No player sees it.
    Unseen,
    /// Seen, up to [`OFFSET_FRAMES`].
    Near,
    /// Beyond, up to [`MAX_FRAMES`].
    Far,
}

/// The band of an offset of `lag` frames (positive: the mouth runs late).
fn band_of(lag: i32) -> Band {
    if (-EARLY_UNSEEN_FRAMES..=LATE_UNSEEN_FRAMES).contains(&lag) {
        Band::Unseen
    } else if lag.abs() <= OFFSET_FRAMES {
        Band::Near
    } else {
        Band::Far
    }
}

/// The seed of the shuffled copies, fixed so a file is judged the same each time.
const SEED: u64 = 0x2545_F491_4F6C_DD1D;

/// `shown` with its runs of one head frame inside speech in a seeded random order, each keeping its length; the
/// frames outside speech stay where they are.
fn shuffled(shown: &[u8], speaking: &[bool], seed: &mut u64) -> Vec<u8> {
    let mut runs: Vec<(u8, usize)> = Vec::new();
    for (i, &frame) in shown.iter().enumerate() {
        if !speaking[i] {
            continue;
        }
        match runs.last_mut() {
            Some((f, len)) if *f == frame && i > 0 && speaking[i - 1] => *len += 1,
            _ => runs.push((frame, 1)),
        }
    }
    for i in (1..runs.len()).rev() {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        runs.swap(i, (*seed % (i as u64 + 1)) as usize);
    }
    let mut out = shown.to_vec();
    let frames = runs.iter().flat_map(|&(f, len)| std::iter::repeat_n(f, len));
    for (o, f) in out
        .iter_mut()
        .zip(speaking)
        .filter(|(_, s)| **s)
        .map(|(o, _)| o)
        .zip(frames)
    {
        *o = f;
    }
    out
}

/// The frames more than [`QUIET_MARGIN`] frames from any speech.
fn quiet_frames(speaking: &[bool]) -> Vec<usize> {
    let n = speaking.len();
    (0..n)
        .filter(|&i| !speaking[i.saturating_sub(QUIET_MARGIN)..(i + QUIET_MARGIN + 1).min(n)].contains(&true))
        .collect()
}

/// Spearman's rank correlation of how wide the mouth opens `lag` frames later ([`OPENNESS`]) with the loudness, over
/// the speech frames; `None` when either is constant.
fn width_correlation(shown: &[u8], db: &[f32], speaking: &[bool], lag: i32) -> Option<f64> {
    let (mut width, mut loudness) = (Vec::new(), Vec::new());
    for (i, &d) in db.iter().enumerate() {
        let Some(j) = i.checked_add_signed(lag as isize).filter(|&j| j < shown.len()) else {
            continue;
        };
        if speaking[i] {
            width.push(f64::from(OPENNESS[usize::from(shown[j])]));
            loudness.push(f64::from(d));
        }
    }
    pearson(&ranks(&width), &ranks(&loudness))
}

/// Each value's rank from 1, ties sharing their mean rank.
fn ranks(v: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..v.len()).collect();
    order.sort_by(|&a, &b| v[a].total_cmp(&v[b]));
    let mut out = vec![0.0; v.len()];
    let mut i = 0;
    while i < order.len() {
        let j = i + order[i..]
            .iter()
            .take_while(|&&k| v[k].total_cmp(&v[order[i]]).is_eq())
            .count();
        let rank = (i + j + 1) as f64 / 2.0;
        for &k in &order[i..j] {
            out[k] = rank;
        }
        i = j;
    }
    out
}

/// Pearson's correlation; `None` when either series is constant.
fn pearson(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len() as f64;
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
        syy += (b - my) * (b - my);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
}

/// Closed-lips shapes ([`CLOSED`]) in or within 50 ms of speech, against the text's m, b and p, each placed where
/// it falls with the line's phones spread evenly over `span`, the frames from the first speech to the last. A run of
/// them (the m b of "remember", the m b of "come back") is one closure, as the lips close once for it; a phrase
/// break ends a run. Not placed by `lipgen`'s alignment, which would let our generator judge its own files.
fn text_fit(shown: &[u8], speaking: &[bool], span: (usize, usize), text: &[Token]) -> TextFit {
    let n = shown.len();
    let mut closures = Vec::new();
    let mut start = 0;
    for i in 1..=n {
        if i == n || shown[i] != shown[start] {
            if shown[start] == CLOSED && speaking[start.saturating_sub(5)..(i + 5).min(n)].contains(&true) {
                closures.push((start + i) as f64 / 2.0);
            }
            start = i;
        }
    }
    // Each phone (a vocal noise counts as one) and whether it is m, b or p; `None` for a phrase break.
    let phones: Vec<Option<bool>> = text
        .iter()
        .flat_map(|t| match t {
            Token::Word(p) => p
                .iter()
                .map(|p| Some(matches!(p.sound, Arpabet::M | Arpabet::B | Arpabet::P)))
                .collect(),
            Token::Noise => vec![Some(false)],
            Token::Break => vec![None],
        })
        .collect();
    let slots = phones.iter().flatten().count();
    let (from, to) = (span.0 as f64, span.1 as f64);
    let each = (to - from) / slots.max(1) as f64;
    // The runs, as the middle of each in phone slots.
    let mut runs = Vec::new();
    let (mut slot, mut run) = (0usize, None::<usize>);
    for p in phones.iter().chain([None].iter()) {
        match (p, run) {
            (Some(true), None) => run = Some(slot),
            (Some(true), Some(_)) => {}
            (_, Some(first)) => {
                runs.push((first + slot) as f64 / 2.0);
                run = None;
            }
            _ => {}
        }
        slot += usize::from(p.is_some());
    }
    let placed = runs
        .iter()
        .filter(|&&r| closures.iter().any(|&c| (c - (from + r * each)).abs() <= NEAR_FRAMES))
        .count();
    TextFit {
        bilabials: runs.len(),
        closures: closures.len(),
        placed,
    }
}

/// The chance that a speech frame showing an open mouth `lag` frames later is louder than one showing it at rest
/// (Mann-Whitney, ties counted half); `None` without both.
fn alignment(shown: &[u8], db: &[f32], speaking: &[bool], lag: i32) -> Option<f64> {
    let (mut opened, mut shut) = (Vec::new(), Vec::new());
    for (i, &d) in db.iter().enumerate() {
        let Some(j) = i.checked_add_signed(lag as isize).filter(|&j| j < shown.len()) else {
            continue;
        };
        if speaking[i] {
            match shown[j] {
                CLOSED => {}
                0 => shut.push(d),
                _ => opened.push(d),
            }
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
        talking().0
    }

    /// [`speech`], and the sample ranges of its bursts.
    fn talking() -> (Audio, Vec<(usize, usize)>) {
        let rate = 22050;
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |lo: usize, hi: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed % (hi - lo) as u64) as usize
        };
        let (mut samples, mut bursts) = (vec![0i16; rate / 4], Vec::new());
        while samples.len() < 3 * rate - rate / 2 {
            let on = next(80, 250) * rate / 1000;
            let start = samples.len();
            samples.extend((0..on).map(|i| (12000.0 * ((start + i) as f64 * 0.04).sin()) as i16));
            bursts.push((start, samples.len()));
            samples.extend(std::iter::repeat_n(0, next(40, 150) * rate / 1000));
        }
        samples.resize(3 * rate, 0);
        (Audio::from_i16(22050, 1, samples), bursts)
    }

    /// A lip file opening the mouth (head frame 3) over each burst, `late` samples after it (before it when
    /// negative), and at rest between.
    fn following(audio: &Audio, bursts: &[(usize, usize)], late: isize) -> Lip {
        following_with(audio, bursts, late, &[1])
    }

    /// [`following`], each burst showing `codes` in turn, evenly.
    fn following_with(audio: &Audio, bursts: &[(usize, usize)], late: isize, shapes: &[u8]) -> Lip {
        let last = 2 * audio.len() as i32;
        let at = |s: usize| (2 * s.saturating_add_signed(late)).min(audio.len() * 2) as i32;
        let (mut codes, mut markers) = (vec![0], vec![Marker { kind: 1, position: 0 }]);
        for &(a, b) in bursts {
            for (k, &code) in shapes.iter().enumerate() {
                codes.push(code);
                markers.push(Marker {
                    kind: 1,
                    position: at(a + (b - a) * k / shapes.len()),
                });
            }
            codes.push(0);
            markers.push(Marker {
                kind: 1,
                position: at(b),
            });
        }
        markers.push(Marker {
            kind: 1,
            position: last,
        });
        Lip::new("x", codes, markers)
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
        assert!(q.alignment.unwrap() > SET_ALIGNMENT, "{q:?}");
        assert!(q.openness.unwrap() > SET_OPENNESS, "{q:?}");
        assert_eq!(q.problems(), []);
    }

    #[test]
    fn a_mouth_shut_through_the_speech_is_an_error() {
        let audio = speech();
        let q = quality(&whole(0, &audio), &audio, None);
        // A mouth that never opens does not move either, and holds its shape through the longest stretch of speech.
        assert_eq!(
            q.problems(),
            [
                QualityProblem::Shut { share: 1.0 },
                QualityProblem::Movement { rate: 0.0 },
                QualityProblem::Held { seconds: 1.23 }
            ]
        );
        assert_eq!(q.problems()[0].severity(), Severity::Error);
    }

    #[test]
    fn a_mouth_running_late_is_a_warning_naming_the_offset() {
        let (audio, bursts) = talking();
        // In time: nothing to say.
        assert_eq!(quality(&following(&audio, &bursts, 0), &audio, None).problems(), []);
        // A quarter second late.
        let q = quality(&following(&audio, &bursts, 22050 / 4), &audio, None);
        let [QualityProblem::Offset { ms }] = q.problems()[..] else {
            panic!("{q:?}")
        };
        assert!((200.0..=300.0).contains(&ms), "{ms}");
        assert_eq!(q.problems()[0].severity(), Severity::Warning);
        // Cut shorter, the line first shows no far offset, then none at all.
        let cut = |tenths: usize| {
            let short = Audio::from_i16(22050, 1, audio.to_i16()[..22050 * tenths / 10].to_vec());
            quality(&following(&short, &bursts, 22050 / 4), &short, None)
        };
        let q = cut(25);
        assert!(q.offset.is_some() && q.far_offset.is_none(), "{q:?}");
        let q = cut(19);
        assert_eq!((q.offset, q.far_offset, q.problems()), (None, None, vec![]));
    }

    #[test]
    fn a_mouth_100_ms_late_is_seen_and_100_ms_early_is_not() {
        let (audio, bursts) = talking();
        let q = quality(&following(&audio, &bursts, 22050 / 10), &audio, None);
        assert_eq!(q.problems(), [QualityProblem::Offset { ms: 100.0 }], "{q:?}");
        let q = quality(&following(&audio, &bursts, -22050 / 10), &audio, None);
        assert_eq!(q.problems(), [], "{q:?}");
    }

    #[test]
    fn an_offset_no_better_than_shuffled_copies_is_not_reported() {
        let q = |chance| Quality {
            shut_while_loud: Some(0.0),
            closed_while_loud: Some(0.0),
            alignment: Some(0.6),
            openness: None,
            offset: Some(Offset {
                alignment: 0.9,
                ms: 200.0,
                chance,
            }),
            far_offset: None,
            open_while_quiet: None,
            movement: None,
            longest_hold: None,
            text: None,
        };
        assert_eq!(q(Some(0.8)).problems(), [QualityProblem::Offset { ms: 200.0 }]);
        assert_eq!(q(Some(0.9)).problems(), []);
        // Not weighed against chance: not standing out in the first place.
        assert_eq!(q(None).problems(), []);
    }

    #[test]
    fn closed_lips_count_neither_as_open_nor_as_rest() {
        let (audio, bursts) = talking();
        // The lips close for the first half of each burst: loud and shut, yet aligned as well as a mouth open all
        // through (better, as the onsets' quieter frames drop out); counted as rest, they would be most of the shut.
        let q = quality(&following_with(&audio, &bursts, 0, &[17, 1]), &audio, None);
        let all = quality(&following(&audio, &bursts, 0), &audio, None);
        assert!(q.alignment.unwrap() >= all.alignment.unwrap(), "{q:?} {all:?}");
        // The shut frames at rest are the same in both; the closed lips come on top.
        let at_rest = |q: &Quality| q.shut_while_loud.unwrap() - q.closed_while_loud.unwrap();
        assert!((at_rest(&q) - at_rest(&all)).abs() < 1e-9, "{q:?} {all:?}");
        assert!(q.closed_while_loud.unwrap() > 0.3, "{q:?}");
        assert_eq!(all.closed_while_loud, Some(0.0));
    }

    #[test]
    fn movement_and_holds_are_judged_at_their_limits() {
        let q = |movement, longest_hold| {
            Quality {
                shut_while_loud: Some(0.0),
                closed_while_loud: Some(0.0),
                alignment: None,
                openness: None,
                offset: None,
                far_offset: None,
                open_while_quiet: None,
                movement: Some(movement),
                longest_hold: Some(longest_hold),
                text: None,
            }
            .problems()
        };
        assert_eq!(q(MOVEMENT.0, HOLD_LIMIT), []);
        assert_eq!(
            q(1.9, 0.66),
            [
                QualityProblem::Movement { rate: 1.9 },
                QualityProblem::Held { seconds: 0.66 }
            ]
        );
    }

    #[test]
    fn a_mouth_held_open_from_start_to_end_is_warned_about() {
        let audio = speech();
        let q = quality(&whole(1, &audio), &audio, None);
        // Never shut in speech, so neither alignment nor width says anything: the quiet and the stillness do.
        assert_eq!((q.alignment, q.openness), (None, None));
        assert_eq!(
            q.problems(),
            [
                QualityProblem::OpenWhileQuiet { share: 1.0 },
                QualityProblem::Movement { rate: 0.0 },
                QualityProblem::Held { seconds: 1.23 }
            ]
        );
    }

    #[test]
    fn a_mouth_changing_shape_every_frame_jitters() {
        let audio = speech();
        let last = 2 * audio.len() as i32;
        // A new shape every 10 ms (221 samples), open and wide by turns.
        let positions: Vec<i32> = (0..)
            .map(|i| i * 2 * 221)
            .take_while(|&p| p < last)
            .chain([last])
            .collect();
        let codes = (0..positions.len() - 1)
            .map(|i| if i % 2 == 0 { 1 } else { 2 })
            .collect();
        let markers = positions
            .into_iter()
            .map(|position| Marker { kind: 1, position })
            .collect();
        let q = quality(&Lip::new("x", codes, markers), &audio, None);
        assert!(
            q.problems()
                .iter()
                .any(|p| matches!(p, QualityProblem::Movement { rate } if *rate > MOVEMENT.1)),
            "{q:?}"
        );
    }

    #[test]
    fn quiet_frames_keep_their_distance_from_speech() {
        let speaking: Vec<bool> = [vec![false; 20], vec![true; 5], vec![false; 40]].concat();
        let want: Vec<usize> = (0..5).chain(40..65).collect();
        assert_eq!(quiet_frames(&speaking), want);
    }

    #[test]
    fn a_line_without_m_b_or_p_says_so() {
        let fit = TextFit {
            bilabials: 0,
            closures: 2,
            placed: 0,
        };
        assert_eq!(
            (fit.ratio(), fit.to_string().as_str()),
            (None, "the line has no m, b or p to check the closed lips against")
        );
    }

    #[test]
    fn ranks_share_ties_and_a_constant_series_has_no_correlation() {
        assert_eq!(ranks(&[3.0, 1.0, 3.0, 2.0]), [3.5, 1.0, 3.5, 2.0]);
        assert_eq!(pearson(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]), Some(1.0));
        assert_eq!(pearson(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), None);
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
        // "Remember, come back": R IH M EH M B ER, K AH M B AE K. The m b of each closes the lips once, a phrase
        // break or no.
        let words = crate::g2p::tokens("Remember, come back.");
        let fit = quality(&whole(1, &audio), &audio, Some(&words)).text.unwrap();
        assert_eq!(fit.bilabials, 3);
    }

    #[test]
    fn a_set_is_graded_by_its_medians_once_it_is_large_enough() {
        let file = |alignment, shut| Quality {
            shut_while_loud: Some(shut),
            closed_while_loud: Some(0.0),
            alignment: Some(alignment),
            openness: None,
            offset: None,
            far_offset: None,
            open_while_quiet: None,
            movement: None,
            longest_hold: None,
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
        // Width, quiet and movement: graded by their medians once enough files have them.
        let moving = |openness, quiet, movement| Quality {
            openness: Some(openness),
            open_while_quiet: Some(quiet),
            movement: Some(movement),
            ..file(0.75, 0.1)
        };
        let g = grade(&vec![moving(0.5, 0.0, 7.0); SET_MIN]).unwrap();
        assert_eq!(
            (g.openness, g.open_while_quiet, g.movement, g.problems()),
            (Some(0.5), Some(0.0), Some(7.0), vec![])
        );
        assert_eq!(
            grade(&vec![moving(0.0, 1.0, 0.0); SET_MIN]).unwrap().problems(),
            [
                SetProblem::Openness(0.0),
                SetProblem::OpenWhileQuiet(1.0),
                SetProblem::Movement(0.0)
            ]
        );
        let mut few = vec![moving(0.0, 1.0, 0.0); SET_MIN - 1];
        few.push(file(0.75, 0.1));
        assert_eq!(grade(&few).unwrap().problems(), []);
    }
}
