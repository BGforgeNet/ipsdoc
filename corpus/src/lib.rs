#![warn(
    missing_docs,
    unreachable_pub,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]
//! Finds sound streams in game installs and mod trees: loose files by magic, and the WAV resources (type 4) of
//! Infinity Engine BIFF archives, compressed (BIFC V1.0, whole-file BIF V1.0) or not.

use std::io::{self, Read, Seek, SeekFrom};

use ipsdoc::detect::{Format, detect};

/// The container of a stream starting with `head`, if it is a sound the games use.
pub fn sniff(head: &[u8]) -> Option<Format> {
    Some(detect(head)).filter(|&f| f != Format::Other)
}

/// One resource of a BIFF archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Position in the archive's file table.
    pub index: usize,
    /// Byte offset in the archive.
    pub offset: u64,
    /// Length in bytes.
    pub len: u64,
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The WAV resources of an uncompressed BIFF V1, or `None` if `r` holds no BIFF V1.
///
/// # Errors
///
/// On a read error, or a file table or entry outside the archive.
pub fn biff_sounds<R: Read + Seek>(r: &mut R) -> io::Result<Option<Vec<Entry>>> {
    let len = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut head = [0u8; 20];
    if len < 20 {
        return Ok(None);
    }
    r.read_exact(&mut head)?;
    if &head[..8] != b"BIFFV1  " {
        return Ok(None);
    }
    let (count, table) = (u32_at(&head, 8) as u64, u32_at(&head, 16) as u64);
    if table + 16 * count > len {
        return Err(invalid(format!(
            "file table of {count} entries at {table} runs past the end ({len} bytes)"
        )));
    }
    r.seek(SeekFrom::Start(table))?;
    let mut entries = vec![0u8; 16 * count as usize];
    r.read_exact(&mut entries)?;
    let mut sounds = Vec::new();
    for (index, e) in entries.as_chunks::<16>().0.iter().enumerate() {
        if u16::from_le_bytes([e[12], e[13]]) != 4 {
            continue;
        }
        let (offset, size) = (u64::from(u32_at(e, 4)), u64::from(u32_at(e, 8)));
        if offset + size > len {
            return Err(invalid(format!(
                "entry {index} ({size} bytes at {offset}) runs past the end ({len} bytes)"
            )));
        }
        sounds.push(Entry {
            index,
            offset,
            len: size,
        });
    }
    Ok(Some(sounds))
}

/// The BIFF inside a compressed archive (BIFC V1.0 or whole-file BIF V1.0), or `None` if `bytes` is neither.
///
/// # Errors
///
/// On a truncated archive, corrupt zlib data, or a size the header contradicts.
pub fn inflate_bif(bytes: &[u8]) -> io::Result<Option<Vec<u8>>> {
    let field = |at: usize| -> io::Result<usize> {
        bytes
            .get(at..at + 4)
            .map(|b| u32_at(b, 0) as usize)
            .ok_or_else(|| invalid(format!("truncated at byte {at}")))
    };
    let chunk = |at: usize, len: usize| -> io::Result<&[u8]> {
        bytes
            .get(at..at + len)
            .ok_or_else(|| invalid(format!("{len} bytes at {at} run past the end")))
    };
    // Inflated no further than the size the header states, so a block claiming little cannot expand to much.
    let unzip = |z: &[u8], expected: usize| -> io::Result<Vec<u8>> {
        let out = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(z, expected)
            .map_err(|e| invalid(format!("block of size {expected} does not inflate: {e}")))?;
        if out.len() != expected {
            return Err(invalid(format!(
                "block inflates to {} bytes, its header says size {expected}",
                out.len()
            )));
        }
        Ok(out)
    };
    if bytes.starts_with(b"BIFCV1.0") {
        let total = field(8)?;
        // Grown block by block: the header's total is not trusted for an allocation.
        let (mut out, mut at) = (Vec::new(), 12);
        while at < bytes.len() {
            let (size, zlen) = (field(at)?, field(at + 4)?);
            out.extend(unzip(chunk(at + 8, zlen)?, size)?);
            at += 8 + zlen;
        }
        if out.len() != total {
            return Err(invalid(format!(
                "blocks inflate to {} bytes, the header says size {total}",
                out.len()
            )));
        }
        return Ok(Some(out));
    }
    if bytes.starts_with(b"BIF V1.0") {
        let name = field(8)?;
        let (size, zlen) = (field(12 + name)?, field(16 + name)?);
        return Ok(Some(unzip(chunk(20 + name, zlen)?, size)?));
    }
    Ok(None)
}

/// One line of the manifest the `corpus` tool writes: `container, path, offset, length, origin`, tab-separated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    /// The container's name ([`Format::name`]).
    pub container: &'a str,
    /// The file holding the stream.
    pub path: &'a str,
    /// The stream's byte offset in the file.
    pub offset: u64,
    /// The stream's length in bytes.
    pub len: u64,
    /// `-` for a loose file, else the archive and entry it came from.
    pub origin: &'a str,
}

