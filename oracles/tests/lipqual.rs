//! `lipqual`'s limits against lip files of known quality: Fallout 2's own (Interplay's), ours from the line's text
//! and from the sound alone, wav2lip's, and spoiled copies of the originals: another line's lip file stretched to
//! fit, the original moved 100 ms early and 100 ms, 200 ms, 500 ms and 1 s late, its shapes shuffled with their
//! lengths kept, and one open shape held through the line. Prints each set's share of files with each problem and its figures' spread, the
//! directory grades, and the head frames' median loudness, from which `lipqual`'s openness order comes.
//!
//! Run with `--ignored` and the inputs of `lips.rs` (its header): `IPSDOC_LIPS`, `IPSDOC_MSG` and
//! `IPSDOC_LIPS_WAV2LIP`.
#![expect(
    clippy::unwrap_used,
    reason = "test harness code outside #[test] fns: a panic is the failure report"
)]
#![expect(
    clippy::print_stdout,
    reason = "a report helper the test calls: the test runner captures its output"
)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ipsdoc::g2p::tokens;
use ipsdoc::input::{ReadOptions, read};
use ipsdoc::lip::{self, Lip, Marker};
use ipsdoc::lipgen::{Params, generate};
use ipsdoc::lipqual::{Quality, QualityProblem, SetProblem, grade, head_frames, quality};
use ipsdoc::pcm::Audio;
use ipsdoc::speech::{self, SpeechParams};

const SETS: [&str; 12] = [
    "interplay",
    "ours, text",
    "ours, audio",
    "wav2lip",
    "other line",
    "100 ms early",
    "100 ms late",
    "200 ms late",
    "500 ms late",
    "1 s late",
    "shuffled",
    "one open shape",
];

fn env_dir(key: &str) -> PathBuf {
    PathBuf::from(std::env::var(key).unwrap_or_else(|_| panic!("set {key}; see this file's header")))
}

fn voiced_lines(dialog: &Path) -> HashMap<String, String> {
    let mut text = HashMap::new();
    for e in std::fs::read_dir(dialog).unwrap() {
        let p = e.unwrap().path();
        let entries = ipsdoc::msg::parse(&std::fs::read(&p).unwrap(), ipsdoc::language::Language::English)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        for x in entries.into_iter().filter(|x| !x.sound.is_empty()) {
            text.entry(x.sound.to_ascii_lowercase()).or_insert(x.text);
        }
    }
    text
}

/// `l` with every shape `ms` later (earlier when negative), the last running to the sound's end.
fn late(l: &Lip, ms: i64, audio: &Audio) -> Lip {
    let shift = ms * i64::from(audio.rate) / 1000 * 2 * i64::from(audio.channels);
    let last = l.last();
    let mut markers = l.markers.clone();
    let end = markers.len() - 1;
    for m in &mut markers[1..end] {
        m.position = (i64::from(m.position) + shift).clamp(0, i64::from(last)) as i32;
    }
    Lip::new(&l.name(), l.phonemes.clone(), markers)
}

/// `l`'s shapes in a seeded random order, each keeping its length.
fn shuffled(l: &Lip, seed: &mut u64) -> Lip {
    let mut shapes: Vec<(u8, i32)> = l
        .phonemes
        .iter()
        .zip(l.markers.windows(2))
        .map(|(&c, w)| (c, w[1].position - w[0].position))
        .collect();
    for i in (1..shapes.len()).rev() {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        shapes.swap(i, (*seed % (i as u64 + 1)) as usize);
    }
    let mut at = 0;
    let mut markers = vec![Marker { kind: 1, position: 0 }];
    for &(_, len) in &shapes {
        at += len;
        markers.push(Marker { kind: 1, position: at });
    }
    Lip::new(&l.name(), shapes.iter().map(|s| s.0).collect(), markers)
}

/// One open shape (head frame 3) from the start to the end.
fn one_open_shape(l: &Lip) -> Lip {
    let markers = vec![
        Marker { kind: 1, position: 0 },
        Marker {
            kind: 1,
            position: l.last(),
        },
    ];
    Lip::new(&l.name(), vec![1], markers)
}

/// The kind of a problem, as a table column.
fn kind(p: &QualityProblem) -> &'static str {
    match p {
        QualityProblem::Shut { .. } => "shut",
        QualityProblem::Offset { .. } => "offset",
        QualityProblem::Closures(_) => "closures",
        QualityProblem::OpenWhileQuiet { .. } => "open quiet",
        QualityProblem::Movement { .. } => "movement",
        QualityProblem::Held { .. } => "held",
    }
}

