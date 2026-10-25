#!/usr/bin/env bash
# Gathers the licences the release binaries ship with into dist/: ipsdoc's own, and one notices file for the software
# built into them, the crates' licences followed by the CMU Pronouncing Dictionary's.
set -euo pipefail

mkdir -p dist
cp LICENSE dist/
cargo about generate --locked about.hbs -o dist/THIRD-PARTY-NOTICES.txt
{
    printf '\n================================================================================\n'
    printf 'The CMU Pronouncing Dictionary\n\n'
    cat data/cmudict/LICENSE
} >>dist/THIRD-PARTY-NOTICES.txt
