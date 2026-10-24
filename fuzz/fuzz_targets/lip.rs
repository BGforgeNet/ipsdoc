//! The lip file reader and its checks, on any bytes.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(lip) = ipsdoc::lip::Lip::parse(data) {
        let _ = lip.validate(None);
    }
});
