//! Writing [`Audio`]: ACM and WAVC lossless with true headers (16-bit, so deeper audio is dithered), or from the ACM
//! stream the audio was read from, kept under a new header; RIFF at 8 or 16 bits (deeper audio dithered to 16 unless
//! asked to keep it), Ogg Vorbis through libvorbis (aoTuV and Lancer builds, via `vorbis_rs`).

use std::io::Cursor;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;

#[cfg(test)]
use crate::detect::Format;
use crate::pcm::{Audio, Samples};

/// A container to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormat {
    /// Bare ACM (Fallout, Infinity Engine music).
    Acm,
    /// WAVC V1 (Infinity Engine sounds, named `.wav` in a game).
    Wavc,
    /// RIFF WAVE PCM.
    Riff,
    /// Ogg Vorbis (Enhanced Edition sounds).
    Ogg,
}

impl OutputFormat {
    /// The format a file name's extension asks for: `.acm`, `.wavc`, `.wav`, `.ogg`.
    pub fn from_path(path: &Path) -> Option<OutputFormat> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "acm" => Some(OutputFormat::Acm),
            "wavc" => Some(OutputFormat::Wavc),
            "wav" => Some(OutputFormat::Riff),
            "ogg" => Some(OutputFormat::Ogg),
            _ => None,
        }
    }
}

/// How to write.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WriteOptions {
    /// Vorbis quality on oggenc's scale, -1 to 10.
    pub vorbis_quality: f32,
    /// RIFF at the source's depth; otherwise audio deeper than 16 bits is dithered to 16.
    pub riff_full_depth: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            vorbis_quality: 5.0,
            riff_full_depth: false,
        }
    }
}

