//! `corpus <manifest.tsv> <extract-dir> <source>...`: lists every sound stream under the sources.
//!
//! Each manifest line is `container, path, offset, length, origin`, tab-separated. Loose files and entries of
//! uncompressed BIFFs are referenced in place; entries of compressed BIFs are extracted into `<extract-dir>` first,
//! with `origin` naming the archive and entry. Fallout DAT archives are not read: extract them first (`dat3 x`).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use corpus::{biff_sounds, inflate_bif, sniff};
use ipsdoc::detect::Format as Container;

#[derive(Default)]
struct Scan {
    files: u64,
    archives: u64,
    streams: BTreeMap<Container, u64>,
    /// BIF WAV entries with no known sound magic.
    unknown_entries: u64,
    errors: Vec<String>,
    extracted: u64,
}

struct Out {
    manifest: BufWriter<File>,
    extract: PathBuf,
}

fn field(path: &Path) -> io::Result<String> {
    let s = path.to_string_lossy().into_owned();
    if s.contains(['\t', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path with a tab or newline: {s:?}"),
        ));
    }
    Ok(s)
}

fn row(
    out: &mut Out,
    scan: &mut Scan,
    c: Container,
    path: &Path,
    offset: u64,
    len: u64,
    origin: &str,
) -> io::Result<()> {
    *scan.streams.entry(c).or_default() += 1;
    writeln!(
        out.manifest,
        "{}\t{}\t{offset}\t{len}\t{origin}",
        c.name(),
        field(path)?
    )
}

fn scan_file(path: &Path, out: &mut Out, scan: &mut Scan) -> io::Result<()> {
    scan.files += 1;
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut head = [0u8; 8];
    let n = f.read(&mut head)?;
    let head = &head[..n];
    if let Some(c) = sniff(head) {
        return row(out, scan, c, path, 0, len, "-");
    }
    if head == b"BIFFV1  " {
        scan.archives += 1;
        for e in biff_sounds(&mut f)?.unwrap_or_default() {
            f.seek(SeekFrom::Start(e.offset))?;
            let mut magic = [0u8; 4];
            let k = f.read(&mut magic[..e.len.min(4) as usize])?;
            match sniff(&magic[..k]) {
                Some(c) => row(out, scan, c, path, e.offset, e.len, &format!("#{}", e.index))?,
                None => scan.unknown_entries += 1,
            }
        }
        return Ok(());
    }
    if head == b"BIFCV1.0" || head == b"BIF V1.0" {
        scan.archives += 1;
        let mut bytes = Vec::with_capacity(len as usize);
        f.seek(SeekFrom::Start(0))?;
        f.read_to_end(&mut bytes)?;
        let Some(inner) = inflate_bif(&bytes)? else {
            return Ok(());
        };
        for e in biff_sounds(&mut Cursor::new(&inner))?.unwrap_or_default() {
            let data = &inner[e.offset as usize..(e.offset + e.len) as usize];
            let Some(c) = sniff(data) else {
                scan.unknown_entries += 1;
                continue;
            };
            scan.extracted += 1;
            let target = out.extract.join(format!("{}.{}", scan.extracted, c.name()));
            std::fs::write(&target, data)?;
            row(
                out,
                scan,
                c,
                &target,
                0,
                e.len,
                &format!("{}#{}", field(path)?, e.index),
            )?;
        }
    }
    Ok(())
}

fn walk(dir: &Path, out: &mut Out, scan: &mut Scan) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => return scan.errors.push(format!("{}: {e}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            scan.errors.push(format!("{}: unreadable metadata", path.display()));
            continue;
        };
        // The extract directory may sit under a source; its files are already listed.
        if meta.is_dir() && path.file_name().is_some_and(|n| n != ".git") && path != out.extract {
            walk(&path, out, scan);
        } else if meta.is_file()
            && let Err(e) = scan_file(&path, out, scan)
        {
            scan.errors.push(format!("{}: {e}", path.display()));
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [manifest, extract, sources @ ..] = args.as_slice() else {
        eprintln!("usage: corpus <manifest.tsv> <extract-dir> <source>...");
        return ExitCode::from(2);
    };
    if sources.is_empty() {
        eprintln!("usage: corpus <manifest.tsv> <extract-dir> <source>...");
        return ExitCode::from(2);
    }
    let opened = std::fs::create_dir_all(extract).and_then(|()| File::create(manifest));
    let manifest = match opened {
        Ok(f) => BufWriter::new(f),
        Err(e) => {
            eprintln!("corpus: {}: {e}", manifest.display());
            return ExitCode::FAILURE;
        }
    };
    let extract = match std::path::absolute(extract) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("corpus: {}: {e}", extract.display());
            return ExitCode::FAILURE;
        }
    };
    let mut out = Out { manifest, extract };
    let mut scan = Scan::default();
    for source in sources {
        walk(source, &mut out, &mut scan);
    }
    if let Err(e) = out.manifest.flush() {
        scan.errors.push(format!("manifest: {e}"));
    }
    eprintln!(
        "{} files, {} archives, {} streams {:?}, {} extracted, {} BIF WAV entries of unknown format",
        scan.files,
        scan.archives,
        scan.streams.values().sum::<u64>(),
        scan.streams.iter().map(|(c, n)| (c.name(), n)).collect::<Vec<_>>(),
        scan.extracted,
        scan.unknown_entries
    );
    for e in &scan.errors {
        eprintln!("error: {e}");
    }
    if scan.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
