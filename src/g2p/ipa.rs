//! IPA, as eSpeak writes a word, to ARPAbet: the phones of every language ipsdoc reads map onto the English set, so
//! the aligner, its model and the mouth-shape codes serve them all.
//!
//! The mouth decides where a language has no English phone: rounded front vowels take the rounded back vowels'
//! shapes, the palatal and velar fricatives the open-mouthed HH, every rhotic R and every lateral L. Length, tone,
//! nasality, palatalisation and aspiration change no shape and are dropped; a glottal stop shows nothing.

use super::{Arpabet, Phone};

/// A symbol with no mapping here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("IPA symbol {symbol:?} (U+{code:04X}) in {word:?} has no ARPAbet mapping")]
pub struct UnknownSymbol {
    /// The symbol.
    pub symbol: char,
    /// Its code point.
    pub code: u32,
    /// The word it is in.
    pub word: String,
}

/// Marks after a sound that change it in ways the mouth does not show: length, aspiration, palatalisation and the
/// like, and combining diacritics (nasal, dental, syllabic, non-syllabic).
fn is_modifier(c: char) -> bool {
    matches!(c, 'ː' | 'ˑ' | 'ʰ' | 'ʲ' | 'ʷ' | 'ˠ' | 'ˤ' | 'ʼ' | '˞' | 'ⁿ' | 'ˀ')
        || ('\u{0300}'..='\u{036F}').contains(&c)
        // eSpeak's own phoneme names the port leaves unconverted after a sound: `"` (centralised, Russian `u"` in
        // "заключённых") and `^` (palatalised, Russian "теперь" `tʲipʲˈerɪ^`); neither changes the mouth.
        || matches!(c, '"' | '^')
}

/// Pairs eSpeak writes as two symbols that English has one phone for: diphthongs and affricates.
fn pair(a: char, b: char) -> Option<Arpabet> {
    use Arpabet::*;
    Some(match (a, b) {
        ('a', 'ɪ' | 'i' | 'j') => Ay,
        ('a', 'ʊ' | 'u' | 'w') => Aw,
        ('ɔ' | 'o', 'ø' | 'ʏ' | 'ɪ' | 'y' | 'j') => Oy,
        ('e', 'ɪ' | 'i') => Ey,
        ('ə' | 'o', 'ʊ') => Ow,
        ('t', 'ʃ' | 'ɕ' | 'ʂ') => Ch,
        ('d', 'ʒ' | 'ʑ' | 'ʐ') => Jh,
        _ => return None,
    })
}

/// One symbol's sound; `Some(None)` for one that shows nothing (a glottal stop).
fn single(c: char) -> Option<Option<Arpabet>> {
    use Arpabet::*;
    Some(Some(match c {
        'a' | 'ɑ' | 'ä' => Aa,
        'æ' => Ae,
        'ɐ' | 'ə' | 'ʌ' | 'ɤ' | 'ɘ' | 'ɞ' => Ah,
        'ɒ' | 'ɔ' | 'œ' | 'ɶ' => Ao,
        'ɜ' | 'ɚ' | 'ɝ' => Er,
        'e' => Ey,
        'ɛ' => Eh,
        'i' => Iy,
        'ɪ' | 'ɨ' | 'ɯ' => Ih,
        'o' | 'ø' | 'ɵ' => Ow,
        'u' | 'y' | 'ʉ' => Uw,
        'ʊ' | 'ʏ' => Uh,
        'p' => P,
        'b' | 'ɓ' | 'β' => B,
        't' | 'ʈ' => T,
        'd' | 'ɖ' | 'ɗ' => D,
        'k' | 'c' | 'q' => K,
        'g' | 'ɡ' | 'ɟ' | 'ɢ' | 'ɠ' | 'ʄ' | 'ɣ' => G,
        'm' | 'ɱ' => M,
        'n' | 'ɳ' | 'ɲ' => N,
        'ŋ' | 'ɴ' => Ng,
        'r' | 'ɾ' | 'ɽ' | 'ɹ' | 'ɻ' | 'ʀ' | 'ʁ' | 'ɺ' => R,
        'l' | 'ɫ' | 'ɭ' | 'ʎ' | 'ʟ' | 'ɬ' | 'ɮ' => L,
        'f' | 'ɸ' => F,
        'v' | 'ʋ' => V,
        'θ' => Th,
        'ð' => Dh,
        's' => S,
        'z' => Z,
        // ɧ: the Swedish sj-sound, rounded.
        'ʃ' | 'ʂ' | 'ɕ' | 'ɧ' => Sh,
        'ʒ' | 'ʐ' | 'ʑ' => Zh,
        'h' | 'ɦ' | 'x' | 'χ' | 'ç' | 'ħ' | 'ʕ' => Hh,
        'j' | 'ʝ' => Y,
        'w' | 'ɥ' | 'ɰ' => W,
        'ʔ' => return Some(None),
        _ => return None,
    }))
}

