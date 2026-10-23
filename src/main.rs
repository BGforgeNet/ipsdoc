//! `ipsdoc`: converts, inspects, checks and repairs Interplay ACM, WAVC and other game sound files.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

use ipsdoc::acm::Overflow;
use ipsdoc::detect::{Format, detect};
use ipsdoc::input::{self, Note, Raw, ReadOptions};
use ipsdoc::lip::{self, Lip};
use ipsdoc::output::{self, OutputFormat, WriteOptions};
use ipsdoc::repair::{self, Field};
use ipsdoc::{g2p, lipgen, msg};

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

#[derive(Debug, Args)]
struct LipArgs {
    /// Sounds to write lip files for: each gets a `.lip` beside it (and an empty `.txt`, for LIP Editor, if none is
    /// there). Positions follow each sound's own samples: give the ACM the game plays, or the WAV it is made from.
    #[arg(required_unless_present = "rescale")]
    sounds: Vec<PathBuf>,
    /// The dialog message file holding each sound's line, found by its sound field (`{103}{Cas0103}{...}`).
    /// Without it, a non-empty `.txt` beside the sound is the line's text; without that, the sound alone is used.
    #[arg(long, conflicts_with = "rescale")]
    msg: Option<PathBuf>,
    /// Where to write, for a single sound.
    #[arg(short, long, conflicts_with = "rescale")]
    output: Option<PathBuf>,
    /// A lip file to rescale in place, to fit the sound given with `--for` (replaces lipdoc).
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
    if lip::looks_like(&bytes) {
        return lip_info(path, &bytes);
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
    let (mut checked, mut lips, mut failed) = (0u64, 0u64, 0u64);
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
            if lip::looks_like(&bytes) {
                lips += 1;
                if !check_lip(&f, &bytes) {
                    failed += 1;
                }
                continue;
            }
            // Inside a directory only sounds and lip files are checked; a file named outright is checked whatever
            // it is.
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
        let mut bytes = match read_file(f) {
            Ok(b) => b,
            Err(Failure(_, message)) => {
                eprintln!("{message}");
                failed += 1;
                continue;
            }
        };
        if lip::looks_like(&bytes) {
            if !repair_lip(f, &mut bytes, dry_run) {
                failed += 1;
            }
            continue;
        }
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

/// The sound a lip file belongs to: the ACM of the same name beside it (`.acm` or `.ACM`), decoded.
fn sound_beside(lip: &Path) -> Option<(PathBuf, ipsdoc::pcm::Audio)> {
    ["acm", "ACM"].iter().find_map(|ext| {
        let p = lip.with_extension(ext);
        let bytes = std::fs::read(&p).ok()?;
        let audio = input::read(&bytes, &ReadOptions::default()).ok()?.audio;
        Some((p, audio))
    })
}

fn base_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn severity(p: &lip::Problem) -> &'static str {
    match p.severity() {
        lip::Severity::Error => "error",
        lip::Severity::Warning => "warning",
    }
}

fn lip_info(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
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
    if let Some((acm, audio)) = sound_beside(path) {
        let secs = audio.frames() as f64 / f64::from(audio.rate.max(1));
        let fit = match lip::fit(l.last(), audio.len()) {
            lip::Fit::Matches => "fits it".to_owned(),
            lip::Fit::Scaled(f) => format!("is {f}x its length"),
            lip::Fit::Other(r) => format!("is {r:.4} of its length"),
        };
        println!("  sound {}: {secs:.3} s; the lip file {fit}", acm.display());
    }
    for p in l.validate(Some(&base_name(path))) {
        println!("  {}: {p}", severity(&p));
    }
    Ok(())
}

/// Checks a lip file on its own and against the sound beside it; reports and returns whether it passes.
fn check_lip(path: &Path, bytes: &[u8]) -> bool {
    let l = match Lip::parse(bytes) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return false;
        }
    };
    let mut ok = true;
    for p in l.validate(Some(&base_name(path))) {
        eprintln!("{}: {}: {p}", path.display(), severity(&p));
        ok &= p.severity() != lip::Severity::Error;
    }
    if let Some((acm, audio)) = sound_beside(path) {
        match lip::fit(l.last(), audio.len()) {
            lip::Fit::Matches => {}
            lip::Fit::Scaled(f) => {
                eprintln!(
                    "{}: made for another rate or channel count: its length is {f}x that of {} (ipsdoc lip --rescale \
                     if the sound is right, re-encode the sound if the lip file is)",
                    path.display(),
                    acm.display()
                );
                ok = false;
            }
            lip::Fit::Other(r) => {
                eprintln!(
                    "{}: made for another recording: its length is {r:.4} of that of {}",
                    path.display(),
                    acm.display()
                );
                ok = false;
            }
        }
    }
    ok
}

