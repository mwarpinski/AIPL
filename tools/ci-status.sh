#!/bin/sh
# Where the current (or last) local CI run is: the step it is on, the test
# binaries finished so far (of about 62), and any failures. Run it any time:
#   sh tools/ci-status.sh [/path/to/checkout]     # default: this checkout
T="$(cd "${1:-$(dirname "$0")/..}" && pwd)/target"
if [ -f "$T/ci_steps.log" ]; then
  echo "== steps (started $(head -1 "$T/ci_steps.log" | cut -c1-19))"
  tail -n +2 "$T/ci_steps.log"
fi
if [ -f "$T/ci_tests.log" ]; then
  done=$(grep -c "^test result" "$T/ci_tests.log")
  passed=$(grep "^test result" "$T/ci_tests.log" | awk '{p+=$4} END {print p+0}')
  echo "== tests: $done test binaries done, $passed tests passed so far"
  grep -E "^test .* FAILED|panicked at" "$T/ci_tests.log" | head -10
fi
if pgrep -f "cargo test --release --no-fail-fast" > /dev/null || pgrep -f "tools/fuzz.py\|tools/run_fuzz.py" > /dev/null; then
  echo "== still running"
else
  echo "== not running"
fi
