//! Dialog text to words: numbers and money spelled out, stage directions dropped or kept as noises, punctuation as
//! phrase breaks.

/// A piece of normalised text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// A word, upper case: letters and apostrophes.
    Word(String),
    /// A word written in capitals in the text (`NCR`), which may be spelled out.
    Capitals(String),
    /// A phrase break: `, . ! ? ; :`, an ellipsis or a dash.
    Break,
    /// A bracketed vocal noise: `(cough)`, `(sigh)`, `*Cough*`.
    Noise,
}

/// Words that, alone in brackets or asterisks, are noises the actor makes rather than a direction.
pub const NOISES: &[&str] = &[
    "AH",
    "AHEM",
    "CHUCKLE",
    "CHUCKLES",
    "CHUCKLING",
    "COUGH",
    "COUGHS",
    "COUGHING",
    "GASP",
    "GASPS",
    "GROAN",
    "GROANS",
    "GRUNT",
    "GRUNTS",
    "HA",
    "HAH",
    "HEH",
    "HMM",
    "HMMM",
    "HUH",
    "LAUGH",
    "LAUGHS",
    "LAUGHING",
    "MUMBLE",
    "MUMBLES",
    "SCREAM",
    "SCREAMS",
    "SIGH",
    "SIGHS",
    "SNIFF",
    "SNIFFS",
    "SNORT",
    "SNORTS",
    "SOB",
    "SOBS",
    "WHEEZE",
    "WHEEZES",
    "YAWN",
    "YAWNS",
];

/// Stands for a noise in the text between bracket handling and tokenising.
const NOISE: char = '\u{1}';

/// Abbreviations said as words, when a full stop follows them.
const ABBREVIATIONS: &[(&str, &str)] = &[("DR", "DOCTOR"), ("MR", "MISTER"), ("MRS", "MISSUS"), ("MS", "MIZ")];

/// Normalises a line of dialog.
pub fn normalise(text: &str) -> Vec<Piece> {
    tokenise(&brackets(&fold(text)))
}

/// Typographic punctuation to ASCII, accented Latin letters to plain ones, anything else non-ASCII to a blank.
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '`' => out.push('\''),
            '\u{201C}' | '\u{201D}' => out.push('"'),
            '\u{2013}' | '\u{2014}' => out.push_str(" -- "),
            '\u{2026}' => out.push_str("..."),
            c if c.is_ascii() => out.push(c),
            c => out.push(match c.to_lowercase().next().unwrap_or(c) {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
                'ç' => 'c',
                'è' | 'é' | 'ê' | 'ë' => 'e',
                'ì' | 'í' | 'î' | 'ï' => 'i',
                'ñ' => 'n',
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
                'ù' | 'ú' | 'û' | 'ü' => 'u',
                'ý' | 'ÿ' => 'y',
                _ => ' ',
            }),
        }
    }
    out
}