/// Sets a lip file's length field to its last marker, in place; reports anything else. Returns whether it read.
fn repair_lip(path: &Path, bytes: &mut [u8], dry_run: bool) -> bool {
    let l = match Lip::parse(bytes) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return false;
        }
    };
    let mut ok = true;
    if l.len == l.last() {
        println!("{}: nothing to repair", path.display());
    } else {
        println!("{}: lip length field {} -> {}", path.display(), l.len, l.last());
        if !dry_run {
            bytes[lip::LEN_OFFSET..lip::LEN_OFFSET + 4].copy_from_slice(&l.last().to_be_bytes());
            if let Err(e) = std::fs::write(path, &*bytes) {
                eprintln!("{}: {e}", path.display());
                ok = false;
            }
        }
    }
    for p in l.validate(Some(&base_name(path))) {
        if !matches!(p, lip::Problem::Len { .. }) {
            println!("{}: not repaired: {}: {p}", path.display(), severity(&p));
        }
    }
    ok
}

/// `ipsdoc lip`.
fn lip_files(a: &LipArgs) -> Result<(), Failure> {
    if let (Some(l), Some(sound)) = (&a.rescale, &a.for_sound) {
        return rescale_lip(l, sound);
    }
    if a.output.is_some() && a.sounds.len() > 1 {
        return Err(fail(2, "--output takes a single sound"));
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
        _ => std::fs::read(sound.with_extension("txt"))
            .ok()
            .filter(|b| !b.iter().all(u8::is_ascii_whitespace))
            .map(|b| msg::decode(&b)),
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
    let l = lipgen::generate(&name, &audio, tokens.as_deref(), &lipgen::Params::default());
    let out = a.output.clone().unwrap_or_else(|| sound.with_extension("lip"));
    std::fs::write(&out, l.to_bytes()).map_err(|e| fail(1, format!("{}: {e}", out.display())))?;
    let txt = out.with_extension("txt");
    if !txt.exists() {
        std::fs::write(&txt, b"").map_err(|e| fail(1, format!("{}: {e}", txt.display())))?;
    }
    Ok(())
}

fn rescale_lip(path: &Path, sound: &Path) -> Result<(), Failure> {
    let mut l = Lip::parse(&read_file(path)?).map_err(|e| fail(1, format!("{}: {e}", path.display())))?;
    let audio = input::read(&read_file(sound)?, &ReadOptions::default())
        .map_err(|e| fail(1, format!("{}: {}", sound.display(), chain(&e))))?
        .audio;
    let new_last =
        i32::try_from(2 * audio.len()).map_err(|_| fail(1, format!("{}: too long for a lip file", sound.display())))?;
    let old = l.last();
    l.rescale(new_last);
    std::fs::write(path, l.to_bytes()).map_err(|e| fail(1, format!("{}: {e}", path.display())))?;
    println!("{}: last marker {old} -> {new_last}", path.display());
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match (&cli.command, &cli.convert) {
        (Some(Command::Convert(a)), _) | (None, Some(a)) => convert(a),
        (Some(Command::Info { files }), _) => info_files(files),
        (Some(Command::Check { paths }), _) => check(paths),
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
