#!/bin/sh
# Runs AIPL test and verification steps in Linux containers on Apple Silicon
# via Docker Desktop (Task 4 in docs/APPLE_SILICON_HANDOFF.md).
#
# Usage:
#   sh tools/container_test.sh arm64   # Test on linux/arm64 (native speed)
#   sh tools/container_test.sh amd64   # Test on linux/amd64 (Rosetta emulation)
#   sh tools/container_test.sh qemu    # Test x86_64 seed & native binaries via QEMU
#
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-arm64}"

# Find Docker binary (handles Docker Desktop on macOS where CLI may be in /Applications/...)
DOCKER="docker"
if ! command -v "$DOCKER" > /dev/null 2>&1; then
    if [ -x "/Applications/Docker.app/Contents/Resources/bin/docker" ]; then
        DOCKER="/Applications/Docker.app/Contents/Resources/bin/docker"
    else
        echo "Error: docker not found in PATH or in /Applications/Docker.app" >&2
        exit 1
    fi
fi

case "$MODE" in
    arm64)
        echo "== Running verification in linux/arm64 container (native speed)..."
        "$DOCKER" run --rm --platform linux/arm64 \
            -v "$ROOT":/repo:ro \
            rust:1 bash -c '
                set -eu
                rustup component add clippy
                mkdir -p /build && tar --exclude=./target -cf - -C /repo . | tar -xf - -C /build && cd /build
                echo "Container OS / Arch:" && uname -a
                echo "Rust version:" && rustc --version
                echo "Building release:"
                cargo build --release
                echo "Running clippy:"
                cargo clippy --release --all-targets -- -D warnings
                echo "Running test suite:"
                cargo test --release --no-fail-fast
                echo "Running AIPL test suite:"
                ./target/release/aipl test aipl_src/test_suite.aipl
                echo "Running fuzz.py (3000 cases):"
                python3 tools/fuzz.py --cases 3000 --seed 1
                echo "Running run_fuzz.py (500 cases):"
                python3 tools/run_fuzz.py --cases 500 --seed 1
                echo "Running opt_diff.py:"
                python3 tools/opt_diff.py
                echo "Running opt_diff.py --fuzz (500 cases):"
                python3 tools/opt_diff.py --fuzz 500 --seed 1
                echo "Running harness.py selftest:"
                python3 tools/harness/harness.py selftest
            '
        ;;
    amd64)
        echo "== Running in linux/amd64 container under Rosetta emulation..."
        "$DOCKER" run --rm --platform linux/amd64 \
            -v "$ROOT":/repo:ro \
            rust:1 bash -c '
                set -eu
                mkdir -p /build && tar --exclude=./target -cf - -C /repo . | tar -xf - -C /build && cd /build
                echo "Container OS / Arch:" && uname -a
                echo "Rust version:" && rustc --version
                echo "Building:" && cargo build --release -q
                echo "Running tests:"
                cargo test --release --no-fail-fast
            '
        ;;
    qemu)
        echo "== Running x86_64 seed and native compiler under QEMU user-mode..."
        "$DOCKER" run --rm --platform linux/arm64 \
            -v "$ROOT":/repo:ro \
            rust:1 bash -c '
                set -eu
                apt-get update -qq && apt-get install -y -qq qemu-user-static > /dev/null
                mkdir -p /build && tar --exclude=./target -cf - -C /repo . | tar -xf - -C /build && cd /build
                export AIPL_STD=/build/aipl_src/std/
                echo "Testing seed binary (bootstrap/aipl-linux-x86_64):"
                qemu-x86_64-static ./bootstrap/aipl-linux-x86_64 --version
                echo "Compiling stage 1 with seed:"
                qemu-x86_64-static ./bootstrap/aipl-linux-x86_64 compile --exe aipl_src/aipl.aipl -o /tmp/aipl-stage1
                chmod +x /tmp/aipl-stage1
                echo "Testing generated stage 1 binary:"
                qemu-x86_64-static /tmp/aipl-stage1 --version
                echo "Success: seed and generated native binary execute cleanly under QEMU."
            '
        ;;
    *)
        echo "Usage: $0 [arm64|amd64|qemu]" >&2
        exit 1
        ;;
esac
