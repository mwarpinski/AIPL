#!/bin/sh
# The AIPL compiler on AArch64 Linux, over ssh to an Apple Silicon Mac with Docker:
#   sh tools/arm64_bootstrap.sh HOST
# 1. this checkout's `aipl` (target/release/aipl, or $AIPL) builds the compiler for AArch64
#    (stage 1);
# 2. on HOST, in a linux/arm64 container, stage 1 compiles the compiler again (stage 2),
#    which must be stage 1's bytes: a fixed point on AArch64, with no Rust and no x86-64;
# 3. stage 1 there compiles every program in examples/, benchmarks/, and tests/aipl to wasm,
#    and each must be the bytes `aipl compile` makes here.
# 4. also on HOST, `sh bootstrap/build.sh` builds the compiler from the checked-in AArch64 seed
#    (bootstrap/aipl-linux-aarch64), and the result is stage 1's bytes: the same compiler built
#    from the same source by an independent route, whether or not the seed is from this source.
# 5. stage 1 there runs a program (`aipl run`, which builds it, starts it, and removes it) and a
#    file of tests (`aipl test`, a fresh process per test), as it does on x86-64.
# Exits 0 only if all of that holds. HOST keeps its files in ~/aipl-arm-self.
set -u
host=${1:?usage: sh tools/arm64_bootstrap.sh HOST}
here=$(cd "$(dirname "$0")/.." && pwd)
aipl=${AIPL:-$here/target/release/aipl}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$here" || exit 2

echo "== stage 1: built here for AArch64"
"$aipl" compile --exe --target arm64 --allow-run '*' aipl_src/aipl.aipl -o "$work/stage1" > /dev/null || exit 1

echo "== the programs, compiled here"
mkdir "$work/ref"
find examples benchmarks tests/aipl -name '*.aipl' | sort > "$work/list.txt"
: > "$work/refused.txt"
while read -r f; do
  n=$(echo "$f" | tr / _)
  "$aipl" compile "$f" -o "$work/ref/$n.wasm" > /dev/null 2>&1 || { rm -f "$work/ref/$n.wasm"; echo "$f" >> "$work/refused.txt"; }
done < "$work/list.txt"

echo "== sending to $host"
ssh -o BatchMode=yes "$host" "mkdir -p ~/aipl-arm-self/out && rm -rf ~/aipl-arm-self/out/*.wasm ~/aipl-arm-self/seed_out ~/aipl-arm-self/bootstrap ~/aipl-arm-self/aipl_src ~/aipl-arm-self/examples ~/aipl-arm-self/benchmarks ~/aipl-arm-self/tests" || exit 2
rsync -rlR aipl_src bootstrap examples benchmarks tests/aipl "$host":aipl-arm-self/ || exit 2
scp -q "$work/stage1" "$work/list.txt" "$host":aipl-arm-self/ || exit 2

echo "== on $host: stage 1 compiles itself, then the programs"
ssh -o BatchMode=yes "$host" '
  export PATH=/opt/homebrew/bin:/usr/local/bin:$PATH
  cd ~/aipl-arm-self && chmod +x stage1
  docker run --rm --platform linux/arm64 -v "$PWD":/w -w /w alpine sh -c "
    uname -m
    /w/stage1 compile --exe --target arm64 --allow-run \"*\" aipl_src/aipl.aipl -o stage2 > /dev/null || echo STAGE2-FAILED
    cmp stage1 stage2 && echo STAGE2-IDENTICAL
    sh bootstrap/build.sh /w/seed_out > /dev/null && echo SEED-BUILT
    cmp seed_out/aipl stage1 && echo SEED-CHAIN-MATCHES
    [ \"\$(/w/stage1 run examples/quicksort.aipl)\" = \"median of 5000 random values: 502\" ] && echo RUN-WORKS
    /w/stage1 test tests/aipl/testing/rows.aipl 2>&1 | tail -1 | grep -q \"7 tests: 3 passed, 4 failed\" && echo TEST-WORKS
    while read f; do
      n=\$(echo \$f | tr / _)
      /w/stage1 compile \$f -o out/\$n.wasm > /dev/null 2>&1 || rm -f out/\$n.wasm
    done < list.txt"' | tee "$work/remote.log"

rsync -r "$host":aipl-arm-self/out/ "$work/out/" || exit 2
ok=1
grep -q '^aarch64' "$work/remote.log" || { echo "arm64_bootstrap: not an AArch64 machine"; ok=0; }
grep -q '^SEED-BUILT' "$work/remote.log" || { echo "arm64_bootstrap: bootstrap/build.sh failed from the AArch64 seed"; ok=0; }
grep -q '^SEED-CHAIN-MATCHES' "$work/remote.log" || { echo "arm64_bootstrap: the seed's chain builds a different compiler than stage 1"; ok=0; }
grep -q '^RUN-WORKS' "$work/remote.log" || { echo "arm64_bootstrap: aipl run did not build and run a program"; ok=0; }
grep -q '^TEST-WORKS' "$work/remote.log" || { echo "arm64_bootstrap: aipl test did not give the expected counts"; ok=0; }
grep -q '^STAGE2-IDENTICAL' "$work/remote.log" || { echo "arm64_bootstrap: stage 2 is not stage 1"; ok=0; }
same=0
for f in "$work"/ref/*.wasm; do
  b=$(basename "$f")
  if cmp -s "$f" "$work/out/$b"; then same=$((same + 1)); else echo "arm64_bootstrap: different bytes: $b"; ok=0; fi
done
# a program Rust refuses must be refused there too
for f in $(cat "$work/refused.txt"); do
  n=$(echo "$f" | tr / _)
  [ -e "$work/out/$n.wasm" ] && { echo "arm64_bootstrap: compiled $f, which Rust refuses"; ok=0; }
done
echo "arm64_bootstrap: $same programs identical, $(wc -l < "$work/refused.txt" | tr -d ' ') refused by both"
[ "$ok" = 1 ] && echo "arm64_bootstrap: ALL PASSED" || echo "arm64_bootstrap: FAILED"
[ "$ok" = 1 ]
