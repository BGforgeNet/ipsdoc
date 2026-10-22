//! Other encoders and decoders against ours, on the fixtures (`cargo run -p oracles --bin fixtures`).
//!
//! `IPSDOC_TOOLS` names a directory holding `fixtures/<name>.wav` and `ours/<name>.acm` (our lossless encoding of
//! each), and any of: `snd2acm/<name>.acm` and `acmtools/<name>.acm` (those encoders' output for each fixture),
//! `acmtools-decoded/<name>.wav` (AcmTools' decoding of `ours/<name>.acm`). `IPSDOC_TOOLS_REQUIRE` lists the
//! directories that must be there (comma-separated), so a tool that silently produced nothing fails the run.

use std::path::{Path, PathBuf};

use oracles::Oracle;

mod common;
use common::disagreements;

fn samples_of(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    ipsdoc::input::read(&bytes, &Default::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .audio
        .to_i16()
}

#[test]
#[ignore = "needs IPSDOC_TOOLS (see the module documentation)"]
fn other_tools_agree_with_ours() {
    let root = PathBuf::from(std::env::var("IPSDOC_TOOLS").expect("IPSDOC_TOOLS names the tools directory"));
    let required = std::env::var("IPSDOC_TOOLS_REQUIRE").unwrap_or_default();
    let mut names: Vec<String> = std::fs::read_dir(root.join("fixtures"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no fixtures");
    let mut failures = Vec::new();
    let mut judged = Vec::new();
    for dir in ["snd2acm", "acmtools", "acmtools-decoded"] {
        let present = root.join(dir).is_dir();
        if required.split(',').any(|r| r == dir) && !present {
            failures.push(format!("{dir}: required, missing"));
        }
        if present {
            judged.push(dir);
        }
    }
    for name in &names {
        let fixture = samples_of(&root.join("fixtures").join(format!("{name}.wav")));
        // Ours, as a baseline: lossless.
        let ours = std::fs::read(root.join("ours").join(format!("{name}.acm"))).unwrap();
        if ipsdoc::acm::decode(&ours).unwrap().samples != fixture {
            failures.push(format!("{name}: our lossless file does not decode to the fixture"));
        }
        // Other encoders' files decode in ours exactly as in the engines.
        for dir in ["snd2acm", "acmtools"].into_iter().filter(|d| judged.contains(d)) {
            let path = root.join(dir).join(format!("{name}.acm"));
            let Ok(acm) = std::fs::read(&path) else {
                failures.push(format!("{}: missing", path.display()));
                continue;
            };
            let h = ipsdoc::acm::Header::parse(&acm).unwrap();
            eprintln!(
                "{dir}/{name}: {} channels, {} Hz, levels {}, rows {}",
                h.channels, h.rate, h.levels, h.rows
            );
            match Oracle::Fallout2Ce.decode(&acm) {
                Ok(reference) => failures.extend(
                    disagreements(&acm, &[Oracle::GemRb], &reference)
                        .into_iter()
                        .map(|f| format!("{dir}/{name}: {f}")),
                ),
                Err(e) => failures.push(format!("{dir}/{name}: {e}")),
            }
        }
        // Another decoder reads our lossless files exactly.
        if judged.contains(&"acmtools-decoded") {
            let path = root.join("acmtools-decoded").join(format!("{name}.wav"));
            if !path.exists() {
                failures.push(format!("{}: missing", path.display()));
            } else if samples_of(&path) != fixture {
                failures.push(format!(
                    "{}: AcmTools reads our lossless file differently",
                    path.display()
                ));
            }
        }
    }
    eprintln!("judged: {judged:?} over {} fixtures", names.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
