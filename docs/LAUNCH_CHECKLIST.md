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
- [x] **`aipl serve` and `web/` are untested** (removed 2026-10-07: the
  HTTP server for agents and the browser demo, with the `tiny_http` and
  `serde_json` dependencies; agents use the CLI).

- [x] **Views are not checked** (found and fixed 2026-10-19, MEMORY_PLAN.md M1; tests/test_memory.rs). A
  `str.Bytes` is an address and a length with no generation, so a
  view of a freed buffer reads whatever reuses the memory, in code with
  no `unsafe` (a three-line program prints the next buffer's contents,
  in the VM and natively). The fix, with the never-freed allocations
  that keep long-running programs from being possible:
  docs/design/MEMORY_PLAN.md (M1, checked views as value structs; then
  `(no_leak)`). The spec no longer claims `buf.free` ends views.

## 2. When something goes wrong (crash locations and errors)

The pitch is that agents fix code from error messages, so these matter
more than usual.

- [x] **Compiled traps name their functions** (2026-10-06). Both compilers
  emit a wasm name section; `aipl-run` and native executables print the
  call chain after the reason (`  at math.div`, `  at main`; at most 32,
  then `  ... N more`), the same lines byte for byte, for traps, failed
  contracts, and bounds checks, in threads too. Native code walks its
  frame pointers through a table of function addresses and names.
- [x] **Source positions at run time** (2026-10-07). Both compilers emit a
  line table (`aipl.lines`, docs/design/LINES_PLAN.md): the position of
  every call and trapping instruction, byte for byte the same. `aipl-run`
  and native executables print it in the call chain,
  `  at math.div (math.aipl:4:5)`, identically. Eight planted bugs
  (compilers, native lowering and lookup, aipl-run) were each caught; the
  table adds about a fifth to a module's size.
