#!/usr/bin/env bash
# The lint job's checks, run by CI and before a commit. Needs on PATH the tools the lint job installs, shellcheck, Go
# (for actionlint), curl and sha256sum (for oxfmt); and RUNNER_TEMP, an absolute scratch directory outside the
# repository.
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
# The fuzz targets are a workspace of their own, run by cargo-fuzz on nightly; here they are kept compiling.
cargo fmt --manifest-path fuzz/Cargo.toml --check
cargo clippy --manifest-path fuzz/Cargo.toml --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny --locked --workspace check
# The fuzz workspace's own lockfile; its targets never ship, so their licences are not checked.
cargo deny --locked --manifest-path fuzz/Cargo.toml check advisories bans sources
cargo shear
shellcheck oracles/*.sh tests/*.sh .github/scripts/*.sh
.github/scripts/build-oxfmt.sh
# install-action has no actionlint; the Go module proxy serves this tag checksummed.
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12
zizmor --offline .github/workflows
