//! The command line, run as a user runs it.
#![expect(
    clippy::unwrap_used,
    reason = "fixture helpers outside #[test] fns: a panic is the failure report"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ipsdoc::input::{ReadOptions, read};
use ipsdoc::output::{OutputFormat, WriteOptions, write};
use ipsdoc::pcm::{Audio, Samples};

/// A scratch directory, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "ipsdoc-cli-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The names of the files in it, sorted.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ipsdoc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ipsdoc")).args(args).output().unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// One second of true stereo at 22050 Hz.
fn stereo() -> Audio {
    let samples = (0..2 * 22050)
        .map(|i| (6000.0 * ((i / 2) as f64 * 0.01 * (1 + i % 2) as f64).sin()) as i16)
        .collect();
    Audio::from_i16(22050, 2, samples)
}

fn put(path: &Path, audio: &Audio, format: OutputFormat) {
    std::fs::write(path, write(audio, format, &WriteOptions::default()).unwrap()).unwrap();
}

fn load(path: &Path) -> Audio {
    read(&std::fs::read(path).unwrap(), &ReadOptions::default())
        .unwrap()
        .audio
}

#[track_caller]
fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{:?}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_old_call_converts_a_wav_to_lossless_acm_through_paths_with_spaces() {
    let d = Dir::new();
    let (src, dst) = (d.path("my sound.wav"), d.path("out file.acm"));
    put(&src, &stereo(), OutputFormat::Riff);
    ok(&ipsdoc(&[s(&src), s(&dst)]));
    let h = ipsdoc::acm::Header::parse(&std::fs::read(&dst).unwrap()).unwrap();
    assert_eq!((h.channels, h.rate, h.levels), (2, 22050, 0));
    assert_eq!(load(&dst), stereo());
}

#[test]
fn without_an_output_a_wav_becomes_an_acm_beside_it() {
    let d = Dir::new();
    let src = d.path("Voice 01.WAV");
    put(&src, &stereo(), OutputFormat::Riff);
    ok(&ipsdoc(&[s(&src)]));
    assert_eq!(load(&d.path("Voice 01.acm")), stereo());
}

#[test]
fn without_an_output_an_acm_becomes_a_wav() {
    let d = Dir::new();
    let src = d.path("music.acm");
    put(&src, &stereo(), OutputFormat::Acm);
    ok(&ipsdoc(&["convert", s(&src)]));
    let out = d.path("music.wav");
    assert_eq!(
        ipsdoc::detect::detect(&std::fs::read(&out).unwrap()),
        ipsdoc::detect::Format::Riff
    );
    assert_eq!(load(&out), stereo());
}

#[test]
fn converting_over_an_existing_output_says_so() {
    let d = Dir::new();
    let (src, dst) = (d.path("music.acm"), d.path("music.wav"));
    put(&src, &stereo(), OutputFormat::Acm);
    assert!(!ok(&ipsdoc(&[s(&src)])).contains("replaced"));
    let again = ok(&ipsdoc(&[s(&src)]));
    assert_eq!(again, format!("{}: replaced\n", s(&dst)));
    assert_eq!(load(&dst), stereo());
}

#[test]
fn a_failed_conversion_keeps_the_output_it_would_replace() {
    let d = Dir::new();
    let (src, dst) = (d.path("music.acm"), d.path("music.wav"));
    put(&src, &stereo(), OutputFormat::Acm);
    std::fs::write(&dst, "earlier").unwrap();
    // The write goes through a sibling file first; a directory in its place makes that write fail.
    std::fs::create_dir(d.path("music.wav.ipsdoc-tmp")).unwrap();
    let out = ipsdoc(&[s(&src), s(&dst)]);
    assert_eq!(out.status.code(), Some(1), "{}", all_output(&out));
    assert_eq!(std::fs::read(&dst).unwrap(), b"earlier");
}

#[test]
fn refuses_to_write_over_its_input() {
    let d = Dir::new();
    let src = d.path("a.wav");
    put(&src, &stereo(), OutputFormat::Wavc);
    let out = ipsdoc(&[s(&src)]);
    // The default output for a WAVC named .wav would be the input itself.
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(load(&src), stereo());
}

