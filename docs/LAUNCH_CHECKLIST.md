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
  (ROADMAP.md item 11). Packages need it later.
- [x] **The size limits, decided on purpose.** *(2026-11-07: results are values that can be struct fields, array elements, and type arguments; a value holds 16 scalars; the rest are in docs/LIMITS.md.)* A result cannot be a struct field or an array element; a value struct
  holds at most 8 scalars and a union 8 slots; `match` has no nested patterns; there are no unsigned types. Lift what
  1.0 needs, and document the rest in one place, each with its workaround.
- [x] **Threads labelled honestly.** *(2026-11-07: experimental in the spec, README, and FAQ, and outside the version promise.)* A threaded module needs wasi-threads, which wasmtime removed in version 47, and
  threads need `unsafe`. Mark threads experimental for 1.0, or commit to what replaces them.
- [x] **The open friction items** *(2026-11-07: row 8 by an `aipl lint` rule, `sys-print-newline`; row 31 by the circular-import message naming the hidden module; row 11 (constant arrays) and row 35 deferred, both additive.)*, each closed or deferred by name (docs/design/AGENT_FRICTION.md): `sys.print` and
  `print` disagreeing on the newline (row 8), constant arrays (row 11), a file in the program's folder hiding a std
  module (rows 31 and 35).
- [ ] **One seed refresh**, then PROMPT_GUIDE_FOR_AIS.md's pitfall table updated once.

## 2. Specification

The spec has to define the language without pointing at an implementation. That is also what lets Rust go later.

