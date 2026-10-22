//! `ipsdoc`: converts, inspects, checks and repairs Interplay ACM, WAVC and other game sound files.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

use ipsdoc::acm::Overflow;
use ipsdoc::detect::{Format, detect};
use ipsdoc::input::{self, Note, Raw, ReadOptions};
use ipsdoc::output::{self, OutputFormat, WriteOptions};
use ipsdoc::repair::{self, Field};

/// Converts, inspects, checks and repairs Interplay ACM, WAVC and other game sound files.
///
/// `ipsdoc IN [OUT]` converts, as `ipsdoc convert` does.
#[derive(Debug, Parser)]
#[command(version, args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    convert: Option<ConvertArgs>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Converts any readable sound to ACM (lossless), WAVC, RIFF WAV or Ogg Vorbis.
    Convert(ConvertArgs),
    /// Shows a sound's headers, how it reads, and what in its headers is wrong.
    Info {
        /// Files to inspect.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Decodes sounds (directories: every sound inside, recursively) and fails on any that does not decode.
    Check {
        /// Files or directories.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Rewrites wrong ACM and WAVC header fields in place, without re-encoding.
    Repair {
        /// Show the fixes without writing them.
        #[arg(long)]
        dry_run: bool,
        /// Files to repair.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum To {
    /// Bare ACM, lossless.
    Acm,
    /// WAVC, lossless (Infinity Engine sounds).
    Wavc,
    /// RIFF WAV PCM, at the source's depth.
    Wav,
    /// Ogg Vorbis.
    Ogg,
}

#[derive(Debug, Args)]
struct ConvertArgs {
    /// The sound to read.
    input: PathBuf,
    /// Where to write; by default beside the input, as `.wav` for an ACM or WAVC input, else `.acm`.
    output: Option<PathBuf>,
    /// The output format; by default from the output's extension (`.acm`, `.wavc`, `.wav`, `.ogg`).
    #[arg(long, value_enum)]
    to: Option<To>,
    /// Resample to this rate.
    #[arg(long)]
    rate: Option<u32>,
    /// Read the input as having this many channels, whatever its header and content say.
    #[arg(long)]
    channels: Option<u16>,
    /// Read the input as having this rate, whatever its header says.
    #[arg(long)]
    input_rate: Option<u32>,
    /// Read the input as headerless PCM (see --raw-rate, --raw-channels, --raw-bits).
    #[arg(long)]
    raw: bool,
    /// Raw input's rate.
    #[arg(long, default_value_t = 22050, requires = "raw")]
    raw_rate: u32,
    /// Raw input's channels.
    #[arg(long, default_value_t = 1, requires = "raw")]
    raw_channels: u16,
    /// Raw input's bits per sample: 16 (signed little-endian) or 8 (unsigned).
    #[arg(long, default_value_t = 16, requires = "raw")]
    raw_bits: u16,
    /// Vorbis quality, -1 to 10.
    #[arg(long, default_value_t = 5.0)]
    quality: f32,
    /// Wrap ACM samples beyond 16 bits as the engines do (an audible click), instead of clamping them.
    #[arg(long)]
    wrap: bool,
}

/// A failure, with its exit code: 1 for a bad file, 2 for a bad request.
struct Failure(u8, String);

fn fail(code: u8, message: impl Into<String>) -> Failure {
    Failure(code, message.into())
}

/// An error and its causes, outermost first.
fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        s.push_str(": ");
        s.push_str(&cause.to_string());
        source = cause.source();
    }
    s
}

fn read_file(path: &Path) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|e| fail(1, format!("{}: {e}", path.display())))
}

