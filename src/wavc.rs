//! WAVC V1, the Infinity Engine's compressed WAV: a 28-byte header over an ACM stream.
//!
//! Layout (little-endian): `WAVC`, `V1.0`, `u32` uncompressed size (2 bytes per sample), `u32` compressed size (the
//! ACM's length), `u32` ACM offset (28), `u16` channels, `u16` bits (16), `u16` rate, `u16` of unknown meaning
//! (values vary across the games; written as 0).

use crate::acm;

/// The first eight bytes of a WAVC V1.
pub const MAGIC: [u8; 8] = *b"WAVCV1.0";

/// A WAVC header, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Header {
    /// Uncompressed size in bytes.
    pub uncompressed: u32,
    /// Size of the ACM stream in bytes.
    pub compressed: u32,
    /// Offset of the ACM stream.
    pub acm_offset: u32,
    /// Channels as this header states them.
    pub channels: u16,
    /// Bits per sample.
    pub bits: u16,
    /// Sample rate in Hz as this header states it.
    pub rate: u16,
    /// The field at 0x1A.
    pub unknown: u16,
}

/// Why bytes are not a usable WAVC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WavcError {
    /// Fewer than [`Header::LEN`] bytes.
    #[error("{len} bytes, shorter than the 28-byte WAVC header")]
    TooShort {
        /// Bytes available.
        len: usize,
    },
    /// The first eight bytes are not [`MAGIC`].
    #[error("magic {found:?}, not WAVC V1.0")]
    BadMagic {
        /// The bytes found.
        found: [u8; 8],
    },
    /// The ACM offset points at or past the end of the file.
    #[error("ACM offset {offset} is outside the {len}-byte file")]
    AcmOutside {
        /// The stated offset.
        offset: u32,
        /// The file's length.
        len: usize,
    },
}

impl Header {
    /// Header length in bytes.
    pub const LEN: usize = 28;

    /// Parses the header at the start of `bytes`.
    ///
    /// # Errors
    ///
    /// When `bytes` is too short or the magic is wrong.
    pub fn parse(bytes: &[u8]) -> Result<Header, WavcError> {
        let b: &[u8; Header::LEN] = bytes
            .get(..Header::LEN)
            .and_then(|b| b.try_into().ok())
            .ok_or(WavcError::TooShort { len: bytes.len() })?;
        let found: [u8; 8] = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        if found != MAGIC {
            return Err(WavcError::BadMagic { found });
        }
        let u32_at = |at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let u16_at = |at: usize| u16::from_le_bytes([b[at], b[at + 1]]);
        Ok(Header {
            uncompressed: u32_at(0x08),
            compressed: u32_at(0x0C),
            acm_offset: u32_at(0x10),
            channels: u16_at(0x14),
            bits: u16_at(0x16),
            rate: u16_at(0x18),
            unknown: u16_at(0x1A),
        })
    }

    /// The header as stored.
    pub fn to_bytes(&self) -> [u8; Header::LEN] {
        let mut b = [0; Header::LEN];
        b[..8].copy_from_slice(&MAGIC);
        b[0x08..0x0C].copy_from_slice(&self.uncompressed.to_le_bytes());
        b[0x0C..0x10].copy_from_slice(&self.compressed.to_le_bytes());
        b[0x10..0x14].copy_from_slice(&self.acm_offset.to_le_bytes());
        b[0x14..0x16].copy_from_slice(&self.channels.to_le_bytes());
        b[0x16..0x18].copy_from_slice(&self.bits.to_le_bytes());
        b[0x18..0x1A].copy_from_slice(&self.rate.to_le_bytes());
        b[0x1A..0x1C].copy_from_slice(&self.unknown.to_le_bytes());
        b
    }

    /// The header a WAVC holding `acm` (`acm_len` bytes, ACM header `acm`) should carry.
    pub fn for_acm(acm: &acm::Header, acm_len: usize) -> Header {
        Header {
            uncompressed: acm.samples.wrapping_mul(2),
            compressed: acm_len as u32,
            acm_offset: Header::LEN as u32,
            channels: acm.channels,
            bits: 16,
            rate: acm.rate,
            unknown: 0,
        }
    }
}

