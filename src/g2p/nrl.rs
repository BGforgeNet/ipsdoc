//! The NRL letter-to-sound rules (`data/nrl/`): a pronunciation for words the dictionary lacks.
//!
//! A rule `A[B]C=/D/` gives the letters `B` the sounds `D` when the text before them ends in `A` and the text after
//! them starts with `C`. A word is scanned left to right; at each letter the first matching rule of that letter's
//! group applies and the scan moves past `B`. Context symbols: `#` one or more vowels, `:` zero or more consonants,
//! `^` one consonant, `+` one of E I Y, `.` a voiced consonant, `%` a suffix, `&` a sibilant, `@` a consonant that
//! changes a following long U; a space is a word boundary. Vowels are A E I O U Y.

use std::sync::OnceLock;

use super::{Arpabet, Phone};

/// The word's sounds by the rules; `word` is letters and apostrophes, any case.
pub fn phones(word: &str) -> Vec<Phone> {
    symbols(word).into_iter().filter_map(phone).collect()
}

/// A rule symbol as a dictionary phone: `AX` is an unstressed AH, `NX` is NG, `WH` is W; other vowels count as
/// stressed (the rules mark no stress).
fn phone(symbol: &str) -> Option<Phone> {
    let (sound, stress) = match symbol {
        "AX" => (Arpabet::Ah, Some(0)),
        "NX" => (Arpabet::Ng, None),
        "WH" => (Arpabet::W, None),
        s => {
            let sound = Arpabet::from_symbol(s)?;
            (sound, sound.is_vowel().then_some(1))
        }
    };
    Some(Phone { sound, stress })
}

/// The rules' own symbols for `word` (ARPAbet, plus `AX` schwa, `NX` for NG and `WH`).
fn symbols(word: &str) -> Vec<&'static str> {
    // Set off as the rules expect: blanks around the word and around each apostrophe ("DON ' T").
    let mut text = vec![b' '];
    for b in word.bytes().map(|b| b.to_ascii_uppercase()) {
        if b == b'\'' {
            text.extend_from_slice(b" ' ");
        } else {
            text.push(b);
        }
    }
    text.push(b' ');
    let mut out = Vec::new();
    let mut at = 1;
    while at < text.len() - 1 {
        let rule = rules()
            .iter()
            .find(|r| r.body.first() == Some(&text[at]) && r.matches(&text, at));
        match rule {
            Some(r) => {
                out.extend(r.sounds.iter().copied());
                at += r.body.len();
            }
            // Blanks and apostrophes have no rules here: they only shape the contexts.
            None => at += 1,
        }
    }
    out
}

/// A context element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Elem {
    Char(u8),
    /// `#`
    Vowels,
    /// `:`
    Consonants,
    /// `^`
    Consonant,
    /// `+`
    Front,
    /// `.`
    Voiced,
    /// `%`, `&`, `@`: one of a set of strings.
    OneOf(&'static [&'static [u8]]),
}

const SUFFIX: &[&[u8]] = &[b"ER", b"E", b"ES", b"ED", b"ING", b"ELY"];
const SIBILANT: &[&[u8]] = &[b"S", b"C", b"G", b"Z", b"X", b"J", b"CH", b"SH"];
const LONG_U: &[&[u8]] = &[b"T", b"S", b"R", b"D", b"L", b"Z", b"N", b"J", b"TH", b"CH", b"SH"];

fn is_vowel(b: u8) -> bool {
    b"AEIOUY".contains(&b)
}

fn is_consonant(b: u8) -> bool {
    b"BCDFGHJKLMNPQRSTVWXZ".contains(&b)
}

impl Elem {
    fn single(self, b: u8) -> bool {
        match self {
            Elem::Char(c) => b == c,
            Elem::Consonant => is_consonant(b),
            Elem::Front => b"EIY".contains(&b),
            Elem::Voiced => b"BDVGJLMNRWZ".contains(&b),
            Elem::Vowels | Elem::Consonants | Elem::OneOf(_) => false,
        }
    }
}

/// Whether `pattern` matches `text` from `at` onwards (any length).
fn forward(pattern: &[Elem], text: &[u8], at: usize) -> bool {
    let Some((&e, rest)) = pattern.split_first() else {
        return true;
    };
    match e {
        Elem::Vowels | Elem::Consonants => {
            let class = if e == Elem::Vowels { is_vowel } else { is_consonant };
            let run = text[at.min(text.len())..].iter().take_while(|&&b| class(b)).count();
            let least = usize::from(e == Elem::Vowels);
            (least..=run).rev().any(|k| forward(rest, text, at + k))
        }
        Elem::OneOf(set) => set
            .iter()
            .any(|s| text.get(at..at + s.len()) == Some(*s) && forward(rest, text, at + s.len())),
        Elem::Char(_) | Elem::Consonant | Elem::Front | Elem::Voiced => {
            text.get(at).is_some_and(|&b| e.single(b)) && forward(rest, text, at + 1)
        }
    }
}

