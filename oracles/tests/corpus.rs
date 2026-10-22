//! Every real ACM and WAVC stream in a corpus decodes, in our decoder and GemRB's, exactly as in the Fallout 2
//! engine's decoder. libacm and `infinitier` are run too and their differences counted, not failed: on real files
//! they end at the last complete block when a stream ends inside its final block (most do), and `infinitier`
//! saturates samples the engines wrap.
//!
//! Run with `IPSDOC_CORPUS=<manifest.tsv>` (from the `corpus` tool) and `IPSDOC_ORACLES` set, and `--ignored`.
//! `IPSDOC_CORPUS_LIMIT=N` judges only the first N unique streams.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test harness code outside #[test] fns: a panic is the failure report"
)]

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use oracles::Oracle;

mod common;
use common::{disagreements, oracle_disagreements};

/// A unique stream, read again when judged so the corpus never sits in memory whole.
struct Stream {
    container: String,
    path: String,
    offset: u64,
    len: u64,
    /// Where it was first seen, and how many copies the corpus holds.
    origin: String,
    copies: u64,
}

#[derive(Default)]
struct Tally {
    judged: BTreeMap<String, u64>,
    shapes: BTreeMap<(u8, u16), u64>,
    fillers: [u64; 32],
    overrun_streams: u64,
    max_overrun_bits: u64,
    failures: Vec<String>,
    overflowed: u64,
    /// Differences in the non-engine decoders.
    noted: Vec<String>,
}

fn read(path: &str, offset: u64, len: u64) -> Vec<u8> {
    let mut f = File::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    f.seek(SeekFrom::Start(offset)).unwrap();
    let mut bytes = vec![0; len as usize];
    f.read_exact(&mut bytes)
        .unwrap_or_else(|e| panic!("{path} at {offset}: {e}"));
    bytes
}

fn load() -> Vec<Stream> {
    let manifest = std::env::var("IPSDOC_CORPUS").expect("IPSDOC_CORPUS names the corpus manifest");
    let limit = std::env::var("IPSDOC_CORPUS_LIMIT").map_or(usize::MAX, |n| n.parse().expect("a number"));
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{manifest}: {e}"));
    let mut streams: Vec<Stream> = Vec::new();
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [container, path, offset, len, origin] = f[..] else {
            panic!("bad manifest line: {line}")
        };
        if container != "acm" && container != "wavc" {
            continue;
        }
        let (offset, len) = (offset.parse().unwrap(), len.parse().unwrap());
        let mut h = DefaultHasher::new();
        read(path, offset, len).hash(&mut h);
        match seen.get(&h.finish()) {
            Some(&i) => streams[i].copies += 1,
            None if streams.len() < limit => {
                seen.insert(h.finish(), streams.len());
                let origin = if origin == "-" {
                    path.to_owned()
                } else {
                    origin.to_owned()
                };
                let (container, path) = (container.to_owned(), path.to_owned());
                streams.push(Stream {
                    container,
                    path,
                    offset,
                    len,
                    origin,
                    copies: 1,
                });
            }
            None => {}
        }
    }
    streams
}

/// The bare ACM of a stream: WAVC holds it at a fixed 28 bytes, which is where GemRB looks.
fn bare<'a>(container: &str, bytes: &'a [u8]) -> Result<&'a [u8], String> {
    if container == "acm" {
        return Ok(bytes);
    }
    let at = bytes
        .get(0x10..0x14)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    match at {
        Some(28) => Ok(&bytes[28..]),
        other => Err(format!("WAVC with its ACM at {other:?}, not 28")),
    }
}

