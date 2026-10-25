//! English text to Fallout lip-sync phonemes.

mod dict;
mod nrl;
mod text;

/// An ARPAbet phoneme, as the CMU Pronouncing Dictionary writes them (39 symbols).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[expect(missing_docs, reason = "each variant is the ARPAbet symbol of its name")]
pub enum Arpabet {
    Aa,
    Ae,
    Ah,
    Ao,
    Aw,
    Ay,
    B,
    Ch,
    D,
    Dh,
    Eh,
    Er,
    Ey,
    F,
    G,
    Hh,
    Ih,
    Iy,
    Jh,
    K,
    L,
    M,
    N,
    Ng,
    Ow,
    Oy,
    P,
    R,
    S,
    Sh,
    T,
    Th,
    Uh,
    Uw,
    V,
    W,
    Y,
    Z,
    Zh,
}

impl Arpabet {
    /// Whether it is a vowel (the dictionary gives vowels a stress digit).
    pub fn is_vowel(self) -> bool {
        use Arpabet::*;
        matches!(
            self,
            Aa | Ae | Ah | Ao | Aw | Ay | Eh | Er | Ey | Ih | Iy | Ow | Oy | Uh | Uw
        )
    }

    /// The phoneme for an upper-case symbol without stress.
    pub fn from_symbol(s: &str) -> Option<Arpabet> {
        SYMBOLS.iter().find(|(sym, _)| *sym == s).map(|&(_, a)| a)
    }
}

pub(crate) const SYMBOLS: [(&str, Arpabet); 39] = [
    ("AA", Arpabet::Aa),
    ("AE", Arpabet::Ae),
    ("AH", Arpabet::Ah),
    ("AO", Arpabet::Ao),
    ("AW", Arpabet::Aw),
    ("AY", Arpabet::Ay),
    ("B", Arpabet::B),
    ("CH", Arpabet::Ch),
    ("D", Arpabet::D),
    ("DH", Arpabet::Dh),
    ("EH", Arpabet::Eh),
    ("ER", Arpabet::Er),
    ("EY", Arpabet::Ey),
    ("F", Arpabet::F),
    ("G", Arpabet::G),
    ("HH", Arpabet::Hh),
    ("IH", Arpabet::Ih),
    ("IY", Arpabet::Iy),
    ("JH", Arpabet::Jh),
    ("K", Arpabet::K),
    ("L", Arpabet::L),
    ("M", Arpabet::M),
    ("N", Arpabet::N),
    ("NG", Arpabet::Ng),
    ("OW", Arpabet::Ow),
    ("OY", Arpabet::Oy),
    ("P", Arpabet::P),
    ("R", Arpabet::R),
    ("S", Arpabet::S),
    ("SH", Arpabet::Sh),
    ("T", Arpabet::T),
    ("TH", Arpabet::Th),
    ("UH", Arpabet::Uh),
    ("UW", Arpabet::Uw),
    ("V", Arpabet::V),
    ("W", Arpabet::W),
    ("Y", Arpabet::Y),
    ("Z", Arpabet::Z),
    ("ZH", Arpabet::Zh),
];

/// A phoneme with its stress, as the dictionary gives it: vowels carry 0 (none), 1 (primary) or 2 (secondary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Phone {
    /// The phoneme.
    pub sound: Arpabet,
    /// A vowel's stress; `None` for consonants.
    pub stress: Option<u8>,
}

impl Phone {
    /// Reads `AH0`, `K` and the like.
    pub fn parse(s: &str) -> Option<Phone> {
        let (sym, stress) = match s.as_bytes().last() {
            Some(&d @ b'0'..=b'2') => (&s[..s.len() - 1], Some(d - b'0')),
            _ => (s, None),
        };
        Some(Phone {
            sound: Arpabet::from_symbol(sym)?,
            stress,
        })
    }
}

