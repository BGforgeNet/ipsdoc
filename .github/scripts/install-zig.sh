#!/usr/bin/env bash
# Puts zig, which cargo-zigbuild links with, on PATH from its checksummed release tarball, fetched into RUNNER_TEMP.
# Replaces mlugg/setup-zig, whose action still runs on the deprecated Node.js 20. The Zig project asks CI to fetch from
# its community mirrors; the checksum makes any of them safe, and a failing one passes to the next.
set -euo pipefail

readonly VERSION=0.16.0
readonly SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
readonly NAME=zig-x86_64-linux-$VERSION
tmp=${RUNNER_TEMP:?RUNNER_TEMP is not set}
path_file=${GITHUB_PATH:?GITHUB_PATH is not set}

mirrors=$(curl -fsSL https://ziglang.org/download/community-mirrors.txt | shuf)
for base in $mirrors "https://ziglang.org/download/$VERSION"; do
    if curl -fsSL --max-time 120 -o "$tmp/zig.tar.xz" "$base/$NAME.tar.xz?source=ipsdoc-ci" &&
        echo "$SHA256  $tmp/zig.tar.xz" | sha256sum -c --quiet; then
        tar -xJf "$tmp/zig.tar.xz" -C "$tmp"
        echo "$tmp/$NAME" >>"$path_file"
        echo "zig $VERSION from $base"
        exit 0
    fi
    echo "zig: $base failed; trying the next source" >&2
done
echo "zig: no source served $NAME.tar.xz with the expected checksum" >&2
exit 1
