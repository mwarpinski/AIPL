#!/bin/sh
# tools/ci.sh on another machine, over ssh: the exact tree of a commit is sent
# there (rsync, by content, so a build already there is reused), every step of
# the CI runs there, and its output comes back as it is written.
#   sh tools/ci_remote.sh HOST [COMMIT]        # COMMIT defaults to HEAD
# The machine needs Linux x86-64, git, rsync, python3, a C compiler, and Rust
# (rustup, with clippy); it keeps the checkout and its build in ~/aipl-ci/work
# (AIPL_CI_DIR overrides it). The exit status is 0 only if the run reached its
# end and nothing in it failed.
set -u
host=${1:?usage: sh tools/ci_remote.sh HOST [COMMIT]}
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
# by content (-c), without times: only changed files change, so cargo rebuilds only what did
rsync -rlpc --delete --exclude=/target/ "$tmp"/ "$host:$dir"/ || exit 2
# tools/docs_check.py reads `git ls-files`: the directory is a repository whose index holds this commit's files
git -C "$here" ls-tree -r --name-only -z "$sha" | ssh -o BatchMode=yes "$host" "
  cd '$dir' && { [ -d .git ] || git init -q; } && git rm -r -q --cached . > /dev/null 2>&1
  xargs -0 git add -f -- > /dev/null" || exit 2
ssh -o BatchMode=yes "$host" "
  [ -f \"\$HOME/.cargo/env\" ] && . \"\$HOME/.cargo/env\"
  ulimit -n 65536 2>/dev/null || ulimit -n \$(ulimit -Hn) 2>/dev/null
  cd '$dir' && sh tools/ci.sh \"\$PWD\"" 2>&1 | tee "$log"
# the verdict: it ended, the tests passed, and nothing in it says it failed
ok=1
grep -q '^== done' "$log" || { echo "ci_remote: the run did not reach its end"; ok=0; }
grep -Eq '[0-9]+ passed, 0 failed' "$log" || { echo "ci_remote: no passing test summary"; ok=0; }
grep -q 'All groups passed' "$log" || { echo "ci_remote: the AIPL test suite did not pass"; ok=0; }
grep -Eq '^0 findings' "$log" || { echo "ci_remote: the documentation check found something"; ok=0; }
grep -Eq 'FAILED|[1-9][0-9]* failing|^clippy:|^seed build FAILED' "$log" && { echo "ci_remote: something in the run failed"; ok=0; }
rm -f "$log"
[ "$ok" = 1 ] && echo "ci_remote: ALL PASSED ($(printf %.7s "$sha") on $host)" || echo "ci_remote: FAILED ($(printf %.7s "$sha") on $host)"
[ "$ok" = 1 ]