/// A piece of a line, for lip sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A word's phonemes.
    Word(Vec<Phone>),
    /// A word drawn out in the writing ("Riiiight", "Ahhh"), as said: its phonemes, collapsed.
    Drawn(Vec<Phone>),
    /// A phrase break.
    Break,
    /// A vocal noise (a cough, a laugh).
    Noise,
}

/// The Fallout phoneme code for a phone: the code Interplay's own lip files give each ARPAbet sound.
pub fn code(p: Phone) -> u8 {
    use Arpabet::*;
    match p.sound {
        Aa => 6,
        Ae => 5,
        Ah => 13,
        Ao => 7,
        Aw => 15,
        Ay => 14,
        B => 18,
        Ch => 39,
        D => 20,
        Dh => 26,
        Eh => 4,
        // Interplay's choice for ER over R (38): 86% of aligned ER.
        Er => 11,
        Ey => 3,
        F => 23,
        G => 22,
        Hh => 31,
        Ih => 2,
        Iy => 1,
        Jh => 40,
        K => 21,
        L => 35,
        M => 32,
        N => 33,
        Ng => 34,
        Ow => 8,
        Oy => 16,
        P => 17,
        R => 38,
        S => 27,
        Sh => 29,
        T => 19,
        Th => 25,
        Uh => 9,
        Uw => 10,
        V => 24,
        W => 36,
        Y => 37,
        Z => 28,
        Zh => 30,
    }
}

/// A line of English dialog as tokens.
pub fn tokens(text: &str) -> Vec<Token> {
    text::normalise(text)
        .into_iter()
        .map(|p| match p {
            text::Piece::Word(w) => word(&w, pronounce(&w)),
            text::Piece::Capitals(w) => word(
                &w,
                match dict::lookup(&w) {
                    Some(p) => p,
                    None if !w.bytes().any(|b| b"AEIOUY".contains(&b)) => spell(&w),
                    None => pronounce(&w),
                },
            ),
            text::Piece::Break => Token::Break,
            text::Piece::Noise => Token::Noise,
        })
        .filter(|t| !matches!(t, Token::Word(p) | Token::Drawn(p) if p.is_empty()))
        .collect()
}

/// `written`'s phonemes as a word, drawn out when it runs three or more of a letter.
fn word(written: &str, phones: Vec<Phone>) -> Token {
    if collapse(written, 2) == written {
        Token::Word(phones)
    } else {
        Token::Drawn(phones)
    }
}

/// Fallout names the dictionary lacks and the rules misread (from the unknown words of the Unofficial Patch's voiced
/// lines), in ARPAbet.
const NAMES: &[(&str, &str)] = &[
    ("fev", "EH1 F IY1 V IY1"),
    ("holodisk", "HH AA1 L OW0 D IH2 S K"),
    ("klamath", "K L AE1 M AH0 TH"),
    ("tandi", "T AE1 N D IY0"),
    ("xander", "Z AE1 N D ER0"),
];

fn names_lookup(word: &str) -> Option<Vec<Phone>> {
    let (_, p) = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(word))?;
    p.split(' ').map(Phone::parse).collect()
}

/// A known pronunciation: the names, then the dictionary, as written and without outer apostrophes.
fn known(word: &str) -> Option<Vec<Phone>> {
    let trimmed = word.trim_matches('\'');
    names_lookup(trimmed)
        .or_else(|| dict::lookup(word))
        .or_else(|| dict::lookup(trimmed))
}

/// Runs of three or more of a letter cut to `keep` (`REEEAL`, `HMMMM`).
fn collapse(word: &str, keep: usize) -> String {
    let mut out = String::with_capacity(word.len());
    let b = word.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let run = b[i..].iter().take_while(|&&c| c == b[i]).count();
        let n = if run >= 3 { keep } else { run };
        out.extend(std::iter::repeat_n(char::from(b[i]), n));
        i += run;
    }
    out
}

