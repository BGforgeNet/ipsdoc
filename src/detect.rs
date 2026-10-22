//! Sound containers by magic; file names and extensions are not trusted (Infinity Engine `.wav` resources hold
//! RIFF, WAVC or Ogg Vorbis).

/// A container, by its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    /// Interplay ACM.
    Acm,
    /// WAVC V1: a 28-byte header over an ACM.
    Wavc,
    /// RIFF WAVE.
    Riff,
    /// Ogg (Vorbis in the games).
    Ogg,
    /// Anything else: left to the general decoder (FLAC, MP3 and others), or not a sound.
    Other,
}

impl Format {
    /// A short lowercase name.
    pub fn name(self) -> &'static str {
        match self {
            Format::Acm => "acm",
            Format::Wavc => "wavc",
            Format::Riff => "riff",
            Format::Ogg => "ogg",
            Format::Other => "other",
        }
    }
}

/// The container of a stream starting with `head`.
pub fn detect(head: &[u8]) -> Format {
    if head.starts_with(&crate::acm::header::MAGIC) {
        Format::Acm
    } else if head.starts_with(b"WAVC") {
        Format::Wavc
    } else if head.starts_with(b"RIFF") {
        Format::Riff
    } else if head.starts_with(b"OggS") {
        Format::Ogg
    } else {
        Format::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_containers_by_magic() {
        assert_eq!(detect(&[0x97, 0x28, 0x03, 0x01, 0]), Format::Acm);
        assert_eq!(detect(b"WAVCV1.0"), Format::Wavc);
        assert_eq!(detect(b"RIFF\0\0\0\0WAVE"), Format::Riff);
        assert_eq!(detect(b"OggS\0"), Format::Ogg);
        assert_eq!(detect(b"fLaC"), Format::Other);
        assert_eq!(detect(b"Ri"), Format::Other);
        assert_eq!(detect(b""), Format::Other);
    }
}
