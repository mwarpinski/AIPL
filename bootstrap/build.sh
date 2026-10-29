#!/bin/sh
# Builds the aipl compiler from the checked-in seed, with no Rust
# (Linux x86-64; bootstrap/README.md):
#
#   sh bootstrap/build.sh [OUT_DIR]        (default: target/bootstrap)
#
# The seed builds aipl_src/aipl.aipl (stage 1), stage 1 builds it again
# (stage 2), and stage 2 must rebuild itself unchanged (stage 3). Stage 2 is
# the result: OUT_DIR/aipl.

set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/bootstrap}"
seed="$root/bootstrap/aipl-linux-x86_64"

say() { printf 'bootstrap: %s\n' "$1"; }
err() {
    printf 'bootstrap: error: %s\n' "$1" >&2
    exit 1
}

case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) ;;
    *) err "the seed is a Linux x86-64 executable; elsewhere, build with cargo (README.md, Install)" ;;
esac
[ -x "$seed" ] || err "no seed at $seed"

mkdir -p "$out"
cd "$root"
export AIPL_STD="$root/aipl_src/std/"

# stage COMPILER OUTPUT: COMPILER builds the aipl command into OUTPUT
# (executable: the compiler sets the bit, with fs.chmod).
stage() {
    "$1" compile --exe aipl_src/aipl.aipl -o "$2" >/dev/null || err "$(basename "$1") could not build $2"
}

stage "$seed" "$out/aipl-stage1"
say "stage 1: built by the seed"
stage "$out/aipl-stage1" "$out/aipl-stage2"
say "stage 2: built by stage 1"
stage "$out/aipl-stage2" "$out/aipl-stage3"
cmp -s "$out/aipl-stage2" "$out/aipl-stage3" || err "stage 2 does not rebuild itself unchanged"
say "stage 3: identical to stage 2 (a fixed point)"
cp "$out/aipl-stage2" "$out/aipl"
rm -f "$out/aipl-stage1" "$out/aipl-stage3"
say "built $out/aipl ($("$out/aipl" --version))"