/// Why audio cannot be written.
#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    /// ACM headers hold a 16-bit rate.
    #[error("ACM cannot hold a rate of {rate} Hz (at most 65535)")]
    AcmRate {
        /// The rate.
        rate: u32,
    },
    /// The encoder refused the samples.
    #[error("ACM")]
    Acm(#[from] crate::acm::EncodeError),
    /// The ACM to wrap in a WAVC has no readable header.
    #[error("ACM header")]
    AcmHeader(#[from] crate::acm::HeaderError),
    /// RIFF writing failed.
    #[error("RIFF")]
    Riff(#[from] hound::Error),
    /// Vorbis encoding failed.
    #[error("Vorbis")]
    Vorbis(#[from] vorbis_rs::VorbisError),
    /// A quality outside -1 to 10.
    #[error("Vorbis quality {quality} is outside -1 to 10")]
    VorbisQuality {
        /// The quality asked for.
        quality: f32,
    },
    /// Vorbis takes 1 to 255 channels, at a non-zero rate.
    #[error("Vorbis cannot take {channels} channels at {rate} Hz")]
    VorbisLayout {
        /// Channels.
        channels: u16,
        /// Rate.
        rate: u32,
    },
}

/// Writes `audio` as `format`.
///
/// # Errors
///
/// When the format cannot hold the audio's rate or layout, or an encoder fails.
pub fn write(audio: &Audio, format: OutputFormat, opts: &WriteOptions) -> Result<Vec<u8>, OutputError> {
    match format {
        OutputFormat::Acm => acm(audio),
        OutputFormat::Wavc => Ok(crate::wavc::wrap(&acm(audio)?)?),
        OutputFormat::Riff if riff_dithers(audio, opts) => {
            riff(&Audio::from_i16(audio.rate, audio.channels, audio.to_i16()))
        }
        OutputFormat::Riff => riff(audio),
        OutputFormat::Ogg => vorbis(audio, opts.vorbis_quality),
    }
}

/// Writes `audio` as `format`, as [`write()`] does, except that ACM and WAVC keep `acm`, the ACM stream `audio` was read
/// from, instead of encoding it anew: its bitstream byte for byte, under a header stating `audio`'s channel count and
/// rate. The caller vouches that `acm` decodes to `audio`'s samples.
///
/// # Errors
///
/// When `acm` has no readable header, an ACM header cannot hold `audio`'s rate, or [`write()`] fails.
pub fn write_keeping(
    audio: &Audio,
    acm: &[u8],
    format: OutputFormat,
    opts: &WriteOptions,
) -> Result<Vec<u8>, OutputError> {
    let relabelled = || -> Result<Vec<u8>, OutputError> {
        let header = crate::acm::Header::parse(acm)?
            .relabelled(audio.channels, audio.rate)
            .ok_or(OutputError::AcmRate { rate: audio.rate })?;
        let mut out = header.to_bytes().to_vec();
        out.extend_from_slice(&acm[crate::acm::Header::LEN..]);
        Ok(out)
    };
    match format {
        OutputFormat::Acm => relabelled(),
        OutputFormat::Wavc => Ok(crate::wavc::wrap(&relabelled()?)?),
        OutputFormat::Riff | OutputFormat::Ogg => write(audio, format, opts),
    }
}

/// Whether RIFF output of `audio` is dithered to 16 bits: Infinity Engine games read WAV up to 16-bit, and so do GemRB
/// and Near Infinity.
pub fn riff_dithers(audio: &Audio, opts: &WriteOptions) -> bool {
    !opts.riff_full_depth && audio.deeper_than_16()
}

fn acm(audio: &Audio) -> Result<Vec<u8>, OutputError> {
    let rate = u16::try_from(audio.rate).map_err(|_| OutputError::AcmRate { rate: audio.rate })?;
    Ok(crate::acm::encode(&audio.to_i16(), audio.channels, rate)?)
}

fn riff(audio: &Audio) -> Result<Vec<u8>, OutputError> {
    let (bits, float) = match &audio.samples {
        Samples::Int { bits, .. } => (*bits, false),
        Samples::Float(_) => (32, true),
    };
    let spec = hound::WavSpec {
        channels: audio.channels,
        sample_rate: audio.rate,
        bits_per_sample: bits,
        sample_format: if float {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let mut out = Vec::new();
    let mut w = hound::WavWriter::new(Cursor::new(&mut out), spec)?;
    match &audio.samples {
        Samples::Int { bits, data } => {
            for &v in data {
                w.write_sample(v >> (32 - bits))?;
            }
        }
        Samples::Float(data) => {
            for &v in data {
                w.write_sample(v)?;
            }
        }
    }
    w.finalize()?;
    Ok(out)
}

fn vorbis(audio: &Audio, quality: f32) -> Result<Vec<u8>, OutputError> {
    if !(-1.0..=10.0).contains(&quality) {
        return Err(OutputError::VorbisQuality { quality });
    }
    let layout = OutputError::VorbisLayout {
        channels: audio.channels,
        rate: audio.rate,
    };
    let channels = u8::try_from(audio.channels)
        .ok()
        .and_then(NonZeroU8::new)
        .ok_or(layout)?;
    let rate = NonZeroU32::new(audio.rate).ok_or(OutputError::VorbisLayout {
        channels: audio.channels,
        rate: 0,
    })?;
    let mut builder = vorbis_rs::VorbisEncoderBuilder::new(rate, channels, Vec::new())?;
    builder.bitrate_management_strategy(vorbis_rs::VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: quality / 10.0,
    });
    let mut encoder = builder.build()?;
    let interleaved = audio.to_f32();
    let n = usize::from(audio.channels);
    for chunk in interleaved.chunks(4096 * n) {
        let planar: Vec<Vec<f32>> = (0..n)
            .map(|c| chunk.iter().skip(c).step_by(n).copied().collect())
            .collect();
        encoder.encode_audio_block(&planar)?;
    }
    Ok(encoder.finish()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{ReadOptions, read};

    /// `frames` frames, each channel a sine of its own frequency (true stereo, not one signal across channels).
    fn sine(frames: usize, channels: u16) -> Vec<i16> {
        let n = usize::from(channels);
        (0..frames * n)
            .map(|i| (8000.0 * ((i / n) as f64 * 0.01 * (1 + i % n) as f64).sin()) as i16)
            .collect()
    }

    #[test]
    fn riff_keeps_16_bit_samples_exactly() {
        let a = Audio::from_i16(22050, 2, sine(1000, 2));
        let back = read(
            &write(&a, OutputFormat::Riff, &WriteOptions::default()).unwrap(),
            &ReadOptions::default(),
        )
        .unwrap();
        assert_eq!(back.format, Format::Riff);
        assert_eq!(back.audio, a);
    }

    fn riff_round_trip(a: &Audio, opts: &WriteOptions) -> Audio {
        read(&write(a, OutputFormat::Riff, opts).unwrap(), &ReadOptions::default())
            .unwrap()
            .audio
    }

    #[test]
    fn riff_is_16_bit_unless_asked_for_the_full_depth() {
        let data: Vec<i32> = (0..500).map(|n| (n * 104_729 % 0x100_0000 - 0x80_0000) << 8).collect();
        let deep = Audio {
            rate: 96000,
            channels: 1,
            samples: Samples::Int {
                bits: 24,
                data: data.clone(),
            },
        };
        let float = Audio {
            rate: 44100,
            channels: 2,
            samples: Samples::Float(vec![0.0, 0.25, -0.5, 1.0]),
        };
        let eight = Audio {
            rate: 8000,
            channels: 1,
            samples: Samples::Int {
                bits: 8,
                data: data.iter().map(|v| v & !0xFF_FFFF).collect(),
            },
        };
        let full = WriteOptions {
            riff_full_depth: true,
            ..WriteOptions::default()
        };
        for a in [&deep, &float, &eight] {
            assert_eq!(riff_round_trip(a, &full), *a);
        }
        // By default deeper audio is dithered to 16 bits, as for ACM; 8 bits stay as they are.
        for a in [&deep, &float] {
            assert_eq!(
                riff_round_trip(a, &WriteOptions::default()),
                Audio::from_i16(a.rate, a.channels, a.to_i16())
            );
        }
        assert_eq!(riff_round_trip(&eight, &WriteOptions::default()), eight);
    }

    #[test]
    fn acm_is_lossless_with_true_headers() {
        let a = Audio::from_i16(44100, 2, sine(3000, 2));
        let b = write(&a, OutputFormat::Acm, &WriteOptions::default()).unwrap();
        let h = crate::acm::Header::parse(&b).unwrap();
        assert_eq!((h.channels, h.rate, h.samples), (2, 44100, 6000));
        assert_eq!(read(&b, &ReadOptions::default()).unwrap().audio, a);
    }

    #[test]
    fn wavc_carries_both_headers() {
        let a = Audio::from_i16(22050, 1, sine(700, 1));
        let b = write(&a, OutputFormat::Wavc, &WriteOptions::default()).unwrap();
        let h = crate::wavc::Header::parse(&b).unwrap();
        assert_eq!(
            (h.channels, h.rate, h.bits, h.acm_offset, h.uncompressed),
            (1, 22050, 16, 28, 1400)
        );
        assert_eq!(h.compressed as usize, b.len() - 28);
        let back = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!((back.format, back.audio), (Format::Wavc, a));
    }

    #[test]
    fn acm_needs_a_16_bit_rate() {
        let a = Audio::from_i16(96000, 1, vec![0; 10]);
        assert!(matches!(
            write(&a, OutputFormat::Acm, &WriteOptions::default()),
            Err(OutputError::AcmRate { rate: 96000 })
        ));
    }

    #[test]
    fn a_kept_stream_needs_a_16_bit_rate_too() {
        let acm = crate::acm::encode(&[1, 2, 3, 4], 1, 22050).unwrap();
        let a = Audio::from_i16(96000, 1, vec![1, 2, 3, 4]);
        assert!(matches!(
            write_keeping(&a, &acm, OutputFormat::Acm, &WriteOptions::default()),
            Err(OutputError::AcmRate { rate: 96000 })
        ));
    }

    #[test]
    fn vorbis_round_trips_closely() {
        let a = Audio::from_i16(44100, 2, sine(44100, 2));
        let b = write(&a, OutputFormat::Ogg, &WriteOptions::default()).unwrap();
        let back = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(
            (back.format, back.audio.rate, back.audio.channels),
            (Format::Ogg, 44100, 2)
        );
        assert_eq!(back.audio.frames(), a.frames());
        let (x, y) = (a.to_f32(), back.audio.to_f32());
        let err = x.iter().zip(&y).map(|(p, q)| (p - q).powi(2)).sum::<f32>() / x.len() as f32;
        let power = x.iter().map(|p| p * p).sum::<f32>() / x.len() as f32;
        // At the default quality the error stays some 30 dB below the signal.
        assert!(err < power / 1000.0, "error {err}, signal {power}");
    }

    #[test]
    fn rejects_an_out_of_range_vorbis_quality() {
        let a = Audio::from_i16(44100, 1, vec![0; 100]);
        let opts = WriteOptions {
            vorbis_quality: 11.0,
            ..WriteOptions::default()
        };
        assert!(matches!(
            write(&a, OutputFormat::Ogg, &opts),
            Err(OutputError::VorbisQuality { .. })
        ));
    }

    #[test]
    fn formats_come_from_extensions() {
        assert_eq!(OutputFormat::from_path("a/B.ACM".as_ref()), Some(OutputFormat::Acm));
        assert_eq!(OutputFormat::from_path("x.wav".as_ref()), Some(OutputFormat::Riff));
        assert_eq!(OutputFormat::from_path("x.ogg".as_ref()), Some(OutputFormat::Ogg));
        assert_eq!(OutputFormat::from_path("x.wavc".as_ref()), Some(OutputFormat::Wavc));
        assert_eq!(OutputFormat::from_path("x.mp3".as_ref()), None);
    }
}
