//! Fallout lip-sync files (`.lip`, version 2): a mouth shape per stretch of a speech sound.
//!
//! Big-endian. A 44-byte header (eight `i32`, an 8-byte name, a 4-byte extension), a phoneme byte per shape, then a
//! (type, position) `i32` pair per marker. Phoneme `i` shows from marker `i` to marker `i + 1`; the last marker ends
//! the line. A position is a byte offset into the sound's decoded 16-bit data, all channels interleaved: 2 x a sample
//! index, whatever rate or channel count the sound plays at.

/// Header size; phonemes start here.
pub const HEADER_LEN: usize = 44;
/// Where the length field (a big-endian `i32`) sits.
pub const LEN_OFFSET: usize = 16;
/// The only version read and written; version 1 is an engine save-file layout no shipped file uses.
pub const VERSION: i32 = 2;
/// The second header field, unused by the engine, in every original file.
pub const FIELD_04: i32 = 0x5800;
/// The extension field of every original file; the engine replaces it with `ACM`.
pub const EXTENSION: [u8; 4] = *b"VOC\0";
/// Phoneme codes the engine knows: 0 to 41. Fallout 2 CE reads larger ones as 0; the original exe indexes its
/// tables with them unchecked.
pub const PHONEME_COUNT: u8 = 42;
/// With this many markers or fewer the engine logs "Too few markers to stop speech" and skips its wrap check.
pub const FEW_MARKERS: usize = 5;

/// A lip-sync file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lip {
    /// Header field at 0x04 (unused; [`FIELD_04`] in original files).
    pub field_04: i32,
    /// Bit 0 makes the line loop.
    pub flags: i32,
    /// Header field at 0x0C (unused; 0 in original files).
    pub field_0c: i32,
    /// The sound's length in position units, as stored; the engine ignores it.
    pub len: i32,
    /// Header field at 0x18 (unused; 0 in original files).
    pub field_18: i32,
    /// The name of the sound the engine loads for this line, NUL-padded.
    pub name: [u8; 8],
    /// The extension field ([`EXTENSION`] in original files).
    pub extension: [u8; 4],
    /// One code per shape.
    pub phonemes: Vec<u8>,
    /// One more than the phonemes: where each shape starts, then the end.
    pub markers: Vec<Marker>,
}

/// Where a shape starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    /// 1 or 0 (unused by the engine; original files mark placed boundaries 1 and interpolated ones 0).
    pub kind: i32,
    /// Byte offset into the decoded sound.
    pub position: i32,
}

/// Why bytes are not a readable lip file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LipError {
    /// Not a version 2 lip file.
    #[error("not a lip file")]
    NotLip,
    /// The engine's save-file layout.
    #[error("version 1 lip file, not supported")]
    Version1,
    /// A phoneme or marker count below 1.
    #[error("phoneme count {phonemes} or marker count {markers} below 1")]
    EmptyCounts {
        /// As stored.
        phonemes: i32,
        /// As stored.
        markers: i32,
    },
    /// The counts need more bytes than the file has.
    #[error("truncated: its counts need {needed} bytes, the file has {have}")]
    Truncated {
        /// Bytes the counts call for.
        needed: usize,
        /// Bytes present.
        have: usize,
    },
}

/// How much a problem matters to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Harmless to the engine, or only logged by it.
    Warning,
    /// Misplays, or misreads in the original exe.
    Error,
}

/// A problem in a readable lip file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Phoneme codes of [`PHONEME_COUNT`] or more.
    BadCodes {
        /// How many.
        count: usize,
        /// The first one's index.
        first: usize,
    },
    /// A marker before the one preceding it.
    Backwards {
        /// Its index.
        index: usize,
    },
    /// The first marker is not (1, 0).
    BadFirstMarker(Marker),
    /// Marker count other than phoneme count + 1.
    CountMismatch {
        /// Phonemes.
        phonemes: usize,
        /// Markers.
        markers: usize,
    },
    /// [`FEW_MARKERS`] markers or fewer.
    FewMarkers(usize),
    /// Flags other than 0.
    Flags(i32),
    /// `len` other than the last marker's position.
    Len {
        /// As stored.
        stored: i32,
        /// The last marker's position.
        last: i32,
    },
    /// A name field other than the file's own base name.
    Name {
        /// The name field.
        stored: String,
        /// The file's base name.
        expected: String,
    },
}

