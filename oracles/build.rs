//! Fetches the reference decoders' sources at pinned commits and compiles them with the harnesses in `csrc/`.
//!
//! `IPSDOC_ORACLE_SRC` names a directory (outside the repository) that holds the checkouts. The sources are never
//! vendored: they are third-party code under their own licences, used only to test against.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Upstream {
    name: &'static str,
    url: &'static str,
    commit: &'static str,
    paths: &'static [&'static str],
}

const FALLOUT2_CE: Upstream = Upstream {
    name: "fallout2-ce",
    url: "https://github.com/alexbatalov/fallout2-ce.git",
    commit: "e97087b9582f37075db347a89898887320753f8b",
    paths: &["src/sound_decoder.cc", "src/sound_decoder.h"],
};

const GEMRB: Upstream = Upstream {
    name: "gemrb",
    url: "https://github.com/gemrb/gemrb.git",
    commit: "bcb5cfaa9f19ffbe8a51f25a3c014fa344173351",
    paths: &["gemrb/plugins/ACMReader/"],
};

const LIBACM: Upstream = Upstream {
    name: "libacm",
    url: "https://github.com/markokr/libacm.git",
    commit: "a704d07a2aeee95cda696754d84d78b504386dcb",
    paths: &["src/"],
};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("cannot run git in {}: {e}", dir.display()));
    if !out.status.success() {
        panic!(
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Makes `root/<name>` a sparse checkout of `upstream.paths` at `upstream.commit`, fetching only when it is not.
fn checkout(root: &Path, upstream: &Upstream) -> PathBuf {
    let dir = root.join(upstream.name);
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
        git(&dir, &["init", "-q"]);
        git(&dir, &["remote", "add", "origin", upstream.url]);
    }
    let head = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "-q", "--verify", "HEAD"])
        .output();
    if head.is_ok_and(|h| String::from_utf8_lossy(&h.stdout).trim() == upstream.commit) {
        return dir;
    }
    let mut sparse = vec!["sparse-checkout", "set", "--no-cone"];
    sparse.extend(upstream.paths);
    git(&dir, &sparse);
    git(
        &dir,
        &[
            "fetch",
            "-q",
            "--depth",
            "1",
            "--filter=blob:none",
            "origin",
            upstream.commit,
        ],
    );
    git(&dir, &["checkout", "-q", "--detach", "FETCH_HEAD"]);
    dir
}

/// Third-party sources are compiled as published; their warnings are not ours to fix, so they are silenced.
fn build() -> cc::Build {
    let mut b = cc::Build::new();
    b.warnings(false).flag_if_supported("-w");
    b
}

fn main() {
    println!("cargo::rerun-if-env-changed=IPSDOC_ORACLE_SRC");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=csrc");
    let root = std::env::var_os("IPSDOC_ORACLE_SRC")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| panic!("set IPSDOC_ORACLE_SRC to an absolute directory outside the repository"));

    let f2 = checkout(&root, &FALLOUT2_CE);
    build()
        .cpp(true)
        .std("c++17")
        .include(f2.join("src"))
        .file(f2.join("src/sound_decoder.cc"))
        .file("csrc/fallout2ce.cc")
        .compile("oracle_fallout2ce");

    let gemrb = checkout(&root, &GEMRB).join("gemrb/plugins/ACMReader");
    build()
        .cpp(true)
        .std("c++17")
        .include("csrc/gemrb_shim")
        .include(&gemrb)
        .file(gemrb.join("unpacker.cpp"))
        .file(gemrb.join("decoder.cpp"))
        .file("csrc/gemrb.cc")
        .compile("oracle_gemrb");

    let libacm = checkout(&root, &LIBACM).join("src");
    build()
        .include(&libacm)
        .file(libacm.join("decode.c"))
        .file(libacm.join("util.c"))
        .file("csrc/libacm.c")
        .compile("oracle_libacm");
}
