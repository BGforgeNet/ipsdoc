#!/usr/bin/env bash
# Checks the formatting of every markdown file with oxfmt's checksummed release binary, fetched into RUNNER_TEMP.
# install-action knows no oxfmt release this recent; the release binary formats markdown without Node.
set -euo pipefail

readonly VERSION=0.73.0
readonly SHA256=ac905d463471646d0cf94d9ce3ca108146ed9fc964d49f627546b21ea2a4bb78
readonly NAME=oxfmt-x86_64-unknown-linux-gnu
tmp=${RUNNER_TEMP:?RUNNER_TEMP is not set}

curl -fsSL -o "$tmp/oxfmt.tar.gz" "https://github.com/oxc-project/oxc/releases/download/oxfmt_v$VERSION/$NAME.tar.gz"
echo "$SHA256  $tmp/oxfmt.tar.gz" | sha256sum -c
tar -xzf "$tmp/oxfmt.tar.gz" -C "$tmp"
"$tmp/$NAME" --check '**/*.md'