#[test]
fn converts_to_ogg_and_resamples() {
    let d = Dir::new();
    let (src, ogg, wav) = (d.path("a.wav"), d.path("b.ogg"), d.path("c.wav"));
    put(&src, &stereo(), OutputFormat::Riff);
    ok(&ipsdoc(&["convert", s(&src), s(&ogg), "--quality", "3"]));
    assert_eq!((load(&ogg).rate, load(&ogg).channels), (22050, 2));
    ok(&ipsdoc(&["convert", s(&src), s(&wav), "--rate", "44100"]));
    assert_eq!(
        (load(&wav).rate, load(&wav).frames(), load(&wav).bits()),
        (44100, 44100, Some(16))
    );
}

#[test]
fn deep_wav_output_is_16_bit_unless_asked_for_the_full_depth() {
    let d = Dir::new();
    let (src, dst) = (d.path("deep.wav"), d.path("out.wav"));
    let data = (0..1000).map(|n| (n * 7919 % 0x100_0000 - 0x80_0000) << 8).collect();
    let deep = Audio {
        rate: 44100,
        channels: 1,
        samples: Samples::Int { bits: 24, data },
    };
    let full = WriteOptions {
        riff_full_depth: true,
        ..WriteOptions::default()
    };
    std::fs::write(&src, write(&deep, OutputFormat::Riff, &full).unwrap()).unwrap();
    let said = |input: &str| {
        format!(
            "{}: 16-bit, from {input} input: Infinity Engine games read WAV up to 16-bit; add --full-depth to get lossless output\n",
            s(&dst)
        )
    };
    let out = ipsdoc(&["convert", s(&src), s(&dst)]);
    ok(&out);
    assert_eq!(String::from_utf8_lossy(&out.stderr), said("24-bit"));
    assert_eq!(load(&dst), Audio::from_i16(44100, 1, deep.to_i16()));
    let out = ipsdoc(&["convert", "--full-depth", s(&src), s(&dst)]);
    ok(&out);
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    assert_eq!(load(&dst), deep);
    // Vorbis decodes to float.
    let ogg = d.path("a.ogg");
    put(&ogg, &stereo(), OutputFormat::Ogg);
    let out = ipsdoc(&["convert", s(&ogg), s(&dst)]);
    ok(&out);
    assert_eq!(String::from_utf8_lossy(&out.stderr), said("float"));
    assert_eq!(load(&dst).bits(), Some(16));
    // 16-bit input has nothing to say; other formats have no depth to keep.
    let out = ipsdoc(&["convert", s(&d.path("out.wav")), s(&d.path("same.wav"))]);
    ok(&out);
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let out = ipsdoc(&["convert", "--full-depth", s(&src), s(&d.path("out.acm"))]);
    assert_eq!(out.status.code(), Some(2), "{}", all_output(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "ipsdoc: --full-depth is for WAV output only\n"
    );
    assert!(!d.path("out.acm").exists());
}

#[test]
fn writes_wavc_when_asked() {
    let d = Dir::new();
    let (src, dst) = (d.path("a.wav"), d.path("b.wav"));
    put(&src, &stereo(), OutputFormat::Riff);
    ok(&ipsdoc(&["convert", s(&src), s(&dst), "--to", "wavc"]));
    assert_eq!(
        ipsdoc::detect::detect(&std::fs::read(&dst).unwrap()),
        ipsdoc::detect::Format::Wavc
    );
    assert_eq!(load(&dst), stereo());
}

#[test]
fn reads_raw_pcm() {
    let d = Dir::new();
    let (src, dst) = (d.path("a.raw"), d.path("a.acm"));
    std::fs::write(&src, [0x01, 0x00, 0xFF, 0xFF, 0x00, 0x80]).unwrap();
    ok(&ipsdoc(&[
        "convert",
        s(&src),
        s(&dst),
        "--raw",
        "--raw-rate",
        "11025",
        "--raw-channels",
        "1",
        "--raw-bits",
        "16",
    ]));
    assert_eq!(load(&dst), Audio::from_i16(11025, 1, vec![1, -1, i16::MIN]));
}

#[test]
fn refuses_zero_and_out_of_range_values_before_reading() {
    let d = Dir::new();
    let (wav, raw, dst) = (d.path("a.wav"), d.path("a.raw"), d.path("b.acm"));
    put(&wav, &stereo(), OutputFormat::Riff);
    std::fs::write(&raw, [0; 4]).unwrap();
    for (input, flag, value) in [
        (&wav, "--rate", "0"),
        (&wav, "--channels", "0"),
        (&wav, "--input-rate", "0"),
        (&wav, "--quality", "11"),
        (&wav, "--quality", "-1.5"),
        (&raw, "--raw-rate", "0"),
        (&raw, "--raw-channels", "0"),
        (&raw, "--raw-bits", "12"),
    ] {
        let raw_flag: &[&str] = if input == &raw { &["--raw"] } else { &[] };
        let out = ipsdoc(&[&["convert", s(input), s(&dst)], raw_flag, &[flag, value]].concat());
        assert_eq!(out.status.code(), Some(2), "{flag} {value}: {}", all_output(&out));
        // Refused as a value of its option, not as some other error.
        let text = all_output(&out);
        assert!(text.contains(&format!("invalid value '{value}' for '{flag}")), "{text}");
        assert!(!dst.exists(), "{flag} {value} wrote an output");
    }
}

#[test]
fn converts_at_a_negative_vorbis_quality() {
    let d = Dir::new();
    let (wav, ogg) = (d.path("a.wav"), d.path("a.ogg"));
    put(&wav, &stereo(), OutputFormat::Riff);
    ok(&ipsdoc(&["convert", s(&wav), s(&ogg), "--quality", "-1"]));
    assert_eq!(load(&ogg).channels, 2);
}

#[test]
fn info_reports_headers_and_findings() {
    let d = Dir::new();
    let src = d.path("speech.acm");
    std::fs::write(&src, ipsdoc::acm::encode(&[1, 2, 3], 2, 22050).unwrap()).unwrap();
    let text = ok(&ipsdoc(&["info", s(&src)]));
    for needle in [
        "ACM: 3 samples, 2 channels, 22050 Hz",
        "reads as: 1 channels, 22050 Hz",
        "header 2, reads as 1 (odd sample count)",
        "wrong: ACM channels is 2, should be 1",
    ] {
        assert!(text.contains(needle), "{needle:?} missing from:\n{text}");
    }
}

#[test]
fn says_in_words_why_a_zero_channel_header_reads_as_mono() {
    let d = Dir::new();
    let src = d.path("zero.acm");
    let mut acm = ipsdoc::acm::encode(&[1, 2, 3, 4], 1, 22050).unwrap();
    acm[8..10].copy_from_slice(&0u16.to_le_bytes());
    std::fs::write(&src, acm).unwrap();
    let text = ok(&ipsdoc(&["info", s(&src)]));
    assert!(
        text.contains("header 0, reads as 1 (no stereo claim, so mono)"),
        "{text}"
    );
}

#[test]
fn check_fails_on_a_broken_file_and_names_it() {
    let d = Dir::new();
    put(&d.path("good.acm"), &stereo(), OutputFormat::Acm);
    let mut broken = ipsdoc::acm::encode(&[0; 1000], 1, 22050).unwrap();
    broken.truncate(16);
    std::fs::write(d.path("broken.acm"), broken).unwrap();
    std::fs::write(d.path("notes.txt"), "not a sound").unwrap();
    let out = ipsdoc(&["check", s(&d.0)]);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("broken.acm"), "{text}");
    assert!(!text.contains("good.acm: "), "{text}");
    std::fs::remove_file(d.path("broken.acm")).unwrap();
    ok(&ipsdoc(&["check", s(&d.0)]));
}