/// The ending `-s` or `-'s` after `base`: IH Z after a sibilant, S after another voiceless sound, else Z.
fn plural_ending(base: &[Phone]) -> Vec<Phone> {
    use Arpabet::*;
    let p = |sound, stress| Phone { sound, stress };
    match base.last().map(|x| x.sound) {
        Some(S | Z | Sh | Zh | Ch | Jh) => vec![p(Ih, Some(0)), p(Z, None)],
        Some(P | T | K | F | Th) => vec![p(S, None)],
        _ => vec![p(Z, None)],
    }
}

/// A word's pronunciation: known as written, or once drawn-out letters are collapsed; else a known or ruled base with
/// its `-'s` or `-s` ending; else the rules.
fn pronounce(word: &str) -> Vec<Phone> {
    let trimmed = word.trim_matches('\'');
    if let Some(p) = known(word)
        .or_else(|| known(&collapse(trimmed, 1)))
        .or_else(|| known(&collapse(trimmed, 2)))
    {
        return p;
    }
    let base = trimmed
        .strip_suffix("'S")
        .or_else(|| trimmed.strip_suffix('S').filter(|b| b.len() >= 3 && !b.ends_with('S')));
    if let Some(base) = base {
        let mut p = known(base).unwrap_or_else(|| nrl::phones(base));
        if !p.is_empty() {
            let ending = plural_ending(&p);
            p.extend(ending);
            return p;
        }
    }
    nrl::phones(trimmed)
}

