//! Reading any supported sound into [`Audio`]: ACM and WAVC by our decoder, with their headers checked against the
//! content; raw PCM by description; everything else (RIFF, Ogg Vorbis, FLAC, MP3, AAC, ALAC) through Symphonia.

use std::io::Cursor;
use std::num::{NonZeroU16, NonZeroU32};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::formats::TrackType;
use symphonia::core::io::MediaSourceStream;

use crate::acm::{self, Overflow};
pub use crate::analysis::ChannelEvidence;
use crate::analysis::swapped_rate;
pub use crate::detect::Format;
pub use crate::pcm::{Audio, Samples};
use crate::wavc;

/// How to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadOptions {
    /// What to do with ACM samples beyond 16 bits.
    pub overflow: Overflow,
    /// The channel count to use, over the header and the content's evidence.
    pub channels: Option<NonZeroU16>,
    /// The rate to use, over the header.
    pub rate: Option<NonZeroU32>,
    /// Read the input as headerless PCM of this description.
    pub raw: Option<Raw>,
}

/// Headerless PCM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raw {
    /// Frames per second.
    pub rate: NonZeroU32,
    /// Channels per frame.
    pub channels: NonZeroU16,
    /// The sample encoding.
    pub bits: RawBits,
}

/// A headerless sample encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawBits {
    /// Unsigned 8-bit, as in WAV.
    U8,
    /// Signed 16-bit little-endian.
    S16,
}

/// What reading found that the stored headers do not say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Note {
    /// The channel count used differs from the header's, or was weighed against the content.
    Channels {
        /// The header's count (for a WAVC, the larger of its two headers').
        header: u16,
        /// The count used.
        used: u16,
        /// Why.
        evidence: ChannelEvidence,
    },
    /// The stored rate is a byte-swapped standard rate.
    SwappedRate {
        /// As stored.
        stored: u16,
        /// As used.
        used: u32,
    },
    /// ACM samples beyond 16 bits, clamped or wrapped as [`ReadOptions::overflow`] says.
    Overflowed {
        /// How many.
        samples: u64,
    },
}

/// A read sound.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The container.
    pub format: Format,
    /// The audio, with the channel count and rate to use.
    pub audio: Audio,
    /// What the headers do not say.
    pub notes: Vec<Note>,
}