#[test]
fn repair_fixes_headers_in_place_and_dry_run_does_not() {
    let d = Dir::new();
    let src = d.path("speech.acm");
    let original = ipsdoc::acm::encode(&[1, 2, 3], 2, 22050).unwrap();
    std::fs::write(&src, &original).unwrap();
    let dry = ok(&ipsdoc(&["repair", "--dry-run", s(&src)]));
    assert!(dry.contains("ACM channels 2 -> 1 (dry run)"), "{dry}");
    assert_eq!(std::fs::read(&src).unwrap(), original);
    let real = ok(&ipsdoc(&["repair", s(&src)]));
    assert!(real.contains("ACM channels 2 -> 1\n"), "{real}");
    let h = ipsdoc::acm::Header::parse(&std::fs::read(&src).unwrap()).unwrap();
    assert_eq!((h.channels, h.rate), (1, 22050));
    assert_eq!(std::fs::read(&src).unwrap()[14..], original[14..]);
}

/// Runs ipsdoc with stdout and stderr on pipes whose readers are gone, as after `| head -1`: every write fails.
fn ipsdoc_into_closed_pipes(args: &[&str]) -> std::process::ExitStatus {
    let (out_reader, out_writer) = std::io::pipe().unwrap();
    let (err_reader, err_writer) = std::io::pipe().unwrap();
    drop((out_reader, err_reader));
    Command::new(env!("CARGO_BIN_EXE_ipsdoc"))
        .args(args)
        .stdout(out_writer)
        .stderr(err_writer)
        .status()
        .unwrap()
}

