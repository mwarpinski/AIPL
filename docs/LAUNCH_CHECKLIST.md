# Launch checklist

What is left before AIPL launches, rewritten 2026-11-06 from a principal-architect review of the repository and merged
with the checklist that came before it. That checklist, with every item done from 2026-10-06 to 2026-11-06 and why, is
[history/LAUNCH_CHECKLIST_2026-10.md](history/LAUNCH_CHECKLIST_2026-10.md). Work top to bottom; tick items as they land,
with the commit or date. When a section is all ticked, it can move to the record.

**The bar.** A newcomer can install AIPL, write and run a program, get errors that point at the fix, and trust that the
language will not change under them without warning. Nothing a reader finds in the first hour is embarrassing, and every
claim survives being checked.

**Two decisions shape this list** (the owner, 2026-11-06):

- **No thesis work before launch.** Whether agents write correct programs more often in AIPL is measured after launch,
  on the final language (After launch, below). The pitch at launch is what AIPL is and how it is checked, not a result.
- **Rust is a development-time referee only.** Through alpha and beta it runs in development CI to check that the
  self-hosted toolchain does what the spec says. The release ships no Rust compiler and builds only one. Removing Rust
  entirely is its own track after launch (the last section).

## Where we stand (2026-11-06)

v0.2.1 is the latest release; `main` and `development` also have `aipl lint`, error lines with a caret, arguments counted
from 1, a "did you mean" for undefined names, and `aipl repl` (b42503e). Section 1 below is built on the branch
`features/language-1` (2026-11-07) and is waiting for its seed refresh. Linux ships the self-hosted `aipl` plus
`aipl-net`; macOS and Windows ship the Rust `aipl` and `aipl-run`. The language is versioned from 1 (the first
release that carries section 1) and the FAQ still says it is not ready for real use.

Section 1 comes first: its changes break programs, and they should share one seed refresh. Sections 2 and 7 then
describe the language section 1 leaves.

## 1. Language decisions that break programs

Settle these together, then refresh the seed once, from the commit that has all of them. After launch, each one becomes
a deprecation instead of an edit.

- [x] **Integer overflow.** *(2026-11-07: `+ - *` on `i32` and `i64` trap; `wrap.add`, `wrap.sub`, `wrap.mul` wrap; the optimizer and constant folding keep the trap. The recommendation below is what was built.)* `(+ 2147483647 1)` gives `-2147483648` in every backend, and `checked.add` is opt-in.
  Recommendation: checked by default, with wrapping spelled explicitly (a form such as wrap.add, or separate
  operators). Silent wrapping is the kind of mistake the README says AIPL exists to catch.
