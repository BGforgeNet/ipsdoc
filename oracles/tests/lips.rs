//! Lip-sync quality against Fallout 2's own lip files: ours from the line's text, ours from the sound alone, and
//! wav2lip's, each scored against the original over 10 ms frames of head frames.
//!
//! Measures: agreement (share of frames showing the original's head frame; higher is better); loud frames, within
//! 15 dB of the line's 95th-percentile level, shown silent (lower); changes per second and median hold, as distance
//! from the original's (lower). Speaker directories sorted by name alternate between a tuning half (even), used to
//! choose `lipgen::Params`, and a held-out half (odd), the only one asserted on.
//!
//! Run with `--ignored` and `IPSDOC_LIPS=<dir>` (`sound/speech` extracted from `master.dat`: a directory per speaker
//! holding each line's ACM and LIP), `IPSDOC_MSG=<dir>` (the English `data/text/english/dialog` of the Unofficial
//! Patch) and `IPSDOC_LIPS_WAV2LIP=<dir>` (wav2lip's lip file per line, `docs/development.md`).
//! `IPSDOC_LIPS_LIMIT=N` shortens the run (`docs/development.md` says how).
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
use ipsdoc::lip::{self, Lip};
use ipsdoc::lipgen::{Params, generate};
use ipsdoc::lipqual::head_frames;
use ipsdoc::pcm::Audio;

#[derive(Default)]
struct Score {
    frames: u64,
    agree: u64,
    loud: u64,
    loud_silent: u64,
    changes: u64,
    holds: Vec<u32>,
}

impl Score {
    fn add(&mut self, series: &[u8], reference: &[u8], loud: &[bool]) {
        let mut run = 1;
        for i in 0..series.len() {
            self.frames += 1;
            self.agree += u64::from(series[i] == reference[i]);
            if loud[i] {
                self.loud += 1;
                self.loud_silent += u64::from(series[i] == 0);
            }
            if i > 0 && series[i] != series[i - 1] {
                self.changes += 1;
                self.holds.push(run);
                run = 1;
            } else if i > 0 {
                run += 1;
            }
        }
    }

    fn agreement(&self) -> f64 {
        self.agree as f64 / self.frames.max(1) as f64
    }

    fn loud_silent(&self) -> f64 {
        self.loud_silent as f64 / self.loud.max(1) as f64
    }

    fn changes_per_second(&self) -> f64 {
        self.changes as f64 / (self.frames.max(1) as f64 / 100.0)
    }

    fn median_hold_ms(&mut self) -> f64 {
        self.holds.sort_unstable();
        f64::from(self.holds.get(self.holds.len() / 2).copied().unwrap_or(0) * 10)
    }
}

fn env_dir(key: &str) -> PathBuf {
    PathBuf::from(std::env::var(key).unwrap_or_else(|_| panic!("set {key}; see this file's header")))
}