#[test]
fn a_closed_pipe_drops_the_output_and_the_work_goes_on() {
    let d = Dir::new();
    let (good, bad, wrong) = (d.path("good.acm"), d.path("bad.acm"), d.path("wrong.acm"));
    put(&good, &stereo(), OutputFormat::Acm);
    std::fs::write(&bad, &std::fs::read(&good).unwrap()[..300]).unwrap();
    std::fs::write(&wrong, ipsdoc::acm::encode(&[1, 2, 3], 2, 22050).unwrap()).unwrap();
    // The exit status stays what it would be: 1, for the file that does not read.
    assert_eq!(ipsdoc_into_closed_pipes(&["info", s(&good), s(&bad)]).code(), Some(1));
    assert_eq!(ipsdoc_into_closed_pipes(&["check", s(&d.0)]).code(), Some(1));
    assert_eq!(ipsdoc_into_closed_pipes(&["repair", s(&wrong)]).code(), Some(0));
    let h = ipsdoc::acm::Header::parse(&std::fs::read(&wrong).unwrap()).unwrap();
    assert_eq!(h.channels, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn output_that_cannot_be_written_is_a_failure() {
    let d = Dir::new();
    let good = d.path("good.acm");
    put(&good, &stereo(), OutputFormat::Acm);
    // Every write to /dev/full fails as on a full disk.
    let out = Command::new(env!("CARGO_BIN_EXE_ipsdoc"))
        .args(["info", s(&good)])
        .stdout(std::fs::File::create("/dev/full").unwrap())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "ipsdoc: writing output: No space left on device (os error 28)\n"
    );
}

#[test]
fn info_and_repair_go_on_past_a_bad_file() {
    let d = Dir::new();
    let good = d.path("good.acm");
    std::fs::write(&good, ipsdoc::acm::encode(&[1, 2, 3], 2, 22050).unwrap()).unwrap();
    let missing = d.path("missing.acm");
    let out = ipsdoc(&["info", s(&missing), s(&good)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("good.acm"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = ipsdoc(&["repair", s(&missing), s(&good)]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        ipsdoc::acm::Header::parse(&std::fs::read(&good).unwrap())
            .unwrap()
            .channels,
        1
    );
}

#[cfg(unix)]
#[test]
fn check_goes_on_past_an_unreadable_directory() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new();
    put(&d.path("good.acm"), &stereo(), OutputFormat::Acm);
    let locked = d.path("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let out = ipsdoc(&["check", s(&d.0)]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("1 sounds checked"), "{text}");
    assert!(text.contains("locked"), "{text}");
}

#[cfg(unix)]
#[test]
fn check_reads_only_the_head_of_a_file_in_a_directory_that_is_no_sound() {
    let d = Dir::new();
    put(&d.path("good.acm"), &stereo(), OutputFormat::Acm);
    // A sparse terabyte, as a game archive stands in a game directory: read whole, it could not even be held.
    let archive = std::fs::File::create(d.path("data.bif")).unwrap();
    archive.set_len(1 << 40).unwrap();
    let text = ok(&ipsdoc(&["check", s(&d.0)]));
    assert!(text.contains("1 sounds checked"), "{text}");
}

#[cfg(unix)]
#[test]
fn check_walks_a_tree_once_through_links_back_into_it() {
    let d = Dir::new();
    let sub = d.path("sub");
    std::fs::create_dir(&sub).unwrap();
    put(&sub.join("good.acm"), &stereo(), OutputFormat::Acm);
    // A loop, and a second link to the same tree.
    std::os::unix::fs::symlink("..", sub.join("up")).unwrap();
    std::os::unix::fs::symlink("sub", d.path("again")).unwrap();
    let text = ok(&ipsdoc(&["check", s(&d.0)]));
    assert!(text.contains("1 sounds checked"), "{text}");
}

#[test]
fn a_missing_input_is_an_error() {
    let d = Dir::new();
    let out = ipsdoc(&[s(&d.path("nope.wav")), s(&d.path("x.acm"))]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nope.wav"));
}

/// 0.2 s silence, 0.6 s of a voiced tone, 0.2 s silence, at 22050 Hz mono.
fn speech() -> Audio {
    let samples = (0..22050)
        .map(|i| {
            let t = f64::from(i) / 22050.0;
            if (0.2..0.8).contains(&t) {
                (9000.0 * (t * 150.0 * std::f64::consts::TAU).sin()) as i16
            } else {
                0
            }
        })
        .collect();
    Audio::from_i16(22050, 1, samples)
}

fn lip_at(path: &Path) -> ipsdoc::lip::Lip {
    ipsdoc::lip::Lip::parse(&std::fs::read(path).unwrap()).unwrap()
}

/// Whether the lip file shows closed lips (m, p, b) anywhere.
fn closes_lips(lip: &ipsdoc::lip::Lip) -> bool {
    lip.phonemes
        .iter()
        .any(|&c| ipsdoc::lipgen::HEAD_FRAME[usize::from(c)] == 6)
}

fn all_output(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn lip_writes_a_lip_beside_the_sound_and_nothing_else() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    let out = ipsdoc(&["lip", s(&src)]);
    ok(&out);
    assert!(all_output(&out).contains("audio only"), "{}", all_output(&out));
    let lip = lip_at(&d.path("x.lip"));
    assert_eq!(lip.last(), 2 * 22050);
    assert_eq!(lip.name(), "X");
    assert_eq!(d.files(), ["x.acm", "x.lip"]);
    ok(&ipsdoc(&["check", s(&d.path("x.lip"))]));
    assert!(!ok(&out).contains("replaced"), "{}", all_output(&out));
    // A second run replaces the lip file, and says so.
    let again = ok(&ipsdoc(&["lip", s(&src)]));
    assert!(again.contains(&format!("{}: replaced", s(&d.path("x.lip")))), "{again}");
}

#[test]
fn lip_refuses_an_output_that_is_its_sound() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    let before = std::fs::read(&src).unwrap();
    let out = ipsdoc(&["lip", s(&src), "-o", s(&src)]);
    assert_eq!(out.status.code(), Some(2), "{}", all_output(&out));
    assert!(
        all_output(&out).contains("would overwrite the input"),
        "{}",
        all_output(&out)
    );
    assert_eq!(std::fs::read(&src).unwrap(), before);
}

#[test]
fn lip_reads_the_line_from_a_msg() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    let msg = d.path("dialog.msg");
    std::fs::write(&msg, "{100}{}{You see a man.}\r\n{101}{X}{Map, be.}\r\n").unwrap();
    let out = ipsdoc(&["lip", "--msg", s(&msg), s(&src)]);
    ok(&out);
    assert!(!all_output(&out).contains("audio only"), "{}", all_output(&out));
    assert!(closes_lips(&lip_at(&d.path("x.lip"))));
    // A sound the MSG has no line for: an error, and nothing written.
    let other = d.path("y.acm");
    put(&other, &speech(), OutputFormat::Acm);
    let out = ipsdoc(&["lip", "--msg", s(&msg), s(&other)]);
    assert_eq!(out.status.code(), Some(1), "{}", all_output(&out));
    assert!(all_output(&out).contains("no line for y"), "{}", all_output(&out));
    assert!(!d.path("y.lip").exists());
}

#[test]
fn lip_warns_when_a_msg_names_the_sound_twice_with_other_text() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    let msg = d.path("dialog.msg");
    std::fs::write(&msg, "{1}{x}{Map, be.}{2}{x}{Hello.}").unwrap();
    let out = ipsdoc(&["lip", "--msg", s(&msg), s(&src)]);
    ok(&out);
    assert!(all_output(&out).contains("other text"), "{}", all_output(&out));
}

#[test]
fn lip_uses_a_transcript_beside_the_sound() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    std::fs::write(d.path("x.txt"), "map be").unwrap();
    let out = ipsdoc(&["lip", s(&src)]);
    ok(&out);
    assert!(!all_output(&out).contains("audio only"), "{}", all_output(&out));
    assert!(closes_lips(&lip_at(&d.path("x.lip"))));
    assert_eq!(
        std::fs::read(d.path("x.txt")).unwrap(),
        b"map be",
        "the transcript stays"
    );
}

#[test]
fn lip_fails_on_a_transcript_it_cannot_read() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    // A directory where the transcript would be: there, but unreadable as one.
    std::fs::create_dir(d.path("x.txt")).unwrap();
    let out = ipsdoc(&["lip", s(&src)]);
    assert_eq!(out.status.code(), Some(1), "{}", all_output(&out));
    assert!(all_output(&out).contains("x.txt"), "{}", all_output(&out));
    assert!(!d.path("x.lip").exists());
}

