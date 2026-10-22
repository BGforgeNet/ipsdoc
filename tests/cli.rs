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
use ipsdoc::pcm::Audio;

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
fn info_reports_headers_and_findings() {
    let d = Dir::new();
    let src = d.path("speech.acm");
    std::fs::write(&src, ipsdoc::acm::encode(&[1, 2, 3], 2, 17580).unwrap()).unwrap();
    let text = ok(&ipsdoc(&["info", s(&src)]));
    for needle in [
        "ACM: 3 samples, 2 channels, 17580 Hz",
        "reads as: 1 channels, 44100 Hz",
        "header 2, reads as 1 (odd sample count)",
        "rate 17580 is 44100 byte-swapped",
        "wrong: ACM channels is 2, should be 1",
        "wrong: ACM rate is 17580, should be 44100",
    ] {
        assert!(text.contains(needle), "{needle:?} missing from:\n{text}");
    }
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
    let original = ipsdoc::acm::encode(&[1, 2, 3], 2, 17580).unwrap();
    std::fs::write(&src, &original).unwrap();
    ok(&ipsdoc(&["repair", "--dry-run", s(&src)]));
    assert_eq!(std::fs::read(&src).unwrap(), original);
    ok(&ipsdoc(&["repair", s(&src)]));
    let h = ipsdoc::acm::Header::parse(&std::fs::read(&src).unwrap()).unwrap();
    assert_eq!((h.channels, h.rate), (1, 44100));
    assert_eq!(std::fs::read(&src).unwrap()[14..], original[14..]);
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

#[test]
fn a_missing_input_is_an_error() {
    let d = Dir::new();
    let out = ipsdoc(&[s(&d.path("nope.wav")), s(&d.path("x.acm"))]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nope.wav"));
}
