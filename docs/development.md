# Development

The gate is `.github/workflows/build.yml`: a release waits on the jobs its `release` job needs. Before a commit, run the
commands of the jobs that need no Windows runner, other encoder or cross build (`lint`, `test`, `oracles`) locally. They
need the tools the `lint` job installs, at its pinned versions, on `PATH`; Go, for `actionlint`; `RUNNER_TEMP` set to an
absolute scratch directory outside the repository, for `.github/scripts/build-oxfmt.sh`; and the reference decoders
(below), for `oracles`.

## Reference decoders

`oracles` is a test-only crate comparing against other ACM decoders: the Fallout 2 engine's (as decompiled by
fallout2-ce), GemRB's, libacm and the `infinitier_acm_decoder` crate. The first three are built once, from pinned
upstream commits, into a directory of your choice (absolute, outside the repository); this needs `git` and a C and C++
compiler:

```sh
oracles/build.sh /path/to/oracles
IPSDOC_ORACLES=/path/to/oracles cargo test -p oracles
```

Each built decoder also works by hand: `/path/to/oracles/bin/acm-gemrb in.acm out.raw` writes 16-bit little-endian PCM.
Plain cargo commands leave the crate's tests out. libacm rejects `levels = 0` streams, so it checks only transformed
files.

## Damaged input

`tests/malformed.rs` feeds every reader damaged copies of valid files and random bytes, seeded;
`IPSDOC_MALFORMED_ROUNDS=N` (default 300) runs a longer sweep.

`fuzz/` holds coverage-guided fuzz targets for the readers (its own workspace; the gate only keeps it compiling). They
run with cargo-fuzz on a nightly toolchain, one target at a time (`cargo fuzz list` names them):

```sh
cargo +nightly fuzz run acm -- -max_total_time=600
```

## Real files

The `corpus` tool lists every sound stream under some directories (game installs, mod trees): loose files by magic, and
the WAV resources of BIFF archives, compressed ones extracted into a directory of your choice. Fallout DAT archives are
not read; extract their `.acm` files first (`dat3 x`).

```sh
cargo run --release -p corpus --bin corpus -- /path/to/manifest.tsv /path/to/extracted /path/to/games ...
IPSDOC_CORPUS=/path/to/manifest.tsv IPSDOC_ORACLES=/path/to/oracles cargo test --release -p oracles --test corpus -- --ignored --nocapture
```

The corpus tests check that every unique ACM and WAVC stream decodes in ours exactly as in the engines' decoders,
re-encodes losslessly (also read back by the Fallout 2 engine's decoder), and that every RIFF and Ogg stream reads.
`IPSDOC_CORPUS_LIMIT=N` judges only the first N unique streams.

`cargo run --release -p corpus --bin features -- /path/to/manifest.tsv` writes the channel detector's labelled feature
vectors, as committed in `tests/data/channel-features.tsv`.

## Other encoders

The `snd2acm` and `cross-tools` CI jobs compare with Interplay's `snd2acm.exe` (on Windows, `oracles/snd2acm.sh`) and
AcmTools (built from source, `oracles/cross-tools.sh`): their output decodes in ours as in the engines, and AcmTools'
decoder reads our lossless files exactly. The fixtures come from `cargo run -p oracles --bin fixtures --
<dir>`; `oracles/tests/tools.rs` describes the directory the comparison reads.

## Lip sync quality

`oracles/tests/lips.rs` scores `ipsdoc lip` (with the line's text, and from the sound alone) and wav2lip against Fallout
2's own lip files, and asserts on half the speakers, the half `lipgen::Params` is not tuned on. It reads:

- `IPSDOC_LIPS`: `sound/speech` from Fallout 2's `master.dat` (`dat3 x master.dat 'sound/speech/*'`), a directory per
  speaker with each line's ACM and LIP.
- `IPSDOC_MSG`: the Unofficial Patch's `data/text/english/dialog`, for each line's text.
- `IPSDOC_LIPS_WAV2LIP`: wav2lip's lip file per line, named as the line. wav2lip (a 32-bit Windows program) runs under
  wine on each line decoded to a 22050 Hz 16-bit stereo WAV (the mono channel twice), as
  `wav2lip.exe -i <line>.wav -noACM -noAdj`; run it in a container with no network.

```sh
IPSDOC_LIPS=... IPSDOC_MSG=... IPSDOC_LIPS_WAV2LIP=... cargo test --release -p oracles --test lips -- --ignored --nocapture
```

`IPSDOC_LIPS_LIMIT=N` takes the first N lip files of each half, before the ones without a usable reference are skipped;
the run passes only with 100 held-out lines scored. The table it prints is the one to tune `Params` by, on the tuning
half only.

`oracles/tests/lipqual.rs`, on the same inputs, measures `check`'s lip-sync limits: how often each problem is reported
on Fallout 2's own files, ours, wav2lip's and spoiled copies of the originals (another line's, moved late, shuffled, one
shape held), and how each set's directories grade. The figures in `src/lipqual.rs` come from its report.

```sh
IPSDOC_LIPS=... IPSDOC_MSG=... IPSDOC_LIPS_WAV2LIP=... cargo test --release -p oracles --test lipqual -- --ignored --nocapture
```

`src/lipmodel.rs`, the model `ipsdoc lip` places a line's sounds with, is fitted by `oracles/tests/lipmodel.rs` to
forced alignments of the same lines, which fails and prints the table when the committed one differs. `IPSDOC_MFA` holds
the alignments: one TextGrid per line, `<speaker>/<line>.TextGrid`, from the Montreal Forced Aligner 3.4 with its
`english_us_arpa` acoustic model, dictionary and G2P model
(`mfa align --g2p_model_path english_us_arpa <corpus> english_us_arpa english_us_arpa <out>`), over each line's sound as
a WAV beside its text: the transcript beside the sound where there is one, else its dialog text, a vocal noise (a cough,
a laugh) written `[noise]`.

```sh
IPSDOC_LIPS=... IPSDOC_MFA=... cargo test --release -p oracles --test lipmodel -- --ignored --nocapture
```
