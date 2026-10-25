//! `ipsdoc`: converts, inspects, checks and repairs Interplay ACM, WAVC and other game sound files.

use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

use ipsdoc::acm::Overflow;
use ipsdoc::detect::{self, Format, detect};
use ipsdoc::input::{self, Note, Raw, RawBits, ReadOptions};
use ipsdoc::lip::{self, Lip};
use ipsdoc::output::{self, OutputFormat, WriteOptions};
use ipsdoc::pcm::Audio;
use ipsdoc::repair::{self, Field};
// Pipe-safe: std's panic once a reader has gone (`ipsdoc info *.acm | head -1`).
use ipsdoc::{eprintln, println};
use ipsdoc::{g2p, lipgen, lipqual, msg};

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
    /// Converts any readable sound to ACM (lossless, or an ACM or WAVC input's own stream under a new header when its
    /// samples stay as they are), WAVC, RIFF WAV or Ogg Vorbis.
    Convert(ConvertArgs),
    /// Shows a sound's headers, how it reads, and what in its headers is wrong.
    Info {
        /// Files to inspect.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Dialog message files, or directories of them, giving each line's text: each lip file's closed lips are
        /// then also shown against its line's words (English).
        #[arg(long)]
        msg: Vec<PathBuf>,
    },
    /// Decodes sounds and checks lip files (directories: every ACM, WAVC, RIFF, Ogg and lip file inside,
    /// recursively); fails on any that does not decode or check.
    Check {
        /// Files or directories.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Dialog message files, or directories of them, giving each line's text: each lip file is then also judged
        /// against its line's words (English).
        #[arg(long)]
        msg: Vec<PathBuf>,
    },
    /// Writes Fallout lip-sync files for speech: from each sound and its line's text (from a dialog MSG, or a
    /// transcript beside the sound), or from the sound alone; or rescales a lip file to another sound.
    Lip(LipArgs),
    /// Rewrites wrong ACM and WAVC header fields, and lip files' length fields, in place, without re-encoding.
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
    /// Bare ACM: lossless, or an ACM or WAVC input's own stream when its samples stay as they are.
    Acm,
    /// WAVC (Infinity Engine sounds), holding an ACM as above.
    Wavc,
    /// RIFF WAV PCM, 8 or 16-bit: deeper input is dithered to 16 bits, unless --full-depth.
    Wav,
    /// Ogg Vorbis.
    Ogg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RawBitsArg {
    /// Unsigned 8-bit.
    #[value(name = "8")]
    U8,
    /// Signed 16-bit little-endian.
    #[value(name = "16")]
    S16,
}

/// A Vorbis quality on oggenc's scale, refused at parse time rather than after the input is read.
fn vorbis_quality(s: &str) -> Result<f32, String> {
    let q: f32 = s.parse().map_err(|e| format!("{e}"))?;
    if (-1.0..=10.0).contains(&q) {
        Ok(q)
    } else {
        Err(format!("{q} is not within -1 to 10"))
    }
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
    rate: Option<NonZeroU32>,
    /// Read the input as having this many channels, whatever its header and content say.
    #[arg(long, conflicts_with = "raw")]
    channels: Option<NonZeroU16>,
    /// Read the input as having this rate, whatever its header says.
    #[arg(long, conflicts_with = "raw")]
    input_rate: Option<NonZeroU32>,
    /// Read the input as headerless PCM (see --raw-rate, --raw-channels, --raw-bits).
    #[arg(long)]
    raw: bool,
    /// Raw input's rate.
    #[arg(long, default_value = "22050", requires = "raw")]
    raw_rate: NonZeroU32,
    /// Raw input's channels.
    #[arg(long, default_value = "1", requires = "raw")]
    raw_channels: NonZeroU16,
    /// Raw input's bits per sample.
    #[arg(long, value_enum, default_value = "16", requires = "raw")]
    raw_bits: RawBitsArg,
    /// Vorbis quality, -1 to 10.
    #[arg(long, default_value_t = 5.0, value_parser = vorbis_quality, allow_negative_numbers = true)]
    quality: f32,
    /// Wrap ACM samples beyond 16 bits as the engines do (an audible click), instead of clamping them; ACM or WAVC
    /// output then keeps an ACM or WAVC input's stream even where it overflows.
    #[arg(long)]
    wrap: bool,
    /// Write a WAV at the input's depth (24 or 32-bit, or float) instead of dithering it to 16 bits. Infinity Engine
    /// games read WAV up to 16-bit.
    #[arg(long)]
    full_depth: bool,
}

