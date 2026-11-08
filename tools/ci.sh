#!/bin/sh
# Every step of .github/workflows/ci.yml (the self-hosted job, then the referee job), in the given checkout; each step
# also logged, timestamped, to target/ci_steps.log (tools/ci-status.sh reads it).
# The tests run one binary at a time, as cargo test runs them, through
# tools/ci_tests.py, which records each test's start, end, and time in
# target/ci_tests.jsonl; CI_PLAIN=1 runs plain `cargo test` instead.
#   sh tools/ci.sh /path/to/checkout
cd "$1" || exit 1
# a fresh worktree has no target/ until the first build
mkdir -p target
L=target/ci_steps.log
date '+%Y-%m-%d %H:%M:%S CI run' > $L
rm -f target/ci_tests.jsonl
step() { echo "== $1"; echo "$(date +%H:%M:%S) $1" >> $L; }
note() { echo "$1"; echo "           $1" >> $L; }
# ===== The self-hosted toolchain: what ships. Built from the seed with no Rust; every check here runs on it. =====
step "seed build"; mkdir -p target; sh bootstrap/build.sh > target/ci_seed.log 2>&1 || { note "seed build FAILED: $(tail -1 target/ci_seed.log)"; exit 1; }
export AIPL="$PWD/target/bootstrap/aipl" AIPL_STD="$PWD/aipl_src/std/"
step "self-hosted: aipl test suite"; note "$("$AIPL" run aipl_src/test_suite.aipl 2>&1 | tail -1)"
step "self-hosted: conformance"; note "$(python3 tools/conformance.py --aipl "$AIPL" --backend run --backend wasm --backend native -j 4 2>&1 | tail -4 | tr '\n' ' ')"
step "self-hosted: aipl lint on the standard library and aipl_src"; note "$("$AIPL" lint aipl_src/std/*.aipl aipl_src/*.aipl aipl_src/native/*.aipl 2>&1 | tail -1)"
step "self-hosted: aipl fmt --check on the repository's programs"; if r=$("$AIPL" fmt --check $(find . -name '*.aipl' -not -path './target/*' -not -path './.git/*' | xargs grep -L -e @reject -e @syntax) 2>&1); then note "fmt: no file needs formatting"; else note "fmt FAILED: $(echo "$r" | head -1)"; fi
step "docs check"; note "$(python3 tools/docs_check.py 2>&1 | tail -1)"
step "grammar check"; note "$(python3 tools/grammar_check.py 2>&1 | tail -1)"
step "standard library reference"; note "$(python3 tools/std_reference.py --check 2>&1 | tail -1)"
step "release facts: one version, the seeds recorded"; if r=$(python3 tools/release_check.py version 2>&1 && python3 tools/release_check.py seeds 2>&1); then note "release facts ok"; else note "release_check FAILED: $(echo "$r" | tail -1)"; fi
unset AIPL AIPL_STD

# ===== The Rust referee: development only. Does the self-hosted toolchain do what the Rust one does, and the spec says? =====
step "build"; cargo build --release -q 2>&1 | tail -5
step "clippy"; r=$(cargo clippy --release -q --all-targets -- -D warnings 2>&1 | tail -20); [ -n "$r" ] && note "clippy: $(echo "$r" | head -1)"; echo "$r"
step "tests"; : > target/ci_tests.log
if [ -n "$CI_PLAIN" ]; then cargo test --release --no-fail-fast > target/ci_tests.log 2>&1
else python3 tools/ci_tests.py > target/ci_tests.log 2>&1; fi
note "$(grep -E "^test result" target/ci_tests.log | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}')"
grep -E "^test .* \.\.\. FAILED" target/ci_tests.log | head -20 | while read -r l; do note "$l"; done
step "native tests, openat2 refused"; note "$(python3 tools/without_openat2.py cargo test --release --test test_native 2>&1 | grep -E "^test result" | tail -1)"
step "referee: aipl test suite, in the Rust VM"; note "$(./target/release/aipl test aipl_src/test_suite.aipl 2>&1 | tail -1)"
step "fuzz.py"; note "$(python3 tools/fuzz.py --cases 3000 --seed 1 2>&1 | tail -1)"
step "run_fuzz.py"; note "$(python3 tools/run_fuzz.py --cases 500 --seed 1 2>&1 | tail -1)"
step "opt_diff.py"; note "$(python3 tools/opt_diff.py 2>&1 | tail -1)"; note "$(python3 tools/opt_diff.py --fuzz 500 --seed 1 2>&1 | tail -1)"
step "conformance, Rust aipl"; note "$(python3 tools/conformance.py --aipl target/release/aipl --backend run --backend wasm --backend native -j 4 2>&1 | tail -4 | tr '\n' ' ')"
step "harness selftest"; note "$(python3 tools/harness/harness.py selftest 2>&1 | tail -1)"
step "tree check: the tests left nothing behind"; note "$(python3 tools/tree_check.py 2>&1 | tail -2 | head -1)"
step "done"
