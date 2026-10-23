#![warn(
    missing_docs,
    unreachable_pub,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]
//! Interplay ACM, WAVC and game sound file handling.

pub mod acm;
pub mod analysis;
pub mod detect;
pub mod input;
pub mod lip;
pub mod output;
pub mod pcm;
pub mod repair;
pub mod wavc;