impl Problem {
    /// How much it matters.
    pub fn severity(&self) -> Severity {
        match self {
            // The engine loads the sound the name field names, so another name plays another sound for the line.
            Problem::BadCodes { .. }
            | Problem::Backwards { .. }
            | Problem::CountMismatch { .. }
            | Problem::Name { .. } => Severity::Error,
            Problem::BadFirstMarker(_) | Problem::FewMarkers(_) | Problem::Flags(_) | Problem::Len { .. } => {
                Severity::Warning
            }
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::BadCodes { count, first } => {
                write!(
                    f,
                    "{count} phoneme codes of {PHONEME_COUNT} or more, the first at {first}"
                )
            }
            Problem::Backwards { index } => write!(f, "marker {index} is before the one preceding it"),
            Problem::BadFirstMarker(m) => write!(f, "the first marker is ({}, {}), not (1, 0)", m.kind, m.position),
            Problem::CountMismatch { phonemes, markers } => {
                write!(
                    f,
                    "{markers} markers for {phonemes} phonemes (should be {})",
                    phonemes + 1
                )
            }
            Problem::FewMarkers(n) => write!(f, "{n} markers: the engine wants {} or more", FEW_MARKERS + 1),
            Problem::Flags(v) => write!(f, "flags {v} (bit 0 loops the line)"),
            Problem::Len { stored, last } => write!(f, "length field {stored}, the last marker is {last}"),
            Problem::Name { stored, expected } => write!(f, "names sound {stored}, the file is {expected}"),
        }
    }
}

/// How a lip file's length (its last marker) fits a sound of `samples` interleaved samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    /// Made for this sound.
    Matches,
    /// Off by exactly this factor (2, 4, 1/2, 1/4): made for the sound at another rate or channel count.
    Scaled(f64),
    /// Off by this ratio: made for another recording.
    Other(f64),
}

/// How `last` (a lip file's last marker) fits a sound of `samples` samples: within 0.01% or 2 bytes, whichever is
/// more (resampling rounds the sample count).
pub fn fit(last: i32, samples: usize) -> Fit {
    let want = 2.0 * samples as f64;
    let last = f64::from(last);
    let near = |a: f64, b: f64| (a - b).abs() <= (b * 1e-4).max(2.0);
    if near(last, want) {
        return Fit::Matches;
    }
    for factor in [2.0, 4.0, 0.5, 0.25] {
        if near(last, want * factor) {
            return Fit::Scaled(factor);
        }
    }
    Fit::Other(if want > 0.0 { last / want } else { f64::INFINITY })
}

fn read_i32(bytes: &[u8], at: usize) -> i32 {
    let mut b = [0; 4];
    b.copy_from_slice(&bytes[at..at + 4]);
    i32::from_be_bytes(b)
}

/// Whether `bytes` start as a version 2 lip file: the version and the extension field, whatever the counts (a
/// truncated file is still recognised, so that it can be reported).
pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER_LEN && read_i32(bytes, 0) == VERSION && bytes[40..44] == EXTENSION
}

impl Lip {
    /// A new file for the sound named `name` (its base name, upper-cased and cut to 8 bytes).
    pub fn new(name: &str, phonemes: Vec<u8>, markers: Vec<Marker>) -> Lip {
        let mut field = [0; 8];
        for (f, b) in field.iter_mut().zip(name.bytes()) {
            *f = b.to_ascii_uppercase();
        }
        let len = markers.last().map_or(0, |m| m.position);
        Lip {
            field_04: FIELD_04,
            flags: 0,
            field_0c: 0,
            len,
            field_18: 0,
            name: field,
            extension: EXTENSION,
            phonemes,
            markers,
        }
    }