- [ ] **Split AIPL_SPEC.md** into a normative reference (grammar, static rules, run-time behaviour, the standard
  library's contract) and implementation notes (source paths, test names, dates, backend detail). Today it is 285 KB,
  with 35 lines over 1,000 characters (the longest 16,210) and about 80 references to source paths.
- [ ] **State the semantics in the spec.** Replace "WebAssembly semantics are the specification" with the rules written
  out, so wasm becomes one lowering among several.
- [ ] **Complete the grammar.** The EBNF lacks `defer`, `try`, `try_opt`, `try_or`, `drop`, `assert`, `abort`, `when`,
  `print`, `println`, `format`, `format_to`, `test`, `zero`, `arr.of`, typed literals (`5i64`, `1.5f32`), named field
  lists (`[x:1 y:2]`), and `(pub)`.
- [ ] **Test the grammar** so it cannot drift again: every program in the repository parses under it.
- [ ] **Fix the opening.** It still says native code is Linux x86-64 only and that the first build takes the Rust
  toolchain.
- [ ] **A conformance suite** tied to the spec's sections: programs with their expected output, traps, and diagnostics,
  independent of either compiler. It is the referee that outlives Rust (the last section).

## 3. Toolchain and the first hour

Each of these lands in the self-hosted `aipl` (aipl_src/), since that is what ships.

- [x] **Error lines, arguments, suggestions, lint, and a REPL** (on `development`: f9dd9f1, 70f63a3, 47fcc6f, b42503e):
  an error shows its source line with a caret, arguments count from 1, an undefined name suggests a near one,
  `aipl lint` has nine rules, and `aipl repl` takes expressions and definitions.
- [ ] **Decide whether the standard library and aipl_src must lint clean in CI** (about 80 findings, most of them real
  dead code or unused names).
- [ ] **`aipl fmt` and `aipl fmt --check`.** The canonical printer exists (aipl_src/printer.aipl); make it a formatter.
  Check first that it keeps comments, since it prints resolved programs today.
- [ ] **Status lines off standard output.** `aipl eval` prints `[AIPL VM] Executing function 'main' from ...` and
  `aipl compile` prints `[AIPL Compiler] Successfully compiled ...`, both to standard output (src/main.rs). Send them to
  standard error, or behind `-v`: pipes and agents both pay for them.
- [ ] **Every error in a function**, not only the first (ROADMAP.md item 3).
- [ ] **"expects 1 arguments"** says "1 argument", in both checkers.
- [ ] **A standard library that cannot be found names the fix.** Run outside a checkout, the self-hosted binary says
  "Cannot resolve import 'show'"; it should name `$AIPL_STD` and the paths it searched.
- [ ] **One table of exit statuses**: main's result, `sys.exit`, a trap (134 today), a failed contract, and what
  `aipl run` passes on.
- [ ] **Editor support at launch**: a TextMate grammar at the least, then tree-sitter (ROADMAP.md item 2). The language
  server can follow.
- [ ] **`aipl run` compared across the two commands** on the whole example corpus in tests/test_bootstrap.rs, as
  `compile` is (ROADMAP.md item 1).

## 4. Release: one compiler, no referee

Today every leg of .github/workflows/release.yml runs `cargo build --release --locked`. The Linux tarball ships the
self-hosted `aipl` and `aipl-net` (Rust: host names and TLS); macOS and Windows ship the Rust `aipl` and `aipl-run`
(wasmtime), and there `compile --exe` bundles a 15 to 20 MB launcher.

- [ ] **Decide the launch platforms.** Recommendation: Linux x86-64 and AArch64 are first-class, built from the seed
  with no Rust compiler; macOS and Windows launch as a preview, since they ship the Rust `aipl` until the Mach-O and PE
  backends exist. Without that split, "no Rust in the release" cannot hold on the first day.
- [ ] **The Linux release builds only from the seed** (`sh bootstrap/build.sh`): no Rust `aipl`, no comparison of two
  compilers. It requires that the tagged commit passed the referee's CI (section 5).
- [ ] **Decide `aipl-net` for launch.** It is the last Rust in the Linux release: keep it as a documented helper for 1.0,
  and replace it on the Rust removal track.
- [ ] **One version source.** The version is bumped by hand in Cargo.toml and aipl_src/aipl.aipl (ROADMAP.md item 19).
  Make aipl_src/aipl.aipl the source, derive or check the rest, and generate the release notes.
- [ ] **Sign the release**: at the least the checksums (minisign or Sigstore). Notarise the macOS build and code-sign
  the Windows one if they ship (ROADMAP.md item 17).
- [ ] **Install-test every leg on a real machine.** Nobody has installed Linux AArch64 (ROADMAP.md item 9); state the
  oldest glibc supported and whether musl is. For the macOS preview, run `sh tools/mac_check.sh` on the Mac, and a
  native executable under Rosetta 2, which has not been run there since the `openat` fallback (record, "Where we
  stand", item 6).
- [ ] **Seeds out of git history.** bootstrap/aipl-linux-x86_64 and bootstrap/aipl-linux-aarch64 (8.6 MB together) add
  to the history at every refresh. Move them to release files or Git LFS, and keep their hashes in bootstrap/README.md.

## 5. Rust as the referee during alpha and beta

The referee answers one question in development CI: does the self-hosted toolchain do what the spec says? Users never
build it, and the release never runs it.

- [ ] **Move the Rust crate out of the root**, into a `referee/` folder or a workspace member, so the repository reads
  as the AIPL toolchain; the README's build from source leads with the seed.
- [ ] **Split CI** into a referee job (`cargo test`, the Rust-against-self-hosted fuzzer, byte parity) and a main job
  that runs the self-hosted commands. The release job only checks that the tagged commit's referee job passed.
- [ ] **The self-hosted commands are CI's main path.** CI runs `aipl test aipl_src/test_suite.aipl` through the Rust VM
  today (docs/design/aipl_standalone.md, S2's leftover).
- [ ] **Decide what the referee checks.** In order of value: (a) the same programs accepted and refused, (b) the same
  diagnostics, (c) the same behaviour at run time, (d) the same bytes. Keep (d) only where it earns its cost, such as the
  bootstrap's fixed point (stage 2 equals stage 3), and let the self-hosted optimizer make other bytes.
- [ ] **The twin-change rule in DEVELOPING.md.** Either a change lands in AIPL first and Rust follows, or the Rust front
  end is frozen and checked through the conformance suite (section 2). AGENT_FRICTION.md rows 46, 62, and 66 are what
  a missed twin costs.
- [ ] **The seed rule, automated.** The compiler's source may not use what the seed does not know (AGENT_FRICTION.md
  rows 14, 32, 52, and 57). A pre-push hook running `sh bootstrap/build.sh` catches it in 3 seconds.

## 6. Repository hygiene

- [ ] **Tests that stay in their own folder.** An empty no-such-dir/ and a stray out.wasm appear in the repository
  root; tests/test_opcode_conformance.rs uses `no-such-dir` paths relative to the working directory. Run the file-system
  tests in a temporary directory.
- [ ] **One copy of each note.** docs/notes keeps every note as both `.md` and `.txt`, and the pairs differ.
- [ ] **Delete the merged branches**: all 11 `features/` branches are merged into `development`.
- [ ] **A security policy** (a SECURITY.md: a private channel for reports, and how soon they are answered), since the
  memory-safety claims will draw them; a CONTRIBUTING.md and issue templates beside it.

## 7. Documentation that matches the release

Last before the final checks, against the language section 1 leaves.

- [ ] **README:** "arguments are counted from 0" sits above `Argument 2 of 'gcd'`; arguments count from 1 since
  70f63a3.
- [ ] **docs/FAQ.md, "What platforms does it run on?":** says Linux x86-64 is the only platform with native executables;
  Linux AArch64 has them too. "Is there editor support?" changes when the grammar ships.
- [ ] **ROADMAP.md, "What the language does not do":** says a result in statement position is discarded silently; it is
  a compile error now (AGENT_FRICTION.md row 70). The list says it was not re-checked after 2026-10-27: re-check every
  line.
- [ ] **`aipl test --help`** still describes the old entry point ("must return an i32 failure count").
- [ ] **One pass** of the README, docs/TOUR.md, and docs/FAQ.md against the self-hosted `aipl` and the modes it drops
  (ROADMAP.md item 5).
- [ ] **tools/docs_check.py checks claims too**, not only names: run the error messages the documents quote and compare
  the text, compare the platform lists with release.yml, and compare `--help` with the spec's command table.

## 8. Right before launch

- [ ] Every box above is ticked or has a stated reason.
- [ ] From a fresh clone, by the README alone: the seed builds the compiler, and the AIPL suite and the benchmarks run
  clean with no Rust. The referee's CI (`cargo test` and both fuzzers, an hour each) passes on the same commit.
- [ ] The documentation check is rerun (`python3 tools/docs_check.py`, also in CI), and the numbers it cannot count are
  measured again on the day (benchmark times, sizes, compile times).
- [ ] Someone who has never seen the project follows the README cold and reports where they got stuck.
- [ ] `development` is merged into `main` and tagged, with the owner's approval.
- [ ] The launch notes say what AIPL is, what is unfinished (the macOS and Windows preview, the thesis not yet
  measured), and how it was built, before anyone has to ask.

## After launch (deferred on purpose)

None of these blocks launch; the launch notes say which are coming.

- **Measuring the thesis.** Run the A/B harness (tools/harness/README.md) on the final language and publish the tasks,
  prompts, transcripts, and numbers, including where AIPL loses (the record, section 5). Before it runs: add tasks where
  contracts, overflow, and error handling matter; count tokens and rounds as well as passes; give Python a guide as long
  as AIPL's, as a control; repeat each task, with several models. The syntax proposals (inferred `let` types, `(f x)`
  without `call`) wait for its numbers (docs/design/CANONICAL_PLAN.md step 7).
- **Packages and a registry** (ROADMAP.md item 4), once the language version exists.
- **The language server, the aipl.sh website and playground, and Homebrew, winget, and apt packages.**
- **Performance**: a register allocator, faster hashing of byte strings, big integers (ROADMAP.md item 15).
- **Concurrency beyond threads**: channels, and a safe way to share data (ROADMAP.md item 14).
- **The compiler's own debts**: codegen_opt.aipl's pattern matcher (the record, section 6), the self-hosted compiler's
  double parse, and exact trap positions inside format placeholders.
- **Later ideas** (ROADMAP.md): a 64-bit target, AI-SQL, secret types.

## After launch: removing Rust entirely

Rust does six jobs today, and each needs its replacement before its code goes
(docs/design/aipl_standalone.md is the plan so far). The largest is the test suite, which is Rust too: 581 tests in
about 22,000 lines of tests/*.rs.

| Rust today | What it does | What replaces it |
|---|---|---|
| The reference toolchain (src/) | The referee: byte parity, and fuzzing against the self-hosted compiler | The conformance suite (section 2), the backends tested against each other, the bootstrap's fixed point |
| The VM (src/vm.rs) | `eval -f`, `test --vm`, `--all-backends`, the AIPL suite in CI, one side of the differential tests | A VM in AIPL (aipl_standalone.md S4), or native compared with wasm alone |
| `aipl-run` (wasmtime) | Running `.wasm` off Linux; the macOS and Windows launcher bundles | Mach-O and PE backends (ROADMAP.md item 6), then a native wasm runner off Linux (S3) |
| `aipl-net` (rustls and the system's resolver) | Host names and TLS in native executables | A resolver in AIPL; for TLS, a decision: the system's library through a way to call foreign code, or an audited implementation (ROADMAP.md item 13) |
| The Rust-only modes of the CLI | `compile --target wasm`, `--self`, `eval --wasm` | The same modes in aipl_src/ |
| The test harness (tests/*.rs) | 581 tests, many of them programs inside Rust strings | `(test ...)` items, and a runner in AIPL or a script |

In order, each step unblocking the next:

- [ ] Write the conformance suite and run it against both toolchains, so the referee's verdict has a home outside Rust.
- [ ] Port the tests in tests/*.rs, the programs inside Rust strings included (AGENT_FRICTION.md row 59).
- [ ] Port the Rust-only modes of the CLI, and decide what becomes of the VM.
- [ ] Write the Mach-O and PE backends, then ship the self-hosted `aipl` on macOS and Windows (ROADMAP.md item 7).
- [ ] Replace `aipl-net`: the TLS decision, and the general way to call a component in another language
  (docs/design/NET_PLAN.md section 7).
- [ ] Record the last seed built by Rust and its hash; after it, seeds build only from seeds. Keep an older seed for
  diverse double-compiling, which takes over Rust's independent check against a compromised compiler.
- [ ] Delete Cargo.toml, src/, and the Rust tests.
