//! What a stream's content says about its header: the true channel count.
//!
//! Fallout's engine ignores the ACM channel field (it plays speech and effects mono, music stereo), so many of its
//! mono files claim two channels. Such a file, read as stereo frames, alternates between consecutive samples of one
//! signal: the steps from left to right and from right to the next left are alike and no larger than a step within
//! a channel. The thresholds separate labelled real files (`tests/data/channel-features.tsv`; the test beside it
//! names the few files no threshold separates).

/// Interleaved mono scores below this; stereo above.
pub const MONO_SCORE_LIMIT: f64 = 1.6;

/// The two cross steps of interleaved mono agree to within this ratio.
const CROSS_STEP_AGREEMENT: f64 = 0.9;

/// Mean absolute steps of a stream read as stereo frames `(L, R)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Features {
    /// `mean |R[n] - L[n]|`.
    pub a: f64,
    /// `mean |L[n+1] - R[n]|`.
    pub b: f64,
    /// Mean step within a channel, `(|L[n+1] - L[n]| + |R[n+1] - R[n]|) / 2`.
    pub w: f64,
}

/// What the features say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// One signal stored across both channels.
    InterleavedMono,
    /// Two channels.
    Stereo,
    /// Silent or constant: no evidence either way.
    Undecidable,
}

impl Features {
    /// The features of `samples` read as stereo frames; `None` with fewer than two frames.
    pub fn of(samples: &[i16]) -> Option<Features> {
        let frames = samples.as_chunks::<2>().0;
        if frames.len() < 2 {
            return None;
        }
        let (mut a, mut b, mut w) = (0.0, 0.0, 0.0);
        for pair in frames.windows(2) {
            let [[l, r], [l1, r1]] = [pair[0], pair[1]].map(|f| f.map(f64::from));
            a += (r - l).abs();
            b += (l1 - r).abs();
            w += ((l1 - l).abs() + (r1 - r).abs()) / 2.0;
        }
        let k = (frames.len() - 1) as f64;
        Some(Features {
            a: a / k,
            b: b / k,
            w: w / k,
        })
    }

    /// `max(a, b) / w`: below [`MONO_SCORE_LIMIT`] for interleaved mono.
    pub fn score(&self) -> f64 {
        self.a.max(self.b) / self.w
    }

    /// The verdict, by the measured thresholds.
    pub fn verdict(&self) -> Verdict {
        let (low, high) = (self.a.min(self.b), self.a.max(self.b));
        if high == 0.0 || self.w == 0.0 {
            return Verdict::Undecidable;
        }
        if low / high > CROSS_STEP_AGREEMENT && self.score() < MONO_SCORE_LIMIT {
            Verdict::InterleavedMono
        } else {
            Verdict::Stereo
        }
    }
}

/// The evidence for a stream's channel count, given its header's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelEvidence {
    /// The header says mono; nothing contradicts that.
    HeaderMono,
    /// A stereo header over an odd sample count: no whole number of frames, so mono.
    OddSampleCount,
    /// A stereo header over content that reads as interleaved mono.
    InterleavedMono {
        /// [`Features::score`].
        score: f64,
    },
    /// A stereo header over stereo content.
    Stereo {
        /// [`Features::score`].
        score: f64,
    },
    /// A stereo header over silent or constant content.
    Undecidable,
    /// More than two channels: not checked.
    NotChecked,
    /// A WAVC whose own header and inner ACM header agree: the Infinity Engine plays it as they say.
    WavcHeaders,
}

impl ChannelEvidence {
    /// Weighs a bare ACM's content against its header's channel count. Bare ACM is Fallout's format, whose engine
    /// ignores the field, so the content decides.
    pub fn of(header_channels: u16, samples: &[i16]) -> ChannelEvidence {
        match header_channels {
            0 | 1 => ChannelEvidence::HeaderMono,
            2 if samples.len() % 2 == 1 => ChannelEvidence::OddSampleCount,
            2 => match Features::of(samples) {
                None => ChannelEvidence::Undecidable,
                Some(f) => match f.verdict() {
                    Verdict::InterleavedMono => ChannelEvidence::InterleavedMono { score: f.score() },
                    Verdict::Stereo => ChannelEvidence::Stereo { score: f.score() },
                    Verdict::Undecidable => ChannelEvidence::Undecidable,
                },
            },
            _ => ChannelEvidence::NotChecked,
        }
    }

    /// Weighs a WAVC's two headers and its content. The Infinity Engine plays a WAVC by its header, and some stereo
    /// effects read as interleaved mono yet play right as stereo (`tests/channel_detector.rs` counts them), so
    /// agreeing headers are trusted; disagreeing ones, or two that claim no channels, are settled as a bare ACM's
    /// would be.
    pub fn of_wavc(wavc_channels: u16, acm_channels: u16, samples: &[i16]) -> ChannelEvidence {
        if wavc_channels == 2 && acm_channels == 2 && samples.len() % 2 == 1 {
            ChannelEvidence::OddSampleCount
        } else if wavc_channels == acm_channels && wavc_channels > 0 {
            ChannelEvidence::WavcHeaders
        } else {
            ChannelEvidence::of(wavc_channels.max(acm_channels), samples)
        }
    }