/// The ACM stream inside a WAVC, from its stated offset to the end.
///
/// # Errors
///
/// When the header is unreadable or the offset lies outside the file.
pub fn acm_of(bytes: &[u8]) -> Result<&[u8], WavcError> {
    let h = Header::parse(bytes)?;
    bytes
        .get(h.acm_offset as usize..)
        .filter(|rest| !rest.is_empty())
        .ok_or(WavcError::AcmOutside {
            offset: h.acm_offset,
            len: bytes.len(),
        })
}

/// A WAVC holding `acm`, with the header [`Header::for_acm`] gives.
///
/// # Errors
///
/// When `acm` has no readable ACM header.
pub fn wrap(acm: &[u8]) -> Result<Vec<u8>, acm::HeaderError> {
    let inner = acm::Header::parse(acm)?;
    let mut out = Header::for_acm(&inner, acm.len()).to_bytes().to_vec();
    out.extend_from_slice(acm);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acm_bytes(samples: u32, channels: u16, rate: u16, body: &[u8]) -> Vec<u8> {
        let mut b = acm::Header {
            samples,
            channels,
            rate,
            levels: 7,
            rows: 16,
        }
        .to_bytes()
        .to_vec();
        b.extend_from_slice(body);
        b
    }

    /// A WAVC as the games ship it: sizes, offset 28, channels, 16 bits, rate, then an unknown field.
    fn wavc_bytes(channels: u16, rate: u16, unknown: u16, inner: &[u8], samples: u32) -> Vec<u8> {
        let mut b = b"WAVCV1.0".to_vec();
        b.extend_from_slice(&(2 * samples).to_le_bytes());
        b.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        b.extend_from_slice(&28u32.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&unknown.to_le_bytes());
        b.extend_from_slice(inner);
        b
    }

    #[test]
    fn parses_every_field() {
        let inner = acm_bytes(100, 1, 22050, &[1, 2, 3]);
        let b = wavc_bytes(1, 22050, 0x77e7, &inner, 100);
        assert_eq!(
            Header::parse(&b).unwrap(),
            Header {
                uncompressed: 200,
                compressed: 17,
                acm_offset: 28,
                channels: 1,
                bits: 16,
                rate: 22050,
                unknown: 0x77e7
            }
        );
    }

    #[test]
    fn finds_the_acm_at_its_offset() {
        let inner = acm_bytes(100, 1, 22050, &[1, 2, 3]);
        let b = wavc_bytes(1, 22050, 0, &inner, 100);
        assert_eq!(acm_of(&b).unwrap(), inner.as_slice());
    }

    #[test]
    fn rejects_an_offset_outside_the_file() {
        let mut b = wavc_bytes(1, 22050, 0, &[], 0);
        b[0x10] = 40;
        assert_eq!(acm_of(&b).unwrap_err(), WavcError::AcmOutside { offset: 40, len: 28 });
    }

    #[test]
    fn rejects_a_short_file_and_a_wrong_magic() {
        assert_eq!(Header::parse(b"WAVCV1.0"), Err(WavcError::TooShort { len: 8 }));
        let mut b = wavc_bytes(1, 22050, 0, &[], 0);
        b[5] = b'2';
        assert_eq!(Header::parse(&b), Err(WavcError::BadMagic { found: *b"WAVCV2.0" }));
    }

    #[test]
    fn wraps_an_acm_with_true_sizes_and_a_zero_unknown_field() {
        let inner = acm_bytes(1000, 2, 44100, &[9; 50]);
        let b = wrap(&inner).unwrap();
        assert_eq!(b, wavc_bytes(2, 44100, 0, &inner, 1000));
    }

    #[test]
    fn writes_what_it_parses() {
        let inner = acm_bytes(7, 1, 11025, &[5]);
        let b = wavc_bytes(1, 11025, 0x9ffd, &inner, 7);
        assert_eq!(Header::parse(&b).unwrap().to_bytes(), b[..28]);
    }
}
