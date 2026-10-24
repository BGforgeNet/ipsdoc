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
use std::hash::{DefaultHasher, Hash, Hasher};
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
    corpus::read_at(path, offset, len).unwrap_or_else(|e| panic!("{path} at {offset}: {e}"))
}

/// The unique ACM and WAVC streams.
fn load() -> Vec<Stream> {
    load_containers(&["acm", "wavc"])
}

/// The unique streams of these containers (the manifest's names).
fn load_containers(containers: &[&str]) -> Vec<Stream> {
    let manifest = std::env::var("IPSDOC_CORPUS").expect("IPSDOC_CORPUS names the corpus manifest");
    let limit = std::env::var("IPSDOC_CORPUS_LIMIT").map_or(usize::MAX, |n| n.parse().expect("a number"));
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{manifest}: {e}"));
    let mut streams: Vec<Stream> = Vec::new();
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for line in text.lines() {
        let row = corpus::Row::parse(line).unwrap_or_else(|e| panic!("{manifest}: {e}"));
        let (container, path, offset, len, origin) = (row.container, row.path, row.offset, row.len, row.origin);
        if !containers.contains(&container) {
            continue;
        }
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

/// The bare ACM of a stream, through our WAVC reader. GemRB looks at a fixed 28 bytes, so a WAVC whose stated
/// offset differs is reported.
fn bare<'a>(container: &str, bytes: &'a [u8]) -> Result<&'a [u8], String> {
    if container == "acm" {
        return Ok(bytes);
    }
    let header = ipsdoc::wavc::Header::parse(bytes).map_err(|e| e.to_string())?;
    if header.acm_offset != 28 {
        return Err(format!("WAVC with its ACM at {}, not 28", header.acm_offset));
    }
    ipsdoc::wavc::acm_of(bytes).map_err(|e| e.to_string())
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

/// Runs `f` over `streams` on every core.
fn for_each_parallel(streams: &[Stream], f: impl Fn(&Stream) + Sync) {
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(s) = streams.get(next.fetch_add(1, Ordering::Relaxed)) {
                    f(s);
                }
            });
        }
    });
}

#[derive(Default)]
struct Sizes {
    streams: u64,
    original: u64,
    lossless: u64,
    pcm: u64,
    failures: Vec<String>,
}

/// Re-encoding a real stream losslessly gives back its decoded samples, in ours and in the Fallout 2 engine's
/// decoder; reports the sizes against 16-bit PCM and against the (lossy) originals.
#[test]
#[ignore = "needs IPSDOC_CORPUS and IPSDOC_ORACLES"]
fn every_corpus_stream_re_encodes_losslessly() {
    let streams = load();
    let sizes = Mutex::new(Sizes::default());
    for_each_parallel(&streams, |s| {
        let name = format!("{} ({} copies)", s.origin, s.copies);
        let bytes = read(&s.path, s.offset, s.len);
        let fail = |m: String| sizes.lock().unwrap().failures.push(format!("{name}: {m}"));
        let acm = match bare(&s.container, &bytes) {
            Ok(a) => a,
            Err(e) => return fail(e),
        };
        let decoded = match ipsdoc::acm::decode(acm) {
            Ok(d) => d,
            Err(e) => return fail(e.to_string()),
        };
        let h = decoded.header;
        let lossless = ipsdoc::acm::encode(&decoded.samples, h.channels, h.rate).unwrap();
        let again = ipsdoc::acm::decode(&lossless).map(|d| d.samples);
        if again.as_ref() != Ok(&decoded.samples) {
            return fail("our decoder does not give back what was encoded".into());
        }
        match Oracle::Fallout2Ce.decode(&lossless) {
            Ok(e) if e == decoded.samples => {}
            Ok(_) => return fail("the Fallout 2 engine's decoder reads our output differently".into()),
            Err(e) => return fail(e.to_string()),
        }
        let mut t = sizes.lock().unwrap();
        t.streams += 1;
        t.original += acm.len() as u64;
        t.lossless += lossless.len() as u64;
        t.pcm += 2 * decoded.samples.len() as u64;
    });
    let t = sizes.into_inner().unwrap();
    eprintln!(
        "{} streams: lossless {:.3} of 16-bit PCM, {:.2} times the lossy originals",
        t.streams,
        t.lossless as f64 / t.pcm as f64,
        t.lossless as f64 / t.original as f64
    );
    report("FAIL", &t.failures);
    assert!(t.failures.is_empty(), "{} failures (by kind above)", t.failures.len());
    assert!(t.streams > 0, "no stream judged");
}

/// Every RIFF and Ogg stream in a corpus reads; reports what they hold.
#[test]
#[ignore = "needs IPSDOC_CORPUS"]
fn every_corpus_riff_and_ogg_stream_reads() {
    let streams = load_containers(&["riff", "ogg"]);
    let kinds: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    let failures = Mutex::new(Vec::new());
    for_each_parallel(&streams, |s| {
        let bytes = read(&s.path, s.offset, s.len);
        match ipsdoc::input::read(&bytes, &Default::default()) {
            Ok(l) => {
                let depth = l.audio.bits().map_or("float".to_owned(), |b| format!("{b}-bit"));
                let kind = format!("{} {depth} {} Hz x{}", s.container, l.audio.rate, l.audio.channels);
                *kinds.lock().unwrap().entry(kind).or_default() += 1;
            }
            Err(e) => failures
                .lock()
                .unwrap()
                .push(format!("{} ({} copies): {e}: {:?}", s.origin, s.copies, e)),
        }
    });
    for (kind, n) in kinds.into_inner().unwrap() {
        eprintln!("{n:6} {kind}");
    }
    let failures = failures.into_inner().unwrap();
    report("FAIL", &failures);
    assert!(failures.is_empty(), "{} unreadable (by kind above)", failures.len());
    assert!(!streams.is_empty(), "no stream judged");
}

#[test]
#[ignore = "needs IPSDOC_CORPUS and IPSDOC_ORACLES"]
fn every_corpus_stream_decodes_as_the_engines_decode() {
    let streams = load();
    let tally = Mutex::new(Tally::default());
    for_each_parallel(&streams, |s| judge(s, &tally));
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