#[test]
fn lip_warns_on_a_rate_the_games_do_not_play_and_wants_one_input_for_an_output() {
    let d = Dir::new();
    let src = d.path("x.wav");
    put(&src, &Audio::from_i16(48000, 1, speech().to_i16()), OutputFormat::Riff);
    let out = ipsdoc(&["lip", s(&src)]);
    ok(&out);
    assert!(all_output(&out).contains("48000 Hz"), "{}", all_output(&out));
    let out = ipsdoc(&["lip", "-o", s(&d.path("o.lip")), s(&src), s(&src)]);
    assert_eq!(out.status.code(), Some(2), "{}", all_output(&out));
}

#[test]
fn lip_rescale_fits_a_lip_to_another_sound() {
    let d = Dir::new();
    let (src, big) = (d.path("x.acm"), d.path("big.acm"));
    put(&src, &speech(), OutputFormat::Acm);
    let quad: Vec<i16> = speech().to_i16().iter().flat_map(|&v| [v, v, v, v]).collect();
    put(&big, &Audio::from_i16(44100, 2, quad), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let lip = d.path("x.lip");
    ok(&ipsdoc(&["lip", "--rescale", s(&lip), "--for", s(&big)]));
    assert_eq!(lip_at(&lip).last(), 8 * 22050);
    // Replaced through a sibling file, which is gone afterwards.
    assert_eq!(d.files(), ["big.acm", "x.acm", "x.lip"]);
}

/// Three seconds of tone bursts 80 to 250 ms long with pauses between, irregular as syllables are.
fn talk() -> Audio {
    let spans = [
        (120, 60),
        (230, 90),
        (90, 140),
        (180, 50),
        (250, 110),
        (100, 70),
        (160, 130),
        (210, 40),
        (80, 100),
        (140, 60),
    ];
    let mut samples = vec![0i16; 22050 / 4];
    for (on, off) in spans {
        let start = samples.len();
        samples.extend((0..on * 2205 / 100).map(|i| (12000.0 * ((start + i) as f64 * 0.04).sin()) as i16));
        samples.extend(std::iter::repeat_n(0, off * 2205 / 100));
    }
    samples.resize(3 * 22050, 0);
    Audio::from_i16(22050, 1, samples)
}

#[test]
fn check_grades_how_lip_files_follow_their_sounds() {
    let d = Dir::new();
    let sounds: Vec<PathBuf> = (0..15).map(|i| d.path(&format!("x{i:02}.acm"))).collect();
    for s in &sounds {
        put(s, &talk(), OutputFormat::Acm);
    }
    let names: Vec<&str> = sounds.iter().map(|p| s(p)).collect();
    ok(&ipsdoc(&[&["lip"][..], &names].concat()));
    // Made from their sounds: graded as a set, and nothing to say.
    let out = ipsdoc(&["check", s(&d.0)]);
    let text = ok(&out);
    assert!(
        text.contains(": lip sync of 15 lip files (medians): alignment 0."),
        "{text}"
    );
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    let info = ok(&ipsdoc(&["info", s(&d.path("x00.lip"))]));
    assert!(info.contains("  sync: alignment 0."), "{info}");
    // A mouth that never opens: an error for each, and the set warned about.
    for s in &sounds {
        let last = 2 * 3 * 22050;
        let shut = ipsdoc::lip::Lip::new(
            &s.file_stem().unwrap().to_string_lossy(),
            vec![0],
            vec![
                ipsdoc::lip::Marker { kind: 1, position: 0 },
                ipsdoc::lip::Marker {
                    kind: 1,
                    position: last,
                },
            ],
        );
        std::fs::write(s.with_extension("lip"), shut.to_bytes()).unwrap();
    }
    let out = ipsdoc(&["check", s(&d.0)]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!(
            "{}: error: the mouth is shut through 100% of the loud speech\n",
            s(&d.path("x00.lip"))
        )),
        "{err}"
    );
    assert!(
        err.contains("warning: its lip files keep the mouth shut through 100% of the loud speech (median), over 35%"),
        "{err}"
    );
    // With the lines' text, from a dialog file in a directory: the lips never close for their m, b and p.
    std::fs::create_dir(d.path("dialog")).unwrap();
    let lines: String = (0..15)
        .map(|i| format!("{{{i}}}{{X{i:02}}}{{Maybe, Bob.}}\r\n"))
        .collect();
    std::fs::write(d.path("dialog/talk.msg"), lines).unwrap();
    let out = ipsdoc(&["check", "--msg", s(&d.path("dialog")), s(&d.path("x00.lip"))]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(&format!(
            "{}: warning: the lips close 0 times for the line's 4 m, b and p, 0 of them nearby\n",
            s(&d.path("x00.lip"))
        )),
        "{}",
        all_output(&out)
    );
    let out = ipsdoc(&["check", "--msg", s(&d.path("none.msg")), s(&d.path("x00.lip"))]);
    assert_eq!(out.status.code(), Some(1), "{}", all_output(&out));
}