/// Why a sound cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    /// A bad WAVC header.
    #[error("WAVC")]
    Wavc(#[from] wavc::WavcError),
    /// An undecodable ACM stream.
    #[error("ACM")]
    Acm(#[from] acm::DecodeError),
    /// Symphonia could not read the container or decode the audio.
    #[error("cannot decode")]
    Decode(#[source] symphonia::core::errors::Error),
    /// The container holds no audio track, or a track without a sample rate or channel layout.
    #[error("no usable audio track")]
    NoAudio,
    /// No rate from the header, and none given ([`ReadOptions::rate`]).
    #[error("sample rate 0")]
    ZeroRate,
    /// No channels from the container, and none given ([`ReadOptions::channels`]).
    #[error("0 channels")]
    ZeroChannels,
    /// Raw 16-bit input of an odd length.
    #[error("{len} bytes of raw 16-bit PCM: not a whole number of samples")]
    RawLength {
        /// Bytes given.
        len: usize,
    },
}

/// Reads a sound held in memory.
///
/// # Errors
///
/// When the container or its audio cannot be decoded.
pub fn read(bytes: &[u8], opts: &ReadOptions) -> Result<Loaded, InputError> {
    if let Some(raw) = opts.raw {
        let audio = read_raw(bytes, raw)?;
        return Ok(Loaded {
            format: Format::Other,
            audio,
            notes: Vec::new(),
        });
    }
    let format = crate::detect::detect(bytes);
    let mut loaded = match format {
        Format::Acm => read_acm(bytes, None, opts)?,
        Format::Wavc => {
            let header = wavc::Header::parse(bytes)?;
            read_acm(wavc::acm_of(bytes)?, Some(header.channels), opts)?
        }
        Format::Riff | Format::Ogg | Format::Other => Loaded {
            format,
            audio: read_general(bytes)?,
            notes: Vec::new(),
        },
    };
    loaded.format = format;
    if let Some(c) = opts.channels {
        loaded.audio.channels = c.get();
    }
    if let Some(r) = opts.rate {
        loaded.audio.rate = r.get();
    }
    if loaded.audio.rate == 0 {
        return Err(InputError::ZeroRate);
    }
    if loaded.audio.channels == 0 {
        return Err(InputError::ZeroChannels);
    }
    // The notes report what is used, overrides included.
    for n in &mut loaded.notes {
        match n {
            Note::Channels { used, .. } => *used = loaded.audio.channels,
            Note::SwappedRate { used, .. } => *used = loaded.audio.rate,
            Note::Overflowed { .. } => {}
        }
    }
    Ok(loaded)
}

fn read_acm(stream: &[u8], wavc_channels: Option<u16>, opts: &ReadOptions) -> Result<Loaded, InputError> {
    let decoded = acm::decode_with(stream, opts.overflow)?;
    let h = decoded.header;
    let mut notes = Vec::new();
    let (header, evidence) = match wavc_channels {
        None => (h.channels, ChannelEvidence::of(h.channels, &decoded.samples)),
        Some(w) => (
            w.max(h.channels),
            ChannelEvidence::of_wavc(w, h.channels, &decoded.samples),
        ),
    };
    let channels = evidence.channels(header);
    if channels != header
        || matches!(
            evidence,
            ChannelEvidence::InterleavedMono { .. } | ChannelEvidence::Stereo { .. }
        )
    {
        notes.push(Note::Channels {
            header,
            used: channels,
            evidence,
        });
    }
    let rate = match swapped_rate(h.rate) {
        Some(r) => {
            notes.push(Note::SwappedRate {
                stored: h.rate,
                used: r.into(),
            });
            u32::from(r)
        }
        None => u32::from(h.rate),
    };
    if decoded.stats.overflowed > 0 {
        notes.push(Note::Overflowed {
            samples: decoded.stats.overflowed,
        });
    }
    let audio = Audio::from_i16(rate, channels, decoded.samples);
    Ok(Loaded {
        format: Format::Acm,
        audio,
        notes,
    })
}

fn read_raw(bytes: &[u8], raw: Raw) -> Result<Audio, InputError> {
    let (bits, data): (u16, Vec<i32>) = match raw.bits {
        RawBits::U8 => (8, bytes.iter().map(|&b| i32::from(b as i8 ^ i8::MIN) << 24).collect()),
        RawBits::S16 if bytes.len().is_multiple_of(2) => (
            16,
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| i32::from(i16::from_le_bytes(b)) << 16)
                .collect(),
        ),
        RawBits::S16 => return Err(InputError::RawLength { len: bytes.len() }),
    };
    Ok(Audio {
        rate: raw.rate.get(),
        channels: raw.channels.get(),
        samples: Samples::Int { bits, data },
    })
}

/// Decodes the default audio track through Symphonia, keeping integer samples at their depth.
fn read_general(bytes: &[u8]) -> Result<Audio, InputError> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(&Default::default(), mss, Default::default(), Default::default())
        .map_err(InputError::Decode)?;
    let track = reader.default_track(TrackType::Audio).ok_or(InputError::NoAudio)?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(InputError::NoAudio)?;
    let (track_id, rate) = (track.id, params.sample_rate.ok_or(InputError::NoAudio)?);
    let channels = params
        .channels
        .as_ref()
        .map(|c| c.count() as u16)
        .ok_or(InputError::NoAudio)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &Default::default())
        .map_err(InputError::Decode)?;
    let (mut ints, mut floats, mut bits) = (Vec::<i32>::new(), Vec::<f32>::new(), None::<u16>);
    while let Some(packet) = reader.next_packet().map_err(InputError::Decode)? {
        if packet.track_id != track_id {
            continue;
        }
        let buf = decoder.decode(&packet).map_err(InputError::Decode)?;
        // The first buffer's format sets the depth; Symphonia converts integers left-aligned, as `Samples` holds them.
        let depth = *bits.get_or_insert(match buf {
            GenericAudioBufferRef::U8(_) | GenericAudioBufferRef::S8(_) => 8,
            GenericAudioBufferRef::U16(_) | GenericAudioBufferRef::S16(_) => 16,
            GenericAudioBufferRef::U24(_) | GenericAudioBufferRef::S24(_) => 24,
            GenericAudioBufferRef::U32(_) | GenericAudioBufferRef::S32(_) => 32,
            GenericAudioBufferRef::F32(_) | GenericAudioBufferRef::F64(_) => 0,
        });
        let mut part = Vec::new();
        if depth == 0 {
            buf.copy_to_vec_interleaved::<f32>(&mut part);
            floats.extend(part);
        } else {
            let mut ipart = Vec::new();
            buf.copy_to_vec_interleaved::<i32>(&mut ipart);
            ints.extend(ipart);
        }
    }
    let samples = match bits {
        Some(0) => Samples::Float(floats),
        b => Samples::Int {
            bits: b.unwrap_or(16),
            data: ints,
        },
    };
    Ok(Audio {
        rate,
        channels,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acm;

    fn riff(
        bits: u16,
        float: bool,
        channels: u16,
        rate: u32,
        write: impl FnOnce(&mut hound::WavWriter<std::io::Cursor<&mut Vec<u8>>>),
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: bits,
            sample_format: if float {
                hound::SampleFormat::Float
            } else {
                hound::SampleFormat::Int
            },
        };
        let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut out), spec).unwrap();
        write(&mut w);
        w.finalize().unwrap();
        out
    }

    #[test]
    fn reads_16_bit_riff_exactly() {
        let samples = [0i16, 1, -1, i16::MAX, i16::MIN, 1234];
        let b = riff(16, false, 2, 44100, |w| {
            samples.iter().for_each(|&s| w.write_sample(s).unwrap())
        });
        let l = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(l.format, Format::Riff);
        assert_eq!((l.audio.rate, l.audio.channels, l.audio.bits()), (44100, 2, Some(16)));
        assert_eq!(l.audio.to_i16(), samples);
    }

    #[test]
    fn keeps_24_bit_depth() {
        let samples = [0i32, 1, -1, 0x7F_FFFF, -0x80_0000];
        let b = riff(24, false, 1, 48000, |w| {
            samples.iter().for_each(|&s| w.write_sample(s).unwrap())
        });
        let l = read(&b, &ReadOptions::default()).unwrap();
        let Samples::Int { bits, data } = &l.audio.samples else {
            panic!("{:?}", l.audio.samples)
        };
        assert_eq!(*bits, 24);
        assert_eq!(data.iter().map(|v| v >> 8).collect::<Vec<_>>(), samples);
    }

    #[test]
    fn reads_8_bit_riff() {
        let b = riff(8, false, 1, 11025, |w| {
            [-128i8, 0, 127].iter().for_each(|&s| w.write_sample(s).unwrap())
        });
        let l = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(l.audio.bits(), Some(8));
        assert_eq!(l.audio.to_i16(), [-128 << 8, 0, 127 << 8]);
    }

    #[test]
    fn reads_float_riff_as_float() {
        let b = riff(32, true, 1, 44100, |w| {
            [0.0f32, 0.5, -0.25].iter().for_each(|&s| w.write_sample(s).unwrap())
        });
        let l = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(l.audio.samples, Samples::Float(vec![0.0, 0.5, -0.25]));
    }

    fn acm_stream(samples: &[i16], channels: u16, rate: u16) -> Vec<u8> {
        acm::encode(samples, channels, rate).unwrap()
    }

    #[test]
    fn an_odd_count_under_a_stereo_acm_header_reads_as_mono() {
        let b = acm_stream(&[1, 2, 3], 2, 22050);
        let l = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(l.format, Format::Acm);
        assert_eq!(l.audio.channels, 1);
        assert_eq!(l.audio.to_i16(), [1, 2, 3]);
        assert!(l.notes.contains(&Note::Channels {
            header: 2,
            used: 1,
            evidence: ChannelEvidence::OddSampleCount
        }));
    }

    #[test]
    fn a_byte_swapped_rate_is_corrected() {
        let b = acm_stream(&[0; 10], 1, 17580);
        let l = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(l.audio.rate, 44100);
        assert!(l.notes.contains(&Note::SwappedRate {
            stored: 17580,
            used: 44100
        }));
    }

    #[test]
    fn options_override_the_header_and_the_evidence() {
        let b = acm_stream(&[1, 2, 3], 2, 17580);
        let opts = ReadOptions {
            channels: NonZeroU16::new(2),
            rate: NonZeroU32::new(22050),
            ..ReadOptions::default()
        };
        let l = read(&b, &opts).unwrap();
        assert_eq!((l.audio.channels, l.audio.rate), (2, 22050));
        // The notes say what was used.
        assert!(l.notes.contains(&Note::Channels {
            header: 2,
            used: 2,
            evidence: ChannelEvidence::OddSampleCount
        }));
        assert!(l.notes.contains(&Note::SwappedRate {
            stored: 17580,
            used: 22050
        }));
    }

    #[test]
    fn a_wavc_with_agreeing_headers_keeps_them() {
        let inner = acm_stream(&[5, -5, 7, -7], 2, 22050);
        let l = read(&crate::wavc::wrap(&inner).unwrap(), &ReadOptions::default()).unwrap();
        assert_eq!(l.format, Format::Wavc);
        assert_eq!((l.audio.channels, l.audio.rate), (2, 22050));
        assert_eq!(l.audio.to_i16(), [5, -5, 7, -7]);
    }

    #[test]
    fn a_wavc_whose_headers_agree_on_no_channels_reads_as_mono() {
        let inner = acm_stream(&[5, -5, 7, -7], 0, 22050);
        let l = read(&crate::wavc::wrap(&inner).unwrap(), &ReadOptions::default()).unwrap();
        assert_eq!(l.audio.channels, 1);
        assert!(l.notes.contains(&Note::Channels {
            header: 0,
            used: 1,
            evidence: ChannelEvidence::HeaderMono
        }));
    }

    #[test]
    fn a_rate_of_zero_is_refused_unless_one_is_given() {
        let b = acm_stream(&[1, 2], 1, 0);
        assert!(matches!(read(&b, &ReadOptions::default()), Err(InputError::ZeroRate)));
        let given = ReadOptions {
            rate: NonZeroU32::new(22050),
            ..ReadOptions::default()
        };
        assert_eq!(read(&b, &given).unwrap().audio.rate, 22050);
    }

    #[test]
    fn an_overflow_is_noted() {
        // A level-1 stream whose output overflows: see the decoder's tests.
        let mut w = crate::acm::bitwriter::BitWriter::default();
        w.put(15, 4);
        w.put(1, 16);
        for v in [32767, -32768i32] {
            w.put(16, 5);
            w.put((v + 32768) as u32, 16);
        }
        let mut b = acm::Header {
            samples: 2,
            channels: 1,
            rate: 22050,
            levels: 1,
            rows: 1,
        }
        .to_bytes()
        .to_vec();
        b.extend(w.finish());
        let l = read(&b, &ReadOptions::default()).unwrap();
        assert_eq!(l.audio.to_i16(), [16384, 32767]);
        assert!(l.notes.contains(&Note::Overflowed { samples: 1 }));
    }

    #[test]
    fn reads_raw_pcm_by_its_description() {
        let opts = |bits| ReadOptions {
            raw: Some(Raw {
                rate: NonZeroU32::new(22050).unwrap(),
                channels: NonZeroU16::MIN,
                bits,
            }),
            ..ReadOptions::default()
        };
        let l = read(&[0x01, 0x80, 0xFF, 0x7F], &opts(RawBits::S16)).unwrap();
        assert_eq!(l.audio.to_i16(), [-32767, 32767]);
        // 8-bit raw is unsigned, as in WAV.
        let l = read(&[0x00, 0x80, 0xFF], &opts(RawBits::U8)).unwrap();
        assert_eq!(l.audio.to_i16(), [-128 << 8, 0, 127 << 8]);
        assert!(matches!(
            read(&[0; 3], &opts(RawBits::S16)),
            Err(InputError::RawLength { len: 3 })
        ));
    }

    #[test]
    fn rejects_what_is_not_sound() {
        assert!(read(b"not a sound file at all", &ReadOptions::default()).is_err());
    }
}
