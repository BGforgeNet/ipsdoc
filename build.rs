//! Prepares the pronouncing dictionary for `src/g2p/dict.rs`: comments and alternate pronunciations dropped, lines
//! sorted by word so that a lookup is a binary search over the text, then compressed into `OUT_DIR`. Building a
//! hash table at run time instead costs about ten times the start-up.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "data/cmudict/cmudict.dict";
    println!("cargo::rerun-if-changed={source}");
    let text = std::fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))?;
    let mut lines: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or_default().trim();
            let (word, phones) = line.split_once(' ')?;
            // `word(2)` lines are alternates; the first pronunciation is kept.
            (!word.ends_with(')')).then_some((word, phones.trim()))
        })
        .collect();
    lines.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    lines.dedup_by(|a, b| a.0 == b.0);
    let mut out = String::with_capacity(text.len());
    for (word, phones) in lines {
        out.push_str(word);
        out.push(' ');
        out.push_str(phones);
        out.push('\n');
    }
    let packed = miniz_oxide::deflate::compress_to_vec(out.as_bytes(), 9);
    let path = PathBuf::from(std::env::var("OUT_DIR")?).join("cmudict.deflate");
    std::fs::write(&path, packed).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}
