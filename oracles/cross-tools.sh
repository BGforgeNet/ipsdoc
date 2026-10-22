#!/bin/sh
# Prepares the cross-tool comparison under <dir> (see oracles/tests/tools.rs): the fixtures, our lossless encoding
# of each, and, given an `acmconverter` command (AcmTools' CLI), its encoding of each fixture and its decoding of
# ours. `snd2acm/` comes from a Windows runner.
# Usage: oracles/cross-tools.sh <dir> [acmconverter command...]
set -eu

dir=${1:?usage: cross-tools.sh <dir> [acmconverter command...]}
shift
cargo run -q --release -p oracles --bin fixtures -- "$dir/fixtures"
cargo build -q --release -p ipsdoc
ipsdoc=target/release/ipsdoc
mkdir -p "$dir/ours"
for wav in "$dir"/fixtures/*.wav; do
    name=$(basename "$wav" .wav)
    "$ipsdoc" convert "$wav" "$dir/ours/$name.acm"
done
if [ $# -gt 0 ]; then
    mkdir -p "$dir/acmtools" "$dir/acmtools-decoded"
    for wav in "$dir"/fixtures/*.wav; do
        name=$(basename "$wav" .wav)
        # AcmTools guesses channels from the duration; the fixture names say them.
        case $name in
        *-stereo-*) channels=stereo ;;
        *) channels=mono ;;
        esac
        "$@" to-acm "$wav" "$dir/acmtools/$name.acm" --channels "$channels"
        "$@" to-audio "$dir/ours/$name.acm" "$dir/acmtools-decoded/$name.wav" --channels "$channels"
    done
fi
