#!/bin/sh
# The AArch64 tests on an Apple Silicon Mac over ssh: the exact tree of a
# commit is sent there (rsync, by content, so a build already there is
# reused), built, and the tests that build AArch64 executables run, each
# executable in a linux/arm64 Docker container on that Mac (tests/arm64).
#   sh tools/arm64_remote.sh HOST [COMMIT]     # COMMIT defaults to HEAD
# This is not tools/ci.sh: that needs Linux x86-64 (the native x86-64 tests,
# openat2, the fuzzers). The Mac needs git, rsync, Rust, a C compiler, and a
# running Docker; it keeps the checkout and build in ~/aipl-ci/work
# (AIPL_CI_DIR overrides it). The exit status is 0 only if every test passed
# and none of them said it had nothing to run on.
set -u
host=${1:?usage: sh tools/arm64_remote.sh HOST [COMMIT]}
commit=${2:-HEAD}
dir=${AIPL_CI_DIR:-aipl-ci/work}
here=$(cd "$(dirname "$0")/.." && pwd)
sha=$(git -C "$here" rev-parse --verify "$commit^{commit}") || exit 2
tmp=$(mktemp -d)
log=$(mktemp)
trap 'rm -rf "$tmp"' EXIT
echo "== sending $(git -C "$here" log -1 --format='%h %s' "$sha" | cut -c1-100) to $host:$dir"
git -C "$here" archive --format=tar "$sha" | tar -x -C "$tmp" || exit 2
ssh -o BatchMode=yes "$host" "mkdir -p '$dir'" || exit 2
rsync -rlpc --delete --exclude=/target/ "$tmp"/ "$host:$dir"/ || exit 2
ssh -o BatchMode=yes "$host" "
  export PATH=/opt/homebrew/bin:/usr/local/bin:\$HOME/.cargo/bin:\$PATH
  cd '$dir' || exit 1
  echo '== build'; cargo build --release -q 2>&1 | tail -5
  echo '== arm64 tests'
  cargo test --release --test test_native_arm64 --test test_native_arm64_programs -- --nocapture 2>&1 | grep -E '^test |test result|not run|panicked|error'
  echo '== done'" 2>&1 | tee "$log"
ok=1
grep -q '^== done' "$log" || { echo "arm64_remote: the run did not reach its end"; ok=0; }
grep -Eq 'test result: ok\. [1-9]' "$log" || { echo "arm64_remote: no passing test summary"; ok=0; }
grep -Eq 'FAILED|panicked|^error|not run' "$log" && { echo "arm64_remote: a test failed or had nothing to run on"; ok=0; }
rm -f "$log"
[ "$ok" = 1 ] && echo "arm64_remote: ALL PASSED ($(printf %.7s "$sha") on $host)" || echo "arm64_remote: FAILED ($(printf %.7s "$sha") on $host)"
[ "$ok" = 1 ]