/// Whether `pattern` matches `text` ending just before `end` (any start).
fn backward(pattern: &[Elem], text: &[u8], end: usize) -> bool {
    let Some((&e, rest)) = pattern.split_last() else {
        return true;
    };
    match e {
        Elem::Vowels | Elem::Consonants => {
            let class = if e == Elem::Vowels { is_vowel } else { is_consonant };
            let run = text[..end].iter().rev().take_while(|&&b| class(b)).count();
            let least = usize::from(e == Elem::Vowels);
            (least..=run).rev().any(|k| backward(rest, text, end - k))
        }
        Elem::OneOf(set) => set
            .iter()
            .any(|s| end >= s.len() && &text[end - s.len()..end] == *s && backward(rest, text, end - s.len())),
        Elem::Char(_) | Elem::Consonant | Elem::Front | Elem::Voiced => {
            end > 0 && e.single(text[end - 1]) && backward(rest, text, end - 1)
        }
    }
}

/// One rule: `left[body]right=/sounds/`.
#[derive(Debug)]
struct Rule {
    left: Vec<Elem>,
    body: Vec<u8>,
    right: Vec<Elem>,
    sounds: Vec<&'static str>,
}

impl Rule {
    fn matches(&self, text: &[u8], at: usize) -> bool {
        text.get(at..at + self.body.len()) == Some(self.body.as_slice())
            && backward(&self.left, text, at)
            && forward(&self.right, text, at + self.body.len())
    }

    fn parse(line: &'static str) -> Option<Rule> {
        let (lhs, sounds) = line.rsplit_once('=')?;
        let (left, rest) = lhs.split_once('[')?;
        let (body, right) = rest.split_once(']')?;
        let context = |s: &str| -> Option<Vec<Elem>> {
            s.bytes()
                .map(|b| match b {
                    b'#' => Some(Elem::Vowels),
                    b':' => Some(Elem::Consonants),
                    b'^' => Some(Elem::Consonant),
                    b'+' => Some(Elem::Front),
                    b'.' => Some(Elem::Voiced),
                    b'%' => Some(Elem::OneOf(SUFFIX)),
                    b'&' => Some(Elem::OneOf(SIBILANT)),
                    b'@' => Some(Elem::OneOf(LONG_U)),
                    b'A'..=b'Z' | b' ' | b'\'' => Some(Elem::Char(b)),
                    _ => None,
                })
                .collect()
        };
        Some(Rule {
            left: context(left)?,
            body: body.as_bytes().to_vec(),
            right: context(right)?,
            sounds: sounds.trim_matches('/').split_whitespace().collect(),
        })
    }
}

static RULES_TXT: &str = include_str!("../../data/nrl/rules.txt");

#[expect(
    clippy::expect_used,
    reason = "the rules are fixed data; `every_rule_reads` checks each line, so a failure is a defect in that file"
)]
fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        RULES_TXT
            .lines()
            .map(|l| Rule::parse(l).expect("data/nrl/rules.txt lines are A[B]C=/D/ rules"))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report's own examples ("RATIO", and the two sentences of its worked test), word by word.
    #[test]
    fn translates_the_reports_examples() {
        let cases = [
            ("RATIO", "R EY SH OW"),
            ("THE", "DH AX"),
            ("TIME", "T AY M"),
            ("HAS", "HH AE Z"),
            ("COME", "K AH M"),
            ("WALRUS", "W AO L R AH S"),
            ("SAID", "S EH D"),
            ("TO", "T UW"),
            ("TALK", "T AO K"),
            ("OF", "AX V"),
            ("MANY", "M EH N IY"),
            ("THINGS", "TH IH NX Z"),
            ("SHOES", "SH OW Z"),
            ("AND", "AE N D"),
            ("SHIPS", "SH IH P S"),
            ("SEALING", "S IY L IH NX"),
            ("WAX", "W AE K S"),
            ("CABBAGES", "K AE B B IH JH IH Z"),
            ("KINGS", "K IH NX Z"),
            ("WHY", "WH AY"),
            ("SEA", "S IY"),
            ("IS", "IH Z"),
            ("BOILING", "B OY L IH NX"),
            ("HOT", "HH AA T"),
            ("WHETHER", "WH EH DH ER"),
            ("PIGS", "P IH G Z"),
            ("HAVE", "HH AE V"),
            ("WINGS", "W IH NX Z"),
            ("SUICIDE", "S UW IH S AY D"),
        ];
        for (word, want) in cases {
            assert_eq!(symbols(word).join(" "), want, "{word}");
        }
    }

    #[test]
    fn ignores_case() {
        assert_eq!(symbols("ratio"), symbols("RATIO"));
    }

    #[test]
    fn reads_apostrophes_as_the_rules_expect() {
        // [ON ' T]=/OW N T/ and #[SN] '=/Z AX N/ are written for apostrophes set off by spaces.
        assert_eq!(symbols("don't").join(" "), "D OW N T");
        assert_eq!(symbols("wasn't").join(" "), "W AA Z AX N T");
    }

    #[test]
    fn maps_its_symbols_to_arpabet() {
        let p = phones("the");
        assert_eq!(
            p[1],
            Phone {
                sound: Arpabet::Ah,
                stress: Some(0)
            }
        );
        assert_eq!(phones("kings")[2].sound, Arpabet::Ng);
        assert_eq!(phones("why")[0].sound, Arpabet::W);
    }

    #[test]
    fn every_rule_reads() {
        assert_eq!(rules().len(), 308);
    }
}
