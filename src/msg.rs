//! Fallout message files (`.msg`): entries `{number}{sound}{text}`, where a voiced line's sound field names its
//! speech file. Text outside braces (comments, blank lines) is ignored; an entry's text may span lines.

use unicode_normalization::UnicodeNormalization;

use crate::language::Language;

/// One entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its number.
    pub number: u32,
    /// The speech file's base name, empty for an unvoiced line.
    pub sound: String,
    /// The text, line breaks inside it read as spaces.
    pub text: String,
}

/// Why a message file does not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MsgError {
    /// A `{` with no `}` after it.
    #[error("line {line}: unterminated field")]
    Unterminated {
        /// Where the field starts, from 1.
        line: usize,
    },
    /// An entry whose first field is not a number.
    #[error("line {line}: entry number {field:?} is not a number")]
    BadNumber {
        /// Where the entry starts, from 1.
        line: usize,
        /// The field as read.
        field: String,
    },
    /// An entry with fewer than three fields.
    #[error("line {line}: entry has fewer than three fields")]
    ShortEntry {
        /// Where the entry starts, from 1.
        line: usize,
    },
}

/// The entry for a sound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<'a> {
    /// The first entry naming the sound.
    pub entry: &'a Entry,
    /// Whether a later entry names it with other text.
    pub conflicting: bool,
}

/// Reads a message file in `language`'s code page.
///
/// # Errors
///
/// When a field is unterminated, an entry is short, or an entry number is not a number.
pub fn parse(bytes: &[u8], language: Language) -> Result<Vec<Entry>, MsgError> {
    let text = decode(bytes, language);
    // Each field with the line it starts on.
    let mut fields: Vec<(usize, String)> = Vec::new();
    let mut line = 1;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            '{' => {
                let start = line;
                let mut field = String::new();
                loop {
                    match chars.next() {
                        None => return Err(MsgError::Unterminated { line: start }),
                        Some('}') => break,
                        Some(c) => {
                            if c == '\n' {
                                line += 1;
                            }
                            field.push(c);
                        }
                    }
                }
                fields.push((start, field));
            }
            _ => {}
        }
    }
    let mut entries = Vec::with_capacity(fields.len() / 3);
    for chunk in fields.chunks(3) {
        let [(line, number), (_, sound), (_, text)] = chunk else {
            return Err(MsgError::ShortEntry { line: chunk[0].0 });
        };
        let number = number.trim().parse().map_err(|_| MsgError::BadNumber {
            line: *line,
            field: number.clone(),
        })?;
        entries.push(Entry {
            number,
            sound: sound.trim().to_owned(),
            text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        });
    }
    Ok(entries)
}

/// Text in `language`'s code page (message files, and the transcripts beside speech files). Windows-1252's five
/// unassigned bytes read as the C1 controls of the same value. Composed (NFC), as cp1258 writes Vietnamese tones as
/// combining marks.
pub fn decode(bytes: &[u8], language: Language) -> String {
    language.encoding().decode_without_bom_handling(bytes).0.nfc().collect()
}

/// The entry whose sound field is `sound`, case-insensitively.
pub fn find<'a>(entries: &'a [Entry], sound: &str) -> Option<Found<'a>> {
    if sound.is_empty() {
        return None;
    }
    let mut named = entries.iter().filter(|e| e.sound.eq_ignore_ascii_case(sound));
    let entry = named.next()?;
    let conflicting = named.any(|e| e.text != entry.text);
    Some(Found { entry, conflicting })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An English file, as most tests read.
    fn parse(bytes: &[u8]) -> Result<Vec<Entry>, MsgError> {
        super::parse(bytes, Language::English)
    }

    #[test]
    fn decodes_each_language_in_its_code_page() {
        // "Гут" in cp1251, as UPU's russian/dialog files are written; the same bytes read as cp1252 are mojibake.
        let bytes = b"{1}{a}{\xc3\xf3\xf2}";
        assert_eq!(super::parse(bytes, Language::Russian).unwrap()[0].text, "Гут");
        assert_eq!(parse(bytes).unwrap()[0].text, "Ãóò");
        // "Bạn" in cp1258: an a, then the dot below as a combining mark, composed on reading.
        assert_eq!(
            super::parse(b"{1}{a}{Ba\xf2n}", Language::Vietnamese).unwrap()[0].text,
            "B\u{1ea1}n"
        );
    }

    // From the Unofficial Patch's data/text/english/dialog/vccasidy.msg (CRLF, as there) and zcbrahmn.msg.
    const CASSIDY: &[u8] = b"{101}{}{You see Cassidy.}\r\n\
{103}{Cas0103}{Well, now. I ain't seen you around before, stranger. Name's Cassidy. I run this place. What can I do for you?}\r\n\
{104}{Cas0104}{Well now, if it ain't the mysterious stranger. What can I do for you today?}\r\n";
    const COMMENTED: &[u8] = b"#############\r\n#  Message File: ZCBrahmn.msg  #\r\n\r\n\
{101}{}{The reason for brahmin having two heads is due to\r\nthe radiation.}\r\n";

    #[test]
    fn reads_entries() {
        let e = parse(CASSIDY).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(
            e[0],
            Entry {
                number: 101,
                sound: String::new(),
                text: "You see Cassidy.".to_owned()
            }
        );
        assert_eq!(e[1].sound, "Cas0103");
        assert!(e[1].text.starts_with("Well, now. I ain't"));
    }

    #[test]
    fn skips_comments_and_joins_lines() {
        let e = parse(COMMENTED).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(
            e[0].text,
            "The reason for brahmin having two heads is due to the radiation."
        );
    }

    #[test]
    fn finds_by_sound_whatever_the_case() {
        let e = parse(CASSIDY).unwrap();
        for name in ["cas0103", "CAS0103", "Cas0103"] {
            let f = find(&e, name).unwrap();
            assert_eq!(f.entry.number, 103);
            assert!(!f.conflicting);
        }
        assert_eq!(find(&e, "cas0105"), None);
        assert_eq!(find(&e, ""), None, "unvoiced lines are no sound's");
    }

    #[test]
    fn a_repeated_sound_takes_the_first_and_flags_other_text() {
        let same = parse(b"{1}{a}{Hi.}{2}{A}{Hi.}").unwrap();
        assert_eq!(
            find(&same, "a").map(|f| (f.entry.number, f.conflicting)),
            Some((1, false))
        );
        let other = parse(b"{1}{a}{Hi.}{2}{a}{Bye.}").unwrap();
        assert_eq!(
            find(&other, "a").map(|f| (f.entry.number, f.conflicting)),
            Some((1, true))
        );
    }

    #[test]
    fn decodes_windows_1252() {
        let e = parse(b"{1}{a}{ain\x92t \x96 caf\xe9}").unwrap();
        assert_eq!(e[0].text, "ain\u{2019}t \u{2013} caf\u{e9}");
    }

    #[test]
    fn reports_malformed_files() {
        assert_eq!(parse(b"{1}{a}{no end\r\n"), Err(MsgError::Unterminated { line: 1 }));
        assert_eq!(
            parse(b"\r\n{x}{a}{b}"),
            Err(MsgError::BadNumber {
                line: 2,
                field: "x".to_owned()
            })
        );
        assert_eq!(parse(b"{1}{a}"), Err(MsgError::ShortEntry { line: 1 }));
    }
}
