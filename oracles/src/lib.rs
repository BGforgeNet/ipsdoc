#![warn(
    missing_docs,
    unreachable_pub,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]
//! Third-party ACM decoders as test references: the Fallout 2 engine's decoder (fallout2-ce), GemRB's, libacm and
//! the `infinitier` crate. Each decodes a bare ACM to interleaved 16-bit samples.

use std::fmt;

unsafe extern "C" {
    fn ipsdoc_oracle_fallout2ce(data: *const u8, len: usize, out: *mut i16, cap: usize) -> std::ffi::c_long;
    fn ipsdoc_oracle_gemrb(data: *const u8, len: usize, out: *mut i16, cap: usize) -> std::ffi::c_long;
    fn ipsdoc_oracle_libacm(data: *const u8, len: usize, out: *mut i16, cap: usize) -> std::ffi::c_long;
}

/// A reference decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Oracle {
    /// The Fallout 2 engine's decoder, as decompiled by fallout2-ce.
    Fallout2Ce,
    /// GemRB's ACM importer, standing in for the Infinity Engine.
    GemRb,
    /// libacm.
    Libacm,
    /// The `infinitier_acm_decoder` crate.
    Infinitier,
}

/// A decoder rejected its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The decoder that failed.
    pub oracle: Oracle,
    /// What it reported.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} rejected the stream: {}", self.oracle, self.message)
    }
}

impl std::error::Error for Error {}

type CDecoder = unsafe extern "C" fn(*const u8, usize, *mut i16, usize) -> std::ffi::c_long;

impl Oracle {
    /// Every reference decoder.
    pub const ALL: [Oracle; 4] = [Oracle::Fallout2Ce, Oracle::GemRb, Oracle::Libacm, Oracle::Infinitier];

    /// Decodes a bare ACM to interleaved samples, as many as the decoder produces up to the header's count.
    ///
    /// # Errors
    ///
    /// When the input is shorter than an ACM header, or the decoder rejects it.
    pub fn decode(self, acm: &[u8]) -> Result<Vec<i16>, Error> {
        let fail = |message: String| Error { oracle: self, message };
        let count = acm
            .get(4..8)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| fail(format!("{} bytes, shorter than an ACM header", acm.len())))?;
        let c_decoder: CDecoder = match self {
            Oracle::Fallout2Ce => ipsdoc_oracle_fallout2ce,
            Oracle::GemRb => ipsdoc_oracle_gemrb,
            Oracle::Libacm => ipsdoc_oracle_libacm,
            Oracle::Infinitier => return infinitier(acm, count).map_err(|e| fail(e.to_string())),
        };
        let mut out = vec![0i16; count];
        // SAFETY: the harness reads `acm.len()` bytes from `acm` and writes at most `out.len()` samples to `out`.
        let got = unsafe { c_decoder(acm.as_ptr(), acm.len(), out.as_mut_ptr(), out.len()) };
        let got = usize::try_from(got).map_err(|_| fail(format!("error code {got}")))?;
        out.truncate(got);
        Ok(out)
    }
}

/// Reads sample by sample rather than through `decode_all`, which zero-fills a short stream to the header's count.
fn infinitier(acm: &[u8], count: usize) -> Result<Vec<i16>, infinitier_acm_decoder::AcmError> {
    let source = infinitier_datasource::DataSource::new(acm);
    let mut decoder =
        infinitier_acm_decoder::AcmDecoder::open(&source, infinitier_acm_decoder::OutputChannels::Original, "oracle")?;
    let mut out = vec![0i16; count];
    let mut got = 0;
    while got < count {
        match decoder.read_samples(&mut out[got..])? {
            0 => break,
            n => got += n,
        }
    }
    out.truncate(got);
    Ok(out)
}
