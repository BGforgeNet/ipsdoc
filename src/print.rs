//! Printing for the binaries that survives a closed pipe.
//!
//! std's `println!` and `eprintln!` panic once the reader has gone (`ipsdoc info *.acm | head -1`), as Rust ignores
//! SIGPIPE. A binary imports these instead (`use ipsdoc::{eprintln, println};`), which shadows std's. A closed pipe
//! drops the rest of the output and the work goes on, as `repair` and `lip` change files.

use std::io::Write;

/// `println!` that drops its line once stdout is a closed pipe; see [`print::line`](fn@crate::print::line).
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => { $crate::print::line(&mut ::std::io::stdout(), format_args!($($arg)*)) };
}

/// `eprintln!` that drops its line once stderr is a closed pipe; see [`print::line`](fn@crate::print::line).
#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => { $crate::print::line(&mut ::std::io::stderr(), format_args!($($arg)*)) };
}

/// Writes `args` and a newline to `w`. A closed pipe drops it; any other failure (a full disk) ends the process with
/// status 1, naming the program, since its output is incomplete.
pub fn line(w: &mut impl Write, args: std::fmt::Arguments<'_>) {
    put(w, format_args!("{args}\n"));
}

/// Writes `args` to `w` with no newline, flushed: a progress line redrawn in place with `\r`. Failures as [`line`](fn@line).
pub fn text(w: &mut impl Write, args: std::fmt::Arguments<'_>) {
    put(w, args);
}

fn put(w: &mut impl Write, args: std::fmt::Arguments<'_>) {
    if let Err(e) = w.write_fmt(args).and_then(|()| w.flush())
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        let program = std::env::args_os()
            .next()
            .and_then(|p| {
                std::path::Path::new(&p)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let _ = writeln!(std::io::stderr(), "{program}: writing output: {e}");
        std::process::exit(1);
    }
}
