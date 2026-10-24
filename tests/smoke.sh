#!/usr/bin/env bash
# Usage: tests/smoke.sh <ipsdoc-binary>
# Runs a release binary on the platform it was cross-built for: a lossless ACM round trip, a Vorbis encode (the C
# part of the build) and a lip file (the bundled dictionary), all of which `check` then reads.
set -euo pipefail

bin="$(cd "$(dirname "${1:?pass the ipsdoc binary to test}")" && pwd)/$(basename "$1")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

head -c 88200 /dev/urandom >in.raw
"$bin" convert --raw --raw-rate 22050 --raw-channels 1 --raw-bits 16 in.raw a.wav
"$bin" a.wav a.acm
"$bin" a.acm b.wav
cmp a.wav b.wav
"$bin" a.wav a.ogg
echo "hello world" >a.txt
"$bin" lip a.acm
"$bin" check .
echo "Smoke test passed: $bin"