- [x] **Errors that compose.** *(2026-11-07: `(try r (ref convert))`.)* `try` requires the function's error type to be the same as the result's ("try's error is
  io.Error, and the function's is str: they must be one type"). Add one way to convert, such as a `try` that takes a
  mapping function or an error interface, so io, json, url, and csv errors meet without a hand-written `match_result`
  at every call.
- [x] **Errors that carry context.** *(2026-11-07: `io.Error` is a value struct of `kind` (`io.Kind`), `errno`, and `path`; `io.describe` says all of it. `proc.Error` and the other modules' errors still copy the old pattern and follow when they are next touched.)* `io.Error` is an enum with no payload: no path, no errno, and `other` stands for
  several failures. Decide what a failure carries before every std module copies the pattern.
- [x] **Searches return an Option.** *(2026-11-07: `str.find`, `find_from`, `rfind`, `find_byte`, and `vec.index_of`.)* `str.find` and `vec.index_of` return -1 for not found (ROADMAP.md, "What the
  language does not do"), against docs/design/LANGUAGE_DECISIONS.md item 2 ("a missing value is an Option").
- [x] **Strings that are safe by default.** *(2026-11-07, not as first written: `format_to` into a `buf` from an allocator, `buf.concat a x y`, and a `(no_leak)` error that names that pattern; no `format_in` form. `aipl lint` flags `format` and `str.concat` in a loop.)* A `str` made at run time is never freed, and `format` and `str.concat` are
  refused in a `(no_leak)` function, so the easiest code is wrong for a program that runs for days. Options: `format`
  takes an allocator, or strings can come from one. At the least, `aipl lint` flags a `format` or `str.concat` in a loop.
- [x] **A language version** *(2026-11-07: `(language N)`, the `aipl.language` custom section, docs/LANGUAGE_VERSIONING.md)* in the module header and in the compiler's output, and a written deprecation policy
  (ROADMAP.md item 9). Packages need it later.
- [x] **The size limits, decided on purpose.** *(2026-11-07: results are values that can be struct fields, array elements, and type arguments; a value holds 16 scalars; the rest are in docs/LIMITS.md.)* A result cannot be a struct field or an array element; a value struct
  holds at most 8 scalars and a union 8 slots; `match` has no nested patterns; there are no unsigned types. Lift what
  1.0 needs, and document the rest in one place, each with its workaround.
- [x] **Threads labelled honestly.** *(2026-11-07: experimental in the spec, README, and FAQ, and outside the version promise.)* A threaded module needs wasi-threads, which wasmtime removed in version 47, and
  threads need `unsafe`. Mark threads experimental for 1.0, or commit to what replaces them.
- [x] **The open friction items** *(2026-11-07: row 8 by an `aipl lint` rule, `sys-print-newline`; row 31 by the circular-import message naming the hidden module; row 11 (constant arrays) and row 35 deferred, both additive.)*, each closed or deferred by name (docs/design/AGENT_FRICTION.md): `sys.print` and
  `print` disagreeing on the newline (row 8), constant arrays (row 11), a file in the program's folder hiding a std
  module (rows 31 and 35).
- [x] **One seed refresh**, then PROMPT_GUIDE_FOR_AIS.md's pitfall table updated once. *(2026-11-07: both seeds rebuilt from 6ddef9d, each its own stage 2; the x86-64 one is 5.7 MB, from 4.8 MB, and the AArch64 one 5.1 MB, from 4.2 MB, because every integer `+ - *` is now checked. The prompt guide's rules 11 and 19 and the pitfall about `-1` searches are updated.)*

## 2. Specification

The spec has to define the language without pointing at an implementation. That is also what lets Rust go later.

- [x] **Split AIPL_SPEC.md** *(2026-11-07: AIPL_SPEC.md is the normative language reference, 189 KB from 285 KB and one table row over a thousand characters; the tools, backends, compiled-module detail, and tests moved to docs/IMPLEMENTATION.md, and the library's contract to docs/STD_REFERENCE.md, generated by `aipl doc` and checked by CI.)* into a normative reference (grammar, static rules, run-time behaviour, the standard
  library's contract) and implementation notes (source paths, test names, dates, backend detail). Today it is 285 KB,
  with 35 lines over 1,000 characters (the longest 16,210) and about 80 references to source paths.
- [x] **State the semantics in the spec.** *(2026-11-07: section 5, with the evaluation order, numbers, memory, traps, host operations, and what implementations may differ in; section 6.4 says what conforming means.)* Replace "WebAssembly semantics are the specification" with the rules written
  out, so wasm becomes one lowering among several.
- [x] **Complete the grammar.** *(2026-11-07: every form, the lexical classes, `(pub)` and `(read ...)`, named field lists, typed literals.)* The EBNF lacks `defer`, `try`, `try_opt`, `try_or`, `drop`, `assert`, `abort`, `when`,
  `print`, `println`, `format`, `format_to`, `test`, `zero`, `arr.of`, typed literals (`5i64`, `1.5f32`), named field
  lists (`[x:1 y:2]`), and `(pub)`.
- [x] **Test the grammar** *(2026-11-07: tools/grammar_check.py reads the EBNF out of the spec and every program in the repository, with its own lexer, in CI; it found the gaps above.)* so it cannot drift again: every program in the repository parses under it.
- [x] **Fix the opening.** *(2026-11-07.)* It still says native code is Linux x86-64 only and that the first build takes the Rust
  toolchain.
- [x] **A conformance suite** *(2026-11-07: tests/conformance/, 173 cases over 35 sections, run by tools/conformance.py on any `aipl` and on other implementations by command; it found that the spec still said a bare `(ok v)` was allowed.)* tied to the spec's sections: programs with their expected output, traps, and diagnostics,
  independent of either compiler. It is the referee that outlives Rust (the last section).

## 3. Toolchain and the first hour

Each of these lands in the self-hosted `aipl` (aipl_src/), since that is what ships.

- [x] **Error lines, arguments, suggestions, lint, and a REPL** (on `development`: f9dd9f1, 70f63a3, 47fcc6f, b42503e):
  an error shows its source line with a caret, arguments count from 1, an undefined name suggests a near one,
  `aipl lint` has nine rules, and `aipl repl` takes expressions and definitions.
- [x] **The standard library and aipl_src lint clean, and CI keeps them so** (decided 2026-11-07; `tools/ci.sh` and
  the workflow run `aipl lint` over `aipl_src/`, its `std/` and `native/`). The findings were dead code and unused
  names, deleted; a few leaks and functions called only by the Rust tests carry a `;; lint:allow` with the reason.
- [x] **`aipl fmt [--check] FILE...` and `aipl fmt -`** (aipl_src/fmtcmd.aipl, in both commands). It settles
  white space and indentation only and keeps every line break and comment (gofmt style); it is idempotent and moves no
  token, over all 403 programs in the repository. A test keeps the repository formatted.
- [x] **Status lines off standard output.** `aipl eval` prints only what the program prints; `aipl compile` is silent
  on success and says what it built with `-v`, to standard error.
- [x] **Every error, one per item.** Each enum, struct, union and function signature reports its first error, in
  that order; once they hold, each function body does; at most 20, then `and N more errors`. Both checkers give
  the same lines (referee/tests/test_bootstrap.rs). A syntax, import or constant error still stops at once.
- [x] **"expects 1 argument"**, in both checkers.
- [x] **A standard library that cannot be found names `$AIPL_STD`.**
- [x] **One table of exit statuses** (AIPL_SPEC.md 6.1): main's result, `sys.exit`, a trap and an out-of-range status
  (134), what `aipl run` passes on, and the tools' (refused program 1, usage 2, lint findings 1, `fmt --check` 1).
  Open: on a trapping program the self-hosted `aipl run` adds one stderr line ("exit with invalid exit status ...")
  that the Rust command does not; the status is the same. It needs a silent exit primitive.
- [x] **Editor support at launch**: the TextMate grammar and a tree-sitter grammar (highlights, indents, folds), both
  checked against every `.aipl` file in this repository (~/projects/aipl-editors; the GitHub remote and the
  Marketplace and Open VSX publishing stay with the owner). The language server can follow.
- [x] **`aipl run` compared across the two commands** over the whole corpus (referee/tests/test_bootstrap.rs).

## 4. Release: one compiler, no referee

Today every leg of .github/workflows/release.yml runs `cargo build --release --locked`. The Linux tarball ships the
self-hosted `aipl` and `aipl-net` (Rust: host names and TLS); macOS and Windows ship the Rust `aipl` and `aipl-run`
(wasmtime), and there `compile --exe` bundles a 15 to 20 MB launcher.

- [x] **Launch platforms** (decided 2026-11-07): Linux x86-64 and AArch64 are first-class, built from the seed with
  no Rust compiler; macOS and Windows are a preview that ships the Rust `aipl` until the Mach-O and PE backends exist
  (the release notes and docs/RELEASING.md say so).
- [x] **The Linux release builds from the seed** (`bootstrap/build.sh`, stage 3 equal to stage 2, then `aipl
  --version` must say the tag's version); no Rust `aipl`, no second compiler. The release's gate job refuses a tag
  that is not on `main` or whose commit has no passing CI run. Rehearse with "Run workflow" (builds, install-tests and
  signs, publishes nothing) or `sh tools/release_dry_run.sh`.
- [x] **`aipl-net` stays for launch** as a documented helper, the one Rust program in the Linux tarball, built by its
  own step; replacing it is on the Rust removal track.
- [x] **One version source**: aipl_src/aipl.aipl. `python3 tools/release_check.py set X.Y.Z` writes it, referee/Cargo.toml and
  Cargo.lock; CI and the release gate fail when they differ; the notes are generated (tools/release_notes.py).
- [x] **Signed release**: Sigstore keyless over `SHA256SUMS` (`SHA256SUMS.bundle`), verified in the workflow as a user
  would. Open, because macOS and Windows are a preview: notarising the macOS build and code-signing the Windows one.
- [x] **Install-tested** (2026-11-07), with these open: the GitHub Actions workflow itself has not run yet, so its
  first run is the manual "Run workflow" rehearsal; and the Windows tarball was not installed this round.
  - Linux x86-64: `tools/release_dry_run.sh` on arcadia: seed build, tarball, `install.sh` into an empty home, run,
    `compile --exe`, `fmt`, `lint`. The `aipl` is a static executable, and ran in Debian 11, Ubuntu 22.04 and Alpine 3.20
    containers (x86-64, under Rosetta 2, which is also the native-executable-under-Rosetta check). `aipl-net` needs glibc
    2.34 or newer as measured on arcadia (the release states 2.35, Ubuntu 22.04's, and the build checks it): it runs on
    Ubuntu 22.04, not on Debian 11 or Alpine, so there host names and https do not work.
  - Linux AArch64: the same script in a `linux/arm64` Debian container on the Mac mini (Apple Silicon): passes. That is
    an AArch64 Linux userland, not a bare-metal AArch64 Linux machine.
  - macOS preview: `sh tools/mac_check.sh` on the Mac mini (macOS 27.2, arm64): 34 of 36, the two failures being a stale
    check (`str.find` answers an option now; fixed) and the document check, which needs a git clone.
- [x] **Seeds stay in git** (decided 2026-11-07; the pack is 5 MB). Each seed's hash is recorded in
  bootstrap/README.md, and CI fails when a seed changes without it (`python3 tools/release_check.py seeds`).

## 5. Rust as the referee during alpha and beta

The referee answers one question in development CI: does the self-hosted toolchain do what the spec says? Users never
build it, and the release never runs it.

- [x] **The Rust crate is out of the root** (2026-11-07): `referee/` is a member of a root Cargo workspace
  (`Cargo.toml` names it and holds the release profile). `src/`, `build.rs` and the Rust tests moved there; `target/`,
  `Cargo.lock`, `tools/` and the language's own test data (`tests/aipl`, `tests/conformance`, `tests/arm64`) stayed, so
  `cargo build --release` and every `target/release/aipl` path still work. The README's build from source leads with the
  seed (`sh bootstrap/build.sh`).
- [x] **CI is split** into `selfhosted` (the compiler built from the seed, then on it: the AIPL suite, conformance,
  lint, `fmt --check`, the documentation, grammar, reference and release checks) and `referee` (clippy, `cargo test`, the
  fuzzers, the optimizer check, conformance on the Rust `aipl`), in `.github/workflows/ci.yml` and `tools/ci.sh`. The
  release gate requires the tagged commit's whole CI run to have passed.
- [x] **The self-hosted commands are CI's main path**: `aipl test`, conformance, lint and fmt run on the compiler built
  from the seed; the Rust VM runs the AIPL suite again only as the referee.
- [x] **What the referee checks** (decided 2026-11-07): the same programs accepted and refused, the same diagnostics, the
  same behaviour; bytes only for the bootstrap's fixed point. Open: the byte comparisons that exist
  (`referee/tests/test_selfhost.rs`, `aipl compile --self`, those in `referee/tests/test_bootstrap.rs`) all pass today and
  are kept as a tripwire until the self-hosted compiler is meant to emit other bytes; DEVELOPING.md says the same change
  then turns each into a behavioural check. Converting them now would trade working coverage for freedom nobody uses yet.
- [x] **The twin rule** is in DEVELOPING.md: AIPL first, the Rust twin follows, the spec and conformance suite decide.
- [x] **The seed rule, automated**: `sh tools/install_hooks.sh` installs a pre-push hook (`tools/hooks/pre-push`) in which
  the seed builds `aipl_src/aipl.aipl` (about 5 seconds); CI runs the whole `bootstrap/build.sh`.

## 6. Repository hygiene

- [x] **Tests that stay in their own folder** (2026-11-08). The stray `no-such-dir/` came from one test case: the
  `FsMkdir` case in referee/tests/test_opcode_conformance.rs passed a length one short, so `"no-such-dir/x"` became
  `"no-such-dir/"` and the VM made it in the working directory. The three cases now name `/no-such-dir`, with the right
  lengths. The strays in the root (`adir`, `file.txt/`, `link`, `link2`, `made/`, `_lines.txt`, `out.wasm`) are gone, and
  `tools/tree_check.py` runs at the end of both CI groups and fails when the root, `referee/` or `tests/` holds anything
  not listed in it.
- [x] **One copy of each note** (2026-11-08). The eight `.txt` files were raw model transcripts with tool traces, the
  `.md` files their cleaned copies (the ones the README links, and the ones REVIEW.md quotes). The `.txt` copies are
  removed (in git at bdb2e41); `review_and_next_steps.txt`, markdown already, became `.md`; the notes' links to the
  author's home directory now point into the repository.
- [x] **Delete the merged branches** (2026-11-08): the 16 `features/` branches were all ancestors of `development`; the
  local ones are deleted. **Open: the 16 on origin**, deleted by the owner (`git push origin --delete ...`; the
  assistant's attempt was refused).
- [x] **A security policy** (2026-11-08): SECURITY.md (GitHub's private vulnerability report, the replies promised within 7
  and 14 days, what counts and what does not), CONTRIBUTING.md, bug and feature templates, a pull request template.
  **Open for the owner**: switch on *Private vulnerability reporting* in the repository's Settings, Code security (the
  report link in SECURITY.md needs it), and confirm the 7/14/90-day times are ones you will keep.

## 7. Documentation that matches the release

Last before the final checks, against the language section 1 leaves.

- [x] **README** (2026-11-08): arguments are counted from 1, and the error is printed as the toolchain prints it (source
  line and caret); the native executable sizes were stale (hello 84 KB, word_count 129 KB, jsonpp 410 KB: measured
  again), `aipl lint` points at the rules in USING_AIPL.md, the REPL is marked Rust-only.
- [x] **docs/FAQ.md** (2026-11-08): the platforms answer says Linux x86-64 and AArch64 are first-class with native
  executables and the self-hosted `aipl`, macOS and Windows a preview with the launcher bundle; the editor answer lists
  the grammars and the VS Code extension and says there is no language server yet.
- [x] **ROADMAP.md** (2026-11-08): every line of "What the language does not do" was tried on the self-hosted `aipl`
  or checked in the spec. Wrong and fixed: a result in statement position is a compile error, the searches return an
  option, diagnostics give the first error of each definition (not one per function). The "where it stands" text and the
  first-hour items were rewritten for what is built, and the items renumbered (the references here follow).
- [x] **`aipl test --help`** (2026-11-08): leads with the `(test ...)` items; the failure-count entry point is second.
- [x] **One pass** (2026-11-08) of the README, TOUR, FAQ and USING_AIPL against the self-hosted `aipl`: the nine
  complete programs verify, the TOUR's eight programs print what the text says, the README's example, first program
  and tool commands run. Wrong and fixed: USING_AIPL sent Linux users to `aipl-run` (which only macOS and Windows have;
  `aipl run` works everywhere), the TOUR's error examples were the old short messages, DEVELOPING.md led with `cargo test`.
- [x] **tools/docs_check.py checks claims too** (2026-11-08): the complete programs in the four front documents must
  verify; the error messages they quote must be text the toolchain prints; the platforms in the FAQ and RELEASING.md
  must be the ones release.yml builds; every command in `aipl --help` must be in the command table; the flags are read
  from the sources, so the check no longer needs the Rust binary. It also no longer passes vacuously in a copy that is not
  a git clone (the CI machine's), where it had listed no documents at all.
- Found, not fixed: the self-hosted `aipl run FILE.wasm` prints one extra stderr line when the program traps
  (`exit with invalid exit status`), because it re-exits with the child's 134; it wants a silent-exit primitive in the
  native backend. The documents show traps from a compiled executable, which does not.

## 8. Right before launch

- [x] **Every box above is ticked or has a stated reason** (2026-11-08): sections 1 to 7 are all ticked. What is open,
  and why it does not block the launch: the owner still has to enable private vulnerability reporting in the repository's
  settings and confirm the 7, 14 and 90 day times in SECURITY.md (only the owner can); `aipl-editors` has a local
  commit to push; the self-hosted `aipl run FILE.wasm` prints one extra stderr line when the program traps (cosmetic,
  needs a silent-exit primitive in the native backend); the macOS binary is not notarised and the Windows one is not
  code-signed (the launch notes say so, and a preview is what they are); Linux AArch64 is built and tested on GitHub's
  arm64 machines but the installer has not been run on a physical one (ROADMAP item 7); and one unexplained failure of
  `aipl_run_says_the_same_in_both_commands` on arcadia (once, "cannot read entry file"), not reproduced in 3,200 stress runs
  and every full run since; if it recurs it is to be investigated, not retried.
- [x] **From a fresh clone, by the README alone** (2026-11-08, arcadia, with no Rust on the `PATH`): `git clone`, then
  `sh bootstrap/build.sh` builds the compiler from the seed and reaches the fixed point; `aipl run aipl_src/test_suite.aipl`
  exits 0 (all groups pass); `tools/bench.py` builds and times the eight benchmarks with the self-hosted compiler (it
  used the Rust one before, and now takes `--aipl` or `$AIPL`) and every version of each prints the same output. The
  referee's CI passed on the same line of commits: `tools/ci.sh` on arcadia at 0c483e4 (590 tests, the native tests
  without openat2, 3,000 mutated and 500 generated programs 0 failing, the optimizer against itself, conformance 174,
  124 and 124, lint, fmt, docs, release facts), and GitHub's five jobs (selfhosted, referee, arm64, install, windows)
  at 45eda72, which differs from 0c483e4 only in documents. (The fresh clone was at 83f46e1; what came after changed
  the release workflow, the Windows packaging script, the documents and the checklist, not the compiler or the seed.)
- [x] **The release workflow rehearsed** (2026-11-08, Actions, "Run workflow" on `development`, three runs): the first
  two found real faults, and the third (45eda72) passed gate, all four builds, signing and the check a user would make
  of the signature, publishing nothing. Found and fixed: GitHub no longer provides `macos-14` (now `macos-15`), and the
  Windows `.sha256` file had CRLF line endings, so it disagreed with `SHA256SUMS` (now one LF-ended line, and CI checks it).
- [x] **The documentation check is rerun, and the numbers it cannot count are measured again** (2026-11-08):
  `python3 tools/docs_check.py` reports 0 findings in 75 documents. On a quiet arcadia (the Windows VM that keeps its
  processors busy was paused; with it running every time was up to twice as long), from a fresh clone: `sh
  bootstrap/build.sh` takes 16 s (the documents said 8 and 13), the compiler compiling itself 5.3 s (said 3), the AIPL
  suite 3.2 s, `jsonpp` compiles in 0.4 s to 410 KB, `word_count` 129 KB, hello 85 KB (those three were right). The
  benchmark table in docs/BENCHMARKS.md is the day's (best of five), and the speed claims in the README, FAQ and launch
  notes follow it: native 1.2 to 7.1 times C on the seven with a C time here, WebAssembly 1.5 to 6.7. pidigits has no C
  time here (no libgmp-dev on arcadia: `sudo apt install libgmp-dev` would add it). The AArch64 build time (about 8 s a
  stage) is from GitHub's arm64 runner.
- [ ] `development` is merged into `main` and tagged, with the owner's approval.
- [x] **The launch notes** (2026-11-08): docs/LAUNCH_NOTES.md says what AIPL is, what is unfinished (the macOS and
  Windows preview, no notarisation or code signing, the thesis not measured, no stability promise, no packages or
  language server, no TLS in AIPL, the rough edges) and how it was built (by AI agents, and the checks that follow
  from that). It is the text for the release page; the workflow's generated notes (platforms, install, verify,
  commits) go below it.

## After launch (deferred on purpose)

None of these blocks launch; the launch notes say which are coming.

- **Measuring the thesis.** Run the A/B harness (tools/harness/README.md) on the final language and publish the tasks,
  prompts, transcripts, and numbers, including where AIPL loses (the record, section 5). Before it runs: add tasks where
  contracts, overflow, and error handling matter; count tokens and rounds as well as passes; give Python a guide as long
  as AIPL's, as a control; repeat each task, with several models. The syntax proposals (inferred `let` types, `(f x)`
  without `call`) wait for its numbers (docs/design/CANONICAL_PLAN.md step 7).
- **Packages and a registry** (ROADMAP.md item 3), once the language version exists.
- **The language server, the aipl.sh website and playground, and Homebrew, winget, and apt packages.**
- **Performance**: a register allocator, faster hashing of byte strings, big integers (ROADMAP.md item 13).
- **Concurrency beyond threads**: channels, and a safe way to share data (ROADMAP.md item 12).
- **The compiler's own debts**: codegen_opt.aipl's pattern matcher (the record, section 6), the self-hosted compiler's
  double parse, and exact trap positions inside format placeholders.
- **README COLD Test Run**: Someone who has never seen the project follows the README cold and reports where they got stuck.
- **Later ideas** (ROADMAP.md): a 64-bit target, AI-SQL, secret types.

## After launch: removing Rust entirely

Rust does six jobs today, and each needs its replacement before its code goes
(docs/design/aipl_standalone.md is the plan so far). The largest is the test suite, which is Rust too: 581 tests in
about 22,000 lines of tests/*.rs.

| Rust today | What it does | What replaces it |
|---|---|---|
| The reference toolchain (src/) | The referee: byte parity, and fuzzing against the self-hosted compiler | The conformance suite (section 2), the backends tested against each other, the bootstrap's fixed point |
| The VM (referee/src/vm.rs) | `eval -f`, `test --vm`, `--all-backends`, the AIPL suite in CI, one side of the differential tests | A VM in AIPL (aipl_standalone.md S4), or native compared with wasm alone |
| `aipl-run` (wasmtime) | Running `.wasm` off Linux; the macOS and Windows launcher bundles | Mach-O and PE backends (ROADMAP.md item 4), then a native wasm runner off Linux (S3) |
| `aipl-net` (rustls and the system's resolver) | Host names and TLS in native executables | A resolver in AIPL; for TLS, a decision: the system's library through a way to call foreign code, or an audited implementation (ROADMAP.md item 13) |
| The Rust-only modes of the CLI | `compile --target wasm`, `--self`, `eval --wasm` | The same modes in aipl_src/ |
| The test harness (tests/*.rs) | 581 tests, many of them programs inside Rust strings | `(test ...)` items, and a runner in AIPL or a script |

In order, each step unblocking the next:

- [ ] Write the conformance suite and run it against both toolchains, so the referee's verdict has a home outside Rust.
- [ ] Port the tests in tests/*.rs, the programs inside Rust strings included (AGENT_FRICTION.md row 59).
- [ ] Port the Rust-only modes of the CLI, and decide what becomes of the VM.
- [ ] Write the Mach-O and PE backends, then ship the self-hosted `aipl` on macOS and Windows (ROADMAP.md item 5).
- [ ] Replace `aipl-net`: the TLS decision, and the general way to call a component in another language
  (docs/design/NET_PLAN.md section 7).
- [ ] Record the last seed built by Rust and its hash; after it, seeds build only from seeds. Keep an older seed for
  diverse double-compiling, which takes over Rust's independent check against a compromised compiler.
- [ ] Delete Cargo.toml, referee/, and the Rust tests.
