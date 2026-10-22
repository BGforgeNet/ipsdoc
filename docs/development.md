# Development

The gate is the set of commands in `.github/workflows/rust.yml`; run them locally before a commit.

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

## Real files

The `corpus` tool lists every sound stream under some directories (game installs, mod trees): loose files by
magic, and the WAV resources of BIFF archives, compressed ones extracted into a directory of your choice. Fallout DAT
archives are not read; extract their `.acm` files first (`dat3 x`). Then the corpus test decodes every unique ACM
and WAVC stream in ours and in every reference decoder, and fails on any difference:

```sh
cargo run --release -p corpus -- /path/to/manifest.tsv /path/to/extracted /path/to/games ...
IPSDOC_CORPUS=/path/to/manifest.tsv IPSDOC_ORACLES=/path/to/oracles cargo test --release -p oracles --test corpus -- --ignored --nocapture
```

`IPSDOC_CORPUS_LIMIT=N` judges only the first N unique streams.
