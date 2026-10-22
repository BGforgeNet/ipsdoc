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
