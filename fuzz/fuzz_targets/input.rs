//! Reading any sound, as every command does, and the header repairs, on any bytes.
#![no_main]

use ipsdoc::input::{ReadOptions, read};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = read(data, &ReadOptions::default());
    let _ = ipsdoc::repair::fixes(data);
});