#[derive(Debug, Args)]
struct LipArgs {
    /// Sounds to write lip files for: each gets a `.lip` beside it, replacing (and naming) any there. Positions
    /// follow each sound's own samples: give the ACM the game plays, or the WAV it is made from.
    #[arg(required_unless_present = "rescale")]
    sounds: Vec<PathBuf>,
    /// The dialog message file holding each sound's line, found by its sound field (`{103}{Cas0103}{...}`).
    /// Without it, a non-empty `.txt` beside the sound is the line's text; without that, the sound alone is used.
    #[arg(long, conflicts_with = "rescale")]
    msg: Option<PathBuf>,
    /// Where to write, for a single sound.
    #[arg(short, long, conflicts_with = "rescale")]
    output: Option<PathBuf>,
    /// A lip file to rescale in place, to fit the sound given with `--for`.
    #[arg(long, requires = "for_sound", conflicts_with = "sounds")]
    rescale: Option<PathBuf>,
    /// The sound `--rescale` fits the lip file to.
    #[arg(long = "for", requires = "rescale")]
    for_sound: Option<PathBuf>,
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

/// Replaces the file at `path` with `bytes` through a sibling file, on disk before it is renamed over the original, so
/// a failure or a power loss leaves either the original or the new file whole.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".ipsdoc-tmp");
    let tmp = PathBuf::from(tmp);
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        // Best effort: the original is intact either way, and the write error is what the caller reports.
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Refuses an `output` that is `input` itself, by any path to it.
fn refuse_overwriting(input: &Path, output: &Path) -> Result<(), Failure> {
    let same = match (std::fs::canonicalize(input), std::fs::canonicalize(output)) {
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
    Ok(())
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
    refuse_overwriting(&a.input, &output)?;
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
    if a.full_depth && format != OutputFormat::Riff {
        return Err(fail(2, "--full-depth is for WAV output only"));
    }
    let opts = ReadOptions {
        overflow: if a.wrap { Overflow::Wrap } else { Overflow::Clamp },
        channels: a.channels,
        rate: a.input_rate,
        raw: a.raw.then_some(Raw {
            rate: a.raw_rate,
            channels: a.raw_channels,
            bits: match a.raw_bits {
                RawBitsArg::U8 => RawBits::U8,
                RawBitsArg::S16 => RawBits::S16,
            },
        }),
    };
    let loaded = input::read(&bytes, &opts).map_err(|e| fail(1, format!("{}: {}", a.input.display(), chain(&e))))?;
    let mut overflowed = 0;
    for n in &loaded.notes {
        eprintln!("{}: {}", a.input.display(), describe(n));
        if let Note::Overflowed { samples } = n {
            overflowed += samples;
        }
    }
    // The ACM stream the input holds, which ACM and WAVC output keeps when the samples stay as they are.
    let stream = match loaded.format {
        Format::Acm => Some(bytes.as_slice()),
        Format::Wavc => ipsdoc::wavc::acm_of(&bytes).ok(),
        Format::Riff | Format::Ogg | Format::Other => None,
    }
    .filter(|_| matches!(format, OutputFormat::Acm | OutputFormat::Wavc))
    .filter(|_| a.rate.is_none_or(|r| r.get() == loaded.audio.rate));
    // A clamped sample differs from what the stream holds, so the stream is kept only when nothing was clamped.
    let clamped = stream.is_some() && !a.wrap && overflowed > 0;
    let audio = match a.rate {
        Some(r) => loaded.audio.resample(r.get()).map_err(|e| fail(1, chain(&e)))?,
        None => loaded.audio,
    };
    let write_opts = WriteOptions {
        vorbis_quality: a.quality,
        riff_full_depth: a.full_depth,
    };
    let out = match stream.filter(|_| !clamped) {
        Some(acm) => output::write_keeping(&audio, acm, format, &write_opts),
        None => output::write(&audio, format, &write_opts),
    }
    .map_err(|e| fail(1, format!("{}: {}", output.display(), chain(&e))))?;
    let replaced = output.exists();
    replace_file(&output, &out).map_err(|e| fail(1, format!("{}: {e}", output.display())))?;
    if replaced {
        println!("{}: replaced", output.display());
    }
    if format == OutputFormat::Riff && output::riff_dithers(&audio, &write_opts) {
        eprintln!(
            "{}: 16-bit, from {} input: Infinity Engine games read WAV up to 16-bit; add --full-depth to get lossless output",
            output.display(),
            depth(&audio)
        );
    }
    if clamped {
        eprintln!(
            "{}: lossless, from the input's samples with {overflowed} clamped to 16 bits; add --wrap to keep the original stream, which the games play wrapped",
            output.display()
        );
    }
    Ok(())
}

/// The sample format in words: `16-bit`, `float`.
fn depth(a: &Audio) -> String {
    a.bits().map_or_else(|| "float".to_owned(), |b| format!("{b}-bit"))
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
                input::ChannelEvidence::HeaderMono => "no stereo claim, so mono".to_owned(),
                input::ChannelEvidence::Undecidable => "content silent or constant".to_owned(),
                input::ChannelEvidence::NotChecked => "more than two channels, not checked".to_owned(),
                input::ChannelEvidence::WavcHeaders => "its WAVC and ACM headers agree".to_owned(),
            };
            format!("channels: header {header}, reads as {used} ({why})")
        }
        Note::Overflowed { samples } => format!("{samples} samples beyond 16 bits"),
    }
}

