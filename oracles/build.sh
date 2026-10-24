#!/bin/sh
# Builds the reference ACM decoders once: fetches each at a pinned commit into <dir>/src and compiles it with its
# harness from csrc/ into <dir>/bin/acm-<name>. A decoder already built from the same commit and harness is kept.
# Usage: oracles/build.sh <dir>   (absolute, outside the repository; tests find it via IPSDOC_ORACLES)
set -eu

dir=${1:?usage: build.sh <absolute dir>}
case $dir in
/*) ;;
*)
    echo "build.sh: $dir is not absolute" >&2
    exit 2
    ;;
esac
csrc=$(cd "$(dirname "$0")/csrc" && pwd)
cc=${CC:-cc}
cxx=${CXX:-c++}
mkdir -p "$dir/src" "$dir/bin"

# fetch <name> <url> <commit> <path>...: a sparse, shallow checkout of the paths at the commit.
fetch() {
    name=$1 url=$2 commit=$3
    shift 3
    repo=$dir/src/$name
    if [ ! -d "$repo/.git" ]; then
        git init -q "$repo"
        git -C "$repo" remote add origin "$url"
    fi
    # A checkout from an earlier run keeps its remote: point it at this URL, in case the pin moved repositories.
    git -C "$repo" remote set-url origin "$url"
    # A fresh repository has no HEAD yet: rev-parse fails, which means "not at the commit".
    if [ "$(git -C "$repo" rev-parse -q --verify HEAD || true)" != "$commit" ]; then
        git -C "$repo" sparse-checkout set --no-cone "$@"
        git -C "$repo" fetch -q --depth 1 --filter=blob:none origin "$commit"
        git -C "$repo" checkout -q --detach FETCH_HEAD
    fi
}

harness_sum=$(find "$csrc" -type f | sort | xargs cat | cksum)

# fresh <name> <commit>: true when bin/acm-<name> was built from this commit and the current harness sources.
fresh() {
    [ -x "$dir/bin/acm-$1" ] && [ -f "$dir/bin/.acm-$1" ] && [ "$(cat "$dir/bin/.acm-$1")" = "$2 $harness_sum" ]
}

stamp() {
    echo "$2 $harness_sum" >"$dir/bin/.acm-$1"
}

# Third-party sources are compiled as published; their warnings are not ours to fix (-w).

c=0e5d1c7d0c3bf91af0cf323ba48217631de8e6f9
if ! fresh fallout2ce "$c"; then
    fetch fallout2-ce https://github.com/fallout2-ce/fallout2-ce.git "$c" src/sound_decoder.cc src/sound_decoder.h
    s=$dir/src/fallout2-ce/src
    "$cxx" -std=c++17 -O2 -w -I"$csrc" -I"$s" "$s/sound_decoder.cc" "$csrc/fallout2ce.cc" -o "$dir/bin/acm-fallout2ce"
    stamp fallout2ce "$c"
fi

c=bcb5cfaa9f19ffbe8a51f25a3c014fa344173351
if ! fresh gemrb "$c"; then
    fetch gemrb https://github.com/gemrb/gemrb.git "$c" gemrb/plugins/ACMReader/
    s=$dir/src/gemrb/gemrb/plugins/ACMReader
    "$cxx" -std=c++17 -O2 -w -I"$csrc" -I"$csrc/gemrb_shim" -I"$s" "$s/unpacker.cpp" "$s/decoder.cpp" \
        "$csrc/gemrb.cc" -o "$dir/bin/acm-gemrb"
    stamp gemrb "$c"
fi

c=a704d07a2aeee95cda696754d84d78b504386dcb
if ! fresh libacm "$c"; then
    fetch libacm https://github.com/markokr/libacm.git "$c" src/
    s=$dir/src/libacm/src
    "$cc" -O2 -w -I"$csrc" -I"$s" "$s/decode.c" "$s/util.c" "$csrc/libacm.c" -o "$dir/bin/acm-libacm"
    stamp libacm "$c"
fi

echo "built in $dir/bin"
