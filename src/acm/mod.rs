//! Interplay ACM: the header, the bitstream and its codec.

pub(crate) mod bitreader;
pub(crate) mod bitwriter;
pub mod decode;
pub mod encode;
pub mod header;

pub use decode::{DecodeError, Decoded, Overflow, Stats, decode, decode_with};
pub use encode::{EncodeError, encode};
pub use header::{Header, HeaderError};
