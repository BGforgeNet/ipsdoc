#!/bin/sh
# On Windows (Git Bash): encodes the cross-tool fixtures with Interplay's snd2acm.exe into <dir>/snd2acm, as the
# Python ipsdoc did (`-q0`). The executable is the one the Python tool carried, taken from its last commit.
# Usage: oracles/snd2acm.sh <dir>
set -eu

dir=${1:?usage: snd2acm.sh <dir>}
# The last commit of the Python tool, which stored snd2acm.exe base64-encoded in ipsdoc_bin.py.
python_commit=091a5c5a031ad1e365aecc5c18d74a280666cc38
work=$dir/snd2acm-bin
mkdir -p "$work" "$dir/snd2acm"
git show "$python_commit:ipsdoc_bin.py" >"$work/ipsdoc_bin.py"
python -c 'import base64, sys; sys.path.insert(0, sys.argv[1]); import ipsdoc_bin; open(sys.argv[2], "wb").write(base64.b64decode(ipsdoc_bin.snd2acm_base64))' "$work" "$work/snd2acm.exe"
cargo run -q --release -p oracles --bin fixtures -- "$dir/fixtures"
for wav in "$dir"/fixtures/*.wav; do
    name=$(basename "$wav" .wav)
    "$work/snd2acm.exe" "$wav" "$dir/snd2acm/$name.acm" -q0
    [ -s "$dir/snd2acm/$name.acm" ] || {
        echo "snd2acm.sh: snd2acm wrote nothing for $name" >&2
        exit 1
    }
done
