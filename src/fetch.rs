//! Files fetched once over HTTPS and kept in a cache directory: pronouncing dictionaries too large to build in.
//!
//! Each [`Remote`] is pinned by size and SHA-256, so what a run reads is the same file on every machine, and a damaged
//! or tampered copy, cached or downloaded, is refused.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ring::digest;

/// A file to fetch, pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remote {
    /// Its path in the cache directory (`lang/gmw/de`).
    pub file: &'static str,
    /// Where it is published.
    pub url: &'static str,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, in lower-case hex.
    pub sha256: &'static str,
}

/// What [`fetch`] is doing, for a progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress<'a> {
    /// The file is not cached (or the cached copy is damaged): downloading starts.
    Started {
        /// Where from.
        url: &'a str,
        /// Bytes to read.
        size: u64,
    },
    /// Bytes read so far.
    Read {
        /// Read so far.
        done: u64,
        /// In all.
        size: u64,
    },
    /// The download is checked and saved.
    Saved {
        /// Where.
        path: &'a Path,
    },
}

/// Why a file could not be fetched.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// Neither the platform's cache directory nor a home directory is set.
    #[error("no cache directory: {0} is not set")]
    NoCacheDir(&'static str),
    /// Reading or writing the cache failed.
    #[error("{path}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// The download failed.
    #[error("downloading {url}")]
    Http {
        /// Where from.
        url: String,
        /// The cause.
        #[source]
        source: ureq::Error,
    },
    /// The download is not the pinned file.
    #[error("{url}: got {got_size} bytes with SHA-256 {got_sha256}, expected {size} bytes with {sha256}")]
    Mismatch {
        /// Where from.
        url: String,
        /// Bytes read.
        got_size: u64,
        /// Their SHA-256.
        got_sha256: String,
        /// The pinned size.
        size: u64,
        /// The pinned SHA-256.
        sha256: String,
    },
}

/// The directory ipsdoc caches downloads in: `ipsdoc` in the platform's cache directory (`%LOCALAPPDATA%` on
/// Windows, `~/Library/Caches` on macOS, `$XDG_CACHE_HOME` or `~/.cache` elsewhere).
///
/// # Errors
///
/// When the variable that locates it is unset or empty.
pub fn cache_dir() -> Result<PathBuf, FetchError> {
    let var = |name: &'static str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or(FetchError::NoCacheDir(name))
    };
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Caches")
    } else {
        // The XDG specification ignores a relative value.
        match var("XDG_CACHE_HOME") {
            Ok(p) if p.is_absolute() => p,
            _ => var("HOME")?.join(".cache"),
        }
    };
    Ok(base.join("ipsdoc"))
}

/// `remote`'s contents: from `dir` when a copy there matches its pin, else downloaded, checked and saved there.
///
/// # Errors
///
/// When the download fails or does not match the pin, or the cache cannot be written.
pub fn fetch(remote: &Remote, dir: &Path, progress: &mut dyn FnMut(Progress<'_>)) -> Result<Vec<u8>, FetchError> {
    let path = dir.join(remote.file);
    match std::fs::read(&path) {
        Ok(bytes) if matches(remote, &bytes) => return Ok(bytes),
        // Damaged or from another version: replaced below.
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(FetchError::Io { path, source }),
    }
    progress(Progress::Started {
        url: remote.url,
        size: remote.size,
    });
    let bytes = download(remote, progress)?;
    save(&path, &bytes)?;
    progress(Progress::Saved { path: &path });
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest::digest(&digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn matches(remote: &Remote, bytes: &[u8]) -> bool {
    bytes.len() as u64 == remote.size && sha256_hex(bytes) == remote.sha256
}

fn download(remote: &Remote, progress: &mut dyn FnMut(Progress<'_>)) -> Result<Vec<u8>, FetchError> {
    let http = |source| FetchError::Http {
        url: remote.url.to_owned(),
        source,
    };
    // A host that never answers fails; the body has no time limit, as a slow link is no fault and its progress shows.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut response = agent.get(remote.url).call().map_err(http)?;
    // One byte past the pin is enough to tell a wrong file; reading on would only spend memory.
    let mut reader = response.body_mut().with_config().limit(remote.size + 1).reader();
    let mut bytes = Vec::with_capacity(usize::try_from(remote.size).unwrap_or(0));
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(http(ureq::Error::Io(e))),
        };
        bytes.extend_from_slice(&chunk[..n]);
        progress(Progress::Read {
            done: bytes.len() as u64,
            size: remote.size,
        });
    }
    if !matches(remote, &bytes) {
        return Err(FetchError::Mismatch {
            url: remote.url.to_owned(),
            got_size: bytes.len() as u64,
            got_sha256: sha256_hex(&bytes),
            size: remote.size,
            sha256: remote.sha256.to_owned(),
        });
    }
    Ok(bytes)
}

/// Writes `bytes` to `path` through a temporary file beside it, so a reader never sees a part-written file.
fn save(path: &Path, bytes: &[u8]) -> Result<(), FetchError> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| FetchError::Io { path, source }
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    let file_name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let part = dir.join(format!("{file_name}.{}.part", std::process::id()));
    let written = std::fs::File::create(&part)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&part, path));
    if let Err(source) = written {
        // Best effort: the error being reported is the write's.
        let _ = std::fs::remove_file(&part);
        return Err(FetchError::Io {
            path: path.to_owned(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("ipsdoc-fetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        Dir(d)
    }

    // "abc", whose SHA-256 is the FIPS 180-2 example.
    const ABC: Remote = Remote {
        file: "abc.txt",
        // The local discard port, refused at once where nothing listens: a test that downloads fails fast, offline.
        url: "http://127.0.0.1:9/abc.txt",
        size: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };

    #[test]
    fn hashes_as_fips_180_2() {
        assert_eq!(sha256_hex(b"abc"), ABC.sha256);
    }

    #[test]
    fn a_matching_cached_copy_is_read_without_downloading() {
        let d = dir("cached");
        std::fs::create_dir_all(&d.0).unwrap();
        std::fs::write(d.0.join("abc.txt"), b"abc").unwrap();
        let mut events = 0;
        let got = fetch(&ABC, &d.0, &mut |_| events += 1).unwrap();
        assert_eq!(got, b"abc");
        assert_eq!(events, 0);
    }

    #[test]
    fn a_damaged_cached_copy_is_downloaded_again() {
        let d = dir("damaged");
        std::fs::create_dir_all(&d.0).unwrap();
        std::fs::write(d.0.join("abc.txt"), b"abd").unwrap();
        let mut started = false;
        // The address refuses, so the download fails; what matters is that it was tried.
        let got = fetch(&ABC, &d.0, &mut |p| started |= matches!(p, Progress::Started { .. }));
        assert!(started);
        assert!(matches!(got, Err(FetchError::Http { .. })), "{got:?}");
        // The damaged copy stays until a good one replaces it.
        assert_eq!(std::fs::read(d.0.join("abc.txt")).unwrap(), b"abd");
    }

    #[test]
    fn saving_replaces_the_file_whole() {
        let d = dir("save");
        let path = d.0.join("lang/gmw/abc.txt");
        save(&path, b"old").unwrap();
        save(&path, b"abc").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert_eq!(
            std::fs::read_dir(d.0.join("lang/gmw")).unwrap().count(),
            1,
            "no part file left"
        );
    }
}