/// `(...)` and `[...]` become a noise when they hold only [`NOISES`], else nothing; `*...*` becomes a noise, or stays
/// as text (emphasis).
fn brackets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['(', '[', '*']) {
        out.push_str(&rest[..i]);
        let open = rest.as_bytes()[i];
        let close = match open {
            b'(' => ')',
            b'[' => ']',
            _ => '*',
        };
        let inner_start = i + 1;
        let (inner, after) = match rest[inner_start..].find(close) {
            Some(j) => (&rest[inner_start..inner_start + j], &rest[inner_start + j + 1..]),
            // Unclosed: a typo, not a direction; the bracket goes and its text stays.
            None => {
                out.push(' ');
                rest = &rest[inner_start..];
                continue;
            }
        };
        let words: Vec<String> = inner
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| !w.is_empty())
            .map(str::to_ascii_uppercase)
            .collect();
        if !words.is_empty() && words.iter().all(|w| NOISES.contains(&w.as_str())) {
            out.push(' ');
            out.push(NOISE);
            out.push(' ');
        } else if open == b'*' {
            out.push_str(inner);
        } else {
            out.push(' ');
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn tokenise(text: &str) -> Vec<Piece> {
    let b = text.as_bytes();
    let mut out: Vec<Piece> = Vec::new();
    let push = |out: &mut Vec<Piece>, p: Piece| {
        // No break first, none twice.
        if p == Piece::Break && matches!(out.last(), None | Some(Piece::Break)) {
            return;
        }
        out.push(p);
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() || (c == b'\'' && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic)) {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'\'') {
                i += 1;
            }
            let raw = &text[start..i];
            let upper = raw.to_ascii_uppercase();
            if b.get(i) == Some(&b'.')
                && let Some(&(_, said)) = ABBREVIATIONS.iter().find(|(a, _)| *a == upper)
            {
                push(&mut out, Piece::Word(said.to_owned()));
                i += 1;
                continue;
            }
            let letters = raw.bytes().filter(u8::is_ascii_alphabetic).count();
            let capitals = letters > 1 && !raw.bytes().any(|c| c.is_ascii_lowercase());
            push(
                &mut out,
                if capitals {
                    Piece::Capitals(upper)
                } else {
                    Piece::Word(upper)
                },
            );
        } else if c.is_ascii_digit() || (c == b'$' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let dollars = c == b'$';
            if dollars {
                i += 1;
            }
            let mut n: u64 = 0;
            while i < b.len() {
                if b[i].is_ascii_digit() {
                    n = n.saturating_mul(10).saturating_add(u64::from(b[i] - b'0'));
                    i += 1;
                } else if b[i] == b','
                    && b.len() >= i + 4
                    && b[i + 1..i + 4].iter().all(u8::is_ascii_digit)
                    && !b.get(i + 4).is_some_and(u8::is_ascii_digit)
                {
                    i += 1;
                } else {
                    break;
                }
            }
            for w in number_words(n) {
                push(&mut out, Piece::Word(w.to_ascii_uppercase()));
            }
            if dollars {
                push(
                    &mut out,
                    Piece::Word(if n == 1 { "DOLLAR" } else { "DOLLARS" }.to_owned()),
                );
            }
        } else {
            match c {
                b',' | b'.' | b'!' | b'?' | b';' | b':' => push(&mut out, Piece::Break),
                // A dash: doubled, or set off by blanks; a hyphen inside a word only separates.
                b'-' if b.get(i + 1) == Some(&b'-')
                    || (i > 0 && b[i - 1] == b' ' && b.get(i + 1).is_none_or(|&n| n == b' ')) =>
                {
                    push(&mut out, Piece::Break)
                }
                b'&' => push(&mut out, Piece::Word("AND".to_owned())),
                b'%' => push(&mut out, Piece::Word("PERCENT".to_owned())),
                x if char::from(x) == NOISE => push(&mut out, Piece::Noise),
                _ => {}
            }
            i += 1;
        }
    }
    out
}

const UNITS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// A whole number in words, as said in dialog: years and the like (1100 to 2999, not on a hundred, and not 2001 to
/// 2009) in pairs ("twenty two forty one"), round hundreds of them as hundreds ("fifteen hundred"), the rest as a
/// cardinal ("three hundred fifty").
pub fn number_words(n: u64) -> Vec<&'static str> {
    let mut out = Vec::new();
    if (1100..=2999).contains(&n) && !n.is_multiple_of(100) && !(2001..=2009).contains(&n) {
        below_hundred(n / 100, &mut out);
        if n % 100 < 10 {
            out.push("oh");
        }
        below_hundred(n % 100, &mut out);
    } else if (1100..=9900).contains(&n) && n.is_multiple_of(100) && !n.is_multiple_of(1000) {
        below_hundred(n / 100, &mut out);
        out.push("hundred");
    } else if n == 0 {
        out.push("zero");
    } else {
        cardinal(n, &mut out);
    }
    out
}

fn below_hundred(n: u64, out: &mut Vec<&'static str>) {
    if n < 20 {
        out.push(UNITS[n as usize]);
    } else {
        out.push(TENS[(n / 10) as usize]);
        if !n.is_multiple_of(10) {
            out.push(UNITS[(n % 10) as usize]);
        }
    }
}

