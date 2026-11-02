#!/bin/sh
# Where the current (or last) local CI run is: the steps, the tests running
# now and for how long, each binary's result, time, and peak memory, the
# slowest tests, the failures. tools/ci_status.py has the details.
#   sh tools/ci-status.sh [/path/to/checkout] [--watch [SECONDS]] [--all]
exec python3 "$(dirname "$0")/ci_status.py" "$@"