fn judge(s: &Stream, tally: &Mutex<Tally>) {
    let name = format!("{} ({} copies)", s.origin, s.copies);
    let fail = |m: String| tally.lock().unwrap().failures.push(format!("{name}: {m}"));
    let bytes = read(&s.path, s.offset, s.len);
    let acm = match bare(&s.container, &bytes) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let reference = match Oracle::Fallout2Ce.decode(acm) {
        Ok(r) => r,
        Err(e) => return fail(e.to_string()),
    };
    // Engine mode must match the engines exactly; the default (clamping) must differ only at the samples it counts
    // as overflowed, each pinned to the range's end.
    let mut failures = disagreements(acm, &[Oracle::GemRb], &reference);
    let ours = ipsdoc::acm::decode(acm);
    if let Ok(d) = &ours {
        let moved: Vec<(usize, i16)> = d
            .samples
            .iter()
            .zip(&reference)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (&a, _))| (i, a))
            .collect();
        let pinned = moved.iter().all(|&(_, v)| v == i16::MAX || v == i16::MIN);
        if moved.len() as u64 != d.stats.overflowed || !pinned {
            failures.push(format!(
                "clamping moved {} samples (first {:?}), {} counted as overflowed",
                moved.len(),
                moved.first(),
                d.stats.overflowed
            ));
        }
    }
    // libacm wraps like the engines; infinitier saturates, so it is held against the clamped output.
    let levels = ours.as_ref().map_or(0, |d| d.header.levels);
    let mut noted = Vec::new();
    if levels > 0 {
        noted.extend(oracle_disagreements(acm, &[Oracle::Libacm], &reference));
    }
    if let Ok(d) = &ours {
        noted.extend(oracle_disagreements(acm, &[Oracle::Infinitier], &d.samples));
    }
    let mut t = tally.lock().unwrap();
    t.overflowed += ours.as_ref().map_or(0, |d| d.stats.overflowed);
    t.failures.extend(failures.into_iter().map(|f| format!("{name}: {f}")));
    t.noted.extend(noted.into_iter().map(|f| format!("{name}: {f}")));
    *t.judged.entry(s.container.clone()).or_default() += 1;
    if let Ok(d) = ours {
        *t.shapes.entry((d.header.levels, d.header.rows)).or_default() += 1;
        for (all, n) in t.fillers.iter_mut().zip(d.stats.fillers) {
            *all += n;
        }
        if d.stats.overrun_bits > 0 {
            t.overrun_streams += 1;
            t.max_overrun_bits = t.max_overrun_bits.max(d.stats.overrun_bits);
        }
    }
}

/// Prints `lines` grouped by decoder and kind (numbers folded to `N`), with each kind's first instance.
fn report(label: &str, lines: &[String]) {
    let mut kinds: BTreeMap<String, (u64, &str)> = BTreeMap::new();
    for line in lines {
        let what = line.rsplit_once("): ").map_or(line.as_str(), |(_, w)| w);
        let mut kind = String::new();
        for c in what.chars() {
            match (c.is_ascii_digit(), kind.ends_with('N')) {
                (true, true) => {}
                (true, false) => kind.push('N'),
                (false, _) => kind.push(c),
            }
        }
        kinds.entry(kind).or_insert((0, line)).0 += 1;
    }
    for (kind, (n, first)) in &kinds {
        eprintln!("{label} x{n} {kind}\n    e.g. {first}");
    }
}

#[test]
#[ignore = "needs IPSDOC_CORPUS and IPSDOC_ORACLES"]
fn every_corpus_stream_decodes_as_the_engines_decode() {
    let streams = load();
    let tally = Mutex::new(Tally::default());
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(s) = streams.get(next.fetch_add(1, Ordering::Relaxed)) {
                    judge(s, &tally);
                }
            });
        }
    });
    let t = tally.into_inner().unwrap();
    let copies: u64 = streams.iter().map(|s| s.copies).sum();
    eprintln!(
        "{} unique streams ({copies} with copies); judged {:?}",
        streams.len(),
        t.judged
    );
    eprintln!("(levels, rows): {:?}", t.shapes);
    let used: Vec<(usize, u64)> = t.fillers.iter().copied().enumerate().filter(|&(_, n)| n > 0).collect();
    eprintln!("filler columns: {used:?}");
    eprintln!(
        "{} streams read past their end, at most {} bits",
        t.overrun_streams, t.max_overrun_bits
    );
    eprintln!(
        "{} samples beyond 16 bits (clamped by default, wrapped by the engines)",
        t.overflowed
    );
    report("NOTED", &t.noted);
    report("FAIL", &t.failures);
    assert!(
        t.failures.is_empty(),
        "{} disagreements (by kind above)",
        t.failures.len()
    );
    for container in ["acm", "wavc"] {
        assert!(
            t.judged.get(container).copied().unwrap_or(0) > 0,
            "no {container} stream judged"
        );
    }
}