/// A non-zero number as a cardinal.
fn cardinal(n: u64, out: &mut Vec<&'static str>) {
    let mut rest = n;
    for (scale, name) in [
        (1_000_000_000_000_000_000, "quintillion"),
        (1_000_000_000_000_000, "quadrillion"),
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ] {
        if rest >= scale {
            cardinal(rest / scale, out);
            out.push(name);
            rest %= scale;
        }
    }
    if rest >= 100 {
        out.push(UNITS[(rest / 100) as usize]);
        out.push("hundred");
        rest %= 100;
    }
    if rest > 0 {
        below_hundred(rest, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> String {
        normalise(text)
            .iter()
            .map(|p| match p {
                Piece::Word(w) => w.clone(),
                Piece::Capitals(w) => format!("<{w}>"),
                Piece::Break => "|".to_owned(),
                Piece::Noise => "~".to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn money_and_breaks() {
        // vccasidy.msg 118 (Cas0118).
        assert_eq!(
            words("Beer for $20, shot of whiskey for $30. Nothing special. Why, you want something?"),
            "BEER FOR TWENTY DOLLARS | SHOT OF WHISKEY FOR THIRTY DOLLARS | NOTHING SPECIAL | WHY | YOU WANT SOMETHING |"
        );
        assert_eq!(words("only $1"), "ONLY ONE DOLLAR");
    }

    #[test]
    fn numbers() {
        // hcmarcus.msg 176 ($500), kcsulik.msg 122 ($350), qhprzrch.msg 228 (Vault 13), vclynett.msg 272 (120 years).
        assert_eq!(words("get $500."), "GET FIVE HUNDRED DOLLARS |");
        assert_eq!(words("$350"), "THREE HUNDRED FIFTY DOLLARS");
        assert_eq!(words("Vault 13"), "VAULT THIRTEEN");
        assert_eq!(words("120 years"), "ONE HUNDRED TWENTY YEARS");
        assert_eq!(words("in 2241"), "IN TWENTY TWO FORTY ONE");
        assert_eq!(words("2077"), "TWENTY SEVENTY SEVEN");
        assert_eq!(words("2005"), "TWO THOUSAND FIVE");
        assert_eq!(words("1500"), "FIFTEEN HUNDRED");
        assert_eq!(words("1,000 caps"), "ONE THOUSAND CAPS");
        assert_eq!(words("0"), "ZERO");
    }

    #[test]
    fn noises_and_directions() {
        // qcfrank.msg 108, qhprzrch.msg 135, 169 and 218.
        assert_eq!(words("You (cough), you haven't won"), "YOU ~ | YOU HAVEN'T WON");
        assert_eq!(words("(Heh, heh; cough, cough...) The work"), "~ THE WORK");
        assert_eq!(words("all. (talking to intercom) Hello."), "ALL | HELLO |");
        assert_eq!(words("an *unexpected guest*."), "AN UNEXPECTED GUEST |");
        assert_eq!(words("*Cough* Yes"), "~ YES");
        assert_eq!(words("[He looks you up and down distastefully.] Well"), "WELL");
        assert_eq!(words("age. (chuckles)"), "AGE | ~");
        // An unclosed bracket is a typo, not a direction: its text stays.
        assert_eq!(words("Hello (there, friend"), "HELLO THERE | FRIEND");
    }

    #[test]
    fn dashes_hyphens_and_ellipses() {
        // vclynett.msg 122 and 629, qcfrank.msg 108.
        assert_eq!(words("things - and that"), "THINGS | AND THAT");
        assert_eq!(words("City -- even"), "CITY | EVEN");
        assert_eq!(words("mutie-bastard friends"), "MUTIE BASTARD FRIENDS");
        assert_eq!(words("honor... courage..."), "HONOR | COURAGE |");
        assert_eq!(words("a \u{2014} b"), "A | B");
    }

    #[test]
    fn apostrophes_abbreviations_and_capitals() {
        assert_eq!(words("I ain't seen"), "I AIN'T SEEN");
        assert_eq!(words("ain\u{2019}t"), "AIN'T");
        // Kept: the dictionary has 'em, gettin' and ol' as words.
        assert_eq!(words("Find 'em"), "FIND 'EM");
        assert_eq!(words("gettin' there"), "GETTIN' THERE");
        // vclynett.msg 516.
        assert_eq!(words("Dr. Troy?!"), "DOCTOR TROY |");
        assert_eq!(words("Mr. Bishop"), "MISTER BISHOP");
        assert_eq!(words("the NCR and REAL whiskey"), "THE <NCR> AND <REAL> WHISKEY");
        assert_eq!(words("I"), "I", "a lone capital is a word");
        assert_eq!(words("caf\u{e9}"), "CAFE");
    }

    #[test]
    fn number_words_by_kind() {
        assert_eq!(number_words(7).join(" "), "seven");
        assert_eq!(number_words(1999).join(" "), "nineteen ninety nine");
        assert_eq!(number_words(1905).join(" "), "nineteen oh five");
        assert_eq!(number_words(2000).join(" "), "two thousand");
        assert_eq!(number_words(3000).join(" "), "three thousand");
        assert_eq!(number_words(1_000_001).join(" "), "one million one");
    }
}