fn voiced_lines(dialog: &Path) -> HashMap<String, String> {
    let mut text = HashMap::new();
    for e in std::fs::read_dir(dialog).unwrap() {
        let p = e.unwrap().path();
        let entries =
            ipsdoc::msg::parse(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        for x in entries.into_iter().filter(|x| !x.sound.is_empty()) {
            text.entry(x.sound.to_ascii_lowercase()).or_insert(x.text);
        }
    }
    text
}

/// A candidate's lip file must be valid and fit its sound before it is scored.
fn usable(name: &str, who: &str, l: &Lip, audio: &Audio, failures: &mut Vec<String>) -> bool {
    let errors: Vec<String> = l
        .validate(None)
        .iter()
        .filter(|p| p.severity() == lip::Severity::Error)
        .map(ToString::to_string)
        .collect();
    let fit = lip::fit(l.last(), audio);
    if !errors.is_empty() || fit != lip::Fit::Matches {
        failures.push(format!("{who} {name}: {errors:?}, {fit:?}"));
        return false;
    }
    true
}

#[derive(Default)]
struct Half {
    original: Score,
    text: Score,
    audio: Score,
    wav2lip: Score,
    lines: u64,
    skipped: BTreeMap<&'static str, u64>,
    failures: Vec<String>,
}

fn score_half(dirs: &[PathBuf], text: &HashMap<String, String>, wav2lip: &Path, limit: usize) -> Half {
    let mut h = Half::default();
    let p = Params::default();
    let mut lips: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| std::fs::read_dir(d).unwrap().map(|f| f.unwrap().path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lip")))
        .collect();
    lips.sort();
    for lp in lips.into_iter().take(limit) {
        let name = lp.file_stem().unwrap().to_string_lossy().to_ascii_lowercase();
        let Some(acm) = ["acm", "ACM"].iter().map(|e| lp.with_extension(e)).find(|a| a.exists()) else {
            *h.skipped.entry("no ACM").or_default() += 1;
            continue;
        };
        let original = Lip::parse(&std::fs::read(&lp).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", lp.display()));
        if original.phonemes.iter().any(|&c| c >= lip::PHONEME_COUNT) {
            *h.skipped.entry("original has codes of 42 and more").or_default() += 1;
            continue;
        }
        let Some(line) = text.get(&name) else {
            *h.skipped.entry("no MSG line").or_default() += 1;
            continue;
        };
        let Ok(w2l_bytes) = std::fs::read(wav2lip.join(format!("{name}.lip"))) else {
            *h.skipped.entry("no wav2lip file").or_default() += 1;
            continue;
        };
        let audio = read(&std::fs::read(&acm).unwrap(), &ReadOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", acm.display()))
            .audio;
        // A few original lip files do not end with their sound: no exact reference.
        if lip::fit(original.last(), &audio) != lip::Fit::Matches {
            *h.skipped.entry("original does not fit its sound").or_default() += 1;
            continue;
        }
        let ours_text = generate(&name, &audio, Some(&tokens(line)), &p);
        let ours_audio = generate(&name, &audio, None, &p);
        let w2l = Lip::parse(&w2l_bytes).unwrap_or_else(|e| panic!("wav2lip {name}: {e}"));
        // Ours must always be valid and fit.
        let ours_ok = usable(&name, "ours (text)", &ours_text, &audio, &mut h.failures)
            & usable(&name, "ours (audio)", &ours_audio, &audio, &mut h.failures);
        // wav2lip's own markers go backwards at times: only its length is held to the sound.
        if !ours_ok || lip::fit(w2l.last(), &audio) != lip::Fit::Matches {
            *h.skipped.entry("a candidate did not fit its sound").or_default() += 1;
            continue;
        }
        h.lines += 1;
        let ch = usize::from(audio.channels.max(1));
        let n = audio.frames() * 100 / audio.rate as usize;
        let at = |f: usize| ((f as u64 * u64::from(audio.rate) + 50) / 100) as usize * ch;
        let mono = audio.to_f32();
        let db: Vec<f32> = (0..n)
            .map(|f| {
                let s = &mono[at(f)..at(f + 1).min(mono.len())];
                10.0 * (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32 + 1e-12).log10()
            })
            .collect();
        let mut sorted = db.clone();
        sorted.sort_by(f32::total_cmp);
        let top = sorted[((sorted.len() as f32 - 1.0) * 0.95) as usize];
        let loud: Vec<bool> = db.iter().map(|&d| d > top - 15.0).collect();
        let r = head_frames(&original, &audio, n);
        h.original.add(&r, &r, &loud);
        h.text.add(&head_frames(&ours_text, &audio, n), &r, &loud);
        h.audio.add(&head_frames(&ours_audio, &audio, n), &r, &loud);
        h.wav2lip.add(&head_frames(&w2l, &audio, n), &r, &loud);
    }
    h
}

fn print_table(name: &str, h: &mut Half) {
    println!("{name}: {} lines scored; skipped: {:?}", h.lines, h.skipped);
    for (who, s) in [
        ("original", &mut h.original),
        ("ours, text", &mut h.text),
        ("ours, audio", &mut h.audio),
        ("wav2lip", &mut h.wav2lip),
    ] {
        println!(
            "  {who:12} agreement {:5.1}%  loud shown silent {:5.1}%  changes/s {:5.2}  median hold {:4.0} ms",
            100.0 * s.agreement(),
            100.0 * s.loud_silent(),
            s.changes_per_second(),
            s.median_hold_ms()
        );
    }
}

#[test]
#[ignore = "needs Fallout 2's speech, the Unofficial Patch's dialog and wav2lip's output; see the file header"]
fn lip_sync_beats_wav2lip() {
    let speech = env_dir("IPSDOC_LIPS");
    let text = voiced_lines(&env_dir("IPSDOC_MSG"));
    let wav2lip = env_dir("IPSDOC_LIPS_WAV2LIP");
    let limit = std::env::var("IPSDOC_LIPS_LIMIT").map_or(usize::MAX, |v| v.parse().expect("IPSDOC_LIPS_LIMIT"));
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&speech)
        .unwrap()
        .map(|d| d.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let (tune, held): (Vec<_>, Vec<_>) = dirs.into_iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let strip = |v: Vec<(usize, PathBuf)>| v.into_iter().map(|(_, d)| d).collect::<Vec<_>>();
    let mut t = score_half(&strip(tune), &text, &wav2lip, limit);
    print_table("tuning half", &mut t);
    let mut h = score_half(&strip(held), &text, &wav2lip, limit);
    print_table("held-out half", &mut h);
    assert!(
        t.failures.is_empty() && h.failures.is_empty(),
        "{:?}",
        [&t.failures, &h.failures]
    );
    assert!(h.lines >= 100, "only {} held-out lines scored", h.lines);

    // Interplay's movement is the reference: our text-made files, from the words by other means, change shape at
    // nearly the same rate (7.7 to 7.9 a second against 6.8 to 7.0), between speech's syllable and phoneme rates.
    let (ref_changes, ref_hold) = (h.original.changes_per_second(), h.original.median_hold_ms());
    let w2l_changes = (h.wav2lip.changes_per_second() - ref_changes).abs();
    let w2l_hold = (h.wav2lip.median_hold_ms() - ref_hold).abs();
    let w2l_agreement = h.wav2lip.agreement();
    let w2l_silent = h.wav2lip.loud_silent();
    // Margins set on the first held-out run (text 38.2% to wav2lip's 16.1%, audio 33.9%), then frozen.
    for (who, s, factor) in [("text", &mut h.text, 1.8), ("audio", &mut h.audio, 1.5)] {
        assert!(
            s.agreement() >= factor * w2l_agreement,
            "{who}: agreement {} vs wav2lip {w2l_agreement}",
            s.agreement()
        );
        assert!(
            s.loud_silent() < 0.05 && s.loud_silent() < w2l_silent,
            "{who}: loud shown silent {}",
            s.loud_silent()
        );
        let (c, hl) = (
            (s.changes_per_second() - ref_changes).abs(),
            (s.median_hold_ms() - ref_hold).abs(),
        );
        assert!(c < w2l_changes, "{who}: changes/s off by {c} vs wav2lip {w2l_changes}");
        assert!(hl < w2l_hold, "{who}: hold off by {hl} ms vs wav2lip {w2l_hold}");
    }
    // No text-against-audio bound on loud frames shown silent: m, b and p placed from the words close the lips in
    // voiced speech, and Interplay's own files show 3% where both of ours show under 1%, so lower is not better there.
    assert!(
        h.text.agreement() > h.audio.agreement(),
        "text {} vs audio {}",
        h.text.agreement(),
        h.audio.agreement()
    );
}