#[test]
fn check_compares_a_lip_with_its_sound() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let lip = d.path("x.lip");
    ok(&ipsdoc(&["check", s(&lip)]));
    let good = lip_at(&lip);
    // Made for the sound at twice the samples.
    let mut doubled = good.clone();
    doubled.rescale(2 * good.last());
    std::fs::write(&lip, doubled.to_bytes()).unwrap();
    let out = ipsdoc(&["check", s(&lip)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        all_output(&out).contains("another rate or channel count"),
        "{}",
        all_output(&out)
    );
    // Ending `ms` off the 1 s sound, at 44.1 bytes a millisecond.
    let off = |ms: i32| {
        let mut l = good.clone();
        l.rescale(good.last() + ms * 2 * 22050 / 1000);
        std::fs::write(&lip, l.to_bytes()).unwrap();
        ipsdoc(&["check", s(&lip)])
    };
    // Unseen on the talking heads: passes without a word.
    let out = off(20);
    ok(&out);
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    // Perhaps seen: a warning.
    let out = off(100);
    ok(&out);
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("{}: warning: ends 100 ms after {}\n", s(&lip), s(&src))
    );
    let info = ok(&ipsdoc(&["info", s(&lip)]));
    assert!(info.contains("1.000 s; the lip file ends 100 ms after it\n"), "{info}");
    // Seen: an error.
    let out = off(-200);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).starts_with(&format!(
            "{}: error: ends 200 ms before {}\n",
            s(&lip),
            s(&src)
        )),
        "{}",
        all_output(&out)
    );
    // Within the rounding of a resample.
    let mut close = good.clone();
    close.rescale(good.last() + 2);
    std::fs::write(&lip, close.to_bytes()).unwrap();
    ok(&ipsdoc(&["check", s(&lip)]));
}