    /// Reads a version 2 file; any extension field is accepted, as the engine accepts it.
    ///
    /// # Errors
    ///
    /// When `bytes` are not a version 2 lip file, or its counts do not fit them.
    pub fn parse(bytes: &[u8]) -> Result<Lip, LipError> {
        if bytes.len() < HEADER_LEN {
            return Err(LipError::NotLip);
        }
        match read_i32(bytes, 0) {
            VERSION => {}
            1 => return Err(LipError::Version1),
            _ => return Err(LipError::NotLip),
        }
        let (phonemes, markers) = (read_i32(bytes, 20), read_i32(bytes, 28));
        if phonemes < 1 || markers < 1 {
            return Err(LipError::EmptyCounts { phonemes, markers });
        }
        let (n, m) = (phonemes as usize, markers as usize);
        let needed = HEADER_LEN + n + 8 * m;
        if bytes.len() < needed {
            return Err(LipError::Truncated {
                needed,
                have: bytes.len(),
            });
        }
        let mut name = [0; 8];
        name.copy_from_slice(&bytes[32..40]);
        let mut extension = [0; 4];
        extension.copy_from_slice(&bytes[40..44]);
        let at = HEADER_LEN + n;
        Ok(Lip {
            field_04: read_i32(bytes, 4),
            flags: read_i32(bytes, 8),
            field_0c: read_i32(bytes, 12),
            len: read_i32(bytes, LEN_OFFSET),
            field_18: read_i32(bytes, 24),
            name,
            extension,
            phonemes: bytes[HEADER_LEN..at].to_vec(),
            markers: (0..m)
                .map(|i| Marker {
                    kind: read_i32(bytes, at + 8 * i),
                    position: read_i32(bytes, at + 8 * i + 4),
                })
                .collect(),
        })
    }

    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.phonemes.len() + 8 * self.markers.len());
        for v in [
            VERSION,
            self.field_04,
            self.flags,
            self.field_0c,
            self.len,
            self.phonemes.len() as i32,
            self.field_18,
            self.markers.len() as i32,
        ] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&self.name);
        out.extend_from_slice(&self.extension);
        out.extend_from_slice(&self.phonemes);
        for m in &self.markers {
            out.extend_from_slice(&m.kind.to_be_bytes());
            out.extend_from_slice(&m.position.to_be_bytes());
        }
        out
    }

    /// The name field as text, up to its first NUL (bytes read as Latin-1).
    pub fn name(&self) -> String {
        self.name
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| char::from(b))
            .collect()
    }

    /// The last marker's position: the sound's length in position units, by the markers.
    pub fn last(&self) -> i32 {
        self.markers.last().map_or(0, |m| m.position)
    }

    /// Its problems; `base_name` (the file's name without extension), when given, is compared with the name field
    /// (case-insensitively, over the 8 bytes the field holds).
    pub fn validate(&self, base_name: Option<&str>) -> Vec<Problem> {
        let mut out = Vec::new();
        let bad: Vec<usize> = (0..self.phonemes.len())
            .filter(|&i| self.phonemes[i] >= PHONEME_COUNT)
            .collect();
        if let Some(&first) = bad.first() {
            out.push(Problem::BadCodes {
                count: bad.len(),
                first,
            });
        }
        for (i, w) in self.markers.windows(2).enumerate() {
            if w[1].position < w[0].position {
                out.push(Problem::Backwards { index: i + 1 });
            }
        }
        if let Some(&first) = self.markers.first()
            && first != (Marker { kind: 1, position: 0 })
        {
            out.push(Problem::BadFirstMarker(first));
        }
        if self.markers.len() != self.phonemes.len() + 1 {
            out.push(Problem::CountMismatch {
                phonemes: self.phonemes.len(),
                markers: self.markers.len(),
            });
        }
        if self.markers.len() <= FEW_MARKERS {
            out.push(Problem::FewMarkers(self.markers.len()));
        }
        if self.flags != 0 {
            out.push(Problem::Flags(self.flags));
        }
        if self.len != self.last() {
            out.push(Problem::Len {
                stored: self.len,
                last: self.last(),
            });
        }
        if let Some(base) = base_name {
            let stored = self.name();
            let want: String = base.chars().take(8).collect();
            if !stored.eq_ignore_ascii_case(&want) {
                out.push(Problem::Name {
                    stored,
                    expected: base.to_owned(),
                });
            }
        }
        out
    }

    /// Scales every position so that the last becomes `new_last` (rounded to the nearest byte, so the order holds),
    /// and sets `len` to it.
    pub fn rescale(&mut self, new_last: i32) {
        let old = i64::from(self.last());
        for m in &mut self.markers {
            m.position = if old > 0 {
                let scaled = (2 * i64::from(m.position) * i64::from(new_last) + old) / (2 * old);
                scaled.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            } else {
                m.position
            };
        }
        if let Some(last) = self.markers.last_mut() {
            last.position = new_last;
        }
        self.len = new_last;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(kind: i32, position: i32) -> Marker {
        Marker { kind, position }
    }

    /// Six markers, five shapes, for a sound of 1000 samples.
    fn six() -> Lip {
        Lip::new(
            "cas0103",
            vec![0, 17, 1, 32, 0],
            vec![
                marker(1, 0),
                marker(1, 200),
                marker(1, 600),
                marker(1, 1000),
                marker(1, 1600),
                marker(1, 2000),
            ],
        )
    }

    fn be(v: i32) -> [u8; 4] {
        v.to_be_bytes()
    }

    #[test]
    fn writes_the_original_layout() {
        let mut want = Vec::new();
        for v in [2, 0x5800, 0, 0, 2000, 5, 0, 6] {
            want.extend_from_slice(&be(v));
        }
        want.extend_from_slice(b"CAS0103\0VOC\0");
        want.extend_from_slice(&[0, 17, 1, 32, 0]);
        for p in [0, 200, 600, 1000, 1600, 2000] {
            want.extend_from_slice(&be(1));
            want.extend_from_slice(&be(p));
        }
        assert_eq!(six().to_bytes(), want);
    }

    #[test]
    fn reads_back_what_it_writes() {
        let lip = six();
        assert_eq!(Lip::parse(&lip.to_bytes()), Ok(lip));
    }

    #[test]
    fn an_eight_byte_name_has_no_terminator() {
        let lip = Lip::new("abcdefghij", vec![0], vec![marker(1, 0), marker(1, 2)]);
        assert_eq!(&lip.name, b"ABCDEFGH");
        assert_eq!(lip.name(), "ABCDEFGH");
    }

    #[test]
    fn recognises_by_version_and_extension() {
        let bytes = six().to_bytes();
        assert!(looks_like(&bytes));
        assert!(looks_like(&bytes[..HEADER_LEN]), "a truncated file is still recognised");
        assert!(!looks_like(&bytes[..HEADER_LEN - 1]));
        let acm = [0x97, 0x28, 0x03, 0x01, 0, 0, 0, 0, 1, 0, 0x22, 0x56, 0x07, 0];
        assert!(!looks_like(&acm));
        assert!(!looks_like(b"RIFF\0\0\0\0WAVEfmt "));
        let mut little = bytes.clone();
        little[..4].copy_from_slice(&2i32.to_le_bytes());
        assert!(!looks_like(&little), "little-endian files are rejected by the engine");
    }

    #[test]
    fn a_file_shorter_than_its_counts_is_truncated() {
        let bytes = six().to_bytes();
        let have = bytes.len() - 3;
        assert_eq!(
            Lip::parse(&bytes[..have]),
            Err(LipError::Truncated {
                needed: bytes.len(),
                have
            })
        );
    }

    #[test]
    fn version_1_and_empty_counts_are_errors() {
        let mut v1 = six().to_bytes();
        v1[..4].copy_from_slice(&be(1));
        assert_eq!(Lip::parse(&v1), Err(LipError::Version1));
        let mut empty = six().to_bytes();
        empty[20..24].copy_from_slice(&be(0));
        assert_eq!(
            Lip::parse(&empty),
            Err(LipError::EmptyCounts {
                phonemes: 0,
                markers: 6
            })
        );
        assert_eq!(Lip::parse(b"RIFF"), Err(LipError::NotLip));
    }

    #[test]
    fn a_good_file_has_no_problems() {
        assert_eq!(six().validate(Some("cas0103")), vec![]);
        assert_eq!(six().validate(Some("CAS0103")), vec![]);
    }

    #[test]
    fn finds_each_problem() {
        let mut lip = six();
        lip.phonemes[1] = 42;
        lip.phonemes[3] = 204;
        lip.markers[3].position = 500;
        lip.markers[0] = marker(0, 0);
        lip.flags = 1;
        lip.len = 1999;
        let problems = lip.validate(Some("cas0104"));
        assert_eq!(
            problems,
            vec![
                Problem::BadCodes { count: 2, first: 1 },
                Problem::Backwards { index: 3 },
                Problem::BadFirstMarker(marker(0, 0)),
                Problem::Flags(1),
                Problem::Len {
                    stored: 1999,
                    last: 2000
                },
                Problem::Name {
                    stored: "CAS0103".to_owned(),
                    expected: "cas0104".to_owned()
                },
            ]
        );
        let severities: Vec<Severity> = problems.iter().map(Problem::severity).collect();
        use Severity::{Error, Warning};
        // A name other than the file's makes the game play another sound for the line.
        assert_eq!(severities, vec![Error, Error, Warning, Warning, Warning, Error]);
    }

    #[test]
    fn code_41_is_valid() {
        let mut lip = six();
        lip.phonemes[1] = 41;
        assert_eq!(lip.validate(None), vec![]);
    }

    #[test]
    fn few_markers_and_count_mismatch() {
        let five = Lip::new("x", vec![0, 1, 0, 1], (0..5).map(|i| marker(1, i * 2)).collect());
        assert_eq!(five.validate(None), vec![Problem::FewMarkers(5)]);
        assert_eq!(Problem::FewMarkers(5).severity(), Severity::Warning);
        let mut lip = six();
        lip.phonemes.pop();
        assert_eq!(
            lip.validate(None),
            vec![Problem::CountMismatch {
                phonemes: 4,
                markers: 6
            }]
        );
        assert_eq!(
            Problem::CountMismatch {
                phonemes: 4,
                markers: 6
            }
            .severity(),
            Severity::Error
        );
    }

    #[test]
    fn rescales_proportionally() {
        let mut lip = six();
        lip.rescale(4000);
        let positions: Vec<i32> = lip.markers.iter().map(|m| m.position).collect();
        assert_eq!(positions, vec![0, 400, 1200, 2000, 3200, 4000]);
        assert_eq!(lip.len, 4000);
        lip.rescale(2000);
        assert_eq!(lip, six());
    }

    #[test]
    fn rescaling_rounds_within_two_bytes_and_stays_in_order() {
        let mut lip = six();
        lip.markers[1].position = 201;
        lip.markers[2].position = 202;
        let before = lip.clone();
        lip.rescale(2 * 2000 + 2);
        lip.rescale(2000);
        for (a, b) in lip.markers.iter().zip(&before.markers) {
            assert!((a.position - b.position).abs() <= 2, "{a:?} vs {b:?}");
        }
        assert!(lip.markers.windows(2).all(|w| w[0].position <= w[1].position));
        assert_eq!(lip.last(), 2000);
    }

    #[test]
    fn fits_by_the_last_marker() {
        assert_eq!(fit(2000, 1000), Fit::Matches);
        // Within 2 bytes on a short sound, within 0.01% on a long one.
        assert_eq!(fit(2002, 1000), Fit::Matches);
        assert_eq!(fit(2_000_150, 1_000_000), Fit::Matches);
        assert_eq!(fit(4000, 1000), Fit::Scaled(2.0));
        assert_eq!(fit(8002, 1000), Fit::Scaled(4.0));
        assert_eq!(fit(1000, 1000), Fit::Scaled(0.5));
        assert_eq!(fit(500, 1000), Fit::Scaled(0.25));
        // 0.2% short, as Cassidy Head's adam_dravean files are of their sounds.
        assert!(matches!(fit(1996, 1000), Fit::Other(r) if (r - 0.998).abs() < 1e-9));
    }

    #[test]
    fn problems_read_as_sentences() {
        let cases = [
            (
                Problem::BadCodes { count: 2, first: 1 },
                "2 phoneme codes of 42 or more, the first at 1",
            ),
            (
                Problem::Backwards { index: 3 },
                "marker 3 is before the one preceding it",
            ),
            (
                Problem::BadFirstMarker(marker(0, 0)),
                "the first marker is (0, 0), not (1, 0)",
            ),
            (
                Problem::CountMismatch {
                    phonemes: 4,
                    markers: 6,
                },
                "6 markers for 4 phonemes (should be 5)",
            ),
            (Problem::FewMarkers(5), "5 markers: the engine wants 6 or more"),
            (Problem::Flags(1), "flags 1 (bit 0 loops the line)"),
            (
                Problem::Len {
                    stored: 1999,
                    last: 2000,
                },
                "length field 1999, the last marker is 2000",
            ),
            (
                Problem::Name {
                    stored: "MCS6A".to_owned(),
                    expected: "mcs43alt".to_owned(),
                },
                "names sound MCS6A, the file is mcs43alt",
            ),
        ];
        for (p, want) in cases {
            assert_eq!(p.to_string(), want);
        }
    }
}
