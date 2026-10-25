# Building

Install Rust (the version is pinned in the [toolchain file](../rust-toolchain.toml)) and a C compiler (for libvorbis),
then `cargo build --release`.

The release builds are cross-built with [Zig](https://ziglang.org/) as the C compiler and linker: install zig and
`cargo-zigbuild` and put them on `PATH`, then `cargo zigbuild --release --target x86_64-unknown-linux-musl` (the targets
are in the [release workflow](../.github/workflows/build.yml)).

[Development notes](development.md): the gates, the test oracles and the corpus tests.
