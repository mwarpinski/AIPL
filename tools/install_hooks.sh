#!/bin/sh
# Turns on the repository's git hooks (tools/hooks): a pre-push that checks the seed can build the compiler's source.
#   sh tools/install_hooks.sh
set -e
cd "$(dirname "$0")/.."
chmod +x tools/hooks/*
git config core.hooksPath tools/hooks
echo "install_hooks: git hooks are tools/hooks now (pre-push: the seed builds aipl_src/aipl.aipl)"
