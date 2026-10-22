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
    let unzip = |z: &[u8], expected: usize| -> io::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(expected);
        flate2::read::ZlibDecoder::new(z).read_to_end(&mut out)?;
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
        let (mut out, mut at) = (Vec::with_capacity(total), 12);
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

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::{Cursor, Write};

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
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
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
    fn an_uncompressed_file_is_not_inflated() {
        assert_eq!(inflate_bif(&biff(&[])).unwrap(), None);
    }
}
