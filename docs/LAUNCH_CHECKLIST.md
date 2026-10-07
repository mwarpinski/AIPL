# Launch checklist: before showing AIPL publicly

What a skeptical reader (r/ProgrammingLanguages, r/programming, Hacker
News) would find in the first hour, and what to do about each. Written
2026-10-06 from a hostile-reviewer pass over the repository, a fuzzing run,
and the documentation check of the same day. Work top to bottom; tick items
as they land, with the commit or date.

The bar: nothing a reader can find in five minutes is embarrassing, every
claim survives being checked, and the pitch ("a language for AI agents")
has evidence behind it.

---

## 1. Correctness and robustness (blocking)

- [x] **The self-hosted compiler accepted malformed files** (fixed
  2026-10-06). A fuzzing run found 97 of 1,500 mutated files that `aipl
  verify` rejects but `aiplc` compiled, exiting 0, often to an empty
  module: invalid UTF-8, unclosed or mismatched brackets, input after the
  module, bad imports, tokenizer errors, and unknown items (which it
  dropped). `resolver.aipl` now checks file structure in Rust's order with
  Rust's messages (`str.valid_utf8` is new in `std/str`), passes unknown
  items on to the parser, and notes group heads so errors there point at
  the right column; `generics.aipl` keeps a stray atom an atom.
  `tests/test_resolver_aipl.rs` (`malformed_files_are_rejected_like_rust`,
  22 cases) checks message and file against Rust; with the check disabled
  it fails.
- [x] **The fuzzer is a tool:** `tools/fuzz.py` (both toolchains on mutated
  repository programs; crashes, hangs, and any disagreement in acceptance,
  bytes, or message). CI runs 3,000 cases with seed 1.
- [x] **Message and position differences** (fixed 2026-10-06). After the
  malformed-file fix, 53 of 3,000 fuzz cases still differed; now none do
  apart from float literals (below), on three seeds of 3,000:
  - the generics pass reports Rust's messages at Rust's positions in the
    right file (`file: L:C: generic 'vec.Vec' takes 1 type argument(s)
    (T), got 3`), in Rust's order, and checks generic headers on any item;
  - a substituted type argument keeps its own position, so an error in it
    is reported where the user wrote it. This also fixed the *Rust*
    toolchain, which named the template's file with the user's line and
    column;
  - closing brackets are recorded in the position maps (the parse tree now
    keeps where each `)` is), so errors there land on the right column;
  - a missing module, a circular import, a bad import path, and two
    modules with one name have Rust's wording, after the importing file.
  `tests/test_resolver_aipl.rs` (`malformed_files_are_rejected_like_rust`,
  now 32 cases) holds all of it.
- [x] **Float literals** (2026-10-06). The self-hosted compiler rejected
  literals it could not convert exactly (compile error 973), about 1 fuzz
  case in 700. `std/float.from_decimal` now rounds every literal as Rust's
  parser does (exact big-integer arithmetic where the fast path is not
  exact); 400,000 literals agree bit for bit, and four planted rounding
  bugs were each caught. With them out of the way the fuzzer found five
  more differences, all fixed: after a definition's error checker.aipl
  went on to report body errors; tokens in messages were Rust's debug
  form (`FloatLit(6.9e-5)`, `RParen`), now both toolchains write them as
  the source does (`'6.9e-5'`, `')'`); the constants pass stopped at a
  stray atom among the items; with generics in the program, unions and
  enums were ordered after structs; and a missing module imported by path
  named only its last segment. 50,000 fuzz cases on five seeds agree.
- [x] **Fuzz the VM and compiled programs too** (2026-10-06):
  `tools/run_fuzz.py` generates well-typed, terminating programs (every
  scalar op, structs, arrays, enums, unions, results, references,
  recursion, contracts, raw memory, heap addresses) and compares the VM,
  `aipl-run`, and native executables; CI runs 500. It found three VM bugs,
  all a jump inside an operand: `(return (block (return 1) 2))` returned
  nothing, a `return` in a call's argument was taken by the callee, and a
  `break` in a `set!`'s value assigned `Void` (regression tests in
  `test_control_flow.rs`). Compiled code was right each time. After the
  fixes, 25,000 cases (seeds 5 and 8) agree; four planted bugs (VM, wasm backend, native backend) were each
  caught within 600.
