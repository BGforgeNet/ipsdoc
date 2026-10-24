//! The WAVC header and the ACM stream it points at, on any bytes.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = ipsdoc::wavc::Header::parse(data);
    if let Ok(acm) = ipsdoc::wavc::acm_of(data) {
        let _ = ipsdoc::acm::decode(acm);
    }
});
