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
# Why a wrapper: the Rust command finds the standard library, `aipl doc`'s
# source, and the native backend through the folder it was built in
# (docs/LAUNCH_CHECKLIST.md, "A binary that works wherever it is"). The
# kit's bin/aipl points it at the bundled std, runs `aipl doc` from a
# prebuilt doc.wasm, and hands `compile --exe` to the self-contained
# native compiler, whose output is the Rust toolchain's byte for byte
# (tests/test_bootstrap.rs).
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
mkdir -p "$KIT/bin" "$KIT/libexec" "$KIT/docs"
cp "$WORK/target/release/aipl" "$WORK/target/release/aipl-run" "$KIT/libexec/"
cp "$WORK/target/bootstrap/aipl" "$KIT/bin/aipl-native"
chmod +x "$KIT/bin/aipl-native"
cp -r "$WORK/aipl_src/std" "$KIT/std"
# Each doc starts with the line `aipl --version` prints, so a copy that is
# older than the toolchain beside it shows itself.
STAMP="aipl $(sed -n 's/^version = "\(.*\)"/\1/p' "$WORK/Cargo.toml" | head -1) ($SHORT)"
for doc in PROMPT_GUIDE_FOR_AIS.md AIPL_SPEC.md docs/TOUR.md; do
  { echo "$STAMP"; echo; cat "$WORK/$doc"; } > "$KIT/docs/$(basename "$doc")"
done
"$KIT/libexec/aipl" compile "$WORK/aipl_src/doc.aipl" -o "$KIT/libexec/doc.wasm" > /dev/null

cat > "$KIT/bin/aipl" <<'EOF'
#!/bin/sh
# The AIPL command, from this kit: the Rust toolchain (libexec/aipl), with
# `aipl doc` run from a prebuilt doc.wasm and `aipl compile --exe` by the
# self-contained native compiler (bin/aipl-native), so nothing reads the
# folder the toolchain was built in.
KIT="$(cd "$(dirname "$0")/.." && pwd)"
export AIPL_PATH="$KIT/std${AIPL_PATH:+:$AIPL_PATH}"
export AIPL_STD="$KIT/std/"
case "${1:-}" in
  doc)
    shift
    exec "$KIT/libexec/aipl-run" "$KIT/libexec/doc.wasm" "$@" ;;
  compile)
    for a in "$@"; do
      if [ "$a" = "--exe" ]; then
        # the native compiler writes the file without its execute bit
        out=""; prev=""
        for b in "$@"; do [ "$prev" = "-o" ] && out="$b"; prev="$b"; done
        "$KIT/bin/aipl-native" "$@" || exit $?
        [ -n "$out" ] && chmod +x "$out"
        exit 0
      fi
    done ;;
esac
exec "$KIT/libexec/aipl" "$@"
EOF
chmod +x "$KIT/bin/aipl"

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
40+; check with \`ldd --version\`). bin/aipl-native is fully static and
runs on any x86-64 Linux: it compiles (\`aipl-native compile [--exe] FILE
-o OUT\`) but does not run or test; with it, \`chmod +x\` an executable it
writes, and set AIPL_STD to the kit's std/ folder.

Read first: docs/PROMPT_GUIDE_FOR_AIS.md (the rules, for an AI writing
AIPL), then docs/AIPL_SPEC.md (the full language) and docs/TOUR.md. The
standard library's source is std/.

Not in this kit: \`aipl test --native\` and \`aipl compile --self\` (they
need the repository).
EOF

say "testing the kit as another machine sees it (the worktree's source hidden)"
T="$(mktemp -d)"
cp -r "$KIT" "$T/kit"
mkdir "$T/work"
cat > "$T/work/prog.aipl" <<'EOF'
(module prog
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
    (let r:(ptr Row) (new Row a))
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
