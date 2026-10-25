//! The `corpus` binary, run as a user runs it.
#![expect(clippy::unwrap_used, reason = "a failed setup step is a failed test")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A fresh directory, removed afterwards.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("corpus-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn acm(path: &Path) {
    std::fs::write(path, ipsdoc::acm::encode(&[1, 2, 3, 4], 1, 22050).unwrap()).unwrap();
}

#[test]
fn a_relative_source_skips_the_extract_dir_inside_it_and_lists_absolute_paths() {
    let d = Dir::new("relative");
    std::fs::create_dir_all(d.0.join("s/zz")).unwrap();
    acm(&d.0.join("s/a.acm"));
    // Left by an earlier run: listed then with its archive as origin, never as a loose file.
    acm(&d.0.join("s/zz/1.acm"));
    let out = Command::new(env!("CARGO_BIN_EXE_corpus"))
        .args(["m.tsv", "s/zz", "s"])
        .current_dir(&d.0)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let manifest = std::fs::read_to_string(d.0.join("m.tsv")).unwrap();
    let paths: Vec<&str> = manifest.lines().map(|l| l.split('\t').nth(1).unwrap()).collect();
    // The listed path is the working directory as the system reports it joined with the source, so it is compared
    // as a file: a macOS temp directory resolves through a symlink, and Windows joins with its own separator.
    assert_eq!(paths.len(), 1, "{paths:?}");
    let listed = Path::new(paths[0]);
    assert!(listed.is_absolute(), "{listed:?}");
    assert_eq!(
        listed.canonicalize().unwrap(),
        d.0.join("s").join("a.acm").canonicalize().unwrap()
    );
}
