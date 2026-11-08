# Contributing to AIPL

Thank you. AIPL is a young language written for AI agents to write; its tools are written in AIPL, so a change usually
means writing some AIPL. Read [DEVELOPING.md](DEVELOPING.md) first: it has the layout, how to build and test, and the
conventions below in full.

## Before you start

- **Open an issue first** for anything bigger than a fix: a new language form, a standard library module, a change to a
  message. The language is versioned ([docs/LANGUAGE_VERSIONING.md](docs/LANGUAGE_VERSIONING.md)) and a change to it is a
  decision, not a patch.
- **Questions and ideas** go in an issue as well; there is no chat.
- **Security problems** go through the private channel in [SECURITY.md](SECURITY.md), not an issue.

## Building and testing

```sh
sh bootstrap/build.sh                      # the compiler, from the checked-in seed, with no Rust: target/bootstrap/aipl
target/bootstrap/aipl run aipl_src/test_suite.aipl
python3 tools/conformance.py --aipl target/bootstrap/aipl --backend run --backend wasm --backend native
```

`sh tools/ci.sh .` runs everything CI runs, including the Rust referee (`cargo test` takes a long time). Install the
pre-push hook once with `sh tools/install_hooks.sh`.

## What a change needs

- **Tests.** A bug fix gets a test that fails without it. Programs for the language's behaviour live in `tests/aipl/` and
  `tests/conformance/`; a test that writes files does it in a temporary directory, never the working directory
  (`python3 tools/tree_check.py` fails on a stray file).
- **AIPL first, Rust follows.** The self-hosted toolchain in `aipl_src/` is the product; the Rust crate in `referee/` is the
  referee and is brought along so both pass the conformance suite and print the same errors, word for word.
- **Formatted and lint-clean.** `aipl fmt` the files you touch (`aipl fmt --check` runs in CI); `aipl lint` reports no
  findings on `aipl_src/`.
- **The seed rule.** The compiler's source may use only what the checked-in seed knows; the pre-push hook checks it.
- **Documentation that does not lie.** `python3 tools/docs_check.py` checks the documents against the repository; if you
  change a message, a command, or a number, change the document that quotes it. `docs/STD_REFERENCE.md` is generated
  (`python3 tools/std_reference.py`).
- **One logical change per commit**, with a message that says why. Open the pull request against `development`;
  `main` is the release branch and the owner merges to it.

## Licence

By contributing you agree your work is released under the repository's [MIT licence](LICENSE).
