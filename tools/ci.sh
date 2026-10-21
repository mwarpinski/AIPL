#!/bin/sh
# Every step of .github/workflows/ci.yml, in the given checkout; each step
# also logged, timestamped, to target/ci_steps.log (tools/ci-status.sh reads it).
# The tests run one binary at a time, as cargo test runs them, through
# tools/ci_tests.py, which records each test's start, end, and time in
# target/ci_tests.jsonl; CI_PLAIN=1 runs plain `cargo test` instead.
#   sh tools/ci.sh /path/to/checkout
cd "$1" || exit 1
L=target/ci_steps.log
date '+%Y-%m-%d %H:%M:%S CI run' > $L
rm -f target/ci_tests.jsonl
step() { echo "== $1"; echo "$(date +%H:%M:%S) $1" >> $L; }
note() { echo "$1"; echo "           $1" >> $L; }
step "build"; cargo build --release -q 2>&1 | tail -5
step "seed build"; sh bootstrap/build.sh > target/ci_seed.log 2>&1 || { note "seed build FAILED: $(tail -1 target/ci_seed.log)"; exit 1; }
step "clippy"; r=$(cargo clippy --release -q --all-targets -- -D warnings 2>&1 | tail -20); [ -n "$r" ] && note "clippy: $(echo "$r" | head -1)"; echo "$r"
step "tests"; : > target/ci_tests.log
if [ -n "$CI_PLAIN" ]; then cargo test --release --no-fail-fast > target/ci_tests.log 2>&1
else python3 tools/ci_tests.py > target/ci_tests.log 2>&1; fi
note "$(grep -E "^test result" target/ci_tests.log | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}')"
grep -E "^test .* \.\.\. FAILED" target/ci_tests.log | head -20 | while read -r l; do note "$l"; done
step "aipl test suite"; note "$(./target/release/aipl test aipl_src/test_suite.aipl 2>&1 | tail -1)"
step "fuzz.py"; note "$(python3 tools/fuzz.py --cases 3000 --seed 1 2>&1 | tail -1)"
step "run_fuzz.py"; note "$(python3 tools/run_fuzz.py --cases 500 --seed 1 2>&1 | tail -1)"
step "done"
