//! Decoded audio at its source depth, and its reduction to 16 bits.

/// Interleaved audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// Frames per second; never 0 ([`crate::input::read`] refuses it).
    pub rate: u32,
    /// Channels per frame; never 0 ([`crate::input::read`] refuses it).
    pub channels: u16,
    /// The samples, all channels interleaved.
    pub samples: Samples,
}

/// Samples in their source's format.
#[derive(Debug, Clone, PartialEq)]
pub enum Samples {
    /// Integers left-aligned in 32 bits (an `n`-bit sample `v` is `v << (32 - n)`), with their significant bits.
    Int {
        /// Significant bits: 8, 16, 24 or 32.
        bits: u16,
        /// The samples.
        data: Vec<i32>,
    },
    /// Floating point, full scale at +-1.
    Float(Vec<f32>),
}

impl Audio {
    /// 16-bit samples.
    pub fn from_i16(rate: u32, channels: u16, samples: Vec<i16>) -> Audio {
        let data = samples.into_iter().map(|s| i32::from(s) << 16).collect();
        Audio {
            rate,
            channels,
            samples: Samples::Int { bits: 16, data },
        }
    }

    /// Significant bits of integer samples; `None` for float.
    pub fn bits(&self) -> Option<u16> {
        match &self.samples {
            Samples::Int { bits, .. } => Some(*bits),
            Samples::Float(_) => None,
        }
    }

    /// True for integers deeper than 16 bits, and for float.
    pub fn deeper_than_16(&self) -> bool {
        !matches!(self.bits(), Some(b) if b <= 16)
    }

    /// Samples, all channels together.
    pub fn len(&self) -> usize {
        match &self.samples {
            Samples::Int { data, .. } => data.len(),
            Samples::Float(data) => data.len(),
        }
    }

    /// True when there are no samples.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whole frames.
    pub fn frames(&self) -> usize {
        self.len() / usize::from(self.channels)
    }

    /// The samples as 16-bit integers: exact at 16 bits or fewer, else with TPDF dither (reproducible: a fixed
    /// seed) and clamped.
    pub fn to_i16(&self) -> Vec<i16> {
        let mut dither = Dither::new();
        match &self.samples {
            Samples::Int { bits, data } if *bits <= 16 => data.iter().map(|&v| (v >> 16) as i16).collect(),
            Samples::Int { data, .. } => data.iter().map(|&v| dither.quantise(f64::from(v) / 65536.0)).collect(),
            Samples::Float(data) => data.iter().map(|&v| dither.quantise(f64::from(v) * 32768.0)).collect(),
        }
    }

    /// The samples as floats, full scale at +-1.
    pub fn to_f32(&self) -> Vec<f32> {
        match &self.samples {
            Samples::Int { data, .. } => data.iter().map(|&v| (f64::from(v) / 2_147_483_648.0) as f32).collect(),
            Samples::Float(data) => data.clone(),
        }
    }

    /// The audio at another rate, at the source's depth (integer samples are dithered back to it), through a
    /// fixed-ratio FFT resampler with its delay trimmed; at the same rate, unchanged.
    ///
    /// # Errors
    ///
    /// When the resampler cannot be built for these rates or channels.
    pub fn resample(&self, rate: u32) -> Result<Audio, ResampleError> {
        use rubato::Resampler;
        use rubato::audioadapter_buffers::direct::InterleavedSlice;
        if rate == self.rate {
            return Ok(self.clone());
        }
        let channels = usize::from(self.channels);
        let input = self.to_f32();
        let frames = self.frames();
        let mut r = rubato::Fft::<f32>::new(
            self.rate as usize,
            rate as usize,
            1024,
            channels,
            rubato::FixedSync::Input,
        )?;
        let adapter = InterleavedSlice::new(&input[..frames * channels], channels, frames)?;
        let out = r.process_all(&adapter, frames, None)?.take_data();
        let samples = match self.bits() {
            None => Samples::Float(out),
            Some(bits) => {
                let mut dither = Dither::new();
                let scale = f64::from(1u32 << (bits - 1));
                let data = out
                    .iter()
                    .map(|&v| dither.quantise_to(f64::from(v) * scale, bits) << (32 - bits))
                    .collect();
                Samples::Int { bits, data }
            }
        };
        Ok(Audio {
            rate,
            channels: self.channels,
            samples,
        })
    }
}