- [x] **Running out of stack** (2026-10-06; found by the program fuzzer).
  Deep recursion stopped `aipl-run` cleanly but killed a native executable
  with a segfault (exit 139, no message), and the VM aborted on its own
  stack overflow. Now native function prologues check a per-thread stack
  limit and trap with aipl-run's message and exit status; every native
  stack (the main thread's too, so `ulimit -s` does not matter) is 8 MiB
  with a guard page; the VM fails a call past 192 MiB of stack with an
  error naming the function. Limits in AIPL_SPEC.md 7.11.
- [ ] **`aipl serve` and `web/` are untested** (AIPL_SPEC.md 6.1 says so).
  Test them, or remove them before the post: a broken demo is worse than
  none.

## 2. When something goes wrong (crash locations and errors)

The pitch is that agents fix code from error messages, so these matter
more than usual.

- [x] **Compiled traps name their functions** (2026-10-06). Both compilers
  emit a wasm name section; `aipl-run` and native executables print the
  call chain after the reason (`  at math.div`, `  at main`; at most 32,
  then `  ... N more`), the same lines byte for byte, for traps, failed
  contracts, and bounds checks, in threads too. Native code walks its
  frame pointers through a table of function addresses and names.
- [ ] **Source positions at run time.** Map wasm code offsets back to source
  lines (a small line table, like DWARF's but simpler) so a trap and a
  failed compiled contract say `file:line:col`. The VM's runtime errors
  other than contracts need positions too.
- [x] **Types in AIPL syntax** (2026-10-06). Messages said
  `Ptr(Struct("Point"))`, `Array(I32)`, and `Add on ...`; both checkers and
  the compiler's own errors now write `(ptr Point)`, `(arr i32)`, `+`.
- [x] **More than one error per run** (2026-10-06). Both checkers report
  each failing function's first error, one per line; a syntax error or a
  definition's error is still reported alone.
- [x] **Type errors name the file** in the Rust toolchain (2026-10-06), as
  `aiplc` already did. The CLI also prints `Error: MESSAGE` plainly instead
  of in Rust's debug quoting.

## 3. Claims (every sentence must survive being checked)

- [x] **`Cargo.toml`** (fixed 2026-10-06): `authors = ["AI Swarm Team <ai@antigravity.internal>"]`
  must go (use your name); the comment "Only the aipl-run launcher uses
  wasmtime; the compiler does not" is false (the CLI runs the native
  backend in wasmtime); the description says the toolchain is "written in
  AIPL itself" without the bootstrap caveat.
- [ ] **Never say "memory-safe".** It is not: `mem.*` on computed addresses,
  `ptr.cast`, and reads of freed heap memory are unchecked. Say what is
  true: bounds-checked arrays, typed pointers, contracts, a guarded
  runtime block, checked frees.
- [ ] **"Self-hosted" always with the caveat** that the first build takes
  the Rust toolchain, until the stage-0 seed (ROADMAP.md, "Next" 4) exists.
- [ ] **The README disclaimer** ("generated by AI models ... no guarantees")
  reads as a hedge. Replace it with a plain statement of how the project
  is made (below) and the usual license disclaimer.
- [ ] **Say how it was built, up front.** Commits carry AI co-author lines
  and the docs are long; readers will notice, and some subreddits have
  rules on AI-generated content (check each one's rules before posting).
  State it in the README: designed by you, implemented with AI coding
  agents, checked by the test suite described there. Point at the
  evidence: VM-against-wasmtime differential testing, byte-for-byte parity
  between two independent compilers, the compiler rebuilding itself,
  mutation-checked tests, the fuzzing run.
- [ ] **Benchmarks framed honestly** (docs/BENCHMARKS.md already is): native
  code takes 1.1-40x C's time depending on the benchmark, wasm under
  wasmtime 0.6-14x; pidigits is
  against GMP; binarytrees uses an arena where C uses malloc. Lead with
  what is true, never with the one benchmark AIPL wins.
- [ ] **Re-run the documentation check** (as on 2026-10-06) right before
  posting; every number in the README (sizes, test counts, timings) must
  be current.

## 4. The first five minutes

- [ ] **README rewrite** for a newcomer, in this order: what AIPL is in two
  sentences; why it exists (the agent argument, with the evidence from
  section 5); one example program and its output; install; the three
  commands to try; honest status (what works, what does not, a link to
  ROADMAP.md); how it is built and tested. Move the repository
  layout table to a contributor doc.
- [ ] **An FAQ** answering the predictable first comments: why
  S-expressions; why so verbose (`(call (vec.push i32) v x)`); why not
  just use Rust, Zig, or Python; why WebAssembly; is it really
  self-hosted; was it written by AI; is it memory-safe; how fast is it.
- [ ] **Install in two commands.** `cargo install --git ...` at least;
  better, prebuilt `aipl` and `aipl-run` binaries on a GitHub release for
  Linux x86-64 (and macOS/Windows through the launcher).
- [x] **CI** (added 2026-10-06, `.github/workflows/ci.yml`; check its first run on GitHub). A GitHub Actions workflow running `cargo build`, `cargo test`,
  and `aipl test aipl_src/test_suite.aipl` on Linux, with the badge in the
  README. The native tests need Linux x86-64, which GitHub's runners are.
- [x] **Repository root** (2026-10-06): `attic/` and the stray `out.wasm`
  are gone; the root holds README, LICENSE, the spec, the prompt guide,
  ROADMAP.md, DEVELOPING.md, and the build files.
- [x] **`cargo clippy` is clean** (2026-10-06, all targets; CI fails on any
  new warning).
- [ ] **A showcase.** One program a reader would want to run: the compiler
  compiling itself natively in 0.37 s is a good one; a small, real tool
  (a JSON pretty-printer, a `wc`/`grep` clone, a Markdown-to-HTML
  converter) would be better, written the way an agent would write it.
- [ ] **A ten-minute tour** for humans: the spec is 1,350 lines and the
  prompt guide is written for models. One page that walks through a
  program, the types, contracts, and compiling it.

## 5. Evidence for the pitch (the post's real content)

"A language designed for AI agents" invites "prove it". The strongest post
is a small, honest experiment:

- [ ] **Task set:** 20-30 small programs with tests (parse a format,
  aggregate a file, a data structure, a small simulation), each specified
  in plain language.
- [ ] **Conditions:** the same model writes each task in AIPL (with
  PROMPT_GUIDE_FOR_AIS.md as its only documentation) and in Python and
  Rust; several attempts each; the model may read compiler errors and
  retry, up to a fixed number of rounds.
- [ ] **Measures:** compiles on the first try; passes the tests on the
  first try; rounds to pass; how many bugs were caught by the checker or a
  contract rather than by a wrong answer; length of the program.
- [ ] **Publish everything:** the tasks, prompts, transcripts, and scripts,
  in the repository, so anyone can rerun it.
- [ ] **Report it straight**, including where AIPL loses. If it loses
  everywhere, that is worth knowing before the post, not in the comments.

## 6. Language questions readers will ask (answer, or say when)

Not all need building before a post; each needs an honest answer in the
FAQ or ROADMAP.md.

- [ ] **Option types / generic unions** (`(Option T)`, a real `Result`): the
  most visible missing feature for a statically typed language.
- [ ] **Interfaces or constraints on generics**, so `map` takes any key and
  a template is checked once.
- [ ] **Closures** (or a clear statement that function references plus an
  argument are the design).
- [ ] **Visibility** (`pub`), so a module's helpers are private.
- [ ] **Standard library breadth:** a data format (JSON), directory listing
  and file information, running processes; networking later.
- [ ] **Structs by value and arrays of structs.**
- [ ] **Faster native code** (docs/BENCHMARKS.md, "Locals in registers": a
  rewrite of the instruction translation is the real gain).

## 7. Documentation: delete, merge, keep

- [x] **Done 2026-10-06.** The repository had 3,800 lines of Markdown, and
  a reader saw sprawl and self-criticism before the language. Now:

| Was | Now |
|---|---|
| `attic/` ("Quarantined Fabricated Modules"), `docs/gemini-audit.md`, `docs/NATIVE_TARGET.md` | Deleted (git keeps them); the native note's reasoning opens `docs/design/NATIVE_BACKEND_PLAN.md` |
| `AIPL_Structural_Audit.md` at the root | `docs/history/AUDIT_2026-10.md`, with an archive note; its open findings moved to ROADMAP.md |
| `PROGRESS.md` | `DEVELOPING.md` (environment, verifying, conventions, direction, decisions, lessons) and `docs/history/WORK_LOG.md` (the dated record) |
| `LANGUAGE_GAPS.md` and PROGRESS.md's next steps | `ROADMAP.md`: done, next in order, and everything AIPL does not do yet |
| The five plans in `docs/` | `docs/design/`, each with its status at the top |
| `docs/BENCHMARKS.md`, the spec, the prompt guide | Kept |

- [ ] **Later:** split the spec into a reference and a ten-minute tour
  (section 4); rewrite the README for newcomers (section 4).

## 8. Right before posting

- [ ] Every box above is ticked or has a stated reason.
- [ ] `cargo test`, the AIPL suite, both fuzzers (an hour each), and the
  benchmarks run clean on a fresh clone, from the README's instructions
  alone.
- [ ] The documentation check is rerun; the numbers are today's.
- [ ] Someone who has never seen the project follows the README cold and
  reports where they got stuck.
- [ ] The post says what AIPL is, what is unfinished, and how it was built,
  before anyone has to ask.
