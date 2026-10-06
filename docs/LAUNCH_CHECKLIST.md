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
  bytes, or message). Still to do: a short fixed-seed run in the test suite.
- [ ] **Remaining fuzz differences** (3,000 cases, seed 11, after the fix:
  53, none an acceptance bug except the float case below; no crashes or
  hangs):
  - *Generics-pass messages* differ in wording and lack a position
    (`wrong number of type arguments for generic vec.Vec` against Rust's
    `15:17: generic 'vec.Vec' takes 1 type argument(s) (T), got 3`); Rust
    also checks generic headers on items that are not `fn`/`struct`.
    Bring `generics.aipl`'s errors to Rust's text and positions.
  - *An error inside a type argument* (`(vec.Vec 0palette.Color)`) is
    reported inside the standard library's template (`vec.aipl` 12:31)
    instead of where the user wrote it: the generics pass does not record
    where substituted arguments came from.
  - *Errors at a closing bracket* (`Unexpected token parsing expression:
    RParen`, `Expected type constructor, got RParen`) land a few columns
    early: closing brackets are not recorded in the origin maps (the AST
    keeps no position for them).
  - *A missing module* is `cannot find module: X` in AIPL and `Cannot
    resolve import 'X': no 'X.aipl' found in ...` in Rust. Pick one text.
  - *Float literals* the self-hosted compiler cannot convert exactly are
    compile error 973 (AIPL_SPEC.md 6.4) where Rust compiles them: a real
    gap between the toolchains. Exact decimal-to-double conversion in AIPL
    closes it.
- [ ] **Fuzz the VM and compiled programs too:** generated well-typed
  programs (not just mutated text) run in the VM, under `aipl-run`, and
  natively, compared as the differential test does. This is where a
  reader's "I wrote 20 lines and got a different answer natively" would
  come from.
- [ ] **`aipl serve` and `web/` are untested** (AIPL_SPEC.md 6.1 says so).
  Test them, or remove them before the post: a broken demo is worse than
  none.

## 2. When something goes wrong (crash locations and errors)

The pitch is that agents fix code from error messages, so these matter
more than usual.

- [ ] **Compiled traps name nothing.** `./prog: wasm trap: integer divide by
  zero` gives no function or line. Emit a wasm name section (function
  names) in both compilers; have `aipl-run` and the native trap routine
  print the function, ideally a short stack of them.
- [ ] **Source positions at run time.** Map wasm code offsets back to source
  lines (a small line table, like DWARF's but simpler) so a trap and a
  failed compiled contract say `file:line:col`. The VM's runtime errors
  other than contracts need positions too.
- [ ] **Types in AIPL syntax.** Messages say `Ptr(Struct("Point"))` and
  `Array(I32)`; print `(ptr Point)` and `(arr i32)`. Both checkers, word
  for word.
- [ ] **More than one error per run.** The checkers stop at the first; an
  agent fixing code wants all of them (or at least one per function).
- [ ] **Type errors name the file** in the Rust toolchain (`aiplc` already
  does).

## 3. Claims (every sentence must survive being checked)

- [ ] **`Cargo.toml`:** `authors = ["AI Swarm Team <ai@antigravity.internal>"]`
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
- [ ] **CI.** A GitHub Actions workflow running `cargo build`, `cargo test`,
  and `aipl test aipl_src/test_suite.aipl` on Linux, with the badge in the
  README. The native tests need Linux x86-64, which GitHub's runners are.
- [x] **Repository root** (2026-10-06): `attic/` and the stray `out.wasm`
  are gone; the root holds README, LICENSE, the spec, the prompt guide,
  ROADMAP.md, DEVELOPING.md, and the build files.
- [ ] **`cargo clippy` is clean** (25 style warnings today; a reader running
  it first should see none).
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
- [ ] `cargo test`, the AIPL suite, the fuzzer (one hour), and the
  benchmarks run clean on a fresh clone, from the README's instructions
  alone.
- [ ] The documentation check is rerun; the numbers are today's.
- [ ] Someone who has never seen the project follows the README cold and
  reports where they got stuck.
- [ ] The post says what AIPL is, what is unfinished, and how it was built,
  before anyone has to ask.
