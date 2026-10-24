//! The CMU Pronouncing Dictionary (`data/cmudict/`), built into the binary compressed.

use std::sync::LazyLock;

use super::Phone;

/// The pronunciation of `word`, case-insensitively; `None` when the dictionary lacks it.
///
/// # Panics
///
/// On first use, if the embedded dictionary does not inflate: the build made it, so that is a build defect.
pub(super) fn lookup(word: &str) -> Option<Vec<Phone>> {
    let d = dict();
    let key = word.to_ascii_lowercase();
    let i = d
        .starts
        .binary_search_by(|&s| d.word(s).as_bytes().cmp(key.as_bytes()))
        .ok()?;
    d.phones(d.starts[i])
}

static PACKED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/cmudict.deflate"));

/// `word PH ON ES` lines, sorted by word, one pronunciation each (`build.rs`), and where each line starts.
struct Dict {
    text: String,
    starts: Vec<usize>,
}

impl Dict {
    fn line(&self, start: usize) -> &str {
        let rest = &self.text[start..];
        &rest[..rest.find('\n').unwrap_or(rest.len())]
    }

    fn word(&self, start: usize) -> &str {
        let line = self.line(start);
        &line[..line.find(' ').unwrap_or(line.len())]
    }

    /// `None` for a phone the dictionary should not hold; `holds_only_known_phones` checks every line.
    fn phones(&self, start: usize) -> Option<Vec<Phone>> {
        self.line(start).split(' ').skip(1).map(Phone::parse).collect()
    }
}

#[expect(
    clippy::expect_used,
    reason = "the embedded dictionary is build output; a failure here is a build defect, not an input error"
)]
static DICT: LazyLock<Dict> = LazyLock::new(|| {
    let bytes = miniz_oxide::inflate::decompress_to_vec(PACKED).expect("build.rs deflated the dictionary");
    let text = String::from_utf8(bytes).expect("cmudict.dict is ASCII");
    let starts = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .filter(|&s| s < text.len())
        .collect();
    Dict { text, starts }
});

fn dict() -> &'static Dict {
    &DICT
}

#[cfg(test)]
mod tests {
    use super::super::{Arpabet::*, Phone};
    use super::*;

    fn phones(s: &str) -> Vec<Phone> {
        s.split(' ').map(|p| Phone::parse(p).unwrap()).collect()
    }

    #[test]
    fn finds_words() {
        assert_eq!(lookup("cassidy"), Some(phones("K AE1 S IH0 D IY0")));
        assert_eq!(lookup("though"), Some(phones("DH OW1")));
        assert_eq!(lookup("tough"), Some(phones("T AH1 F")));
        assert_eq!(lookup("xqzzy"), None);
    }

    #[test]
    fn ignores_case() {
        assert_eq!(lookup("Tough"), lookup("tough"));
        assert_eq!(lookup("TOUGH"), lookup("tough"));
    }

    #[test]
    fn drops_comments() {
        // "aalborg AO1 L B AO0 R G # place, danish"
        assert_eq!(lookup("aalborg"), Some(phones("AO1 L B AO0 R G")));
    }

    #[test]
    fn keeps_the_first_pronunciation() {
        // "read R EH1 D", then "read(2) R IY1 D".
        let read = lookup("read").unwrap();
        assert_eq!(read[1].sound, Eh);
    }

    #[test]
    fn holds_every_word() {
        // Distinct words in cmudict.dict at the vendored commit (135166 lines, alternates included).
        assert_eq!(dict().starts.len(), 126052);
    }

    #[test]
    fn holds_only_known_phones() {
        let d = dict();
        for &s in &d.starts {
            assert!(d.phones(s).is_some_and(|p| !p.is_empty()), "{}", d.line(s));
        }
    }

    #[test]
    fn finds_the_first_and_last_words() {
        let d = dict();
        for &s in [d.starts[0], d.starts[d.starts.len() - 1]].iter() {
            assert!(lookup(d.word(s)).is_some(), "{}", d.word(s));
        }
    }
}
