//! Header fields an ACM or WAVC gets wrong, and their correction in place: no re-encoding, the bitstream untouched.
//! The correct values are what reading establishes (`input::read`): the channel count the content supports, an
//! unswapped rate, and WAVC sizes and fields that match the ACM inside.

use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

use crate::analysis::swapped_rate;
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

/// Bytes to write over a file's own, at an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// Where they start.
    pub at: usize,
    /// What to write there.
    pub bytes: Vec<u8>,
}

/// What a file needs: the wrong fields, and the corrected headers that fix them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Repair {
    /// The wrong fields, in header order.
    pub fixes: Vec<Fix>,
    /// The corrected headers, each written whole through its own serialisation; empty when nothing is wrong.
    pub patches: Vec<Patch>,
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

/// What an ACM or WAVC needs.
///
/// # Errors
///
/// When the file is neither ACM nor WAVC, or does not read.
pub fn fixes(bytes: &[u8]) -> Result<Repair, RepairError> {
    let format = detect::detect(bytes);
    if !matches!(format, detect::Format::Acm | detect::Format::Wavc) {
        return Err(RepairError::NotRepairable);
    }
    let loaded = input::read(bytes, &ReadOptions::default())?;
    let channels = loaded.audio.channels;
    let wavc_header = match format {
        detect::Format::Wavc => Some(wavc::Header::parse(bytes).map_err(InputError::from)?),
        detect::Format::Acm | detect::Format::Riff | detect::Format::Ogg | detect::Format::Other => None,
    };
    let (acm_at, inner) = match wavc_header {
        Some(w) => (w.acm_offset as usize, wavc::acm_of(bytes).map_err(InputError::from)?),
        None => (0, bytes),
    };
    let a = acm::Header::parse(inner)
        .map_err(acm::DecodeError::from)
        .map_err(InputError::from)?;
    // The rate reading uses, kept at the header's 16 bits: the stored one, unswapped.
    let rate = swapped_rate(a.rate).unwrap_or(a.rate);
    // Each header as it should be, the fields reading cannot establish kept as stored.
    let headers = wavc_header.map(|w| {
        let correct = wavc::Header {
            uncompressed: a.samples.wrapping_mul(2),
            compressed: inner.len() as u32,
            channels,
            bits: 16,
            rate,
            ..w
        };
        (w, correct)
    });
    let correct_acm = acm::Header { channels, rate, ..a };
    let mut fixes = Vec::new();
    let mut check = |field, stored: u32, correct: u32| {
        if stored != correct {
            fixes.push(Fix { field, stored, correct });
        }
    };
    if let Some((w, c)) = headers {
        check(Field::WavcChannels, w.channels.into(), c.channels.into());
        check(Field::WavcRate, w.rate.into(), c.rate.into());
        check(Field::WavcUncompressed, w.uncompressed, c.uncompressed);
        check(Field::WavcCompressed, w.compressed, c.compressed);
        check(Field::WavcBits, w.bits.into(), c.bits.into());
    }
    check(Field::AcmChannels, a.channels.into(), correct_acm.channels.into());
    check(Field::AcmRate, a.rate.into(), correct_acm.rate.into());
    let mut patches = Vec::new();
    if let Some((w, c)) = headers
        && c != w
    {
        patches.push(Patch {
            at: 0,
            bytes: c.to_bytes().to_vec(),
        });
    }
    if correct_acm != a {
        patches.push(Patch {
            at: acm_at,
            bytes: correct_acm.to_bytes().to_vec(),
        });
    }
    Ok(Repair { fixes, patches })
}

/// Writes `patches` over the file at `path`, in place. The file is never truncated or rewritten whole: a failure
/// part-way leaves every byte outside the patches as it was.
///
/// # Errors
///
/// When the file cannot be opened for writing or a write fails.
pub fn write(path: &Path, patches: &[Patch]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).open(path)?;
    for p in patches {
        f.seek(SeekFrom::Start(p.at as u64))?;
        f.write_all(&p.bytes)?;
    }
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patched(bytes: &[u8], r: &Repair) -> Vec<u8> {
        let mut b = bytes.to_vec();
        for p in &r.patches {
            b[p.at..p.at + p.bytes.len()].copy_from_slice(&p.bytes);
        }
        b
    }

    #[test]
    fn a_correct_file_needs_nothing() {
        let b = acm::encode(&[1, -1, 2, -2], 1, 22050).unwrap();
        assert_eq!(fixes(&b).unwrap(), Repair::default());
    }

    #[test]
    fn fixes_an_acm_channel_count_and_a_swapped_rate() {
        let b = acm::encode(&[1, 2, 3], 2, 17580).unwrap();
        let r = fixes(&b).unwrap();
        assert_eq!(
            r.fixes,
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
        // One patch, the ACM header alone.
        assert_eq!(r.patches.len(), 1);
        assert_eq!((r.patches[0].at, r.patches[0].bytes.len()), (0, acm::Header::LEN));
        let b = patched(&b, &r);
        let h = acm::Header::parse(&b).unwrap();
        assert_eq!((h.channels, h.rate), (1, 44100));
        assert_eq!(fixes(&b).unwrap(), Repair::default());
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
        let r = fixes(&b).unwrap();
        let fields: Vec<Field> = r.fixes.iter().map(|f| f.field).collect();
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
        // The WAVC header alone: the ACM header inside is right.
        assert_eq!(r.patches.len(), 1);
        assert_eq!(
            (r.patches[0].at, r.patches[0].bytes.len()),
            (0, crate::wavc::Header::LEN)
        );
        let b = patched(&b, &r);
        assert_eq!(fixes(&b).unwrap(), Repair::default());
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
        let r = fixes(&b).unwrap();
        assert_eq!(
            r.fixes,
            [Fix {
                field: Field::AcmChannels,
                stored: 2,
                correct: 1
            }]
        );
        // The ACM header, at the WAVC's stated offset.
        assert_eq!(r.patches.len(), 1);
        assert_eq!(r.patches[0].at, crate::wavc::Header::LEN);
        assert_eq!(fixes(&patched(&b, &r)).unwrap(), Repair::default());
    }

    #[test]
    fn other_formats_have_nothing_to_repair() {
        assert!(matches!(
            fixes(b"RIFF\0\0\0\0WAVEfmt "),
            Err(RepairError::NotRepairable)
        ));
    }

    #[test]
    fn writes_the_patches_over_the_file_and_keeps_its_length() {
        let path = std::env::temp_dir().join(format!("ipsdoc-repair-write-{}", std::process::id()));
        std::fs::write(&path, [0u8; 8]).unwrap();
        let patches = [
            Patch {
                at: 1,
                bytes: vec![1, 2],
            },
            Patch { at: 6, bytes: vec![3] },
        ];
        let written = write(&path, &patches).map(|()| std::fs::read(&path));
        let _ = std::fs::remove_file(&path);
        assert_eq!(written.unwrap().unwrap(), [0, 1, 2, 0, 0, 0, 3, 0]);
    }
}