/// The resampler refused the job.
#[derive(Debug, thiserror::Error)]
pub enum ResampleError {
    /// No resampler for these rates or channels.
    #[error("no resampler for this conversion")]
    Construction(#[from] rubato::ResamplerConstructionError),
    /// The samples do not fill whole frames for the adapter.
    #[error("samples do not fill whole frames")]
    Layout(#[from] rubato::audioadapter_buffers::SizeError),
    /// Resampling failed.
    #[error("resampling failed")]
    Process(#[from] rubato::ResampleError),
}

/// Triangular dither of one 16-bit step, from a xorshift generator.
struct Dither(u64);

impl Dither {
    /// From a fixed seed, so the same input always gives the same output.
    fn new() -> Dither {
        Dither(0x9E37_79B9_7F4A_7C15)
    }

    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `v` (in 16-bit steps) plus triangular noise, rounded and clamped.
    fn quantise(&mut self, v: f64) -> i16 {
        self.quantise_to(v, 16) as i16
    }

    /// `v` (in steps of a `bits`-bit sample) plus triangular noise, rounded and clamped to `bits` bits.
    fn quantise_to(&mut self, v: f64, bits: u16) -> i32 {
        let noise = self.uniform() - self.uniform();
        let top = f64::from(1u32 << (bits - 1));
        (v + noise).round().clamp(-top, top - 1.0) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_bit_samples_pass_through_exactly() {
        let s = vec![i16::MIN, -1, 0, 1, i16::MAX];
        let a = Audio::from_i16(22050, 1, s.clone());
        assert_eq!(a.bits(), Some(16));
        assert_eq!(a.to_i16(), s);
    }

    #[test]
    fn eight_bit_samples_widen_exactly() {
        // Left-aligned: an 8-bit value v is v << 24.
        let a = Audio {
            rate: 8000,
            channels: 1,
            samples: Samples::Int {
                bits: 8,
                data: vec![-128 << 24, 127 << 24],
            },
        };
        assert_eq!(a.to_i16(), [-128 << 8, 127 << 8]);
    }

    #[test]
    fn deeper_samples_are_dithered_to_within_one_step() {
        let data: Vec<i32> = (0..10_000)
            .map(|n| (n * 7919 % 65536 - 32768) << 16 | (n * 31 % 65536))
            .collect();
        let a = Audio {
            rate: 44100,
            channels: 2,
            samples: Samples::Int {
                bits: 24,
                data: data.clone(),
            },
        };
        let out = a.to_i16();
        for (o, d) in out.iter().zip(&data) {
            let exact = f64::from(*d) / 65536.0;
            assert!((f64::from(*o) - exact).abs() <= 1.5, "{o} from {exact}");
        }
        // Dithered, not truncated: some samples round up.
        assert!(out.iter().zip(&data).any(|(o, d)| i32::from(*o) > d >> 16));
        // Reproducible.
        assert_eq!(a.to_i16(), out);
    }

    #[test]
    fn float_samples_are_scaled_and_clamped() {
        let a = Audio {
            rate: 44100,
            channels: 1,
            samples: Samples::Float(vec![0.0, 0.5, -1.0, 1.5, -2.0]),
        };
        let out = a.to_i16();
        assert!(out[0].abs() <= 1);
        assert!((i32::from(out[1]) - 16384).abs() <= 1);
        assert_eq!(&out[2..], [i16::MIN, i16::MAX, i16::MIN]);
    }

    #[test]
    fn resampling_keeps_length_in_time_and_the_signal() {
        // A 441 Hz stereo tone, one second at 22050 Hz.
        let s: Vec<i16> = (0..2 * 22050)
            .map(|i| (10000.0 * ((i / 2) as f64 * 441.0 * std::f64::consts::TAU / 22050.0).sin()) as i16)
            .collect();
        let a = Audio::from_i16(22050, 2, s);
        let b = a.resample(44100).unwrap();
        assert_eq!((b.rate, b.channels), (44100, 2));
        assert_eq!(b.frames(), 44100);
        // Away from the edges, the output is the same tone at the new rate.
        let y = b.to_f32();
        for frame in (2000..42000).step_by(997) {
            let expected = 10000.0 / 32768.0 * (frame as f64 * 441.0 * std::f64::consts::TAU / 44100.0).sin();
            assert!((f64::from(y[2 * frame]) - expected).abs() < 0.01, "frame {frame}");
        }
    }

    #[test]
    fn resampling_keeps_the_source_depth() {
        let a = Audio::from_i16(22050, 1, (0..4000).map(|n| (n * 7 % 2000) as i16 - 1000).collect());
        assert_eq!(a.resample(44100).unwrap().bits(), Some(16));
        let f = Audio {
            rate: 22050,
            channels: 1,
            samples: Samples::Float(vec![0.0; 4000]),
        };
        assert_eq!(f.resample(44100).unwrap().bits(), None);
    }

    #[test]
    fn resampling_to_the_same_rate_changes_nothing() {
        let a = Audio::from_i16(22050, 1, vec![1, 2, 3]);
        assert_eq!(a.resample(22050).unwrap(), a);
    }

    #[test]
    fn frames_count_whole_frames() {
        assert_eq!(Audio::from_i16(22050, 2, vec![0; 6]).frames(), 3);
    }

    #[test]
    fn float_view_of_integers_is_full_scale() {
        let a = Audio::from_i16(22050, 1, vec![i16::MIN, 0, 16384]);
        assert_eq!(a.to_f32(), [-1.0, 0.0, 0.5]);
    }
}
