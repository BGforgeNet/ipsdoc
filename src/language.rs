//! The languages ipsdoc reads dialog in: English, built in, and the Fallout 2 translations eSpeak pronounces.

/// A dialog language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// English, pronounced from the built-in CMU dictionary.
    English,
    /// Czech.
    Czech,
    /// French.
    French,
    /// German.
    German,
    /// Hungarian.
    Hungarian,
    /// Italian.
    Italian,
    /// Polish.
    Polish,
    /// Portuguese.
    Portuguese,
    /// Russian.
    Russian,
    /// Spanish.
    Spanish,
    /// Swedish.
    Swedish,
    /// Vietnamese.
    Vietnamese,
}

const ALL: [Language; 12] = [
    Language::English,
    Language::Czech,
    Language::French,
    Language::German,
    Language::Hungarian,
    Language::Italian,
    Language::Polish,
    Language::Portuguese,
    Language::Russian,
    Language::Spanish,
    Language::Swedish,
    Language::Vietnamese,
];

impl Language {
    /// Every language, English first.
    pub fn all() -> impl Iterator<Item = Language> {
        ALL.into_iter()
    }

    /// Its code, the directory the games' translations keep its text in (`text/<name>/dialog`), and the code page of
    /// those message files (as BGforge's msg2po writes them).
    fn row(self) -> (&'static str, &'static str, &'static encoding_rs::Encoding) {
        use encoding_rs::{WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1258};
        match self {
            Language::English => ("en", "english", WINDOWS_1252),
            Language::Czech => ("cs", "czech", WINDOWS_1250),
            Language::French => ("fr", "french", WINDOWS_1252),
            Language::German => ("de", "german", WINDOWS_1252),
            Language::Hungarian => ("hu", "hungarian", WINDOWS_1250),
            Language::Italian => ("it", "italian", WINDOWS_1252),
            Language::Polish => ("pl", "polish", WINDOWS_1250),
            Language::Portuguese => ("pt", "portuguese", WINDOWS_1252),
            Language::Russian => ("ru", "russian", WINDOWS_1251),
            Language::Spanish => ("es", "spanish", WINDOWS_1252),
            Language::Swedish => ("sv", "swedish", WINDOWS_1252),
            Language::Vietnamese => ("vi", "vietnamese", WINDOWS_1258),
        }
    }

    /// Its ISO 639-1 code, which eSpeak names it by too.
    pub fn code(self) -> &'static str {
        self.row().0
    }

    /// The name of the directory its text sits in (`text/german/dialog`).
    pub fn directory(self) -> &'static str {
        self.row().1
    }

    /// The code page of its message files.
    pub fn encoding(self) -> &'static encoding_rs::Encoding {
        self.row().2
    }

    /// The language a code (`de`) or a directory name (`german`) names, in any case.
    pub fn parse(name: &str) -> Option<Language> {
        Language::all().find(|l| l.code().eq_ignore_ascii_case(name) || l.directory().eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_is_listed_once_and_named_both_ways() {
        let all: Vec<Language> = Language::all().collect();
        for l in &all {
            assert_eq!(all.iter().filter(|x| *x == l).count(), 1, "{l:?}");
            assert_eq!(Language::parse(l.code()), Some(*l));
            assert_eq!(Language::parse(l.directory()), Some(*l));
        }
    }

    #[test]
    fn code_pages_follow_msg2po() {
        assert_eq!(Language::Russian.encoding(), encoding_rs::WINDOWS_1251);
        assert_eq!(Language::Polish.encoding(), encoding_rs::WINDOWS_1250);
        assert_eq!(Language::Vietnamese.encoding(), encoding_rs::WINDOWS_1258);
        assert_eq!(Language::German.encoding(), encoding_rs::WINDOWS_1252);
    }
}
