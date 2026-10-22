# Development

The gate is the set of commands in `.github/workflows/rust.yml`; run them locally before a commit.

## Reference decoders

`oracles` is a test-only crate wrapping other ACM decoders: the Fallout 2 engine's (as decompiled by fallout2-ce),
GemRB's, libacm and the `infinitier_acm_decoder` crate. Its build fetches the first three at pinned commits into the
directory named by `IPSDOC_ORACLE_SRC` (absolute, outside the repository) and compiles them with a C and C++
compiler; `git` must be on `PATH`. Plain cargo commands leave it out.

```sh
IPSDOC_ORACLE_SRC=/path/to/oracle-src cargo test -p oracles
```

libacm rejects `levels = 0` streams, so it checks only transformed files.
