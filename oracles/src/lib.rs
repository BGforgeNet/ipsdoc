#![warn(
    missing_docs,
    unreachable_pub,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]
//! Third-party ACM decoders as test references: the Fallout 2 engine's decoder (fallout2-ce), GemRB's, libacm and
//! the `infinitier` crate. Each decodes a bare ACM to interleaved 16-bit samples.
//!
//! The first three are command-line programs built once by `oracles/build.sh` into the directory named by
//! `IPSDOC_ORACLES`; the last is a crate dependency.

use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

/// A decoder rejected its input, or could not be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The decoder that failed.
    pub oracle: Oracle,
    /// What it reported.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.oracle, self.message)
    }
}

impl std::error::Error for Error {}

impl Oracle {
    /// Every reference decoder.
    pub const ALL: [Oracle; 4] = [Oracle::Fallout2Ce, Oracle::GemRb, Oracle::Libacm, Oracle::Infinitier];

    /// Decodes a bare ACM to interleaved samples, as many as the decoder produces up to the header's count.
    ///
    /// # Errors
    ///
    /// When the decoder rejects the input, or its program is missing or fails to run.
    pub fn decode(self, acm: &[u8]) -> Result<Vec<i16>, Error> {
        let fail = |message: String| Error { oracle: self, message };
        let program = match self {
            Oracle::Fallout2Ce => "acm-fallout2ce",
            Oracle::GemRb => "acm-gemrb",
            Oracle::Libacm => "acm-libacm",
            Oracle::Infinitier => return infinitier(acm).map_err(|e| fail(e.to_string())),
        };
        let dir = std::env::var_os("IPSDOC_ORACLES")
            .ok_or_else(|| fail("IPSDOC_ORACLES is unset; run oracles/build.sh <dir> and point it there".into()))?;
        let path = PathBuf::from(dir).join("bin").join(program);
        let mut child = Command::new(&path)
            .args(["-", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| fail(format!("cannot run {}: {e}", path.display())))?;
        let mut stdin = child.stdin.take().ok_or_else(|| fail("no stdin pipe".into()))?;
        // Write from a thread: the harness reads all input before writing, but a large input must not deadlock
        // against a full stdout pipe either way.
        let input = acm.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let out = child
            .wait_with_output()
            .map_err(|e| fail(format!("{} failed: {e}", path.display())))?;
        writer
            .join()
            .map_err(|_| fail("stdin writer panicked".into()))?
            .map_err(|e| fail(format!("writing stdin: {e}")))?;
        if !out.status.success() {
            return Err(fail(format!(
                "{}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(out
            .stdout
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| i16::from_le_bytes(b))
            .collect())
    }
}

/// Reads sample by sample rather than through `decode_all`, which zero-fills a short stream to the header's count.
fn infinitier(acm: &[u8]) -> Result<Vec<i16>, infinitier_acm_decoder::AcmError> {
    let count = acm
        .get(4..8)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
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
