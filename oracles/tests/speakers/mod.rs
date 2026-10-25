//! Fallout 2's speakers in two halves, for the lip-sync tests.

use std::path::{Path, PathBuf};

/// The speaker directories under `speech` (`sound/speech` from `master.dat`), sorted by name and split: the even
/// entries are the tuning half, which the lip model is fitted to and `lipgen::Params` chosen on; the odd entries are
/// held out, only measured.
pub fn halves(speech: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(speech)
        .unwrap()
        .map(|d| d.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let (tune, held): (Vec<_>, Vec<_>) = dirs.into_iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let strip = |v: Vec<(usize, PathBuf)>| v.into_iter().map(|(_, d)| d).collect();
    (strip(tune), strip(held))
}