fn convert(a: &ConvertArgs) -> Result<(), Failure> {
    let bytes = read_file(&a.input)?;
    let output = match &a.output {
        Some(o) => o.clone(),
        None => a.input.with_extension(match detect(&bytes) {
            Format::Acm | Format::Wavc => "wav",
            Format::Riff | Format::Ogg | Format::Other => "acm",
        }),
    };
    let same = match (std::fs::canonicalize(&a.input), std::fs::canonicalize(&output)) {
        (Ok(i), Ok(o)) => i == o,
        _ => false,
    };
    if same {
        return Err(fail(
            2,
            format!(
                "{}: the output would overwrite the input; name another",
                output.display()
            ),
        ));
    }
    let format = match a.to {
        Some(To::Acm) => OutputFormat::Acm,
        Some(To::Wavc) => OutputFormat::Wavc,
        Some(To::Wav) => OutputFormat::Riff,
        Some(To::Ogg) => OutputFormat::Ogg,
        None => OutputFormat::from_path(&output).ok_or_else(|| {
            fail(
                2,
                format!("{}: no output format for this extension; use --to", output.display()),
            )
        })?,
    };
    let opts = ReadOptions {
        overflow: if a.wrap { Overflow::Wrap } else { Overflow::Clamp },
        channels: a.channels,
        rate: a.input_rate,
        raw: a.raw.then_some(Raw {
            rate: a.raw_rate,
            channels: a.raw_channels,
            bits: a.raw_bits,
        }),
    };
    let loaded = input::read(&bytes, &opts).map_err(|e| fail(1, format!("{}: {}", a.input.display(), chain(&e))))?;
    for n in &loaded.notes {
        eprintln!("{}: {}", a.input.display(), describe(n));
    }
    let audio = match a.rate {
        Some(r) => loaded.audio.resample(r).map_err(|e| fail(1, chain(&e)))?,
        None => loaded.audio,
    };
    let out = output::write(
        &audio,
        format,
        &WriteOptions {
            vorbis_quality: a.quality,
        },
    )
    .map_err(|e| fail(1, format!("{}: {}", output.display(), chain(&e))))?;
    std::fs::write(&output, out).map_err(|e| fail(1, format!("{}: {e}", output.display())))?;
    Ok(())
}

fn describe(n: &Note) -> String {
    match n {
        Note::Channels { header, used, evidence } => {
            let why = match evidence {
                input::ChannelEvidence::OddSampleCount => "odd sample count".to_owned(),
                input::ChannelEvidence::InterleavedMono { score } => {
                    format!("content is interleaved mono, score {score:.2}")
                }
                input::ChannelEvidence::Stereo { score } => format!("content is stereo, score {score:.2}"),
                input::ChannelEvidence::HeaderMono
                | input::ChannelEvidence::Undecidable
                | input::ChannelEvidence::NotChecked
                | input::ChannelEvidence::WavcHeaders => format!("{evidence:?}"),
            };
            format!("channels: header {header}, reads as {used} ({why})")
        }
        Note::SwappedRate { stored, used } => format!("rate {stored} is {used} byte-swapped; reads as {used} Hz"),
        Note::Overflowed { samples } => format!("{samples} samples beyond 16 bits"),
    }
}

fn field_name(f: Field) -> &'static str {
    match f {
        Field::AcmChannels => "ACM channels",
        Field::AcmRate => "ACM rate",
        Field::WavcChannels => "WAVC channels",
        Field::WavcRate => "WAVC rate",
        Field::WavcUncompressed => "WAVC uncompressed size",
        Field::WavcCompressed => "WAVC compressed size",
        Field::WavcBits => "WAVC bits",
    }
}

fn info(path: &Path) -> Result<(), Failure> {
    let bytes = read_file(path)?;
    let format = detect(&bytes);
    println!("{}: {}", path.display(), format.name());
    if format == Format::Wavc
        && let Ok(w) = ipsdoc::wavc::Header::parse(&bytes)
    {
        println!(
            "  WAVC: {} channels, {} Hz, {} bits, sizes {} / {}, ACM at {}, field 0x1A = {:#06x}",
            w.channels, w.rate, w.bits, w.uncompressed, w.compressed, w.acm_offset, w.unknown
        );
    }
    let acm = match format {
        Format::Acm => Some(bytes.as_slice()),
        Format::Wavc => ipsdoc::wavc::acm_of(&bytes).ok(),
        Format::Riff | Format::Ogg | Format::Other => None,
    };
    if let Some(h) = acm.and_then(|a| ipsdoc::acm::Header::parse(a).ok()) {
        println!(
            "  ACM: {} samples, {} channels, {} Hz, levels {}, rows {}",
            h.samples, h.channels, h.rate, h.levels, h.rows
        );
    }
    let loaded = input::read(&bytes, &ReadOptions::default())
        .map_err(|e| fail(1, format!("{}: {}", path.display(), chain(&e))))?;
    let a = &loaded.audio;
    let depth = a.bits().map_or("float".to_owned(), |b| format!("{b}-bit"));
    println!(
        "  reads as: {} channels, {} Hz, {depth}, {} frames, {:.3} s",
        a.channels,
        a.rate,
        a.frames(),
        a.frames() as f64 / f64::from(a.rate.max(1))
    );
    for n in &loaded.notes {
        println!("  note: {}", describe(n));
    }
    if matches!(format, Format::Acm | Format::Wavc)
        && let Ok(fixes) = repair::fixes(&bytes)
    {
        for f in fixes {
            println!(
                "  wrong: {} is {}, should be {} (ipsdoc repair)",
                field_name(f.field),
                f.stored,
                f.correct
            );
        }
    }
    Ok(())
}

