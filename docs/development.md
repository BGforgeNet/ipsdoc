# Development

The gate is the set of commands in `.github/workflows/build.yml` (jobs `lint`, `test`, `oracles`); run them locally
before a commit.

## Reference decoders

`oracles` is a test-only crate comparing against other ACM decoders: the Fallout 2 engine's (as decompiled by
fallout2-ce), GemRB's, libacm and the `infinitier_acm_decoder` crate. The first three are built once, from pinned
upstream commits, into a directory of your choice (absolute, outside the repository); this needs `git` and a C and
C++ compiler:

```sh
oracles/build.sh /path/to/oracles
IPSDOC_ORACLES=/path/to/oracles cargo test -p oracles
```

Each built decoder also works by hand: `/path/to/oracles/bin/acm-gemrb in.acm out.raw` writes 16-bit little-endian
PCM. Plain cargo commands leave the crate's tests out. libacm rejects `levels = 0` streams, so it checks only
transformed files.

## Damaged input

`tests/malformed.rs` feeds every reader damaged copies of valid files and random bytes, seeded;
`IPSDOC_MALFORMED_ROUNDS=N` (default 300) runs a longer sweep.

## Real files

The `corpus` tool lists every sound stream under some directories (game installs, mod trees): loose files by
magic, and the WAV resources of BIFF archives, compressed ones extracted into a directory of your choice. Fallout DAT
archives are not read; extract their `.acm` files first (`dat3 x`).

```sh
cargo run --release -p corpus --bin corpus -- /path/to/manifest.tsv /path/to/extracted /path/to/games ...
IPSDOC_CORPUS=/path/to/manifest.tsv IPSDOC_ORACLES=/path/to/oracles cargo test --release -p oracles --test corpus -- --ignored --nocapture
```

The corpus tests check that every unique ACM and WAVC stream decodes in ours exactly as in the engines' decoders,
re-encodes losslessly (also read back by the Fallout 2 engine's decoder), and that every RIFF and Ogg stream reads.
`IPSDOC_CORPUS_LIMIT=N` judges only the first N unique streams.

`cargo run --release -p corpus --bin features -- /path/to/manifest.tsv` writes the channel detector's labelled
feature vectors, as committed in `tests/data/channel-features.tsv`.

## Other encoders

The `snd2acm` and `cross-tools` CI jobs compare with Interplay's `snd2acm.exe` (on Windows, `oracles/snd2acm.sh`)
and AcmTools (built from source, `oracles/cross-tools.sh`): their output decodes in ours as in the engines, and
AcmTools' decoder reads our lossless files exactly. The fixtures come from `cargo run -p oracles --bin fixtures --
<dir>`; `oracles/tests/tools.rs` describes the directory the comparison reads.

## Lip sync quality

`oracles/tests/lips.rs` scores `ipsdoc lip` (with the line's text, and from the sound alone) and wav2lip against
Fallout 2's own lip files, and asserts on half the speakers, the half `lipgen::Params` is not tuned on. It reads:

- `IPSDOC_LIPS`: `sound/speech` from Fallout 2's `master.dat` (`dat3 x master.dat 'sound/speech/*'`), a directory
  per speaker with each line's ACM and LIP.
- `IPSDOC_MSG`: the Unofficial Patch's `data/text/english/dialog`, for each line's text.
- `IPSDOC_LIPS_WAV2LIP`: wav2lip's lip file per line, named as the line. wav2lip (a 32-bit Windows program) runs
  under wine on each line decoded to a 22050 Hz 16-bit stereo WAV (the mono channel twice), as
  `wav2lip.exe -i <line>.wav -noACM -noAdj`; run it in a container with no network.

```sh
IPSDOC_LIPS=... IPSDOC_MSG=... IPSDOC_LIPS_WAV2LIP=... cargo test --release -p oracles --test lips -- --ignored --nocapture
```

`IPSDOC_LIPS_LIMIT=N` scores the first N lines of each half. The table it prints is the one to tune `Params` by,
on the tuning half only.
