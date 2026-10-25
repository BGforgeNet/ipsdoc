//! The dialog message file reader, on any bytes, in every language's code page: the first byte picks the language.
#![no_main]

use ipsdoc::language::Language;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else {
        return;
    };
    let languages: Vec<Language> = Language::all().collect();
    let language = languages[usize::from(pick) % languages.len()];
    let _ = ipsdoc::msg::parse(rest, language);
});
