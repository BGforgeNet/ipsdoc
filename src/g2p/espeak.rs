//! The translations' pronunciation, by eSpeak NG (the `espeak-ng` crate, a Rust port), its data downloaded per
//! language into the cache directory on first use.
//!
//! The data is the port's own, at the commit its 0.2.0 release was made from, each file pinned by size and SHA-256. A
//! language reads the four shared phoneme files, its dictionary and its language file. Only those go in the data
//! directory: the translator walks a full `lang/` tree on every call, about a hundred times slower per line.

use std::path::{Path, PathBuf};

use super::{Token, ipa, text};
use crate::fetch::{self, FetchError, Progress, Remote};
use crate::language::Language;

/// The cache's directory for eSpeak's data, named for the commit it comes from, so a newer pin starts afresh.
const DATA_DIR: &str = "espeak-ng-data-d47d38b";

macro_rules! remote {
    ($file:literal, $size:literal, $sha256:literal) => {
        Remote {
            file: $file,
            url: concat!(
                "https://raw.githubusercontent.com/eugenehp/espeak-ng-rs/d47d38b4b7cfc6ec8342d08b40590769aff8102a/",
                "espeak-ng-data/",
                $file
            ),
            size: $size,
            sha256: $sha256,
        }
    };
}

/// The phoneme tables every language reads.
const SHARED: [Remote; 4] = [
    remote!(
        "phontab",
        64_056,
        "7462b1d693c4f46b394308052a3ab9f824ff03e1a6ab2ca434fdde0283f4b444"
    ),
    remote!(
        "phonindex",
        48_856,
        "178d6c50c3d9e62303dcc3090348629b50a648ed12f8a831b444a558d112d783"
    ),
    remote!(
        "phondata",
        600_104,
        "fbf85d4b2d5ed7f0da20e22a3063cfb2ad0bb36cfb322d5e21a2e9710ad509b7"
    ),
    remote!(
        "intonations",
        2_584,
        "c4ff28c76c30cf7d9f07643e08ec39e68769ef648098269b5c0ab7731218e219"
    ),
];