- [x] **The VM's runtime errors need positions too** (2026-10-08): `aipl
  eval` and `aipl test` print the same call chain as compiled code, frame
  for frame; the program fuzzer now requires the VM's chain to equal
  compiled code's (a planted bug: 101 of 1,000 cases caught).
- [x] **Types in AIPL syntax** (2026-10-06). Messages said
  `Ptr(Struct("Point"))`, `Array(I32)`, and `Add on ...`; both checkers and
  the compiler's own errors now write `(ptr Point)`, `(arr i32)`, `+`.
- [x] **More than one error per run** (2026-10-06). Both checkers report
  each failing function's first error, one per line; a syntax error or a
  definition's error is still reported alone.
- [x] **Type errors name the file** in the Rust toolchain (2026-10-06), as
  `aiplc` already did. The CLI also prints `Error: MESSAGE` plainly instead
  of in Rust's debug quoting.
- [x] **`when` around a value** (fixed 2026-10-25, CANONICAL_PLAN.md step 4: `when` is a form of its own, and its body's expressions are statements, a value dropped; tests/test_control_flow.rs `a_whens_body_drops_its_value`) (noted 2026-10-18, from
  docs/notes/painpoints.md item 5). `(when c (call f x))` with `f`
  returning `i32` fails with "If branch type mismatch: then is i32, else
  is void", naming an `if` the program does not have (the `when` rewrite
  makes one). `when` has no else, so it cannot give a value: it should
  discard its body's value, as a statement does, in both toolchains.
  Until then a branch that discards a value needs `(let _x:i32 ...)`
  (37 of them in std). A bare call in statement position already
  drops its value correctly in every backend (checked 2026-10-18; the
  note's "stack mismatches in native code" no longer happen).

## 3. Claims (every sentence must survive being checked)

- [ ] **Every claim in docs/notes has a checked verdict** (noted
  2026-10-18). The notes (assessment, final_thoughts, freedom,
  kernel_notes, painpoints, pitfalls, speed-ups, arm64_build; the .txt
  originals hold more than the .md versions) are an outside model's
  review. Readers will point their own models at the repository, so each
  claim gets a verdict with evidence (a test, a command, file:line): true
  and fixed, true and open (with an item here), wrong (and what shows
  it), or by design (and why), all in one document: docs/NOTES_REVIEW.md
  (2026-10-19, all eight notes). Its open items still to do: `str.Bytes` as a
  value struct (before L7); `io.read_file`, `os.arg`, `os.env` returning
  len -1 instead of an Option (after the seed refresh); match binders
  scoped to their arm; "Unknown op/keyword: =" suggesting `eq`; the
  double parse in the self-hosted compiler (a large refactor, for after
  launch). (codegen_opt type-checking its output: done 2026-10-25,
  docs/design/OPT_PLAN.md step 1.)
- [x] **`Cargo.toml`** (fixed 2026-10-06): `authors = ["AI Swarm Team <ai@antigravity.internal>"]`
  must go (use your name); the comment "Only the aipl-run launcher uses
  wasmtime; the compiler does not" is false (the CLI runs the native
  backend in wasmtime); the description says the toolchain is "written in
  AIPL itself" without the bootstrap caveat.
- [x] **"Memory-safe" only with its limits** (2026-10-16, after
  generation checks; before that, 2026-10-15: never say it). README, FAQ,
  AIPL_SPEC.md 3, and ROADMAP.md say "memory-safe outside `unsafe`" and
  then the limits in full: no data-race checking (threads are unsafe;
  `std/par` is the safe way to run work in parallel);
  after an arena reuses its memory a stale pointer is caught unless the
  new data at its generation word happens to equal its old generation; a
  fast arena catches it only once its memory is reused; `ptr.cast`
  pointers are unchecked; the compilers and the standard library's unsafe
  code are trusted.
- [x] **"Self-hosted" always with the caveat** (checked 2026-10-15) that
  the first build takes the Rust toolchain, and so does writing an
  executable (`--exe`), until the stage-0 seed and a host-provided way to
  write executables (ROADMAP.md, "Next" 4) exist: README, FAQ, Cargo.toml,
  AIPL_SPEC.md's opening, and ROADMAP.md say so.
- [x] **The README disclaimer** (replaced 2026-10-07) ("generated by AI models ... no guarantees")
  reads as a hedge. Replace it with a plain statement of how the project
  is made (below) and the usual license disclaimer.
- [x] **Say how it was built, up front** (README, "How it is made and
  tested", 2026-10-07). Commits carry AI co-author lines
  and the docs are long; readers will notice, and some subreddits have
  rules on AI-generated content (check each one's rules before posting).
  State it in the README: designed by you, implemented with AI coding
  agents, checked by the test suite described there. Point at the
  evidence: VM-against-wasmtime differential testing, byte-for-byte parity
  between two independent compilers, the compiler rebuilding itself,
  mutation-checked tests, the fuzzing run.
- [x] **Benchmarks framed honestly** (done 2026-10-15; numbers updated
  2026-10-16 after generation checks): README, FAQ, and docs/BENCHMARKS.md
  give native code as 1.2-17x C's time (wasm under wasmtime 1.0-16x), name
  knucleotide's packed keys and pidigits' GMP as why those two are
  slowest, say binarytrees is not like for like (an arena against malloc
  and free), and give the generation checks' cost.
- [x] **Re-run the documentation check** (2026-10-15; section 8 repeats it
  on the day of posting): every number in the README (sizes, test counts,
  timings) must be current. Corrected: test counts (340), `word_count`'s
  size (64 KB), the self-compile time (0.57 s), the README's lists (value
  structs, JSON, installation done), the spec's test-file list, ROADMAP's
  visibility and native-speed lines, and the suite's expected output.

## 4. The first five minutes

- [x] **README rewrite** (2026-10-07; the layout table moved to
  DEVELOPING.md) for a newcomer, in this order: what AIPL is in two
  sentences; why it exists (the agent argument, with the evidence from
  section 5); one example program and its output; install; the three
  commands to try; honest status (what works, what does not, a link to
  ROADMAP.md); how it is built and tested. Move the repository
  layout table to a contributor doc.
- [x] **An FAQ** (docs/FAQ.md, 2026-10-08) answering the predictable first comments: why
  S-expressions; why so verbose (`(call (vec.push i32) v x)`); why not
  just use Rust, Zig, or Python; why WebAssembly; is it really
  self-hosted; was it written by AI; is it memory-safe; how fast is it.
- [x] **Install in one command** (2026-10-15, `install.sh`): the rustup
  style `curl ... | sh`, from the repository so it can be read first. It
  builds from source into `~/.aipl` (git, Rust, and a C linker needed; it
  installs none of them) and adds `~/.aipl/env` to the shell profiles;
  `--uninstall` undoes it; CI runs shellcheck and a full install. Not
  `cargo install --git`: its binaries point at a cargo checkout that cargo
  deletes after about a month unused. Later, optionally: prebuilt `aipl`
  and `aipl-run` on a GitHub release, so Rust is not needed.
- [x] **A binary that works wherever it is** (done 2026-10-26; noted
  2026-10-18, from docs/notes/freedom.md). The Rust `aipl` carries its
  library (aipl_src/: the standard library and the toolchain's sources,
  built in by build.rs) and, outside its own checkout's target/, writes it
  once to its cache and reads it from there (src/library.rs); `$AIPL_STD`
  now replaces std for programs in the Rust command too. The self-hosted
  `aipl` looks in `$AIPL_STD`, then `share/aipl/std/` beside its folder,
  then `./aipl_src/std/` (aipl_src/stdpath.aipl). tests/test_portable.rs
  copies `aipl` out of the repository and runs every command; the kit
  (tools/make_kit.sh) is the real binaries, tested with the build's source
  hidden. Prebuilt release binaries are now possible. Later: the
  self-hosted binary could embed the library too and become one file; it
  still needs `chmod +x` (WASI cannot set file modes) and has no `verify`
  or `test`.
- [x] **CI** (added 2026-10-06, `.github/workflows/ci.yml`; check its first run on GitHub). A GitHub Actions workflow running `cargo build`, `cargo test`,
  and `aipl test aipl_src/test_suite.aipl` on Linux, with the badge in the
  README. The native tests need Linux x86-64, which GitHub's runners are.
- [x] **Repository root** (2026-10-06): `attic/` and the stray `out.wasm`
  are gone; the root holds README, LICENSE, the spec, the prompt guide,
  ROADMAP.md, DEVELOPING.md, and the build files.
- [x] **`cargo clippy` is clean** (2026-10-06, all targets; CI fails on any
  new warning).
- [ ] **A showcase.** One program a reader would want to run: the compiler
  compiling itself natively in 0.57 s is a good one; a small, real tool
  (a JSON pretty-printer, a `wc`/`grep` clone, a Markdown-to-HTML
  converter) would be better, written the way an agent would write it.
- [x] **A ten-minute tour** (docs/TOUR.md, 2026-10-08; its examples are run by
  tests/test_doc_examples.rs) for humans: the spec is 1,350 lines and the
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

- [x] **A lower memory limit** (decided 2026-10-19, done 2026-10-20: tests/test_memory.rs, test_bootstrap.rs): `aipl compile
  --max-memory 512M` (and `--exe`) records the limit in the module's
  memory declaration, its max pages, so the VM, aipl-run, native
  executables, and any wasm host stop at the same place, with the
  message they give at the 2 GiB cap today; the default stays 2 GiB.
  Only lowering: below 2 GiB every address is a non-negative i32, which
  the signed address comparisons in std and the compiler rely on. Wanted
  with the servers (MEMORY_PLAN.md M5): a runaway request fails cleanly
  instead of taking the machine.
- [ ] **"Out of memory" said as such**: at the limit (2 GiB, or
  `--max-memory`'s) `mem.alloc` does not fail, so the program stops at its
  first access past the end, with "Memory store out of bounds" (VM) or
  "wasm trap: out of bounds memory access" (compiled) and a call chain into
  std/heap. A failed grow should stop at `mem.alloc` with "out of memory:
  the limit is N MiB", alike in every backend and both toolchains.
- [ ] **An address-sized integer, decided on purpose** (before launch):
  `ptr.addr`, `mem.*`, and `mem.alloc` use `i32` addresses, a promise to
  users' (unsafe) code. A 64-bit target (ROADMAP.md, after launch) wants
  an address-sized type such as Rust's `usize`; adding it later breaks
  only unsafe code. Decide whether to add it now or say in the FAQ why
  not.

- [x] **Option types / generic unions** (done 2026-10-12): generic unions
  (`(union (Tree T) ...)`), `std/option`'s `(option.Option T)`, and
  `result` payloads of any type (the 32-bit limit is gone; `result` stays
  its own form). AIPL_SPEC.md 4.F and 4.J.
- [x] **Interfaces or constraints on generics** (done 2026-10-14): interfaces
  with one implementation per type, explicit calls, `where` clauses, and
  templates checked once with sealed type parameters; `std/cmp`, and one
  `(map.Map K V)` for any hashable key. AIPL_SPEC.md 4.H,
  docs/design/CONSTRAINTS_PLAN.md.
- [x] **Closures** (decided 2026-10-14): none, by design; a callback's data
  is a typed context passed as its first argument (`vec.sort_by_with`,
  `thread.spawn_with`, a typed `alloc.custom` state). AIPL_SPEC.md 4.G, FAQ.
- [x] **Visibility** (done 2026-10-14): private by default; `(pub)` items;
  fields private, `(read x:T)`, or `(pub x:T)`; a struct made outside its
  module only if every field is `(pub)`. AIPL_SPEC.md 11.1.
- [x] **Standard library breadth** (done 2026-10-14): `std/json` (strict
  parsing with error offsets; writing as `JSON.stringify` does, numbers in
  the shortest digits that read back) and `std/fs` (sorted directory
  listings, file kind, size, and modification time) over three new WASI
  ops. Starting processes is left out, as WASI preview 1 cannot; networking
  later. AIPL_SPEC.md 4.C and 12.6, ROADMAP.md.
- [x] **Language gaps** (done 2026-10-16): `f32` literals (`1.5f32`) and
  conversions, trapping at wasm's bounds natively too; `u8 i8 u16 i16`
  struct fields and array elements, read as `i32`; constants computed from
  other constants; `inv` checked at entry and exit; `str.concat`,
  `str.same`, `str.from_bytes`, `buf.to_str`, and a guide to the three
  kinds of text (`eq` on `str` is now an error: it would compare addresses
  compiled and contents in the VM); `std/date` (UTC, ISO 8601);
  `par.reduce`; generic value types as plain type arguments. Each in every
  backend and both toolchains, with the same bytes and messages.
  AIPL_SPEC.md 3, 4.A, 4.B, 4.I, 12.6; tests/test_language_gaps.rs.
- [x] **Faster native code** (done 2026-10-14,
  docs/design/NATIVE_SPEED_PLAN.md): a single-pass translator that tracks
  where each value is, guard pages, locals and arguments in registers,
  failed checks out of line. Native code went from 1.6-5.7x wasmtime's
  time to 0.8-1.6x on the eight benchmarks (target: within 2x);
  docs/BENCHMARKS.md.
- [x] **Structs by value and arrays of structs** (done 2026-10-15,
  docs/design/VALUES_PLAN.md V1-V4): declared, immutable value structs of
  at most 8 scalars, held inline in arrays and structs, in every backend;
  nbody with an `(arr Body)` runs 1.9x faster under wasmtime.
  AIPL_SPEC.md 4.E.
- [x] **Unions and `Option` by value** (done 2026-10-15, VALUES_PLAN.md V5):
  a union that does not contain itself, holds no `str`, and fits in 8
  slots allocates nothing; `map.find` returns an `Option`. AIPL_SPEC.md 4.J.
- [x] **Results by value** (done 2026-10-15, VALUES_PLAN.md V5): every
  `ok`/`err` names its other side, `(ok:E v)`; a result without a `str`
  side that fits in 8 slots allocates nothing. AIPL_SPEC.md 4.F.
- [ ] **The self-hosted compiler's fixed tables** (found 2026-10-19). It
  refuses, as numbered compile errors, programs the Rust compiler
  accepts: more than 16 parameters (92), 255 structs, unions, or
  variants, 64 fields, or 31 `call_ref` signatures (94), 1,024 locals
  (93), 8,192 functions (92); each limit is in AIPL_SPEC.md 6.4, and each
  checked 2026-10-19 with a program just past it. The tables should grow
  as wasm.rs's vectors do (as the label stack now does: at 255 nested
  blocks it crashed instead of giving error 100), and a refusal should
  say what it refuses, not "compile error 94 (AIPL_SPEC.md 6.4)". Until
  then the two compilers agree only below those sizes.
- [x] **Using a module without importing it** (decided and done
  2026-10-26, docs/design/LANGUAGE_DECISIONS.md item 1). `m.x` needs
  `(import m)` in that file, checked by both resolvers with one message
  (`'str.Bytes' is in module str, which this file does not import: add
  (import str)`); what the compiler writes for `new`, the formatting
  forms, and a test build is exempt. One repository file needed a line
  (tests/aipl/memory/request_paths.aipl). AIPL_SPEC.md 11,
  tests/test_visibility.rs.
- [ ] **Making a heap struct in one form** (noted 2026-10-18, from
  docs/notes/painpoints.md item 4; a language change, for approval).
  Today it is `(new S)` and a `put` per field (std/regex's Regex: 20), and
  nothing notices a field left out: a new field means finding every place
  that makes one. Proposed: `(new S v1 v2 ...)` with every field, in
  order, as `make` already does for value structs, so a missing or extra
  field is a compile error. The compiler's own source can use it only
  after the seed refresh below.
- [x] **Refresh the stage-0 seed** (done 2026-10-19, for MEMORY_PLAN.md's checked views, which need `(arr u8)`; bootstrap/README.md has the new hash; noted 2026-10-18).
  The compiler's source may use only what bootstrap/aipl-linux-x86_64
  knows, and that seed (2026-10-16) predates `when`, `for`, variadic
  `and`/`or`, `dbg`, `(test ...)`/`expect`, and the formatting forms, so
  the compiler (codegen_opt.aipl included) is still written without them
  (painpoints.md item 2). A refresh rebuilds the seed with the Rust
  toolchain from a chosen commit and records its sha256 in
  bootstrap/README.md; test_bootstrap then shows the new seed is the Rust
  build of that source. Best done once the language settles before
  launch, so it happens once.
- [ ] **codegen_opt.aipl's own friction** (noted 2026-10-18, from
  docs/notes/painpoints.md items 1 and 6; the optimizer is on by default
  since 2026-10-25): matching tree shapes like `(/ (* e (+ e 1)) 2)`
  takes chains of `onode_count`/`onode_nth`/`is_form`/`node_eq`; a
  matcher over ONode taking a pattern with placeholders would replace
  them, as a library in that file, not a language change. Item 6's
  truncation of large literals: checked 2026-10-25, none (both types'
  extremes, folded and not, print alike optimized, unoptimized, and in
  the VM; tests/aipl/opt/literals.aipl, compared in CI by
  tools/opt_diff.py).

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
