#!/usr/bin/env bash
# Gathers a release build of TARGET (the binary EXE) with the licences and notices it ships with into dist/.
set -euo pipefail

target=${TARGET:?TARGET is not set}
exe=${EXE:?EXE is not set}

mkdir -p dist
cp "target/$target/release/$exe" LICENSE dist/
cp data/cmudict/LICENSE dist/CMUDICT-LICENSE.txt
cargo about generate --locked about.hbs -o dist/THIRD-PARTY-NOTICES.txt
