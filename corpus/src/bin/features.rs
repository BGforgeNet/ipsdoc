//! `features <manifest.tsv>`: channel-detector features of the labelled sets in a corpus, as the TSV the detector's
//! test reads (`set, label, name, a, b, w`). Each unique stream with a stereo ACM header is labelled by how its game
//! plays it:
//!
//! - `fallout-speech`, `fallout-sfx`: bare ACMs under `speech/` or `sfx/` (Fallout's layout), which the Fallout engine
//!   plays mono whatever the header says; effects with an odd sample count are left out (the count alone decides).
//! - `music`: ACMs and WAVCs under `music/`, played stereo by both engines.
//! - `ie-sfx`: other WAVCs, which the Infinity Engine plays as their WAVC header says.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::process::ExitCode;

use corpus::Row;
use ipsdoc::acm;
use ipsdoc::analysis::Features;
// Pipe-safe: std's panic once a reader has gone (`features manifest.tsv | head`).
use ipsdoc::{eprintln, println};

fn run(manifest: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(manifest).map_err(|e| format!("{manifest}: {e}"))?;
    let mut seen = HashSet::new();
    println!("set\tlabel\tname\ta\tb\tw");
    for line in text.lines() {
        let row = Row::parse(line).map_err(|e| e.to_string())?;
        let (container, path, origin) = (row.container, row.path, row.origin);
        if container != "acm" && container != "wavc" {
            continue;
        }
        let lower = path.to_lowercase().replace('\\', "/");
        let bytes = row.read().map_err(|e| format!("{path}: {e}"))?;
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        if !seen.insert(h.finish()) {
            continue;
        }
        let (stream, wavc_channels) = if container == "wavc" {
            let header = ipsdoc::wavc::Header::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
            (
                ipsdoc::wavc::acm_of(&bytes).map_err(|e| format!("{path}: {e}"))?,
                Some(header.channels),
            )
        } else {
            (bytes.as_slice(), None)
        };
        let Ok(header) = acm::Header::parse(stream) else {
            continue;
        };
        if header.channels != 2 {
            continue;
        }
        let (set, label) = if lower.contains("/music/") {
            ("music", "stereo")
        } else if container == "acm" && lower.contains("/speech/") {
            ("fallout-speech", "mono")
        } else if container == "acm" && lower.contains("/sfx/") && header.samples % 2 == 0 {
            ("fallout-sfx", "mono")
        } else if let Some(c) = wavc_channels {
            ("ie-sfx", if c == 1 { "mono" } else { "stereo" })
        } else {
            continue;
        };
        let decoded = acm::decode(stream).map_err(|e| format!("{path}: {e}"))?;
        let Some(feat) = Features::of(&decoded.samples) else {
            continue;
        };
        // The file name, or for an archive entry the archive's name and the entry's index (`#n`).
        let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
        let name = match origin {
            "-" => base(&lower),
            o if o.starts_with('#') => format!("{}{o}", base(&lower)),
            o => base(o),
        };
        println!("{set}\t{label}\t{name}\t{:.3}\t{:.3}\t{:.3}", feat.a, feat.b, feat.w);
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [manifest] = args.as_slice() else {
        eprintln!("usage: features <manifest.tsv>");
        return ExitCode::from(2);
    };
    match run(manifest) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("features: {e}");
            ExitCode::FAILURE
        }
    }
}
