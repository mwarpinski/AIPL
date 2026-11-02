#!/bin/sh
# Packs the release build into the tarball install.sh downloads:
#
#   sh tools/package_release.sh [OUT_DIR]      # default: target/dist
#
# It packs target/release/aipl and aipl-run (run `cargo build --release
# --locked` first) and the documents and examples, as
# OUT_DIR/aipl-TARGET.tar.gz with OUT_DIR/aipl-TARGET.tar.gz.sha256 beside
# it, where TARGET is the platform's name as install.sh spells it
# (x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, aarch64-apple-darwin).
# The tarball holds one folder, aipl-TARGET: bin/ (the two programs) and
# share/ (README, the specification, the prompt guide, the licence, and the
# examples). The binaries carry the standard library, so nothing else is
# needed beside them.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/target/dist}"

case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) TARGET=x86_64-unknown-linux-gnu ;;
    Linux/aarch64 | Linux/arm64) TARGET=aarch64-unknown-linux-gnu ;;
    Darwin/arm64) TARGET=aarch64-apple-darwin ;;
    *) echo "package_release: no release target for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

for b in aipl aipl-run; do
    [ -x "$ROOT/target/release/$b" ] || { echo "package_release: build target/release/$b first" >&2; exit 1; }
done

NAME="aipl-$TARGET"
STAGE="$OUT/stage/$NAME"
rm -rf "$OUT/stage"
mkdir -p "$STAGE/bin" "$STAGE/share"
cp "$ROOT/target/release/aipl" "$ROOT/target/release/aipl-run" "$STAGE/bin/"
cp "$ROOT/README.md" "$ROOT/AIPL_SPEC.md" "$ROOT/PROMPT_GUIDE_FOR_AIS.md" "$ROOT/LICENSE" "$STAGE/share/"
cp -R "$ROOT/examples" "$STAGE/share/examples"

mkdir -p "$OUT"
tar -C "$OUT/stage" -czf "$OUT/$NAME.tar.gz" "$NAME"
rm -rf "$OUT/stage"
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$OUT" && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")
else
    (cd "$OUT" && shasum -a 256 "$NAME.tar.gz" >"$NAME.tar.gz.sha256")
fi
echo "package_release: $OUT/$NAME.tar.gz"