/// A language's dictionary and language file; `None` for English, which ipsdoc pronounces itself.
fn own_files(language: Language) -> Option<[Remote; 2]> {
    Some(match language {
        Language::English => return None,
        Language::Czech => [
            remote!(
                "cs_dict",
                66_745,
                "330e302e4083b08db93aba2bd0f27000c0bdc9c264fc762460e2fd090a629102"
            ),
            remote!(
                "lang/zlw/cs",
                23,
                "1477cdba5ba3bce85db559accbbd8a24b86b3f6d9d9988bc587e89357c1c4431"
            ),
        ],
        Language::French => [
            remote!(
                "fr_dict",
                80_907,
                "fad092d77d8f6b3495d76eccdf1f03e75f6fe82f91d4015bd7274f52c7311208"
            ),
            remote!(
                "lang/roa/fr",
                79,
                "95f44834b48c075dad13eace54d2c98ff79b81aa0074dd67eebaf66c2909eef8"
            ),
        ],
        Language::German => [
            remote!(
                "de_dict",
                85_478,
                "a2db14c26049055916a60ded5016fde41b0b5807b0156cae0c34dcf0aee4fc90"
            ),
            remote!(
                "lang/gmw/de",
                42,
                "f3cca92f94b70f8c25a29ee0a4c9ce4c7f1022241532e0647fa2b7f698bf104e"
            ),
        ],
        Language::Hungarian => [
            remote!(
                "hu_dict",
                209_086,
                "7cea3b54d808d3bb6320c5e260661a85fd9d564d76cbf1208bec0eb0dcad8449"
            ),
            remote!(
                "lang/urj/hu",
                73,
                "c575421d2ae397065d35af0eb4f79dff4e85f3435ccf45da874f46b3fee1338b"
            ),
        ],
        Language::Italian => [
            remote!(
                "it_dict",
                171_199,
                "0b5e3b70cbbd1cf23fe52c2b5872f38fa8ff5d9944591402aecffca411fd99c8"
            ),
            remote!(
                "lang/roa/it",
                109,
                "0d9069eb9a96db1c55c131b2bb7d1f5255c68fbddc2199ffd3295b52519a3256"
            ),
        ],
        Language::Polish => [
            remote!(
                "pl_dict",
                93_957,
                "f295924b52a182141572f0fdfaf904310d7f5db45474fc19a14a99e5ab09e3c2"
            ),
            remote!(
                "lang/zlw/pl",
                38,
                "83c586a2b724b24b9f98d21ade5022112c465ac446ca09930286980ceae1c781"
            ),
        ],
        Language::Portuguese => [
            remote!(
                "pt_dict",
                157_447,
                "02009285f0cd5b7c177ebb9fcd2db531b68758a3bfc6b33c6c40f93778de7039"
            ),
            remote!(
                "lang/roa/pt",
                95,
                "a44060072f790e41912c57b7d52f7b6438c4383e0dabdb052f6fdb7aacc23493"
            ),
        ],
        Language::Russian => [
            remote!(
                "ru_dict",
                9_079_506,
                "e87b6829ddec864d14cc69d84761b53e4705a3ed3f8d3f5477bca9e49c408691"
            ),
            remote!(
                "lang/zle/ru",
                57,
                "9f52d00a279aaeaa45a786b4fd3a98b34e95fedbf823f8cdb9fecc1339751d3a"
            ),
        ],
        Language::Spanish => [
            remote!(
                "es_dict",
                66_801,
                "0401269205dc9d6a1fc678bc05622f3cf4494891cac99103d400fd211bb0bbca"
            ),
            remote!(
                "lang/roa/es",
                63,
                "966aa015ea5646d79f0ca4807cf5da7339aabd3782b55cfa5eb0d8c3fc8fc588"
            ),
        ],
        Language::Swedish => [
            remote!(
                "sv_dict",
                73_870,
                "9e879333c6242a7813aabb0aa1ee6314ea43bc7cb9129d3e3178812b707ab156"
            ),
            remote!(
                "lang/gmq/sv",
                183,
                "00693d7fbe71c18c6cfe5d1e50fe921a3b0dc962522cf8b1ffb73b79e5a86853"
            ),
        ],
        Language::Vietnamese => [
            remote!(
                "vi_dict",
                71_116,
                "b7628d94d13f7ee4d78df60e7f4cdafe2a96a3f6dd9daf6777387425393cc23c"
            ),
            remote!(
                "lang/aav/vi",
                111,
                "3199c980f9e23a88a2aa693cd631bf4fcb0f3408c4272bc01b7ac0ff8e79d778"
            ),
        ],
    })
}

/// Bracketed noises a language's dialogs use; the others' brackets are all taken as directions.
fn noises(language: Language) -> &'static [&'static str] {
    match language {
        Language::German => text::GERMAN_NOISES,
        // No list gathered yet: their bracketed noises read as directions, and are dropped.
        Language::English
        | Language::Czech
        | Language::French
        | Language::Hungarian
        | Language::Italian
        | Language::Polish
        | Language::Portuguese
        | Language::Russian
        | Language::Spanish
        | Language::Swedish
        | Language::Vietnamese => &[],
    }
}

