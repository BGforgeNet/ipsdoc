//! English text to Fallout lip-sync phonemes.

pub mod dict;

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
    /// The symbol, upper case, without stress.
    pub fn symbol(self) -> &'static str {
        SYMBOLS[self as usize].0
    }

    /// The phoneme for an upper-case symbol without stress.
    pub fn from_symbol(s: &str) -> Option<Arpabet> {
        SYMBOLS.iter().find(|(sym, _)| *sym == s).map(|&(_, a)| a)
    }
}

const SYMBOLS: [(&str, Arpabet); 39] = [
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_in_variant_order() {
        for (i, (sym, a)) in SYMBOLS.iter().enumerate() {
            assert_eq!(*a as usize, i, "{sym}");
            assert_eq!(a.symbol(), *sym);
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
