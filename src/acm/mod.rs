//! Interplay ACM: the header, the bitstream and its codec.

pub(crate) mod bitreader;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the encoder's writer; only tests use it until the encoder lands"
    )
)]
pub(crate) mod bitwriter;
pub mod decode;
pub mod header;

pub use decode::{DecodeError, Decoded, Overflow, Stats, decode, decode_with};
pub use header::{Header, HeaderError};
