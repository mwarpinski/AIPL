# The conformance suite

Programs, and what must happen to each, written against [AIPL_SPEC.md](../../AIPL_SPEC.md) and independent of both
compilers in this repository. An implementation of AIPL conforms (spec section 6.4) when it passes them. The Rust
toolchain passes them in development CI as the referee it is until it is removed; the self-hosted toolchain passes
the same ones, and they are what lets Rust go.

```
python3 tools/conformance.py --aipl PATH-TO-aipl            # the cases, run as `aipl run FILE` does
python3 tools/conformance.py --aipl PATH --backend wasm --backend native
python3 tools/conformance.py --section 5.4                  # one section of the spec
python3 tools/conformance.py --run-cmd 'mylang run {file} {args}' --check-cmd 'mylang check {file}'
python3 tools/conformance.py --check-cases                  # the suite's own consistency (no implementation needed)
```

## A case

One `.aipl` file. The comments at its top say what it tests and what must happen:

```
;; @spec 5.4                   the sections of the spec it tests
;; @kind trap                  run | trap | reject
;; @trap integer overflow      a trap: the message has this (or any `|` alternative), in any case
```

| Kind | Directives | Passes when |
|---|---|---|
| `run` | `@out LINE` (repeat), `@exit N`, `@err TEXT`, `@args`, `@stdin` | the program ends with that status (default 0), wrote exactly those lines to standard output, and (when given) its standard error has the text |
| `trap` | `@trap TEXT`, optionally `@out`, `@exit`, `@err` | the program ends with a status other than 0 and a message with the text |
| `reject` | `@reject L:C TEXT`, optionally `@syntax` | the implementation refuses the program at line `L`, column `C`, with a message containing the text; `@reject - TEXT` is for an error that has no position; `@syntax` says the program does not match the grammar of spec section 2, which `tools/grammar_check.py` checks too |

Positions count the file as written, directive lines included. A file with no `@kind` is a module that a case imports
(`imports/helper_sibling.aipl`). The module in a case is named after its file.

## What it does not test

Speed; the sign and payload of a NaN; the numbers a host gives as process and socket handles; the text of a message
beyond its key phrase (spec 5.9). Threads are experimental and have no cases.

## Adding a case

Write the program, say what must happen from the spec's text (not from what an implementation prints), and run it
against both toolchains. If they disagree with each other or with the spec, that is the finding: fix the spec, or the
toolchain, and keep the case.