/// Why a language's pronunciation is unavailable, or a line could not be read.
#[derive(Debug, thiserror::Error)]
pub enum EspeakError {
    /// English is ipsdoc's own; eSpeak serves the others.
    #[error("{0:?} is not pronounced by eSpeak")]
    NotEspeak(Language),
    /// The data could not be fetched.
    #[error("eSpeak data for {language:?}")]
    Data {
        /// The language.
        language: Language,
        /// The cause.
        #[source]
        source: FetchError,
    },
    /// eSpeak failed.
    #[error("eSpeak on {text:?}: {message}")]
    Espeak {
        /// The text.
        text: String,
        /// Its error, as it words it.
        message: String,
    },
    /// eSpeak wrote a symbol ipsdoc does not map.
    #[error(transparent)]
    Symbol(#[from] ipa::UnknownSymbol),
}

/// A language eSpeak pronounces, its data in place.
pub struct Espeak {
    language: Language,
    translator: espeak_ng::Translator,
}

impl std::fmt::Debug for Espeak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Espeak")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

/// The directory `language`'s data sits in under `cache`, each file fetched first unless a checked copy is there.
///
/// # Errors
///
/// For English, or when a file cannot be fetched.
pub fn data(language: Language, cache: &Path, progress: &mut dyn FnMut(Progress<'_>)) -> Result<PathBuf, EspeakError> {
    let own = own_files(language).ok_or(EspeakError::NotEspeak(language))?;
    let dir = cache.join(DATA_DIR);
    for remote in SHARED.iter().chain(&own) {
        fetch::fetch(remote, &dir, progress).map_err(|source| EspeakError::Data { language, source })?;
    }
    Ok(dir)
}

impl Espeak {
    /// `language`'s pronunciation from the data in `dir` (see [`data`]).
    ///
    /// # Errors
    ///
    /// When eSpeak cannot load it.
    pub fn new(language: Language, dir: &Path) -> Result<Espeak, EspeakError> {
        let translator = espeak_ng::Translator::new(language.code(), Some(dir)).map_err(|e| EspeakError::Espeak {
            text: String::new(),
            message: e.to_string(),
        })?;
        Ok(Espeak { language, translator })
    }

    /// A line of dialog as tokens: eSpeak's words, a break between its clauses and after closing punctuation, and
    /// the bracketed noises.
    ///
    /// # Errors
    ///
    /// When eSpeak fails on the line, or writes a symbol ipsdoc does not map.
    pub fn tokens(&self, line: &str) -> Result<Vec<Token>, EspeakError> {
        let mut out = Vec::new();
        for segment in text::segments(line, noises(self.language)) {
            match segment {
                text::Segment::Noise => out.push(Token::Noise),
                text::Segment::Text(t) => {
                    let ipa = self.translator.text_to_ipa(&t).map_err(|e| EspeakError::Espeak {
                        text: t.clone(),
                        message: e.to_string(),
                    })?;
                    for (i, clause) in ipa.lines().filter(|c| !c.trim().is_empty()).enumerate() {
                        if i > 0 {
                            out.push(Token::Break);
                        }
                        for w in clause.split_whitespace() {
                            let phones = ipa::word(w)?;
                            if !phones.is_empty() {
                                out.push(Token::Word(phones));
                            }
                        }
                    }
                    if t.ends_with(['.', ',', '!', '?', ';', ':']) {
                        out.push(Token::Break);
                    }
                }
            }
        }
        // No break first, none twice, as English lines have them.
        out.dedup_by(|a, b| *a == Token::Break && *b == Token::Break);
        if out.first() == Some(&Token::Break) {
            out.remove(0);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_translation_has_its_files_and_english_none() {
        for l in Language::all() {
            let own = own_files(l);
            assert_eq!(own.is_none(), l == Language::English, "{l:?}");
            if let Some([dict, lang]) = own {
                assert_eq!(dict.file, format!("{}_dict", l.code()));
                assert!(lang.file.starts_with("lang/") && lang.file.ends_with(&format!("/{}", l.code())));
            }
        }
    }

    #[test]
    fn english_is_refused() {
        assert!(matches!(
            data(Language::English, Path::new("/nonexistent"), &mut |_| {}),
            Err(EspeakError::NotEspeak(Language::English))
        ));
    }

    /// Reads the real data from `IPSDOC_ESPEAK_DATA` (a directory [`data`] filled); tests never download.
    #[test]
    #[ignore = "needs IPSDOC_ESPEAK_DATA"]
    fn a_german_line_reads_with_its_noises() {
        let dir = std::env::var_os("IPSDOC_ESPEAK_DATA").expect("IPSDOC_ESPEAK_DATA names eSpeak's data directory");
        let de = Espeak::new(Language::German, Path::new(&dir)).unwrap();
        let tokens = de.tokens("(hick) Gut. Du siehst Marcus.").unwrap();
        let words = |t: &[Token]| t.iter().filter(|t| matches!(t, Token::Word(_))).count();
        assert_eq!(tokens[0], Token::Noise);
        assert_eq!(words(&tokens), 4);
        assert_eq!(tokens[1], Token::Word(ipa::word("ɡˈuːt").unwrap()));
        assert_eq!(tokens[2], Token::Break);
        assert_eq!(tokens.last(), Some(&Token::Break));
    }
}
