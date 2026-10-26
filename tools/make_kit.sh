#!/bin/sh
# Builds a self-contained AIPL toolchain kit for Linux x86-64: the Rust
# command and runner, the stage-0 native compiler, the standard library,
# and the docs, as one tarball that works from any directory on any
# x86-64 Linux machine (glibc 2.39+ for the Rust command). For handing
# AIPL to another machine or another agent to try.
#
#   sh tools/make_kit.sh [COMMIT] [OUT_DIR]     # defaults: HEAD, target/kit
#
# The commit is built in a private worktree (your checkout is not touched),
# and the kit is tested before it is packed the way another machine would
# see it: copied elsewhere, the worktree's source hidden, then eval, test,
# compile and run, a native executable, a test build, doc, and eval -e.
#
# The binaries are the build's own: the Rust command carries its library
# (the standard library and the toolchain's sources; src/library.rs) and
# writes it to its cache on first use, and the self-hosted command,
# bin/aipl-native, reads share/aipl/std/ beside its folder
# (aipl_src/stdpath.aipl).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMIT="${1:-HEAD}"
OUT="${2:-$ROOT/target/kit}"
SHORT="$(git -C "$ROOT" rev-parse --short "$COMMIT")"
FULL="$(git -C "$ROOT" rev-parse "$COMMIT")"
DATE="$(git -C "$ROOT" show -s --format=%cs "$COMMIT")"
WORK="$ROOT/target/kit-build/$SHORT"
KIT="$OUT/aipl-kit"

say() { echo "make_kit: $*"; }

mkdir -p "$OUT" "$ROOT/target/kit-build"
if [ ! -d "$WORK" ]; then
  say "worktree for $SHORT"
  git -C "$ROOT" worktree add -q --detach "$WORK" "$FULL"
fi
say "building $SHORT (cargo build --release, then the stage-0 compiler)"
(cd "$WORK" && cargo build --release -q)
(cd "$WORK" && sh bootstrap/build.sh > /dev/null)

say "assembling $KIT"
rm -rf "$KIT"
mkdir -p "$KIT/bin" "$KIT/share/aipl" "$KIT/docs"
cp "$WORK/target/release/aipl" "$WORK/target/release/aipl-run" "$KIT/bin/"
cp "$WORK/target/bootstrap/aipl" "$KIT/bin/aipl-native"
chmod +x "$KIT/bin/aipl-native"
cp -r "$WORK/aipl_src/std" "$KIT/share/aipl/std"
# Each doc starts with the line `aipl --version` prints, so a copy that is
# older than the toolchain beside it shows itself.
STAMP="aipl $(sed -n 's/^version = "\(.*\)"/\1/p' "$WORK/Cargo.toml" | head -1) ($SHORT)"
for doc in PROMPT_GUIDE_FOR_AIS.md AIPL_SPEC.md docs/TOUR.md; do
  { echo "$STAMP"; echo; cat "$WORK/$doc"; } > "$KIT/docs/$(basename "$doc")"
done
cat > "$KIT/README.md" <<EOF
# AIPL toolchain kit (Linux x86-64)

AIPL as of commit $SHORT ($DATE), with its standard library and
documentation, made by tools/make_kit.sh. Nothing here reads the folder it
was built in, so it works from any directory on any x86-64 Linux machine.

    tar xzf aipl-kit-$SHORT.tar.gz
    export PATH="\$PWD/aipl-kit/bin:\$PATH"
    aipl eval prog.aipl           # run in the interpreter
    aipl test prog.aipl           # run its (test ...) items
    aipl verify prog.aipl         # type-check only
    aipl compile prog.aipl -o prog.wasm && aipl run prog.wasm
    aipl compile --exe prog.aipl -o prog && ./prog     # native executable
    aipl doc vec                  # a standard module's API; aipl doc --find WORD
    aipl eval -e '(call str.find (call str.from_str "hello") (call str.from_str "l"))'