fn field_name(f: Field) -> &'static str {
    match f {
        Field::AcmChannels => "ACM channels",
        Field::WavcChannels => "WAVC channels",
        Field::WavcRate => "WAVC rate",
        Field::WavcUncompressed => "WAVC uncompressed size",
        Field::WavcCompressed => "WAVC compressed size",
        Field::WavcBits => "WAVC bits",
        Field::LipLength => "lip length field",
    }
}

fn info(path: &Path, entries: Option<&[msg::Entry]>) -> Result<(), Failure> {
    let bytes = read_file(path)?;
    if lip::looks_like(&bytes) {
        return lip_info(path, &bytes, entries);
    }
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
    let depth = depth(a);
    println!(
        "  reads as: {} channels, {} Hz, {depth}, {} frames, {:.3} s",
        a.channels,
        a.rate,
        a.frames(),
        a.frames() as f64 / f64::from(a.rate)
    );
    for n in &loaded.notes {
        println!("  note: {}", describe(n));
    }
    if matches!(format, Format::Acm | Format::Wavc)
        && let Ok(r) = repair::fixes_read(&bytes, &loaded)
    {
        for f in r.fixes {
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

/// Every file under `path` (or `path` itself), sorted; a directory that cannot be listed goes to `errors`. Symlinks
/// are followed, but a directory already in `walked` (reached again through a link: a loop, or a second link to the
/// same tree) is not walked again, so its files are checked once.
fn files_under(path: &Path, walked: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>, errors: &mut Vec<String>) {
    if !path.is_dir() {
        out.push(path.to_path_buf());
        return;
    }
    match std::fs::canonicalize(path) {
        Ok(real) => {
            if !walked.insert(real) {
                return;
            }
        }
        Err(e) => {
            errors.push(format!("{}: {e}", path.display()));
            return;
        }
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
        files_under(&e, walked, out, errors);
    }
}

/// A file's head, enough to tell a sound or a lip file by: a game directory holds archives of hundreds of megabytes,
/// which are not read past it.
fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(detect::HEAD_LEN.max(lip::HEADER_LEN) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The entries of the message files `paths` names, a directory standing for the `.msg` files in it.
fn load_msgs(paths: &[PathBuf]) -> Result<Vec<msg::Entry>, Failure> {
    let mut entries = Vec::new();
    for p in paths {
        let files = if p.is_dir() {
            let mut f: Vec<PathBuf> = std::fs::read_dir(p)
                .and_then(|d| d.map(|e| e.map(|e| e.path())).collect())
                .map_err(|e| fail(1, format!("{}: {e}", p.display())))?;
            f.retain(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("msg")));
            f.sort();
            f
        } else {
            vec![p.clone()]
        };
        for f in files {
            entries.extend(msg::parse(&read_file(&f)?).map_err(|e| fail(1, format!("{}: {e}", f.display())))?);
        }
    }
    Ok(entries)
}

fn check(paths: &[PathBuf], msgs: &[PathBuf]) -> Result<(), Failure> {
    let entries = load_msgs(msgs)?;
    let entries = (!msgs.is_empty()).then_some(entries.as_slice());
    let (mut checked, mut lips, mut failed) = (0u64, 0u64, 0u64);
    // Alignment per directory: a set of lip files is graded together, where chance evens out.
    let mut sets: BTreeMap<PathBuf, Vec<lipqual::Quality>> = BTreeMap::new();
    // The sounds lip files named and that decoded: checking them again as sounds would decode them twice.
    let mut decoded = HashSet::new();
    for p in paths {
        let explicit = !p.is_dir();
        let (mut files, mut errors) = (Vec::new(), Vec::new());
        files_under(p, &mut HashSet::new(), &mut files, &mut errors);
        for e in &errors {
            eprintln!("{e}");
        }
        failed += errors.len() as u64;
        // Lip files first, each with the sound it names; then the sounds.
        let mut sounds = Vec::new();
        for f in files {
            let head = match read_head(&f) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("{}: {e}", f.display());
                    failed += 1;
                    continue;
                }
            };
            if !lip::looks_like(&head) {
                // Inside a directory only sounds and lip files are checked; a file named outright is checked
                // whatever it is.
                if explicit || detect(&head) != Format::Other {
                    sounds.push(f);
                }
                continue;
            }
            lips += 1;
            let (ok, quality) = match std::fs::read(&f) {
                Ok(bytes) => check_lip(&f, &bytes, entries, &mut decoded),
                Err(e) => {
                    eprintln!("{}: {e}", f.display());
                    (false, None)
                }
            };
            failed += u64::from(!ok);
            if let Some(q) = quality {
                sets.entry(f.parent().unwrap_or(Path::new("")).to_path_buf())
                    .or_default()
                    .push(q);
            }
        }
        for f in sounds {
            if decoded.contains(&f) {
                checked += 1;
                continue;
            }
            let bytes = match std::fs::read(&f) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{}: {e}", f.display());
                    failed += 1;
                    continue;
                }
            };
            checked += 1;
            if let Err(e) = input::read(&bytes, &ReadOptions::default()) {
                eprintln!("{}: {}", f.display(), chain(&e));
                failed += 1;
            }
        }
    }
    for (dir, files) in &sets {
        let Some(g) = lipqual::grade(files) else { continue };
        let alignment = g.alignment.map_or("not judged".to_owned(), |a| format!("{a:.2}"));
        // Each further median, when enough files have it.
        let figure =
            |m: Option<f64>, show: &dyn Fn(f64) -> String| m.map_or(String::new(), |m| format!(", {}", show(m)));
        let more = [
            figure(g.openness, &|o| format!("width {o:.2}")),
            figure(g.open_while_quiet, &|q| {
                format!("mouth open through {:.0}% of the quiet", 100.0 * q)
            }),
            figure(g.movement, &|m| format!("{m:.1} shape changes a second")),
            figure(g.closures, &|c| format!("lips closed {c:.2} times per m, b and p")),
        ]
        .concat();
        println!(
            "{}: lip sync of {} lip files (medians): alignment {alignment}, mouth shut through {:.0}% of the loud \
             speech ({:.0}% closed lips){more}",
            dir.display(),
            g.files,
            100.0 * g.shut,
            100.0 * g.closed
        );
        for p in g.problems() {
            eprintln!("{}: warning: {p}", dir.display());
        }
    }
    println!("{checked} sounds checked, {lips} lip files checked, {failed} failed");
    if failed > 0 {
        Err(fail(1, format!("{failed} failed")))
    } else {
        Ok(())
    }
}