/// Letter by letter, as the dictionary names consonants.
fn spell(word: &str) -> Vec<Phone> {
    word.chars()
        .filter(char::is_ascii_alphabetic)
        .flat_map(|c| dict::lookup(c.encode_utf8(&mut [0; 4])).unwrap_or_default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str) -> Vec<Vec<u8>> {
        tokens(text)
            .iter()
            .map(|t| match t {
                Token::Word(p) | Token::Drawn(p) => p.iter().map(|&p| code(p)).collect(),
                Token::Break => vec![255],
                Token::Noise => vec![254],
            })
            .collect()
    }

    #[test]
    fn codes_follow_the_table() {
        let table = [
            ("AA1", 6),
            ("AE1", 5),
            ("AH0", 13),
            ("AH1", 13),
            ("AO1", 7),
            ("AW1", 15),
            ("AY1", 14),
            ("B", 18),
            ("CH", 39),
            ("D", 20),
            ("DH", 26),
            ("EH1", 4),
            ("ER0", 11),
            ("ER1", 11),
            ("EY1", 3),
            ("F", 23),
            ("G", 22),
            ("HH", 31),
            ("IH1", 2),
            ("IY1", 1),
            ("JH", 40),
            ("K", 21),
            ("L", 35),
            ("M", 32),
            ("N", 33),
            ("NG", 34),
            ("OW1", 8),
            ("OY1", 16),
            ("P", 17),
            ("R", 38),
            ("S", 27),
            ("SH", 29),
            ("T", 19),
            ("TH", 25),
            ("UH1", 9),
            ("UW1", 10),
            ("V", 24),
            ("W", 36),
            ("Y", 37),
            ("Z", 28),
            ("ZH", 30),
        ];
        for (s, c) in table {
            assert_eq!(code(Phone::parse(s).unwrap()), c, "{s}");
        }
    }

    #[test]
    fn words_breaks_and_noises() {
        // "map be": M AE1 P, B IY1.
        assert_eq!(
            codes("Map, be (cough)."),
            vec![vec![32, 5, 17], vec![255], vec![18, 1], vec![254], vec![255]]
        );
    }

    #[test]
    fn unknown_words_go_to_the_rules() {
        assert!(dict::lookup("grampy").is_none());
        assert_eq!(tokens("grampy"), vec![Token::Word(nrl::phones("grampy"))]);
        assert!(!nrl::phones("grampy").is_empty());
    }

    #[test]
    fn apostrophes_are_tried_as_written_then_trimmed() {
        assert_eq!(tokens("'em"), vec![Token::Word(dict::lookup("'em").unwrap())]);
        // Not in the dictionary with its apostrophe: found without it.
        assert!(dict::lookup("whiskey'").is_none());
        assert_eq!(tokens("whiskey'"), vec![Token::Word(dict::lookup("whiskey").unwrap())]);
    }

    fn dict(w: &str) -> Vec<Phone> {
        dict::lookup(w).unwrap()
    }

    fn with(mut p: Vec<Phone>, tail: &str) -> Vec<Phone> {
        p.extend(tail.split(' ').map(|s| Phone::parse(s).unwrap()));
        p
    }

    #[test]
    fn possessives_and_plurals_of_known_words() {
        // From the Unofficial Patch's voiced lines: MYRON'S, GECKO'S.
        assert!(dict::lookup("myron's").is_none() && dict::lookup("gecko's").is_none());
        assert_eq!(tokens("Myron's"), vec![Token::Word(with(dict("myron"), "Z"))]);
        assert_eq!(tokens("gecko's"), vec![Token::Word(with(dict("gecko"), "Z"))]);
        // The ending follows the last sound: S after a voiceless one, IH Z after a sibilant.
        assert_eq!(
            tokens("Klamath's"),
            vec![Token::Word(with(names_lookup("klamath").unwrap(), "S"))]
        );
        assert!(dict::lookup("mordinos").is_none());
        assert_eq!(tokens("Mordinos"), vec![Token::Word(with(nrl::phones("mordino"), "Z"))]);
    }

    #[test]
    fn drawn_out_spellings_are_collapsed_and_marked() {
        let drawn = |w: &str| match &tokens(w)[..] {
            [Token::Word(p)] => Token::Drawn(p.clone()),
            t => panic!("{t:?}"),
        };
        assert_eq!(tokens("REEEAL"), [drawn("real")]);
        assert_eq!(tokens("Hmmmm"), [drawn("hmm")]);
        assert_eq!(tokens("Riiiight"), [drawn("right")]);
        // Two of a letter is spelling, not drawing out.
        assert_eq!(tokens("Ahh"), tokens("ah"));
        assert!(matches!(tokens("Ahh")[..], [Token::Word(_)]));
    }

    #[test]
    fn game_names_come_first() {
        // F-E-V, as the game's characters say it; the rules alone read "FEHV".
        assert_eq!(tokens("FEV"), vec![Token::Word(with(Vec::new(), "EH1 F IY1 V IY1"))]);
        assert_eq!(tokens("Tandi"), vec![Token::Word(with(Vec::new(), "T AE1 N D IY0"))]);
        for (name, _) in NAMES {
            assert!(names_lookup(name).is_some(), "{name}");
        }
    }

    #[test]
    fn capitals_are_words_when_known_else_spelled_when_vowelless() {
        assert_eq!(tokens("REAL"), tokens("real"));
        // N C R, letter by letter: EH N, S IY, AA R.
        assert!(dict::lookup("ncr").is_none());
        assert_eq!(codes("NCR"), vec![vec![4, 33, 27, 1, 6, 38]]);
        // With a vowel it is read as a word, by the rules.
        assert!(dict::lookup("bugmen").is_none() && names_lookup("bugmen").is_none());
        assert_eq!(tokens("BUGMEN"), vec![Token::Word(nrl::phones("bugmen"))]);
    }
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    #[test]
    fn symbols_are_in_variant_order() {
        for (i, (sym, a)) in SYMBOLS.iter().enumerate() {
            assert_eq!(*a as usize, i, "{sym}");
        }
    }

    #[test]
    fn parses_phones() {
        assert_eq!(
            Phone::parse("AH0"),
            Some(Phone {
                sound: Arpabet::Ah,
                stress: Some(0)
            })
        );
        assert_eq!(
            Phone::parse("NG"),
            Some(Phone {
                sound: Arpabet::Ng,
                stress: None
            })
        );
        assert_eq!(Phone::parse("XX"), None);
        assert_eq!(Phone::parse("#"), None);
    }
}