fn set_kind(p: &SetProblem) -> &'static str {
    match p {
        SetProblem::Unaligned(_) => "unaligned",
        SetProblem::Shut(_) => "shut",
        SetProblem::Closures(_) => "closures",
        SetProblem::Openness(_) => "openness",
        SetProblem::OpenWhileQuiet(_) => "open quiet",
        SetProblem::Movement(_) => "movement",
    }
}

/// The least, the 5th, 50th and 95th percentiles, and the most of the figures present.
fn spread(v: impl Iterator<Item = Option<f64>>) -> String {
    let mut v: Vec<f64> = v.flatten().collect();
    if v.is_empty() {
        return "     -".to_owned();
    }
    v.sort_by(f64::total_cmp);
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    format!(
        "{:5.2} | {:5.2} {:5.2} {:5.2} | {:5.2} ({})",
        at(0.0),
        at(0.05),
        at(0.5),
        at(0.95),
        at(1.0),
        v.len()
    )
}

#[derive(Default)]
struct Run {
    /// Per set, per speaker directory, the judged files.
    judged: BTreeMap<&'static str, BTreeMap<String, Vec<Quality>>>,
    /// Per head frame, each speech frame's loudness rank within its line (0 to 1), over Interplay's files.
    loudness: BTreeMap<u8, Vec<f64>>,
    lines: usize,
    skipped: BTreeMap<&'static str, usize>,
}

impl Run {
    fn add(&mut self, set: &'static str, dir: &str, q: Quality) {
        self.judged
            .entry(set)
            .or_default()
            .entry(dir.to_owned())
            .or_default()
            .push(q);
    }

    fn skip(&mut self, why: &'static str) {
        *self.skipped.entry(why).or_default() += 1;
    }

    /// The share of `set`'s files with a problem of any of `kinds`.
    fn rate(&self, set: &str, kinds: &[&str]) -> f64 {
        let files: Vec<&Quality> = self.judged[set].values().flatten().collect();
        let hit = files
            .iter()
            .filter(|q| q.problems().iter().any(|p| kinds.contains(&kind(p))))
            .count();
        hit as f64 / files.len().max(1) as f64
    }

    /// The problems of each of `set`'s directory grades.
    fn set_problems(&self, set: &str) -> Vec<Vec<&'static str>> {
        self.judged[set]
            .values()
            .filter_map(|files| grade(files))
            .map(|g| g.problems().iter().map(set_kind).collect())
            .collect()
    }
}

/// Head frame loudness ranks of one Interplay line, over its speech frames.
fn add_loudness(run: &mut Run, l: &Lip, audio: &Audio) {
    let a = speech::analyse(audio, &SpeechParams::default());
    let shown = head_frames(l, audio, a.frames.len());
    let speech: Vec<usize> = a
        .segments
        .iter()
        .flat_map(|s| s.start..s.end.min(a.frames.len()))
        .collect();
    let mut order = speech.clone();
    order.sort_by(|&x, &y| a.frames[x].db.total_cmp(&a.frames[y].db));
    let denominator = (order.len().max(2) - 1) as f64;
    for (rank, &f) in order.iter().enumerate() {
        run.loudness
            .entry(shown[f])
            .or_default()
            .push(rank as f64 / denominator);
    }
}

