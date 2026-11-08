#!/usr/bin/env python3
"""The repository's top level holds only what belongs there, and the tests leave it that way.

Run it after the tests (CI does): a test that writes into the working directory leaves a file here, and this fails
on it. A new top-level name is a decision, so it is added to the lists below with the commit that adds the file.

    python3 tools/tree_check.py
"""
import os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# What may sit in the repository root. target/ and the editor folders are not committed but are expected.
ROOT_OK = {
    ".cargo", ".git", ".gitattributes", ".github", ".gitignore", ".vscode",
    "AIPL_SPEC.md", "CONTRIBUTING.md", "Cargo.lock", "Cargo.toml", "DEVELOPING.md", "LICENSE", "PROMPT_GUIDE_FOR_AIS.md",
    "README.md", "ROADMAP.md", "SECURITY.md",
    "aipl_src", "benchmarks", "bootstrap", "docs", "examples", "install.ps1", "install.sh", "referee", "target",
    "tests", "tools",
}
# The Rust crate: its sources and tests. Cargo's own output goes to the root's target/.
REFEREE_OK = {"Cargo.toml", "build.rs", "src", "tests"}
# Only source files live in the test folders; a test's scratch files go in a temporary directory.
TESTS_OK = {"aipl", "arm64", "conformance"}


def stray(folder, ok):
    path = os.path.join(ROOT, folder) if folder else ROOT
    return [os.path.join(folder, n) for n in sorted(os.listdir(path)) if n not in ok]


def main():
    bad = stray("", ROOT_OK) + stray("referee", REFEREE_OK) + stray("tests", TESTS_OK)
    # tools/__pycache__ is ignored by git and Python makes it; anything else new in tools/ is checked by the docs check
    if bad:
        print("tree check FAILED: not expected here: " + ", ".join(bad))
        print("(a test wrote into the working directory, or a new top-level file needs adding to tools/tree_check.py)")
        return 1
    print("tree check ok: the repository's top level holds only what belongs there")
    return 0


if __name__ == "__main__":
    sys.exit(main())
