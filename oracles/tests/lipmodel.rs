//! Fits `src/lipmodel.rs`, the per-sound model `lipalign` places a line's sounds with, and fails when the committed
//! table differs, printing the fitted one to put in its place.
//!
//! The model comes from forced alignments of Fallout 2's voiced lines by the Montreal Forced Aligner (its English
//! ARPAbet acoustic model and dictionary): per sound, the mean and variance of each `lipalign::features` value over
//! the frames MFA gives it, the mean and spread of the log of its lengths, and how often each sound follows it.
//!
//! Run with `--ignored`, `IPSDOC_LIPS=<dir>` (`sound/speech` extracted from `master.dat`: a directory per speaker
//! holding each line's ACM) and `IPSDOC_MFA=<dir>` (MFA's TextGrid per line, `<speaker>/<line>.TextGrid`).
#![expect(
    clippy::unwrap_used,
    reason = "test harness code outside #[test] fns: a panic is the failure report"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ipsdoc::g2p::Arpabet;
use ipsdoc::input::{ReadOptions, read};
use ipsdoc::lipalign::{CLASSES, FEATURES, NOISE, SILENCE, class_name, features};
use ipsdoc::speech::{SpeechParams, analyse};

/// A class with fewer frames than this takes the model pooled over every class.
const MIN_FRAMES: f64 = 50.0;
/// A class with fewer sounds than this takes a typical length of e^2 frames, spread 0.5.
const MIN_SOUNDS: f64 = 20.0;
/// The least variance per feature: a few dB for the loudness ones, about a tenth of their range for zero crossings and
/// the low band, and 0.1 for the cepstrum and its slope.
fn var_floor(k: usize) -> f64 {
    [4.0, 4.0, 0.0004, 0.002, 4.0].get(k).copied().unwrap_or(0.1)
}

/// The phones tier of an MFA TextGrid: (start, end, label), an empty label for silence.
fn phones(textgrid: &str) -> Vec<(f64, f64, String)> {
    let (mut tier, mut out) = (String::new(), Vec::new());
    let (mut x0, mut x1) = (0.0, 0.0);
    for line in textgrid.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("name = ") {
            tier = v.trim_matches('"').to_owned();
        } else if let Some(v) = line.strip_prefix("xmin = ") {
            x0 = v.parse().unwrap();
        } else if let Some(v) = line.strip_prefix("xmax = ") {
            x1 = v.parse().unwrap();
        } else if let Some(v) = line.strip_prefix("text = ")
            && tier == "phones"
        {
            out.push((x0, x1, v.trim_matches('"').to_owned()));
        }
    }
    out
}

/// The model class of an MFA label: a sound without its stress digit, `spn` (spoken noise) or silence.
fn class(label: &str) -> usize {
    match label {
        "" | "sil" => SILENCE,
        "spn" => NOISE,
        l => Arpabet::from_symbol(l.trim_end_matches(['0', '1', '2'])).unwrap_or_else(|| panic!("label {l}")) as usize,
    }
}

/// `v` to `digits` significant digits.
fn round(v: f64, digits: i32) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let scale = 10f64.powi(digits - 1 - v.abs().log10().floor() as i32);
    (v * scale).round() / scale
}

/// `v` as a literal of four significant digits, or six where four would read as a mathematical constant to clippy
/// (a prefix of its digits, or it rounded to as many places), which rejects such a literal.
fn literal(v: f64) -> String {
    use std::f64::consts::*;
    const NEAR: [f64; 19] = [
        E,
        FRAC_1_PI,
        FRAC_1_SQRT_2,
        FRAC_2_PI,
        FRAC_2_SQRT_PI,
        FRAC_PI_2,
        FRAC_PI_3,
        FRAC_PI_4,
        FRAC_PI_6,
        FRAC_PI_8,
        LN_10,
        LN_2,
        LOG10_E,
        LOG2_E,
        LOG2_10,
        LOG10_2,
        PI,
        SQRT_2,
        TAU,
    ];
    let short = format!("{:?}", round(v, 4) as f32);
    let digits = short.trim_start_matches('-');
    let near =
        |c: &f64| c.to_string().starts_with(digits) || digits == format!("{c:.*}", digits.len().saturating_sub(2));
    if digits.len() > 2 && NEAR.iter().any(near) {
        format!("{:?}", round(v, 6) as f32)
    } else {
        short
    }
}