impl<'a> Row<'a> {
    /// Parses a manifest line.
    ///
    /// # Errors
    ///
    /// When the line does not hold the five fields, or the offset or length is not a number.
    pub fn parse(line: &'a str) -> io::Result<Row<'a>> {
        let f: Vec<&str> = line.split('\t').collect();
        let [container, path, offset, len, origin] = f[..] else {
            return Err(invalid(format!("not a manifest line: {line:?}")));
        };
        let number = |s: &str| s.parse().map_err(|e| invalid(format!("{line:?}: {e}")));
        Ok(Row {
            container,
            path,
            offset: number(offset)?,
            len: number(len)?,
            origin,
        })
    }

    /// The stream's bytes.
    ///
    /// # Errors
    ///
    /// As [`read_at`].
    pub fn read(&self) -> io::Result<Vec<u8>> {
        read_at(self.path, self.offset, self.len)
    }
}

/// `len` bytes of the file at `path`, from `offset`: a manifest row's stream.
///
/// # Errors
///
/// When the file cannot be read, or ends before the stream does.
pub fn read_at(path: &str, offset: u64, len: u64) -> io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let len = usize::try_from(len).map_err(|e| invalid(format!("{path}: {e}")))?;
    let mut bytes = vec![0; len];
    f.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A BIFF V1 with one entry per `(type, data)`, data after the entry table.
    fn biff(entries: &[(u16, &[u8])]) -> Vec<u8> {
        let table = 20;
        let mut data_at = table + 16 * entries.len();
        let mut out = b"BIFFV1  ".to_vec();
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(table as u32).to_le_bytes());
        for (i, (kind, data)) in entries.iter().enumerate() {
            out.extend_from_slice(&(i as u32).to_le_bytes());
            out.extend_from_slice(&(data_at as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            data_at += data.len();
        }
        for (_, data) in entries {
            out.extend_from_slice(data);
        }
        out
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
    }

    #[test]
    fn reads_a_manifest_row_and_its_bytes() {
        let file = std::env::temp_dir().join(format!("corpus-row-{}", std::process::id()));
        std::fs::write(&file, b"..WAVC..").unwrap();
        let line = format!("wavc\t{}\t2\t4\tx.bif#3", file.display());
        let row = Row::parse(&line).unwrap();
        assert_eq!(
            (row.container, row.offset, row.len, row.origin),
            ("wavc", 2, 4, "x.bif#3")
        );
        assert_eq!(row.read().unwrap(), b"WAVC");
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn refuses_a_manifest_row_of_other_fields() {
        for line in [
            "acm\tx.acm\t0\t4",
            "acm\tx.acm\t0\tfour\t-",
            "acm\tx.acm\t0\t4\t-\textra",
        ] {
            assert_eq!(
                Row::parse(line).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{line}"
            );
        }
    }

    #[test]
    fn sniffs_only_sounds() {
        assert_eq!(sniff(b"WAVCV1.0"), Some(Format::Wavc));
        assert_eq!(sniff(b"BIFFV1  "), None);
    }

    #[test]
    fn lists_only_the_sound_entries_of_a_biff() {
        let bif = biff(&[(4, b"abc"), (0x3EE, b"area"), (4, b"de")]);
        let entries = biff_sounds(&mut Cursor::new(&bif)).unwrap().unwrap();
        assert_eq!(
            entries,
            [
                Entry {
                    index: 0,
                    offset: 68,
                    len: 3
                },
                Entry {
                    index: 2,
                    offset: 75,
                    len: 2
                }
            ]
        );
        assert_eq!(&bif[75..77], b"de");
    }

    #[test]
    fn a_non_biff_has_no_entries() {
        assert_eq!(biff_sounds(&mut Cursor::new(b"BIFCV1.0 rest")).unwrap(), None);
    }

    #[test]
    fn rejects_an_entry_outside_the_file() {
        let mut bif = biff(&[(4, b"abc")]);
        bif.truncate(bif.len() - 1);
        let err = biff_sounds(&mut Cursor::new(&bif)).unwrap_err();
        assert!(err.to_string().contains("entry 0"), "{err}");
    }

    #[test]
    fn inflates_a_bifc_of_several_blocks() {
        let inner = biff(&[(4, b"sound data, long enough to split")]);
        let (a, b) = inner.split_at(30);
        let mut bifc = b"BIFCV1.0".to_vec();
        bifc.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        for part in [a, b] {
            let z = zlib(part);
            bifc.extend_from_slice(&(part.len() as u32).to_le_bytes());
            bifc.extend_from_slice(&(z.len() as u32).to_le_bytes());
            bifc.extend_from_slice(&z);
        }
        assert_eq!(inflate_bif(&bifc).unwrap(), Some(inner));
    }

    #[test]
    fn inflates_a_whole_file_compressed_bif() {
        let inner = biff(&[(4, b"x")]);
        let z = zlib(&inner);
        let mut bif = b"BIF V1.0".to_vec();
        bif.extend_from_slice(&5u32.to_le_bytes());
        bif.extend_from_slice(b"a.bif");
        bif.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        bif.extend_from_slice(&(z.len() as u32).to_le_bytes());
        bif.extend_from_slice(&z);
        assert_eq!(inflate_bif(&bif).unwrap(), Some(inner));
    }

    #[test]
    fn rejects_a_bifc_whose_size_disagrees() {
        let inner = biff(&[(4, b"x")]);
        let z = zlib(&inner);
        let mut bifc = b"BIFCV1.0".to_vec();
        bifc.extend_from_slice(&(inner.len() as u32 + 1).to_le_bytes());
        bifc.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        bifc.extend_from_slice(&(z.len() as u32).to_le_bytes());
        bifc.extend_from_slice(&z);
        assert!(inflate_bif(&bifc).unwrap_err().to_string().contains("size"));
    }

    #[test]
    fn stops_inflating_at_the_size_the_header_states() {
        let z = zlib(&[0; 4096]);
        let mut bif = b"BIF V1.0".to_vec();
        bif.extend_from_slice(&0u32.to_le_bytes());
        bif.extend_from_slice(&16u32.to_le_bytes());
        bif.extend_from_slice(&(z.len() as u32).to_le_bytes());
        bif.extend_from_slice(&z);
        let err = inflate_bif(&bif).unwrap_err().to_string();
        assert!(err.contains("block of size 16 does not inflate"), "{err}");
    }

    #[test]
    fn an_uncompressed_file_is_not_inflated() {
        assert_eq!(inflate_bif(&biff(&[])).unwrap(), None);
    }
}