#[test]
fn check_reports_a_truncated_lip_named_or_found() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let lip = d.path("x.lip");
    let mut bytes = std::fs::read(&lip).unwrap();
    bytes.truncate(bytes.len() - 3);
    std::fs::write(&lip, bytes).unwrap();
    let out = ipsdoc(&["check", s(&lip)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(all_output(&out).contains("truncated"), "{}", all_output(&out));
    let out = ipsdoc(&["check", s(&d.0)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(all_output(&out).contains("x.lip"), "{}", all_output(&out));
}

#[test]
fn check_fails_when_the_sound_does_not_decode_and_notes_a_missing_one() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let mut acm = std::fs::read(&src).unwrap();
    acm.truncate(40);
    std::fs::write(&src, acm).unwrap();
    let out = ipsdoc(&["check", s(&d.path("x.lip"))]);
    assert_eq!(out.status.code(), Some(1), "{}", all_output(&out));
    assert!(all_output(&out).contains("x.acm"), "{}", all_output(&out));
    std::fs::remove_file(&src).unwrap();
    let out = ipsdoc(&["check", s(&d.path("x.lip"))]);
    ok(&out);
    assert!(all_output(&out).contains("no sound X"), "{}", all_output(&out));
}

#[test]
fn check_fits_the_sound_the_name_field_names() {
    let d = Dir::new();
    let (a, b) = (d.path("a.acm"), d.path("b.acm"));
    put(&a, &speech(), OutputFormat::Acm);
    put(&b, &Audio::from_i16(22050, 1, vec![0; 30000]), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&a)]));
    // b.lip is a copy of a.lip: the game loads it for line b and plays a.acm, which it fits.
    std::fs::copy(d.path("a.lip"), d.path("b.lip")).unwrap();
    let out = ipsdoc(&["check", s(&d.path("b.lip"))]);
    assert_eq!(out.status.code(), Some(1), "{}", all_output(&out));
    let text = all_output(&out);
    assert!(text.contains("error: names sound A, the file is b"), "{text}");
    assert!(!text.contains(" ms "), "{text}");
}

