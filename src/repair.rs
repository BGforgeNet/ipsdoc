//! Header fields an ACM or WAVC gets wrong, and their correction in place: no re-encoding, the bitstream untouched.
//! The correct values are what reading establishes (`input::read`): the channel count the content supports, an
//! unswapped rate, and WAVC sizes and fields that match the ACM inside.

use crate::input::{self, InputError, ReadOptions};
use crate::{acm, detect, wavc};

/// A header field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// The ACM header's channel count.
    AcmChannels,
    /// The ACM header's rate.
    AcmRate,
    /// The WAVC header's channel count.
    WavcChannels,
    /// The WAVC header's rate.
    WavcRate,
    /// The WAVC header's uncompressed size.
    WavcUncompressed,
    /// The WAVC header's compressed size.
    WavcCompressed,
    /// The WAVC header's bits per sample.
    WavcBits,
}

/// A wrong field and its correct value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fix {
    /// Which field.
    pub field: Field,
    /// As stored.
    pub stored: u32,
    /// As it should be.
    pub correct: u32,
}

/// Why a file cannot be checked for repair.
#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    /// Only ACM and WAVC headers are repaired.
    #[error("only ACM and WAVC headers are repaired")]
    NotRepairable,
    /// The file does not read.
    #[error("unreadable")]
    Input(#[from] InputError),
}

/// The wrong fields of an ACM or WAVC, in header order.
///
/// # Errors
///
/// When the file is neither ACM nor WAVC, or does not read.
pub fn fixes(bytes: &[u8]) -> Result<Vec<Fix>, RepairError> {
    let format = detect::detect(bytes);
    if !matches!(format, detect::Format::Acm | detect::Format::Wavc) {
        return Err(RepairError::NotRepairable);
    }
    let loaded = input::read(bytes, &ReadOptions::default())?;
    let (channels, rate) = (u32::from(loaded.audio.channels), loaded.audio.rate);
    let mut out = Vec::new();
    let mut check = |field, stored: u32, correct: u32| {
        if stored != correct {
            out.push(Fix { field, stored, correct });
        }
    };
    let acm_bytes = match format {
        detect::Format::Wavc => {
            let w = wavc::Header::parse(bytes).map_err(InputError::from)?;
            let inner = wavc::acm_of(bytes).map_err(InputError::from)?;
            let a = acm::Header::parse(inner)
                .map_err(acm::DecodeError::from)
                .map_err(InputError::from)?;
            check(Field::WavcChannels, w.channels.into(), channels);
            check(Field::WavcRate, w.rate.into(), rate);
            check(Field::WavcUncompressed, w.uncompressed, a.samples.wrapping_mul(2));
            check(Field::WavcCompressed, w.compressed, inner.len() as u32);
            check(Field::WavcBits, w.bits.into(), 16);
            inner
        }
        detect::Format::Acm | detect::Format::Riff | detect::Format::Ogg | detect::Format::Other => bytes,
    };
    let a = acm::Header::parse(acm_bytes)
        .map_err(acm::DecodeError::from)
        .map_err(InputError::from)?;
    check(Field::AcmChannels, a.channels.into(), channels);
    check(Field::AcmRate, a.rate.into(), rate);
    Ok(out)
}

/// Writes `fixes` (from [`fixes`] on the same bytes) into the headers.
pub fn apply(bytes: &mut [u8], fixes: &[Fix]) {
    let acm_at = match wavc::Header::parse(bytes) {
        Ok(h) => h.acm_offset as usize,
        Err(_) => 0,
    };
    for f in fixes {
        let (at, width) = match f.field {
            Field::AcmChannels => (acm_at + 8, 2),
            Field::AcmRate => (acm_at + 10, 2),
            Field::WavcUncompressed => (0x08, 4),
            Field::WavcCompressed => (0x0C, 4),
            Field::WavcChannels => (0x14, 2),
            Field::WavcBits => (0x16, 2),
            Field::WavcRate => (0x18, 2),
        };
        bytes[at..at + width].copy_from_slice(&f.correct.to_le_bytes()[..width]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_correct_file_needs_nothing() {
        let b = acm::encode(&[1, -1, 2, -2], 1, 22050).unwrap();
        assert_eq!(fixes(&b).unwrap(), []);
    }

    #[test]
    fn fixes_an_acm_channel_count_and_a_swapped_rate() {
        let mut b = acm::encode(&[1, 2, 3], 2, 17580).unwrap();
        let f = fixes(&b).unwrap();
        assert_eq!(
            f,
            [
                Fix {
                    field: Field::AcmChannels,
                    stored: 2,
                    correct: 1
                },
                Fix {
                    field: Field::AcmRate,
                    stored: 17580,
                    correct: 44100
                }
            ]
        );
        apply(&mut b, &f);
        let h = acm::Header::parse(&b).unwrap();
        assert_eq!((h.channels, h.rate), (1, 44100));
        assert_eq!(fixes(&b).unwrap(), []);
        // The bitstream is untouched: it decodes to the same samples.
        assert_eq!(acm::decode(&b).unwrap().samples, [1, 2, 3]);
    }

    #[test]
    fn fixes_wavc_fields_against_the_acm() {
        // Clearly stereo content (the channels move apart), so a disagreeing WAVC channel count is the wrong one.
        let s: Vec<i16> = (0..200)
            .map(|i| if i % 2 == 0 { 1000 + i } else { -1000 - i })
            .collect();
        let inner = acm::encode(&s, 2, 22050).unwrap();
        let mut b = crate::wavc::wrap(&inner).unwrap();
        let mut h = crate::wavc::Header::parse(&b).unwrap();
        (h.channels, h.rate, h.uncompressed, h.compressed, h.bits, h.unknown) = (1, 11025, 3, 4, 8, 0x77e7);
        b[..28].copy_from_slice(&h.to_bytes());
        let f = fixes(&b).unwrap();
        let fields: Vec<Field> = f.iter().map(|f| f.field).collect();
        assert_eq!(
            fields,
            [
                Field::WavcChannels,
                Field::WavcRate,
                Field::WavcUncompressed,
                Field::WavcCompressed,
                Field::WavcBits
            ]
        );
        apply(&mut b, &f);
        assert_eq!(fixes(&b).unwrap(), []);
        // The unknown field is left alone.
        assert_eq!(crate::wavc::Header::parse(&b).unwrap().unknown, 0x77e7);
    }

    #[test]
    fn a_wavc_mono_header_over_mono_content_fixes_the_acm_header() {
        // Interleaved mono under a stereo ACM header, a mono WAVC header: the content settles it.
        let s: Vec<i16> = (0..20000).map(|n| (3000.0 * (n as f64 * 0.031).sin()) as i16).collect();
        let inner = acm::encode(&s, 2, 22050).unwrap();
        let mut b = crate::wavc::wrap(&inner).unwrap();
        let mut h = crate::wavc::Header::parse(&b).unwrap();
        h.channels = 1;
        b[..28].copy_from_slice(&h.to_bytes());
        assert_eq!(
            fixes(&b).unwrap(),
            [Fix {
                field: Field::AcmChannels,
                stored: 2,
                correct: 1
            }]
        );
    }

    #[test]
    fn other_formats_have_nothing_to_repair() {
        assert!(matches!(
            fixes(b"RIFF\0\0\0\0WAVEfmt "),
            Err(RepairError::NotRepairable)
        ));
    }
}