fn score(dirs: &[PathBuf], text: &HashMap<String, String>, wav2lip: &Path) -> Run {
    let mut run = Run::default();
    let p = Params::default();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for dir in dirs {
        let speaker = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut lips: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|f| f.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lip")))
            .collect();
        lips.sort();
        // The lines that read, with an original fit to judge: (name, lip file, sound).
        let mut lines: Vec<(String, Lip, Audio)> = Vec::new();
        for lp in lips {
            let name = lp.file_stem().unwrap().to_string_lossy().to_ascii_lowercase();
            let Some(acm) = ["acm", "ACM"].iter().map(|e| lp.with_extension(e)).find(|a| a.exists()) else {
                run.skip("no ACM");
                continue;
            };
            let original = Lip::parse(&std::fs::read(&lp).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", lp.display()));
            if original.phonemes.iter().any(|&c| c >= lip::PHONEME_COUNT) {
                run.skip("original has codes of 42 and more");
                continue;
            }
            let audio = read(&std::fs::read(&acm).unwrap(), &ReadOptions::default())
                .unwrap_or_else(|e| panic!("{}: {e}", acm.display()))
                .audio;
            if lip::fit(original.last(), &audio) != lip::Fit::Matches {
                run.skip("original does not fit its sound");
                continue;
            }
            lines.push((name, original, audio));
        }
        for (i, (name, original, audio)) in lines.iter().enumerate() {
            run.lines += 1;
            let words = text.get(name).map(|t| tokens(t));
            let words = words.as_deref();
            let judge = |l: &Lip| quality(l, audio, words);
            add_loudness(&mut run, original, audio);
            run.add("interplay", &speaker, judge(original));
            run.add("ours, text", &speaker, judge(&generate(name, audio, words, &p).0));
            run.add("ours, audio", &speaker, judge(&generate(name, audio, None, &p).0));
            match std::fs::read(wav2lip.join(format!("{name}.lip"))) {
                Ok(b) => {
                    let w = Lip::parse(&b).unwrap_or_else(|e| panic!("wav2lip {name}: {e}"));
                    if lip::fit(w.last(), audio) == lip::Fit::Matches {
                        run.add("wav2lip", &speaker, judge(&w));
                    } else {
                        run.skip("wav2lip's does not fit");
                    }
                }
                Err(_) => run.skip("no wav2lip file"),
            }
            // The speaker's next line's lip file, stretched to this one.
            if lines.len() > 1 {
                let mut other = lines[(i + 1) % lines.len()].1.clone();
                other.rescale(original.last());
                run.add("other line", &speaker, judge(&other));
            }
            for (set, ms) in [
                ("100 ms early", -100),
                ("100 ms late", 100),
                ("200 ms late", 200),
                ("500 ms late", 500),
                ("1 s late", 1000),
            ] {
                run.add(set, &speaker, judge(&late(original, ms, audio)));
            }
            run.add("shuffled", &speaker, judge(&shuffled(original, &mut seed)));
            run.add("one open shape", &speaker, judge(&one_open_shape(original)));
        }
    }
    run
}

fn print_report(run: &Run) {
    println!("{} lines; skipped: {:?}", run.lines, run.skipped);
    println!("head frame: median loudness rank of its speech frames (frames)");
    for (frame, v) in &run.loudness {
        let mut v = v.clone();
        v.sort_by(f64::total_cmp);
        println!("  {frame}: {:.3} ({})", v[v.len() / 2], v.len());
    }
    let kinds = ["shut", "offset", "closures", "open quiet", "movement", "held"];
    println!("share of files with each problem:");
    println!(
        "  {:15} {:>6}  {}",
        "set",
        "files",
        kinds.map(|k| format!("{k:>10}")).join("")
    );
    for set in SETS {
        let Some(dirs) = run.judged.get(set) else { continue };
        let n: usize = dirs.values().map(Vec::len).sum();
        let rates: String = kinds
            .iter()
            .map(|k| format!("{:9.1}%", 100.0 * run.rate(set, &[k])))
            .collect();
        println!("  {set:15} {n:6}  {rates}");
    }
    println!("figures, least | 5th / 50th / 95th percentile | most (files):");
    for set in SETS {
        let Some(dirs) = run.judged.get(set) else { continue };
        let files: Vec<&Quality> = dirs.values().flatten().collect();
        let fig = |f: &dyn Fn(&Quality) -> Option<f64>| spread(files.iter().map(|q| f(q)));
        println!("  {set}");
        println!("    shut while loud   {}", fig(&|q| q.shut_while_loud));
        println!("    closed while loud {}", fig(&|q| q.closed_while_loud));
        println!("    alignment         {}", fig(&|q| q.alignment));
        println!("    openness          {}", fig(&|q| q.openness));
        println!(
            "    offset gain       {}",
            fig(&|q| Some(q.offset?.alignment - q.alignment?))
        );
        println!(
            "    far offset gain   {}",
            fig(&|q| Some(q.far_offset?.alignment - q.alignment?))
        );
        println!(
            "    over chance       {}",
            fig(&|q| {
                let o = q.offset.filter(|o| o.chance.is_some()).or(q.far_offset)?;
                Some(o.alignment - o.chance?)
            })
        );
        println!("    open while quiet  {}", fig(&|q| q.open_while_quiet));
        println!("    movement          {}", fig(&|q| q.movement));
        println!("    longest hold      {}", fig(&|q| q.longest_hold));
        println!("    closures ratio    {}", fig(&|q| q.text?.ratio()));
        let offsets: Vec<f64> = files
            .iter()
            .flat_map(|q| q.problems())
            .filter_map(|p| match p {
                QualityProblem::Offset { ms } => Some(ms),
                QualityProblem::Shut { .. }
                | QualityProblem::Closures(_)
                | QualityProblem::OpenWhileQuiet { .. }
                | QualityProblem::Movement { .. }
                | QualityProblem::Held { .. } => None,
            })
            .collect();
        if !offsets.is_empty() {
            let bucket = |lo: f64, hi: f64| offsets.iter().filter(|m| (lo..hi).contains(m)).count();
            println!(
                "    offsets reported: {} early, {} 60-140 ms late, {} 150-300 ms late, {} 310-600 ms late",
                bucket(-601.0, 0.0),
                bucket(0.0, 141.0),
                bucket(141.0, 301.0),
                bucket(301.0, 601.0)
            );
        }
    }
    println!("directory grades: directories graded, then those warned about, per problem:");
    for set in SETS {
        let Some(dirs) = run.judged.get(set) else { continue };
        let grades: Vec<_> = dirs.values().filter_map(|files| grade(files)).collect();
        let mut warned: BTreeMap<&str, usize> = BTreeMap::new();
        for g in &grades {
            for p in g.problems() {
                *warned.entry(set_kind(&p)).or_default() += 1;
            }
        }
        let range = |f: &dyn Fn(&ipsdoc::lipqual::SetGrade) -> Option<f64>| {
            let v: Vec<f64> = grades.iter().filter_map(f).collect();
            let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            format!("{lo:.2}-{hi:.2}")
        };
        println!(
            "  {set:15} {:2}  {warned:?}\n    alignment {} openness {} shut {} open quiet {} movement {} closures {}",
            grades.len(),
            range(&|g| g.alignment),
            range(&|g| g.openness),
            range(&|g| Some(g.shut)),
            range(&|g| g.open_while_quiet),
            range(&|g| g.movement),
            range(&|g| g.closures),
        );
    }
}

#[test]
#[ignore = "needs Fallout 2's speech, the Unofficial Patch's dialog and wav2lip's output; see the file header"]
fn lipqual_limits_separate_good_lip_files_from_bad() {
    let speech = env_dir("IPSDOC_LIPS");
    let text = voiced_lines(&env_dir("IPSDOC_MSG"));
    let wav2lip = env_dir("IPSDOC_LIPS_WAV2LIP");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&speech)
        .unwrap()
        .map(|d| d.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let run = score(&dirs, &text, &wav2lip);
    print_report(&run);
    assert!(run.lines >= 900, "only {} lines judged", run.lines);

    // Bounds set on the run that chose the limits (Interplay: offsets 0.4%, closures 2.8%, open while quiet 2.1%, held
    // 3.1%), then frozen.
    let at_most = |set: &str, kinds: &[&str], bound: f64| {
        let r = run.rate(set, kinds);
        assert!(r <= bound, "{set}: {kinds:?} on {r}, over {bound}");
    };
    let at_least = |set: &str, kinds: &[&str], bound: f64| {
        let r = run.rate(set, kinds);
        assert!(r >= bound, "{set}: {kinds:?} on {r}, under {bound}");
    };
    at_most("interplay", &["shut", "movement"], 0.0);
    at_most("interplay", &["offset"], 0.01);
    at_most("interplay", &["closures"], 0.04);
    at_most("interplay", &["open quiet"], 0.03);
    at_most("interplay", &["held"], 0.04);
    at_most("ours, text", &["shut", "offset", "open quiet", "movement"], 0.01);
    at_most("ours, audio", &["shut", "offset", "open quiet", "movement"], 0.01);
    at_least("wav2lip", &["shut"], 0.9);
    at_least("wav2lip", &["held"], 0.6);
    // A mouth late shows from 45 ms, early from 125 ms: the one is looked for, the other not.
    at_least("100 ms late", &["offset"], 0.15);
    at_most("100 ms early", &["offset"], 0.03);
    at_least("200 ms late", &["offset"], 0.5);
    at_least("500 ms late", &["offset"], 0.5);
    // Files with too little quiet are judged by their stillness, and only lines under a second of speech escape.
    at_least("one open shape", &["open quiet", "movement"], 0.95);
    // Every directory of Interplay's and ours passes; every directory of a spoiled set is warned about.
    for set in ["interplay", "ours, text"] {
        assert!(
            run.set_problems(set).iter().all(Vec::is_empty),
            "{set}: {:?}",
            run.set_problems(set)
        );
    }
    let near = ["100 ms early", "100 ms late"];
    for set in SETS.iter().skip(3).filter(|s| !near.contains(s)) {
        let problems = run.set_problems(set);
        assert!(
            problems.len() >= 10 && problems.iter().all(|p| !p.is_empty()),
            "{set}: {problems:?}"
        );
    }
}
