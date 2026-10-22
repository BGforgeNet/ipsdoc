//! The 14-byte ACM header.

/// The first four bytes of every ACM stream.
pub const MAGIC: [u8; 4] = [0x97, 0x28, 0x03, 0x01];

/// An ACM header, as stored; the channel count and rate are not checked against the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Header {
    /// Samples in the stream, all channels together.
    pub samples: u32,
    /// Channels as the header states them.
    pub channels: u16,
    /// Sample rate in Hz as the header states it.
    pub rate: u16,
    /// Transform levels, 0..=15; a block has `1 << levels` columns.
    pub levels: u8,
    /// Rows per block, 1..=4095.
    pub rows: u16,
}

/// Why bytes are not an ACM header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    /// Fewer than [`Header::LEN`] bytes.
    #[error("{len} bytes, shorter than the 14-byte ACM header")]
    TooShort {
        /// Bytes available.
        len: usize,
    },
    /// The first four bytes are not [`MAGIC`].
    #[error("magic {found:02x?}, not an ACM stream")]
    BadMagic {
        /// The bytes found.
        found: [u8; 4],
    },
    /// A block of zero rows holds nothing, so the stream cannot be decoded.
    #[error("zero rows per block")]
    ZeroRows,
}

impl Header {
    /// Header length in bytes.
    pub const LEN: usize = 14;

    /// Parses the header at the start of `bytes`.
    ///
    /// # Errors
    ///
    /// When `bytes` is too short, the magic is wrong or the row count is zero.
    pub fn parse(bytes: &[u8]) -> Result<Header, HeaderError> {
        let b: &[u8; Header::LEN] = bytes
            .get(..Header::LEN)
            .and_then(|b| b.try_into().ok())
            .ok_or(HeaderError::TooShort { len: bytes.len() })?;
        let found = [b[0], b[1], b[2], b[3]];
        if found != MAGIC {
            return Err(HeaderError::BadMagic { found });
        }
        let packed = u16::from_le_bytes([b[12], b[13]]);
        let rows = packed >> 4;
        if rows == 0 {
            return Err(HeaderError::ZeroRows);
        }
        Ok(Header {
            samples: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            channels: u16::from_le_bytes([b[8], b[9]]),
            rate: u16::from_le_bytes([b[10], b[11]]),
            levels: (packed & 0xF) as u8,
            rows,
        })
    }

    /// The header as stored.
    pub fn to_bytes(&self) -> [u8; Header::LEN] {
        let mut b = [0; Header::LEN];
        b[..4].copy_from_slice(&MAGIC);
        b[4..8].copy_from_slice(&self.samples.to_le_bytes());
        b[8..10].copy_from_slice(&self.channels.to_le_bytes());
        b[10..12].copy_from_slice(&self.rate.to_le_bytes());
        b[12..14].copy_from_slice(&(self.rows << 4 | u16::from(self.levels & 0xF)).to_le_bytes());
        b
    }

    /// Columns per block.
    pub fn cols(&self) -> usize {
        1 << self.levels
    }

    /// Samples per block, all channels together.
    pub fn block_len(&self) -> usize {
        self.cols() * usize::from(self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0x12345 samples, 2 channels, 22050 Hz, 7 levels, 256 rows.
    const BYTES: [u8; 14] = [
        0x97, 0x28, 0x03, 0x01, 0x45, 0x23, 0x01, 0x00, 2, 0, 0x22, 0x56, 0x07, 0x10,
    ];

    #[test]
    fn parses_every_field() {
        let h = Header::parse(&BYTES).unwrap();
        assert_eq!(
            h,
            Header {
                samples: 0x12345,
                channels: 2,
                rate: 22050,
                levels: 7,
                rows: 256
            }
        );
        assert_eq!((h.cols(), h.block_len()), (128, 128 * 256));
    }

    #[test]
    fn ignores_what_follows_the_header() {
        let mut long = BYTES.to_vec();
        long.extend_from_slice(&[0xFF; 3]);
        assert_eq!(Header::parse(&long).unwrap(), Header::parse(&BYTES).unwrap());
    }

    #[test]
    fn writes_what_it_parses() {
        assert_eq!(Header::parse(&BYTES).unwrap().to_bytes(), BYTES);
    }

    #[test]
    fn rows_take_the_top_twelve_bits() {
        let mut b = BYTES;
        b[12..14].copy_from_slice(&0xFFF3u16.to_le_bytes());
        let h = Header::parse(&b).unwrap();
        assert_eq!((h.levels, h.rows), (3, 4095));
    }

    #[test]
    fn rejects_a_short_header() {
        assert_eq!(Header::parse(&BYTES[..13]), Err(HeaderError::TooShort { len: 13 }));
    }

    #[test]
    fn rejects_a_wrong_magic() {
        let mut b = BYTES;
        b[3] = 0x02;
        assert_eq!(
            Header::parse(&b),
            Err(HeaderError::BadMagic {
                found: [0x97, 0x28, 0x03, 0x02]
            })
        );
    }

    #[test]
    fn rejects_zero_rows() {
        let mut b = BYTES;
        b[12..14].copy_from_slice(&7u16.to_le_bytes());
        assert_eq!(Header::parse(&b), Err(HeaderError::ZeroRows));
    }
}