bin/aipl needs glibc 2.39 or newer (Ubuntu 24.04+, Debian 13, Fedora
40+; check with \`ldd --version\`); it carries its standard library and
writes it, once, to ~/.cache/aipl (or \$AIPL_CACHE). bin/aipl-native is
fully static and runs on any x86-64 Linux: it compiles (\`aipl-native
compile [--exe] FILE -o OUT\`) but does not run or test, reading
share/aipl/std/; \`chmod +x\` an executable it writes.

Read first: docs/PROMPT_GUIDE_FOR_AIS.md (the rules, for an AI writing
AIPL), then docs/AIPL_SPEC.md (the full language) and docs/TOUR.md. The
standard library's source is share/aipl/std/.
EOF

say "testing the kit as another machine sees it (the worktree's source hidden)"
T="$(mktemp -d)"
cp -r "$KIT" "$T/kit"
mkdir "$T/work"
cat > "$T/work/prog.aipl" <<'EOF'
(module prog
  (import alloc)
  (import vec)
  (import text)
  (import str)
  (struct Row [n:i32])
  (fn total [xs:(ptr (vec.Vec i32))] -> i32
    (let s:i32 0)
    (for i 0 (call (vec.len i32) xs) (set! s (+ s (call (vec.at i32) xs i))))
    s)
  (test "sums" (expect (eq (call total (call (vec.make i32) (call alloc.default) 4)) 0)))
  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
    (let r:(ptr Row) (new Row a [n:0]))
    (let xs:(ptr (vec.Vec i32)) (call (vec.make i32) a 4))
    (call (vec.push i32) xs 40)
    (call (vec.push i32) xs 2)
    (let parts:i32 (call (vec.len str.Bytes) (call text.split a (call str.from_str "a,b,c") (call str.from_str ","))))
    (println "total {(call total xs)}, fields {parts}")
    0))
EOF
mv "$WORK/aipl_src" "$WORK/aipl_src.hidden"
restore() { [ -d "$WORK/aipl_src.hidden" ] && mv "$WORK/aipl_src.hidden" "$WORK/aipl_src"; rm -rf "$T"; }
trap restore EXIT
A="$T/kit/bin/aipl"
# a cache of its own, so the kit writes its library as on a new machine
export AIPL_CACHE="$T/cache"
want="total 42, fields 3"
check() {
  if [ "$2" = "$want" ]; then say "  ok  $1"; else say "  FAIL $1: got '$2'"; exit 1; fi
}
cd "$T/work"
check "eval" "$("$A" eval prog.aipl 2>/dev/null | grep -v '^\[AIPL')"
"$A" test prog.aipl > test.out 2>&1 || { cat test.out; say "  FAIL test"; exit 1; }
say "  ok  test"
"$A" compile prog.aipl -o p.wasm > /dev/null
check "compile, run" "$("$A" run p.wasm)"
"$A" compile --exe prog.aipl -o p > /dev/null
check "native executable" "$(./p)"
"$A" compile --tests prog.aipl -o pt.wasm > /dev/null && say "  ok  test build"
"$A" test --native prog.aipl > testn.out 2>&1 || { cat testn.out; say "  FAIL test --native"; exit 1; }
say "  ok  test --native"
N="$T/kit/bin/aipl-native"
env -u AIPL_STD "$N" compile prog.aipl -o pn.wasm > /dev/null && cmp -s pn.wasm p.wasm && say "  ok  aipl-native (the same module)" || { say "  FAIL aipl-native"; exit 1; }
"$A" doc vec 2>&1 | grep -q "std/vec" && say "  ok  doc" || { say "  FAIL doc"; exit 1; }
[ "$("$A" eval -e '(+ 40 2)' 2>&1 | tail -1)" = "42 : i32" ] && say "  ok  eval -e" || { say "  FAIL eval -e"; exit 1; }
v="$("$A" --version)"
for doc in PROMPT_GUIDE_FOR_AIS.md AIPL_SPEC.md TOUR.md; do
  [ "$(head -1 "$T/kit/docs/$doc")" = "$v" ] || { say "  FAIL stamp: $doc does not start with '$v'"; exit 1; }
done
say "  ok  version stamp ($v)"
cd "$OUT"
restore
trap - EXIT

tar czf "aipl-kit-$SHORT.tar.gz" aipl-kit
say "built $OUT/aipl-kit-$SHORT.tar.gz ($(du -h "aipl-kit-$SHORT.tar.gz" | cut -f1))"