/// One word of eSpeak's IPA as phones. Stress marks set the next vowel's stress; language-switch marks (`(en)`), tone
/// digits and link marks are dropped.
///
/// # Errors
///
/// On a symbol this module does not map.
pub fn word(ipa: &str) -> Result<Vec<Phone>, UnknownSymbol> {
    // eSpeak marks a stretch read in another language as `(en)...(vi)`: the marks go, the sounds stay.
    let mut text = String::with_capacity(ipa.len());
    let mut in_mark = false;
    for c in ipa.chars() {
        match c {
            '(' => in_mark = true,
            ')' => in_mark = false,
            c if !in_mark => text.push(c),
            _ => {}
        }
    }
    let symbols: Vec<char> = text.chars().collect();
    let unknown = |symbol: char| UnknownSymbol {
        symbol,
        code: u32::from(symbol),
        word: ipa.to_owned(),
    };
    let mut out = Vec::new();
    let mut stress = None;
    let mut i = 0;
    while i < symbols.len() {
        let c = symbols[i];
        i += 1;
        match c {
            'ˈ' => stress = Some(1),
            'ˌ' => stress = Some(2),
            c if c.is_ascii_digit() || is_modifier(c) || matches!(c, '-' | '_' | '|' | '‿' | '.' | '\'') => {}
            c => {
                // The next sound symbol, past any marks and tone digits on this one.
                let mut j = i;
                while j < symbols.len() && (is_modifier(symbols[j]) || symbols[j].is_ascii_digit()) {
                    j += 1;
                }
                let sound = match symbols.get(j).and_then(|&n| pair(c, n)) {
                    Some(s) => {
                        i = j + 1;
                        Some(s)
                    }
                    None => single(c).ok_or_else(|| unknown(c))?,
                };
                if let Some(sound) = sound {
                    out.push(Phone {
                        sound,
                        stress: sound.is_vowel().then(|| stress.take().unwrap_or(0)),
                    });
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arpa(s: &str) -> Vec<Phone> {
        s.split(' ').map(|p| Phone::parse(p).unwrap()).collect()
    }

    #[test]
    fn german_words_as_espeak_writes_them() {
        // "Gut.", "Du siehst den Mutanten Marcus": eSpeak's output for UPU's German lines.
        assert_eq!(word("ɡˈuːt").unwrap(), arpa("G UW1 T"));
        assert_eq!(word("zˈiːst").unwrap(), arpa("Z IY1 S T"));
        assert_eq!(word("muːtˈantən").unwrap(), arpa("M UW0 T AA1 N T AH0 N"));
        assert_eq!(word("mˈaɾkʊs").unwrap(), arpa("M AA1 R K UH0 S"));
        // Diphthongs and affricates as one phone each: "neunzehn", "tausend", "deutsch".
        assert_eq!(word("nˈɔøntseːn").unwrap(), arpa("N OY1 N T S EY0 N"));
        assert_eq!(word("tˈaʊzənt").unwrap(), arpa("T AW1 Z AH0 N T"));
        assert_eq!(word("dˈɔøtʃ").unwrap(), arpa("D OY1 CH"));
    }

    #[test]
    fn russian_and_vietnamese_marks_are_dropped() {
        // Palatalisation and the Vietnamese tone digits change no mouth shape.
        assert_eq!(word("vʲˈidʲitʲi").unwrap(), arpa("V IY1 D IY0 T IY0"));
        assert_eq!(word("hˈa5j").unwrap(), arpa("HH AY1"));
        assert_eq!(word("ɗˈəɪ1").unwrap(), arpa("D AH1 IH0"));
        // A stretch eSpeak reads in English inside another language.
        assert_eq!(word("(en)jˈuː(vi)").unwrap(), arpa("Y UW1"));
    }

    #[test]
    fn what_the_port_leaves_of_espeaks_own_names() {
        // UPU's Russian "заключённых" and "теперь", and Swedish "popularitet"'s sj-sound, as the port writes them.
        assert_eq!(
            word("zʌkɭʲu\"tʃʲˈennyx").unwrap(),
            arpa("Z AH0 K L UW0 CH EY1 N N UW0 HH")
        );
        assert_eq!(word("tʲipʲˈerɪ^").unwrap(), arpa("T IY0 P EY1 R IH0"));
        assert_eq!(word("pˌɔpʉːlaɧˈuːn").unwrap(), arpa("P AO2 P UW0 L AA0 SH UW1 N"));
    }

    #[test]
    fn an_unknown_symbol_is_named() {
        let e = word("a☃").unwrap_err();
        assert_eq!(e.symbol, '☃');
        assert_eq!(e.word, "a☃");
    }
}
