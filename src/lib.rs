#![warn(
    missing_docs,
    unreachable_pub,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]
//! Interplay ACM, WAVC and game sound file handling.
//!
//! Sounds: [`detect`] names a file's container by its first bytes; [`input`] reads it into [`pcm::Audio`], ACM and
//! WAVC through [`acm`] and [`wavc`] with [`analysis`] weighing their headers against the content; [`output`]
//! writes audio in any container; [`repair`] corrects headers in place from what reading established.
//!
//! Lip sync: [`msg`] finds a voiced line's text, [`g2p`] turns it into phonemes, [`speech`] finds where the sound
//! speaks, and [`lipgen`] aligns the two (or works from the sound alone) into a [`lip::Lip`], the file format [`lip`]
//! reads, writes and checks; [`lipqual`] judges how well one follows its sound.
//!
//! The binaries print through [`print`](mod@print), which survives a closed pipe.

pub mod acm;
pub mod analysis;
pub mod detect;
pub mod g2p;
pub mod input;
pub mod lip;
pub mod lipgen;
pub mod lipqual;
pub mod msg;
pub mod output;
pub mod pcm;
pub mod print;
pub mod repair;
pub mod speech;
pub mod wavc;
