#!/bin/sh
# tools/mac_check.sh: what to run on a Mac (or any machine) to see that AIPL
# works there, and that the fixes found there stay fixed. From a clone:
#
#   sh tools/mac_check.sh                  # about 10 minutes: A, B, C, D
#   sh tools/mac_check.sh --quick          # about 2 minutes: A and B only
#   sh tools/mac_check.sh --full           # also every cargo test (30+ minutes)
#   sh tools/mac_check.sh --install REF    # also the installer, from branch REF
#
# --loops N sets how often A repeats its recompile-and-run (default 100).
# --with-serve (with --full) keeps test_serve's three long loopback tests,
# which take about 7 minutes on macOS: it has only 16,384 ephemeral ports,
# and a closed one waits 30 s (docs/APPLE_SILICON_REPORT.md, Round 3).
#
# A  compile over an executable again and again, as test_opt.rs's flake did
#    (macOS killed the next run of a rewritten binary: SIGKILL, "Code
#    Signature Invalid"; Linux said "Text file busy"): the outputs are new
#    files, a running copy keeps its old file, nothing is left behind.
# B  what the README tells a newcomer: hello in three ways, the tools, the
#    showcase program against its known output.
# C  the Rust tests that matter on another platform, repeated for the flaky one.
# D  the AIPL test suite, the documents, the harness's self-test.
#
# Prints PASS or FAIL for each check, a summary, and exits with the number of
# failures. Needs cargo, and python3 for the showcase and the document checks.

set -u
cd "$(dirname "$0")/.." || exit 99
ROOT=$(pwd)

LOOPS=100; QUICK=no; FULL=no; SERVE=no; INSTALL=""
while [ $# -gt 0 ]; do
    case "$1" in
        --quick) QUICK=yes ;;
        --full) FULL=yes ;;
        --with-serve) SERVE=yes ;;
        --loops) LOOPS="$2"; shift ;;
        --install) INSTALL="$2"; shift ;;
        -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
        *) echo "mac_check: unknown option $1 (--help lists them)"; exit 99 ;;
    esac
    shift
done

PASSED=0; FAILED=0
pass() { PASSED=$((PASSED + 1)); echo "PASS  $1"; }
fail() { FAILED=$((FAILED + 1)); echo "FAIL  $1"; [ -n "${2:-}" ] && echo "$2" | sed 's/^/        /'; }
section() { echo; echo "== $1"; }
# a check that runs a command: its output is shown only when it fails
check() {
    name="$1"; shift
    if out=$("$@" 2>&1); then pass "$name"; else fail "$name" "$(echo "$out" | tail -15)"; fi
}

# a file's inode: stat differs between macOS and Linux
inode() { if [ "$(uname -s)" = Darwin ]; then stat -f %i "$1"; else stat -c %i "$1"; fi; }

section "this machine"
echo "$(uname -srm)"
command -v sw_vers >/dev/null 2>&1 && sw_vers | tr '\n' ' ' && echo
cargo --version; rustc --version
echo "source: $(git rev-parse --short HEAD 2>/dev/null) on $(git rev-parse --abbrev-ref HEAD 2>/dev/null)"