/// Every file under `path` (or `path` itself), sorted; a directory that cannot be listed goes to `errors`.
fn files_under(path: &Path, out: &mut Vec<PathBuf>, errors: &mut Vec<String>) {
    if !path.is_dir() {
        out.push(path.to_path_buf());
        return;
    }
    let mut entries = Vec::new();
    match std::fs::read_dir(path) {
        Ok(listing) => {
            for e in listing {
                match e {
                    Ok(e) => entries.push(e.path()),
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        Err(e) => errors.push(format!("{}: {e}", path.display())),
    }
    entries.sort();
    for e in entries {
        files_under(&e, out, errors);
    }
}

fn check(paths: &[PathBuf]) -> Result<(), Failure> {
    let (mut checked, mut failed) = (0u64, 0u64);
    for p in paths {
        let explicit = !p.is_dir();
        let (mut files, mut errors) = (Vec::new(), Vec::new());
        files_under(p, &mut files, &mut errors);
        for e in &errors {
            eprintln!("{e}");
        }
        failed += errors.len() as u64;
        for f in files {
            let bytes = match std::fs::read(&f) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{}: {e}", f.display());
                    failed += 1;
                    continue;
                }
            };
            // Inside a directory only sounds are checked; a file named outright is checked whatever it is.
            if !explicit && detect(&bytes) == Format::Other {
                continue;
            }
            checked += 1;
            if let Err(e) = input::read(&bytes, &ReadOptions::default()) {
                eprintln!("{}: {}", f.display(), chain(&e));
                failed += 1;
            }
        }
    }
    println!("{checked} sounds checked, {failed} failed");
    if failed > 0 {
        Err(fail(1, format!("{failed} failed")))
    } else {
        Ok(())
    }
}

fn repair_files(files: &[PathBuf], dry_run: bool) -> Result<(), Failure> {
    let mut failed = 0;
    for f in files {
        let mut bytes = match read_file(f) {
            Ok(b) => b,
            Err(Failure(_, message)) => {
                eprintln!("{message}");
                failed += 1;
                continue;
            }
        };
        let fixes = match repair::fixes(&bytes) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{}: {}", f.display(), chain(&e));
                failed += 1;
                continue;
            }
        };
        if fixes.is_empty() {
            println!("{}: nothing to repair", f.display());
            continue;
        }
        for x in &fixes {
            println!("{}: {} {} -> {}", f.display(), field_name(x.field), x.stored, x.correct);
        }
        if !dry_run {
            repair::apply(&mut bytes, &fixes);
            if let Err(e) = std::fs::write(f, &bytes) {
                eprintln!("{}: {e}", f.display());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        Err(fail(1, format!("{failed} not repaired")))
    } else {
        Ok(())
    }
}

/// `info` for each file; a file that fails is reported and the rest still shown.
fn info_files(files: &[PathBuf]) -> Result<(), Failure> {
    let mut failed = 0;
    for f in files {
        if let Err(Failure(_, message)) = info(f) {
            eprintln!("{message}");
            failed += 1;
        }
    }
    if failed > 0 {
        Err(fail(1, format!("{failed} unreadable")))
    } else {
        Ok(())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match (&cli.command, &cli.convert) {
        (Some(Command::Convert(a)), _) | (None, Some(a)) => convert(a),
        (Some(Command::Info { files }), _) => info_files(files),
        (Some(Command::Check { paths }), _) => check(paths),
        (Some(Command::Repair { dry_run, files }), _) => repair_files(files, *dry_run),
        (None, None) => Err(fail(2, "nothing to do; see --help")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure(code, message)) => {
            eprintln!("ipsdoc: {message}");
            ExitCode::from(code)
        }
    }
}
