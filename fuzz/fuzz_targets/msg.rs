//! The dialog message file reader, on any bytes.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = ipsdoc::msg::parse(data);
});