#[test]
fn rescale_reports_the_problems_it_keeps() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let lip = d.path("x.lip");
    let mut l = lip_at(&lip);
    let n = l.markers.len();
    l.markers[n - 2].position = 2;
    std::fs::write(&lip, l.to_bytes()).unwrap();
    let out = ipsdoc(&["lip", "--rescale", s(&lip), "--for", s(&src)]);
    ok(&out);
    assert!(
        all_output(&out).contains("is before the one preceding it"),
        "{}",
        all_output(&out)
    );
}

#[test]
fn repair_fixes_a_lip_length_field_only() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let lip = d.path("x.lip");
    let mut wrong = lip_at(&lip);
    wrong.len = 12345;
    std::fs::write(&lip, wrong.to_bytes()).unwrap();
    ok(&ipsdoc(&["repair", "--dry-run", s(&lip)]));
    assert_eq!(lip_at(&lip).len, 12345);
    ok(&ipsdoc(&["repair", s(&lip)]));
    let fixed = lip_at(&lip);
    assert_eq!(fixed.len, fixed.last());
    assert_eq!(fixed.markers, wrong.markers);
}

#[test]
fn info_shows_a_lip() {
    let d = Dir::new();
    let src = d.path("x.acm");
    put(&src, &speech(), OutputFormat::Acm);
    ok(&ipsdoc(&["lip", s(&src)]));
    let text = ok(&ipsdoc(&["info", s(&d.path("x.lip"))]));
    for needle in ["x.lip: lip", "name X", "1.000 s"] {
        assert!(text.contains(needle), "{needle:?} missing from:\n{text}");
    }
}