section "build"
cargo build --release 2>&1 | tail -2
AIPL="$ROOT/target/release/aipl"
[ -x "$AIPL" ] && [ -x "$ROOT/target/release/aipl-run" ] && pass "aipl and aipl-run are built" || { fail "the build"; exit 1; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/aipl_mac_check.XXXXXX") || exit 99
trap 'exec 9>&- 2>/dev/null; rm -rf "$WORK"' EXIT
cd "$WORK" || exit 99

cat > hello.aipl <<'EOF'
(module hello
  (fn main [] -> i32
    (println "hello, aipl")
    0))
EOF

# ------------------------------------------------------------------ A
section "A. compiling over an executable that was just run ($LOOPS times, with and without the optimizer)"
bad=0; first=""
for i in $(seq "$LOOPS"); do
    flags=""; [ $((i % 2)) -eq 0 ] && flags="--no-opt"
    if ! err=$("$AIPL" compile --exe $flags hello.aipl -o p 2>&1 >/dev/null); then
        bad=$((bad + 1)); [ -z "$first" ] && first="round $i, compile $flags: $err"
        continue
    fi
    got=$(./p 2>&1); status=$?
    if [ "$status" -ne 0 ] || [ "$got" != "hello, aipl" ]; then
        bad=$((bad + 1)); [ -z "$first" ] && first="round $i, run (status $status; 137 is SIGKILL): $got"
    fi
done
[ "$bad" -eq 0 ] && pass "$LOOPS recompiles and runs of one path, all fine" || fail "$bad of $LOOPS rounds failed" "$first"

section "A. the output is a new file; a running copy keeps its own"
cat > waits.aipl <<'EOF'
(module waits (import io)
  (fn main [] -> i32 (match_result (call io.read_stdin) (ok b 3) (err e 1))))
EOF
sed 's/(ok b 3)/(ok b 4)/' waits.aipl > waits2.aipl
"$AIPL" compile --exe waits.aipl -o w >/dev/null 2>&1 || fail "compile the waiting program"
before=$(inode w)
rm -f fifo; mkfifo fifo
./w < fifo & pid=$!
exec 9> fifo                               # opens the writer: the program now waits for input
sleep 1
if err=$("$AIPL" compile --exe waits2.aipl -o w 2>&1 >/dev/null); then pass "compiled over an executable that is running"
else fail "compiled over an executable that is running" "$err"; fi
exec 9>&-                                  # end of input: the old program exits
wait "$pid"; status=$?
[ "$status" -eq 3 ] && pass "the running copy went on as the old program (status 3)" || fail "the running copy" "status $status, wanted 3"
./w < /dev/null; status=$?
[ "$status" -eq 4 ] && pass "the file now holds the new program (status 4)" || fail "the new program" "status $status, wanted 4"
after=$(inode w)
[ "$before" != "$after" ] && pass "the output is a new file (inode $before, then $after)" || fail "the output is the same file, rewritten in place" "inode $before both times"
left=$(ls | grep -c 'tmp[0-9]') ; [ "$left" -eq 0 ] && pass "no temporary file is left behind" || fail "temporary files left behind" "$(ls | grep 'tmp[0-9]')"

section "A. an error leaves the old output alone"
cp w w.kept
printf '(module bad (fn main [] -> i32 (call nothing)))' > bad.aipl
"$AIPL" compile --exe bad.aipl -o w >/dev/null 2>&1 && fail "a program with an error compiled" || pass "a program with an error is refused"
cmp -s w w.kept && pass "the earlier output is untouched" || fail "the earlier output changed"
[ "$(ls | grep -c 'tmp[0-9]')" -eq 0 ] && pass "no temporary file after the error" || fail "temporary file after the error"

# ------------------------------------------------------------------ B
section "B. what the README tells a newcomer"
check "aipl eval hello.aipl" sh -c "'$AIPL' eval hello.aipl | grep -q 'hello, aipl'"
"$AIPL" compile hello.aipl -o hello.wasm >/dev/null 2>&1
check "aipl run hello.wasm" sh -c "'$AIPL' run hello.wasm | grep -q 'hello, aipl'"
"$AIPL" compile --exe hello.aipl -o hello >/dev/null 2>&1
check "the executable runs" sh -c "[ \"\$(./hello)\" = 'hello, aipl' ]"
check "the executable runs from another directory" sh -c "mkdir -p other && cd other && [ \"\$('$WORK/hello')\" = 'hello, aipl' ]"
check "a copied executable runs" sh -c "cp hello hello2 && [ \"\$(./hello2)\" = 'hello, aipl' ]"
check "aipl verify examples/word_count.aipl" "$AIPL" verify "$ROOT/examples/word_count.aipl"
check "aipl doc text" sh -c "'$AIPL' doc text | grep -q split"
check "aipl eval -e" sh -c "[ \"\$('$AIPL' eval -e '(call str.find (call str.from_str \"a,b\") (call str.from_str \",\"))')\" = '1 : i32' ]"
cp "$ROOT/README.md" readme.md
"$AIPL" compile --exe "$ROOT/examples/word_count.aipl" -o wc >/dev/null 2>&1
check "word_count counts a file" sh -c "./wc readme.md | grep -q ."

"$AIPL" compile --exe "$ROOT/examples/jsonpp.aipl" -o jsonpp >/dev/null 2>&1
cat > want.txt <<'EOF'
{
  "a": null,
  "b": [
    1,
    2,
    {}
  ]
}
EOF
echo '{"b":[1,2,{}],"a":null}' | ./jsonpp --sort > got.txt 2>&1
check "jsonpp pretty-prints and sorts" cmp want.txt got.txt
check "jsonpp --query" sh -c "[ \"\$(echo '{\"a\":{\"b\":[10,20]}}' | ./jsonpp -q 'a.b[1]')\" = 20 ]"
check "jsonpp reports a bad document" sh -c "printf '[1,' | ./jsonpp 2>&1 | grep -q 'standard input:1:4'"
if command -v codesign >/dev/null 2>&1; then
    echo "      (informational) codesign on the executable: $(codesign -dv ./hello 2>&1 | grep -E 'Signature|Identifier' | tr '\n' ' ')"
fi

[ "$QUICK" = yes ] && { cd "$ROOT" || exit 99; echo; echo "(--quick: C and D skipped)"; SKIP_REST=yes; } || SKIP_REST=no

# ------------------------------------------------------------------ C and D
if [ "$SKIP_REST" = no ]; then
    cd "$ROOT" || exit 99
    section "C. the Rust tests that matter off Linux x86-64"
    check "test_opt, the flaky one, 20 times over" sh -c 'for i in $(seq 20); do cargo test --release --test test_opt -q 2>&1 | grep -q "test result: ok" || { echo "run $i failed"; cargo test --release --test test_opt 2>&1 | tail -20; exit 1; }; done'
    for t in test_runner test_native_elf test_wasi test_portable test_visibility test_diagnostics test_doc_examples test_showcase test_opcode_conformance test_testing; do
        [ -f "tests/$t.rs" ] || { fail "tests/$t.rs is gone: this script names it"; continue; }
        check "cargo test --test $t" cargo test --release --test "$t" -q
    done

    section "D. the AIPL test suite, the documents, the harness"
    check "aipl test aipl_src/test_suite.aipl" sh -c "'$AIPL' test aipl_src/test_suite.aipl | grep -q 'All groups passed'"
    if command -v python3 >/dev/null 2>&1; then
        check "tools/docs_check.py" python3 tools/docs_check.py
        check "tools/harness/harness.py selftest" python3 tools/harness/harness.py selftest
    else
        fail "python3 is not installed: the document and harness checks could not run"
    fi
fi

# ------------------------------------------------------------------ --full
if [ "$FULL" = yes ] && [ "$QUICK" = no ]; then
    section "every cargo test"
    skip="--skip many_requests_leave_memory_flat --skip twenty_thousand_requests_leave_memory_flat"
    [ "$SERVE" = yes ] && skip=""
    log="$WORK/cargo_test.log"
    # shellcheck disable=SC2086
    cargo test --release --no-fail-fast -- $skip > "$log" 2>&1
    grep -E "^test result|FAILED|failed" "$log" | sort | uniq -c | sort -rn | head -12
    p=$(grep -E "^test result" "$log" | sed -E 's/.* ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored.*/\1 \2 \3/' | awk '{a+=$1; b+=$2; c+=$3} END {print a, b, c}')
    set -- $p
    echo "tests: $1 passed, $2 failed, $3 ignored (Linux x86-64 CI: 456 passed, 0 ignored)"
    [ "$2" -eq 0 ] && pass "cargo test --release: no failure" || { fail "cargo test --release: $2 failed" "$(grep -E '^test .* FAILED|^failures:' "$log" | head -20)"; cp "$log" "$ROOT/target/mac_check_cargo_test.log"; echo "        the whole log: target/mac_check_cargo_test.log"; }
fi

# ------------------------------------------------------------------ --install
if [ -n "$INSTALL" ]; then
    section "the installer, from '$INSTALL', into a scratch AIPL_HOME"
    home="$WORK/aipl_home"
    if AIPL_HOME="$home" sh "$ROOT/install.sh" --ref "$INSTALL" --no-modify-path > "$WORK/install.log" 2>&1; then
        pass "install.sh finished"
        cd "$WORK" || exit 99
        "$home/bin/aipl" compile --exe hello.aipl -o hello_i >/dev/null 2>&1
        check "the installed aipl builds an executable that runs" sh -c "[ \"\$(./hello_i)\" = 'hello, aipl' ]"
        check "the installed aipl finds its library from another directory" sh -c "cd / && '$home/bin/aipl' doc --find split | grep -q split"
    else
        fail "install.sh" "$(tail -15 "$WORK/install.log")"
    fi
fi

echo
echo "== $PASSED passed, $FAILED failed"
[ "$FAILED" -eq 0 ] && echo "All checks passed." || echo "Send the FAIL lines above, and the output of 'sh tools/mac_check.sh --quick', to the owner."
exit "$FAILED"
