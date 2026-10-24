//! The ACM decoder, on any bytes.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = ipsdoc::acm::decode(data);
});
