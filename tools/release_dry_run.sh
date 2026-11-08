#!/bin/sh
# The Linux leg of .github/workflows/release.yml, on this machine (docs/RELEASING.md): the self-hosted aipl built
# from the seed with no Rust, aipl-net built by cargo, the tarball packed, and installed with install.sh into an
# empty home as a user would, then tried. Nothing is published or signed.
#
#   sh tools/release_dry_run.sh [OUT_DIR]      # default: target/release_dry_run
#
# With NO_AIPL_NET=1 it skips cargo (a machine with no Rust, a container): the tarball then lacks aipl-net, which the
# real release never does, and only the compiler is tried.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/target/release_dry_run}"
cd "$ROOT"

say() { printf 'release_dry_run: %s\n' "$1"; }

python3 tools/release_check.py version
python3 tools/release_check.py seeds
version="$(python3 tools/release_check.py show)"

say "the self-hosted aipl, from the seed"
sh bootstrap/build.sh
[ "$(target/bootstrap/aipl --version)" = "aipl $version" ] || { say "the self-hosted aipl says: $(target/bootstrap/aipl --version)"; exit 1; }

if [ "${NO_AIPL_NET:-}" != 1 ]; then
    say "aipl-net"
    cargo build --release --locked --bin aipl-net
    say "aipl-net needs $(objdump -T target/release/aipl-net 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1) (the release states 2.35 or newer)"
fi

say "package"
rm -rf "$OUT"
if [ "${NO_AIPL_NET:-}" = 1 ]; then
    mkdir -p target/release
    [ -x target/release/aipl-net ] || { printf '#!/bin/sh\nexit 2\n' > target/release/aipl-net; chmod +x target/release/aipl-net; stub=1; }
fi
sh tools/package_release.sh "$OUT/dist"
[ "${stub:-}" != 1 ] || rm -f target/release/aipl-net

say "install it as a user would, in an empty home"
home="$(mktemp -d)"
trap 'rm -rf "$home"' EXIT
mkdir -p "$OUT/site/latest/download"
cp "$OUT"/dist/* "$OUT/site/latest/download/"
HOME="$home" AIPL_RELEASES="file://$OUT/site" sh install.sh
HOME="$home" sh -c '. "$HOME/.aipl/env" && cd "$HOME" && aipl --version && aipl doc str | head -1'

say "and use it"
cat > "$home/hello.aipl" <<'AIPL'
(module hello
  (import io)
  (fn main [] -> i32
    (call io.println "hello from a release")
    0))
AIPL
cat > "$home/ugly.aipl" <<'AIPL'
(module ugly
 (fn main [] -> i32   0))
AIPL
HOME="$home" sh -c '. "$HOME/.aipl/env" && cd "$HOME" &&
  [ "$(aipl run hello.aipl)" = "hello from a release" ] &&
  aipl compile --exe hello.aipl -o hello && [ "$(./hello)" = "hello from a release" ] &&
  ! aipl fmt --check ugly.aipl >/dev/null 2>&1 && aipl lint hello.aipl'
say "ok: $(ls "$OUT"/dist/*.tar.gz)"