fn repair_files(files: &[PathBuf], dry_run: bool) -> Result<(), Failure> {
    let mut failed = 0;
    for f in files {
        let bytes = match read_file(f) {
            Ok(b) => b,
            Err(Failure(_, message)) => {
                eprintln!("{message}");
                failed += 1;
                continue;
            }
        };
        // A lip file's other problems are reported, not repaired.
        let found = if lip::looks_like(&bytes) {
            Lip::parse(&bytes).map_err(|e| e.to_string()).map(|l| {
                let kept = l.validate(Some(&base_name(f)));
                (repair::lip(&l), kept)
            })
        } else {
            repair::fixes(&bytes).map_err(|e| chain(&e)).map(|r| (r, Vec::new()))
        };
        let (r, kept) = match found {
            Ok(x) => x,
            Err(message) => {
                eprintln!("{}: {message}", f.display());
                failed += 1;
                continue;
            }
        };
        if r.fixes.is_empty() {
            println!("{}: nothing to repair", f.display());
        }
        for x in &r.fixes {
            let dry = if dry_run { " (dry run)" } else { "" };
            println!(
                "{}: {} {} -> {}{dry}",
                f.display(),
                field_name(x.field),
                x.stored,
                x.correct
            );
        }
        if !dry_run
            && !r.patches.is_empty()
            && let Err(e) = repair::write(f, &r.patches)
        {
            eprintln!("{}: {e}", f.display());
            failed += 1;
        }
        for p in kept {
            if !matches!(p, lip::Problem::Len { .. }) {
                println!("{}: not repaired: {}: {p}", f.display(), label(p.severity()));
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
fn info_files(files: &[PathBuf], msgs: &[PathBuf]) -> Result<(), Failure> {
    let entries = load_msgs(msgs)?;
    let entries = (!msgs.is_empty()).then_some(entries.as_slice());
    let mut failed = 0;
    for f in files {
        if let Err(Failure(_, message)) = info(f, entries) {
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

/// The sound the game plays with a lip file ([`Lip::sound_beside`]), decoded. `Ok(None)` when there is none; an
/// error when it does not decode.
fn named_sound(lip: &Path, l: &Lip) -> Result<Option<(PathBuf, ipsdoc::pcm::Audio)>, String> {
    let Some(p) = l.sound_beside(lip) else { return Ok(None) };
    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let audio = input::read(&bytes, &ReadOptions::default())
        .map_err(|e| format!("{}: {}", p.display(), chain(&e)))?
        .audio;
    Ok(Some((p, audio)))
}

fn base_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn label(s: lip::Severity) -> &'static str {
    match s {
        lip::Severity::Error => "error",
        lip::Severity::Warning => "warning",
    }
}

fn lip_info(path: &Path, bytes: &[u8], entries: Option<&[msg::Entry]>) -> Result<(), Failure> {
    println!("{}: lip", path.display());
    let l = Lip::parse(bytes).map_err(|e| fail(1, format!("{}: {e}", path.display())))?;
    println!(
        "  name {}, flags {}, length field {}, {} phonemes, {} markers, last marker {}",
        l.name(),
        l.flags,
        l.len,
        l.phonemes.len(),
        l.markers.len(),
        l.last()
    );
    let mut problems = Vec::new();
    match named_sound(path, &l) {
        Ok(Some((acm, audio))) => {
            let secs = audio.frames() as f64 / f64::from(audio.rate);
            let fit = lip::fit(l.last(), &audio);
            let fits = match fit {
                lip::Fit::Matches => "fits it".to_owned(),
                lip::Fit::Scaled(f) => format!("is {f}x its length"),
                lip::Fit::Off { ms } => format!("ends {}", off_by(ms, "it")),
            };
            println!("  sound {}: {secs:.3} s; the lip file {fits}", acm.display());
            if fit.for_this_sound() {
                let q = lipqual::quality(&l, &audio, line_tokens(entries, &l).as_deref());
                print_quality(&q);
                problems.extend(q.problems().iter().map(|p| (label(p.severity()), p.to_string())));
            } else {
                println!("  sync: not judged (made for another length, so its shapes are placed by another sound)");
            }
        }
        Ok(None) => println!("  no sound {} beside it", l.name()),
        Err(e) => println!("  sound unreadable: {e}"),
    }
    for p in l.validate(Some(&base_name(path))) {
        problems.push((label(p.severity()), p.to_string()));
    }
    for (severity, p) in problems {
        println!("  {severity}: {p}");
    }
    Ok(())
}

/// `info`'s figures for how a lip file follows its sound.
fn print_quality(q: &lipqual::Quality) {
    match (q.alignment, q.shut_while_loud, q.closed_while_loud) {
        (Some(a), Some(s), Some(c)) => println!(
            "  sync: alignment {a:.2} (0.5 is chance), mouth shut through {:.0}% of the loud speech ({:.0}% closed \
             lips)",
            100.0 * s,
            100.0 * c
        ),
        _ => println!(
            "  sync: not judged (no speech, codes the game misreads, or a mouth that is never open, or never at rest, \
             in speech)"
        ),
    }
    if let Some(o) = q.openness {
        println!("  width: how wide the mouth opens follows the loudness at rank correlation {o:.2} (0 is chance)");
    }
    for (offset, band) in [
        (q.offset, "60 to 300 ms late or 130 to 300 ms early"),
        (q.far_offset, "310 to 600 ms off"),
    ] {
        if let Some(o) = offset {
            let side = if o.ms < 0.0 { "early" } else { "late" };
            let chance = o
                .chance
                .map_or(String::new(), |c| format!(" (shuffled copies reach {c:.2})"));
            println!(
                "  offset: best alignment {band} is {:.2}, {:.0} ms {side}{chance}",
                o.alignment,
                o.ms.abs()
            );
        }
    }
    if let Some(s) = q.open_while_quiet {
        println!("  quiet: mouth open through {:.0}% of the quiet", 100.0 * s);
    }
    if let Some(m) = q.movement {
        println!("  movement: {m:.1} shape changes a second of speech");
    }
    if let Some(h) = q.longest_hold {
        println!(
            "  hold: one shape held through at most {:.0} ms of unbroken speech",
            1000.0 * h
        );
    }
    if let Some(t) = q.text {
        println!("  words: {t}");
    }
    if q.drawn_out {
        println!("  words: the line draws a word out, so a longer hold and less movement pass");
    }
}

/// The words of the line a lip file names, from the dialog `entries`.
fn line_tokens(entries: Option<&[msg::Entry]>, l: &Lip) -> Option<Vec<g2p::Token>> {
    entries
        .and_then(|e| msg::find(e, l.name().trim()))
        .map(|f| g2p::tokens(&f.entry.text))
}

/// Checks a lip file on its own and against the sound beside it; reports and returns whether it passes, and how well
/// it follows the sound when that was judged. With `entries`, the line's text is looked up by the sound the lip file
/// names. A sound that decodes goes into `decoded`.
fn check_lip(
    path: &Path,
    bytes: &[u8],
    entries: Option<&[msg::Entry]>,
    decoded: &mut HashSet<PathBuf>,
) -> (bool, Option<lipqual::Quality>) {
    let l = match Lip::parse(bytes) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return (false, None);
        }
    };
    let mut ok = true;
    for p in l.validate(Some(&base_name(path))) {
        eprintln!("{}: {}: {p}", path.display(), label(p.severity()));
        ok &= p.severity() != lip::Severity::Error;
    }
    let sound = match named_sound(path, &l) {
        Ok(Some(s)) => {
            decoded.insert(s.0.clone());
            Some(s)
        }
        Ok(None) => {
            eprintln!(
                "{}: no sound {} beside it; its length is not checked",
                path.display(),
                l.name()
            );
            None
        }
        Err(e) => {
            eprintln!("{}: its sound does not decode: {e}", path.display());
            ok = false;
            None
        }
    };
    let mut quality = None;
    if let Some((acm, audio)) = sound {
        let fit = lip::fit(l.last(), &audio);
        match (fit, fit.severity()) {
            (_, None) | (lip::Fit::Matches, _) => {}
            (lip::Fit::Scaled(f), _) => {
                eprintln!(
                    "{}: made for another rate or channel count: its length is {f}x that of {} (ipsdoc lip --rescale \
                     if the sound is right, re-encode the sound if the lip file is)",
                    path.display(),
                    acm.display()
                );
                ok = false;
            }
            (lip::Fit::Off { ms }, Some(s)) => {
                eprintln!(
                    "{}: {}: ends {}",
                    path.display(),
                    label(s),
                    off_by(ms, &acm.display().to_string())
                );
                ok &= s != lip::Severity::Error;
            }
        }
        if fit.for_this_sound() {
            let q = lipqual::quality(&l, &audio, line_tokens(entries, &l).as_deref());
            for p in q.problems() {
                eprintln!("{}: {}: {p}", path.display(), label(p.severity()));
                ok &= p.severity() != lip::Severity::Error;
            }
            quality = Some(q);
        }
    }
    (ok, quality)
}

/// `100 ms after <what>`, `20 ms before <what>`.
fn off_by(ms: f64, what: &str) -> String {
    let side = if ms < 0.0 { "before" } else { "after" };
    format!("{:.0} ms {side} {what}", ms.abs())
}

/// `ipsdoc lip`.
fn lip_files(a: &LipArgs) -> Result<(), Failure> {
    if let (Some(l), Some(sound)) = (&a.rescale, &a.for_sound) {
        return rescale_lip(l, sound);
    }
    if let Some(o) = &a.output {
        let [sound] = a.sounds.as_slice() else {
            return Err(fail(2, "--output takes a single sound"));
        };
        refuse_overwriting(sound, o)?;
    }
    let entries = match &a.msg {
        Some(m) => Some(msg::parse(&read_file(m)?).map_err(|e| fail(1, format!("{}: {e}", m.display())))?),
        None => None,
    };
    let mut failed = 0;
    for sound in &a.sounds {
        if let Err(Failure(_, message)) = lip_one(sound, a, entries.as_deref()) {
            eprintln!("{message}");
            failed += 1;
        }
    }
    if failed > 0 {
        Err(fail(1, format!("{failed} not written")))
    } else {
        Ok(())
    }
}

fn lip_one(sound: &Path, a: &LipArgs, entries: Option<&[msg::Entry]>) -> Result<(), Failure> {
    let name = base_name(sound);
    let text = match (entries, &a.msg) {
        (Some(entries), Some(m)) => {
            let found = msg::find(entries, &name)
                .ok_or_else(|| fail(1, format!("{}: no line for {name} in {}", sound.display(), m.display())))?;
            if found.conflicting {
                eprintln!(
                    "{}: {} names {name} again with other text; using entry {}",
                    sound.display(),
                    m.display(),
                    found.entry.number
                );
            }
            Some(found.entry.text.clone())
        }
        _ => {
            let txt = sound.with_extension("txt");
            match std::fs::read(&txt) {
                Ok(b) => Some(b),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(fail(1, format!("{}: {e}", txt.display()))),
            }
            .filter(|b| !b.iter().all(u8::is_ascii_whitespace))
            .map(|b| msg::decode(&b))
        }
    };
    let bytes = read_file(sound)?;
    let audio = input::read(&bytes, &ReadOptions::default())
        .map_err(|e| fail(1, format!("{}: {}", sound.display(), chain(&e))))?
        .audio;
    if !matches!(audio.rate, 22050 | 44100) {
        eprintln!(
            "{}: {} Hz: the lip file follows this sound's samples, and the games play speech at 22050 or 44100 Hz",
            sound.display(),
            audio.rate
        );
    }
    let tokens = text.as_deref().map(g2p::tokens).filter(|t| !t.is_empty());
    if tokens.is_none() {
        eprintln!("{}: no text for it; lip sync from the audio only", sound.display());
    }
    // Positions are 32-bit: about 13 hours of mono speech.
    i32::try_from(lip::position(audio.len() as i64))
        .map_err(|_| fail(1, format!("{}: too long for a lip file", sound.display())))?;
    let (l, source) = lipgen::generate(&name, &audio, tokens.as_deref(), &lipgen::Params::default());
    if tokens.is_some() && source == lipgen::Source::Sound {
        eprintln!(
            "{}: its text has more sounds than the speech has room for; lip sync from the audio only",
            sound.display()
        );
    }
    let out = a.output.clone().unwrap_or_else(|| sound.with_extension("lip"));
    let replaced = out.exists();
    replace_file(&out, &l.to_bytes()).map_err(|e| fail(1, format!("{}: {e}", out.display())))?;
    if replaced {
        println!("{}: replaced", out.display());
    }
    Ok(())
}

fn rescale_lip(path: &Path, sound: &Path) -> Result<(), Failure> {
    let mut l = Lip::parse(&read_file(path)?).map_err(|e| fail(1, format!("{}: {e}", path.display())))?;
    let audio = input::read(&read_file(sound)?, &ReadOptions::default())
        .map_err(|e| fail(1, format!("{}: {}", sound.display(), chain(&e))))?
        .audio;
    let new_last = i32::try_from(lip::position(audio.len() as i64))
        .map_err(|_| fail(1, format!("{}: too long for a lip file", sound.display())))?;
    let old = l.last();
    l.rescale(new_last);
    replace_file(path, &l.to_bytes()).map_err(|e| fail(1, format!("{}: {e}", path.display())))?;
    println!("{}: last marker {old} -> {new_last}", path.display());
    for p in l.validate(Some(&base_name(path))) {
        println!("{}: kept: {}: {p}", path.display(), label(p.severity()));
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match (&cli.command, &cli.convert) {
        (Some(Command::Convert(a)), _) | (None, Some(a)) => convert(a),
        (Some(Command::Info { files, msg }), _) => info_files(files, msg),
        (Some(Command::Check { paths, msg }), _) => check(paths, msg),
        (Some(Command::Repair { dry_run, files }), _) => repair_files(files, *dry_run),
        (Some(Command::Lip(a)), _) => lip_files(a),
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