    /// The channel count to use: mono where the evidence says so, else `header_channels` (for a WAVC, the larger
    /// of its two headers' counts).
    pub fn channels(&self, header_channels: u16) -> u16 {
        match self {
            ChannelEvidence::HeaderMono | ChannelEvidence::OddSampleCount | ChannelEvidence::InterleavedMono { .. } => {
                1
            }
            ChannelEvidence::Stereo { .. }
            | ChannelEvidence::Undecidable
            | ChannelEvidence::NotChecked
            | ChannelEvidence::WavcHeaders => header_channels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth signal: two slow sines, scaled to a few thousand LSB.
    fn smooth(len: usize, f1: f64, f2: f64) -> Vec<i16> {
        (0..len)
            .map(|n| (3000.0 * (n as f64 * f1).sin() + 2000.0 * (n as f64 * f2).sin()) as i16)
            .collect()
    }

    /// Stereo frames from two channels.
    fn interleave(l: &[i16], r: &[i16]) -> Vec<i16> {
        l.iter().zip(r).flat_map(|(&a, &b)| [a, b]).collect()
    }

    #[test]
    fn mono_stored_as_stereo_frames_reads_as_interleaved_mono() {
        // One signal, consecutive samples alternating between "left" and "right".
        let s = smooth(20000, 0.031, 0.0071);
        let e = ChannelEvidence::of(2, &s);
        assert!(matches!(e, ChannelEvidence::InterleavedMono { .. }), "{e:?}");
        assert_eq!(e.channels(2), 1);
    }

    #[test]
    fn two_different_channels_read_as_stereo() {
        let s = interleave(&smooth(10000, 0.02, 0.005), &smooth(10000, 0.013, 0.0031));
        let e = ChannelEvidence::of(2, &s);
        assert!(matches!(e, ChannelEvidence::Stereo { .. }), "{e:?}");
        assert_eq!(e.channels(2), 2);
    }

    #[test]
    fn identical_channels_are_stereo() {
        let m = smooth(10000, 0.02, 0.005);
        let e = ChannelEvidence::of(2, &interleave(&m, &m));
        assert!(matches!(e, ChannelEvidence::Stereo { .. }), "{e:?}");
    }

    #[test]
    fn silence_is_undecidable_and_keeps_the_header() {
        let e = ChannelEvidence::of(2, &[0; 1000]);
        assert_eq!(e, ChannelEvidence::Undecidable);
        assert_eq!(e.channels(2), 2);
    }

    #[test]
    fn an_odd_sample_count_under_a_stereo_header_is_mono() {
        let s = interleave(&smooth(10000, 0.02, 0.005), &smooth(10000, 0.013, 0.0031));
        let e = ChannelEvidence::of(2, &s[..s.len() - 1]);
        assert_eq!(e, ChannelEvidence::OddSampleCount);
        assert_eq!(e.channels(2), 1);
    }

    #[test]
    fn a_mono_header_is_trusted() {
        let e = ChannelEvidence::of(1, &smooth(1000, 0.02, 0.005));
        assert_eq!(e, ChannelEvidence::HeaderMono);
        assert_eq!(e.channels(1), 1);
    }

    #[test]
    fn agreeing_stereo_wavc_headers_over_an_odd_count_still_read_as_mono() {
        // No whole number of stereo frames, whatever the headers say.
        let e = ChannelEvidence::of_wavc(2, 2, &[1, 2, 3]);
        assert_eq!(e, ChannelEvidence::OddSampleCount);
        assert_eq!(e.channels(2), 1);
    }

    #[test]
    fn agreeing_wavc_headers_are_trusted_over_the_content() {
        // The Infinity Engine plays a WAVC by its header; content that reads as interleaved mono plays right there.
        let s = smooth(20000, 0.031, 0.0071);
        let e = ChannelEvidence::of_wavc(2, 2, &s);
        assert_eq!(e, ChannelEvidence::WavcHeaders);
        assert_eq!(e.channels(2), 2);
        assert_eq!(ChannelEvidence::of_wavc(1, 1, &s).channels(1), 1);
    }

    #[test]
    fn disagreeing_wavc_headers_are_settled_by_the_content() {
        let s = smooth(20000, 0.031, 0.0071);
        let e = ChannelEvidence::of_wavc(1, 2, &s);
        assert!(matches!(e, ChannelEvidence::InterleavedMono { .. }), "{e:?}");
        assert_eq!(e.channels(2), 1);
    }

    #[test]
    fn other_channel_counts_are_kept() {
        assert_eq!(ChannelEvidence::of(6, &[0; 60]), ChannelEvidence::NotChecked);
        assert_eq!(ChannelEvidence::NotChecked.channels(6), 6);
    }

    #[test]
    fn the_threshold_sits_between_the_measured_sets() {
        assert_eq!(
            Features {
                a: 100.0,
                b: 100.0,
                w: 100.0 / 1.5
            }
            .verdict(),
            Verdict::InterleavedMono
        );
        assert_eq!(
            Features {
                a: 100.0,
                b: 100.0,
                w: 100.0 / 1.7
            }
            .verdict(),
            Verdict::Stereo
        );
        // The two cross steps must be alike: one much larger than the other is stereo.
        assert_eq!(
            Features {
                a: 100.0,
                b: 80.0,
                w: 100.0
            }
            .verdict(),
            Verdict::Stereo
        );
        assert_eq!(
            Features {
                a: 100.0,
                b: 91.0,
                w: 100.0
            }
            .verdict(),
            Verdict::InterleavedMono
        );
    }
}