#[test]
#[ignore = "needs Fallout 2's speech and MFA's alignments of it; see the file header"]
fn lipmodel_is_fitted_to_the_alignments() {
    let speech = PathBuf::from(std::env::var("IPSDOC_LIPS").expect("set IPSDOC_LIPS; see this file's header"));
    let mfa = PathBuf::from(std::env::var("IPSDOC_MFA").expect("set IPSDOC_MFA; see this file's header"));
    let (mut n, mut s, mut ss) = ([0.0; CLASSES], [[0.0; FEATURES]; CLASSES], [[0.0; FEATURES]; CLASSES]);
    let (mut dn, mut ds, mut dss) = ([0.0; CLASSES], [0.0; CLASSES], [0.0; CLASSES]);
    let mut pairs = [[0.0f64; CLASSES]; CLASSES];
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&mfa).unwrap().map(|d| d.unwrap().path()).collect();
    dirs.sort();
    let mut lines = 0;
    for dir in dirs.iter().filter(|d| d.is_dir()) {
        let speaker = dir.file_name().unwrap();
        let mut grids: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|g| g.unwrap().path()).collect();
        grids.sort();
        for g in grids.iter().filter(|g| g.extension().is_some_and(|e| e == "TextGrid")) {
            let stem = g.file_stem().unwrap().to_string_lossy();
            let acm = ["acm", "ACM"]
                .iter()
                .map(|e| speech.join(speaker).join(format!("{stem}.{e}")))
                .find(|p| p.exists())
                .unwrap_or_else(|| panic!("no sound for {}", g.display()));
            let audio = read(&std::fs::read(&acm).unwrap(), &ReadOptions::default())
                .unwrap()
                .audio;
            let x = features(&analyse(&audio, &SpeechParams::default()));
            let mut before = None;
            for (start, end, l) in phones(&std::fs::read_to_string(g).unwrap()) {
                let c = class(&l);
                if let Some(b) = before.replace(c) {
                    pairs[b][c] += 1.0;
                }
                let (i0, i1) = (
                    (start * 100.0).round() as usize,
                    ((end * 100.0).round() as usize).min(x.len()),
                );
                for f in &x[i0.min(i1)..i1] {
                    n[c] += 1.0;
                    for k in 0..FEATURES {
                        s[c][k] += f64::from(f[k]);
                        ss[c][k] += f64::from(f[k]) * f64::from(f[k]);
                    }
                }
                let d = ((end - start) * 100.0).max(1.0).ln();
                dn[c] += 1.0;
                ds[c] += d;
                dss[c] += d * d;
            }
            lines += 1;
        }
    }
    assert!(lines >= 900, "only {lines} aligned lines under {}", mfa.display());
    let pooled = |sums: &[[f64; FEATURES]; CLASSES], k: usize| sums.iter().map(|x| x[k]).sum::<f64>();
    let all: f64 = n.iter().sum();
    let mut table = String::from(
        "//! The per-sound model `lipalign` places sounds with, written by `oracles/tests/lipmodel.rs` (which says how\n\
         //! it was fitted); do not edit by hand. Rows follow `lipalign`'s classes: the ARPAbet sounds, a vocal noise,\n\
         //! silence. Features follow `lipalign::features`.\n\n\
         use crate::lipalign::{CLASSES, FEATURES};\n\n",
    );
    let (mut mean, mut var) = (String::new(), String::new());
    for c in 0..CLASSES {
        let (cn, cs, css) = if n[c] >= MIN_FRAMES {
            (n[c], s[c], ss[c])
        } else {
            (
                all,
                std::array::from_fn(|k| pooled(&s, k)),
                std::array::from_fn(|k| pooled(&ss, k)),
            )
        };
        let mu: [f64; FEATURES] = std::array::from_fn(|k| cs[k] / cn);
        let v: [f64; FEATURES] = std::array::from_fn(|k| (css[k] / cn - mu[k] * mu[k]).max(var_floor(k)));
        let row = |x: [f64; FEATURES]| x.map(literal).join(", ");
        let _ = writeln!(mean, "    [{}], // {}", row(mu), class_name(c));
        let _ = writeln!(var, "    [{}], // {}", row(v), class_name(c));
    }
    let mut dur = String::new();
    for c in 0..CLASSES {
        let (mu, sd) = if dn[c] >= MIN_SOUNDS {
            let mu = ds[c] / dn[c];
            (mu, (dss[c] / dn[c] - mu * mu).max(0.01).sqrt())
        } else {
            (2.0, 0.5)
        };
        let _ = writeln!(dur, "    ({}, {}), // {}", literal(mu), literal(sd), class_name(c));
    }
    let mut next = String::new();
    for (c, row) in pairs.iter().enumerate() {
        // Add-one smoothing: a pair the alignments never show stays possible.
        let total = row.iter().sum::<f64>() + CLASSES as f64;
        let costs = row.map(|n| literal(-((n + 1.0) / total).ln()));
        let _ = writeln!(next, "    [{}], // {}", costs.join(", "), class_name(c));
    }
    let _ = write!(
        table,
        "/// Per class, each feature's mean.\n#[rustfmt::skip]\npub(crate) const MEAN: [[f32; FEATURES]; CLASSES] = [\n{mean}];\n\n\
         /// Per class, each feature's variance.\n#[rustfmt::skip]\npub(crate) const VAR: [[f32; FEATURES]; CLASSES] = [\n{var}];\n\n\
         /// Per class, the mean and standard deviation of the natural log of its length in 10 ms frames.\n#[rustfmt::skip]\n\
         pub(crate) const DUR: [(f32, f32); CLASSES] = [\n{dur}];\n\n\
         /// Per class, the cost (-ln of the chance) of each class following it.\n#[rustfmt::skip]\n\
         pub(crate) const NEXT: [[f32; CLASSES]; CLASSES] = [\n{next}];\n"
    );
    let committed = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lipmodel.rs")).unwrap();
    if committed != table {
        println!("{table}");
        panic!(
            "src/lipmodel.rs differs from the model fitted to {lines} lines: put the table printed above in its place"
        );
    }
}
