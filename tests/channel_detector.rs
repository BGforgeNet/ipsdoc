//! The channel detector against labelled real files, as feature vectors (`tests/data/channel-features.tsv`, made by
//! the `features` tool from a corpus of BG1EE, BG2EE, ToB, Fallout 2 and mods; no game data is stored).

use std::collections::{BTreeMap, BTreeSet};

use ipsdoc::analysis::{Features, Verdict};

struct Row {
    set: &'static str,
    mono: bool,
    name: &'static str,
    features: Features,
}

fn rows() -> Vec<Row> {
    include_str!("data/channel-features.tsv")
        .lines()
        .skip(1)
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let num = |i: usize| f[i].parse::<f64>().unwrap_or_else(|e| panic!("{line}: {e}"));
            Row {
                set: f[0],
                mono: f[1] == "mono",
                name: f[2],
                features: Features {
                    a: num(3),
                    b: num(4),
                    w: num(5),
                },
            }
        })
        .collect()
}

/// Bare-ACM sets, where the detector decides: every file is read as its game plays it except these. Fallout's
/// ambient tracks are, in effect, 44.1 kHz mono stored as 22050 Hz stereo (the engine plays them right as stereo);
/// `engineon` is an effect the engine plays mono whose two halves differ too much.
#[test]
fn bare_acm_sets_are_read_as_their_engine_plays_them() {
    let mut judged: BTreeMap<&str, usize> = BTreeMap::new();
    let mut misses = BTreeSet::new();
    for r in rows().iter().filter(|r| r.set != "ie-sfx") {
        *judged.entry(r.set).or_default() += 1;
        let mono = match r.features.verdict() {
            Verdict::InterleavedMono => true,
            Verdict::Stereo => false,
            // The (stereo) header stays.
            Verdict::Undecidable => false,
        };
        if mono != r.mono {
            misses.insert(r.name);
        }
    }
    assert_eq!(
        misses,
        BTreeSet::from(["alarm1.acm", "alarm2.acm", "engineon.acm", "wind1.acm", "wind2.acm"])
    );
    for set in ["fallout-speech", "fallout-sfx", "music"] {
        assert!(judged.get(set).copied().unwrap_or(0) > 100, "{set}: {judged:?}");
    }
}

/// Why agreeing WAVC headers are trusted: this many stereo Infinity Engine effects read as interleaved mono, yet the
/// engine plays them as stereo.
#[test]
fn some_stereo_wavc_effects_read_as_interleaved_mono() {
    let read_mono = rows()
        .iter()
        .filter(|r| r.set == "ie-sfx" && !r.mono && r.features.verdict() == Verdict::InterleavedMono)
        .count();
    assert_eq!(read_mono, 32);
}
