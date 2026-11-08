# Developing AIPL

How to build, test, and change AIPL, and the decisions and conventions the
work follows. [ROADMAP.md](ROADMAP.md) has what comes next and what AIPL
does not do yet; [AIPL_SPEC.md](AIPL_SPEC.md) is the language and
[docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) how these tools implement it; [docs/design/](docs/design/) holds the design records of the
larger pieces of work, [docs/history/](docs/history/) the dated work log, the
2026-10 audit, the handoffs and the Apple Silicon report, and [docs/notes/](docs/notes/)
outside reviews; [docs/README.md](docs/README.md) indexes them all.

## Repository layout

| Path | What it is |
|---|---|
| `referee/src/parser.rs`, `checker.rs`, `resolver.rs` | Rust bootstrap front end: S-expressions → AST, type checker, import flattening |
| `referee/src/vm.rs` | Reference interpreter (contracts, real threads and atomics, `std::fs` I/O) |
| `referee/src/compiler/wasm.rs` | Rust wasm backend, the byte-for-byte reference for the self-hosted one |
| `referee/src/generics.rs` | Generic template expansion (twin of `aipl_src/generics.aipl`) |
| `referee/src/bin/aipl_run.rs` | `aipl-run`, the launcher: wasmtime plus WASI, the native part of `aipl compile --exe` executables |
| `aipl_src/resolver.aipl`, `generics.aipl`, `compiler.aipl`, `codegen.aipl`, `driver.aipl` | Self-hosted import resolver, generics expansion, tokenizer and parser, wasm code generator, and the command that chains them |
| `aipl_src/native/` | The Linux x86-64 backend: `native` (wasm in, executable out), `wasm_reader`, `lower` (wasm to machine code), `x64` (encoder), `elf`, `runtime` (start-up, traps), `wasi` (the WASI functions as system calls) |
| `referee/src/native.rs` | Runs the AIPL native backend for `aipl compile --exe` |
| `aipl_src/std/` | Standard library: `io` (printing, whole-file read/write), `str` (byte slices, counting, `parse_int`, UTF-8 checking), `fmt` (number formatting, exact float printing), `vec` (growable list, stable sort), `map` (hash map over any `cmp.Hash` key), `cmp` (`Eq`, `Ord`, `Hash` interfaces), `option` (`(option.Option T)`), `thread` (`spawn_with`, a typed context), `buf` (string builder), `os` (command line, environment), `time` (timing), `heap` (general-purpose allocator with checked free), `arena` (region allocator), `alloc` (any allocator as one value), `bigint` (big integers), `float` (decimal text to `f64`), `fs` (directory listing, file information), `json` (parsing and writing JSON); found by `(import io)` from anywhere |
| `aipl_src/memory.aipl`, `file_io.aipl`, `thread_sync.aipl` | Small verified library modules |
| `aipl_src/test_suite.aipl` | AIPL-native test entry point |
| `examples/` | Tested example programs: `math_core` (contracts), `quicksort`, `matrix_mult` (structs, `f64` arrays), `accounts` (results), `word_count` and `word_freq` (files, collections, a sort comparator) |
| `benchmarks/` | Eight benchmark programs, each in AIPL, C, and Python with expected output ([docs/BENCHMARKS.md](docs/BENCHMARKS.md)) |
| `referee/` | The Rust crate, a member of the root Cargo workspace (`Cargo.toml`): the reference toolchain that judges, in development CI, whether the self-hosted one does what the spec says. `referee/src/` is the crate, `referee/tests/` its integration tests. Users never build it and the release never runs it (docs/RELEASING.md) |
| `tests/` | The language's own test data, shared by both toolchains: `tests/aipl/` (AIPL programs the Rust tests and the tools run), `tests/conformance/` (the conformance suite, spec section 6.4), `tests/arm64/` |
| `tools/` | Development helpers not used by the build or `cargo test`: `fuzz.py` (the two toolchains against each other on mutated programs), `run_fuzz.py` (generated programs in the VM, as wasm, and natively, against each other; CI runs it), `bench.py` (benchmark timing), `x64_vectors.py` (the encoder's test vectors, from GNU as), `bigint_vectors.py` (the bigint test's expected hash, from Python), `make_kit.sh` (a self-contained toolchain kit for another Linux machine or agent: `sh tools/make_kit.sh [COMMIT]` builds it in a private worktree, tests it with the source hidden, and writes `target/kit/aipl-kit-COMMIT.tar.gz`) |

## Environment

- Linux, Rust via `cargo`. Everything builds and tests with plain `cargo build` / `cargo test` from the repo root.
- No external wasm runtime is required. `wasmtime` and `wasmtime-wasi` are Rust dependencies (the `aipl-run` launcher embeds them, and the tests run compiled modules in-process). The `wasmtime` CLI is **not** installed on this machine. Any test that shells out to it would be vacuous, so none do.
- The native-backend tests run the executables AIPL writes, so they need Linux on x86-64. `readelf` is used when installed but not required. `tools/x64_vectors.py` (regenerating the encoder's test vectors) needs Python and GNU binutils, `tools/bigint_vectors.py` needs Python, and `tools/bench.py` needs Python, gcc, and GMP (for the pidigits C reference); the build and tests need none of them.
- `aipl compile --self` finds `aipl_src/` through the crate directory, so it works from anywhere. The examples that read `input.txt` resolve it against the working directory.

## How to verify everything

```bash
cargo test                                   # 551 tests; test_selfhost and test_resolver_aipl take a minute or two each (the self-hosted toolchain runs in the VM)
cargo run --bin aipl -- test aipl_src/test_suite.aipl   # AIPL-native suite, exit 0 = all groups pass
cargo run --bin aipl -- compile --self aipl_src/memory.aipl -o /tmp/m.wasm       # Rust vs self-hosted byte parity
```

Expected AIPL suite output (37 groups):
```
[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
[PASS] memory: allocator + arena
[PASS] file_io: real disk round-trip
[PASS] std/str: byte slices, parse_int, UTF-8, making strings (10 tests)
[PASS] std/fmt: number formatting (14 tests)
[PASS] resolver: imports in AIPL (3 tests)
[PASS] std/os: arguments + environment (3 tests)
[PASS] native/wasm_reader: module structure and bodies (5 tests)
[PASS] native/x64: instruction encodings vs GNU as (15 tests)
[PASS] native/elf: executable layout and headers (4 tests)
[PASS] std/io: file round trip (2 tests)
[PASS] std/vec: generic list + stable sort (8 tests)
[PASS] std/ip: addresses read as Rust reads them (5 tests)
[PASS] std/map: generic hash map over cmp.Hash keys (10 tests)
[PASS] std/buf: string builder (4 tests)
[PASS] std/time: durations and the clock (7 tests)
[PASS] std/date: UTC dates, ISO 8601 (8 tests)
[PASS] std/show: values as text, floats as Python's repr (7 tests)
[PASS] std/text: split, lines, replace, case, parse_f64 (7 tests)
[PASS] std/utf8: decode, encode, count, whitespace (4 tests)
[PASS] std/stream: readers and writers over files, memory, and functions (7 tests)
[PASS] std/arena: typed region allocator (7 tests)
[PASS] std/heap: general-purpose allocator (8 tests)
[PASS] std/alloc: any allocator as one value (9 tests)
[PASS] std/bigint: arbitrary-precision integers (13 tests)
[PASS] std/option: Option T, a generic union (7 tests)
[PASS] std/cmp: Eq, Ord, Hash for built-in types (10 tests)
[PASS] std/thread: spawn_with, a typed context (2 tests)
[PASS] std/par: parallel map and reduce, no unsafe (8 tests)
[PASS] std/fs: directory listing and file information (4 tests)
[PASS] std/json: parsing and writing JSON (6 tests)
[PASS] std/float: decimal text to the nearest f64 (14 tests)
[PASS] consts: constants and enums erased for codegen (5 tests)
[PASS] codegen_opt: the optimizer's folds, inlining, and refusals (7 tests)
[PASS] thread_sync: 4 threads x 1000 atomic adds = 4000
[AIPL Test] All groups passed.
```

`thread_sync.aipl` (real OS threads and atomics; this suite runs it in the VM, and `referee/tests/test_threads.rs` covers threads compiled to wasm) is part of the master suite since P10: `thread.spawn` takes a function reference, which the import resolver renames correctly.

Fuzzing the two toolchains against each other (crashes, hangs, and any
disagreement in what they accept, the bytes they produce, or their error
messages; failing cases are kept under `target/fuzz/`):

```bash
cargo build --release && python3 tools/fuzz.py --cases 3000
```

Fuzzing the three ways of running a program against each other: generated
programs, well-typed and terminating by construction, run in the VM, as wasm
under `aipl-run`, and natively, and must print the same output and fail the
same way (failing cases are kept under `target/run_fuzz/`; CI runs 500 with
seed 1, and `--show N` prints case N's program):

```bash
cargo build --release && python3 tools/run_fuzz.py --cases 5000 --seed 7
```

## Conventions

- **Branches.** Work happens on `features/<name>`, merged with `--no-ff`
  into `development`; `development` merges into `main` at milestones. Only
  `main` and `development` are long-lived.
- **AIPL first, Rust follows (the twin rule).** The self-hosted toolchain (`aipl_src/`) is the product; the Rust crate
  in `referee/` is the referee, in development only. A change to the language or to a diagnostic lands in
  `AIPL_SPEC.md` and `aipl_src/` first, with a case in `tests/conformance/` that says what must happen; then the Rust
  twin is brought along so `referee/` passes the conformance suite and its tests. Where the two disagree, the
  specification and the conformance suite decide, not Rust. What the referee holds, in order: the same programs
  accepted and refused, the same diagnostics (word for word: `referee/tests/test_checker_aipl.rs`,
  `referee/tests/test_resolver_aipl.rs`), the same behaviour at run time (`referee/tests/test_differential.rs`).
  Bytes are the weakest of the four. The codegen comparison (`referee/tests/test_selfhost.rs`, `aipl compile --self`,
  the byte comparisons in `referee/tests/test_bootstrap.rs`) stays while the two compilers are identical, as a tripwire,
  and the bootstrap's fixed point (stage 2 equals stage 3) stays for good. When the self-hosted compiler is meant to
  emit other bytes (an optimizer, say), the same change turns that comparison into a behavioural one. A twin missed
  costs days (`docs/design/AGENT_FRICTION.md` rows 46, 62, 66): a change to what one walk visits is made in its twin
  the same day, and the fuzzers (`tools/fuzz.py`) are what find the misses.
- **The seed rule.** The compiler's source may use only what the checked-in seed knows (`docs/design/AGENT_FRICTION.md`
  rows 14, 32, 52, 57). `sh tools/install_hooks.sh` installs a pre-push hook that has the seed build `aipl_src/aipl.aipl`
  (about 5 seconds); CI's `selfhosted` job runs the whole `sh bootstrap/build.sh`.
- **The VM follows wasm.** When the VM and compiled code disagree, the VM
  is fixed (`referee/tests/test_differential.rs`).
- **Tests assert values.** A test that only checks "returned a number" or
  "produced some bytes" is not a test. Check each new test by breaking the
  code under test on purpose and watching it fail.
- **Measure before keeping an optimisation.** Native-code speed changes are
  kept only when the benchmarks show a gain (docs/BENCHMARKS.md records
  several that did not).
- **Documentation states what is true now.** History goes in
  `docs/history/`, not the spec; a finished plan goes in `docs/design/`
  with its status at the top.

## Direction (agreed 2026-10-02)

These are the project owner's goals. They decide the order below and the answer to most design questions.

- **AIPL programs ship as standalone executables.** `aipl compile --exe prog.aipl -o prog` gives one file you copy anywhere and run.
- **Portability is required.** A program behaves identically on every platform. Wasm semantics are the definition (as they already are for the VM).
- **Native where a backend exists, wasm everywhere else:**
  - On a platform with a native backend, AIPL writes the executable itself, with no Rust and no runtime.
  - Elsewhere, the executable is a small prebuilt shim (Rust, embedding wasmtime) plus the program's wasm, bundled into one file by AIPL.
  - A plain `.wasm` stays available for any WASI host or a browser.
- **Native backends translate wasm, not AIPL:** AIPL → wasm (codegen.aipl) → machine code, written in AIPL. New language features then only touch AIPL → wasm, and every native backend is tested by matching wasmtime's output on the same tests.
- **Platforms:** Linux x86-64 first. Later, by anyone: Linux ARM64, macOS on Apple Silicon, Windows 10/11 on x86-64 and ARM64. Each is a self-contained backend. The Rust toolchain also builds for Windows x86-64 (`x86_64-pc-windows-msvc`, the C runtime linked in; the `windows` job in CI): everything through WebAssembly works, including `std/proc`, sockets and `--serve-fresh` (`referee/tests/test_windows.rs` and `referee/tests/test_net.rs` run in that job), and `tools/package_release.ps1` and `install.ps1` pack and install it. A Windows machine to try it on: build it with `cargo build --release` in a Developer PowerShell.
- **Migrate off Rust.** Rust was the bootstrap language. Before writing anything new in Rust, state why it cannot be AIPL. The irreducible native code is the shim that boots wasmtime on platforms without a native backend; it contains no decisions (flags, permissions policy, and bundling live in AIPL). The end state has no Rust in the compiler: `aipl` is either a native binary built by itself or the shim plus the AIPL compiler as wasm, rebuilt from a pinned stage-0 `aiplc.wasm` like Go and Rust bootstrap from a previous release.
- **Language design:** whatever is more correct and less error-prone for AI agents (strict types, explicit forms); human ergonomics are secondary. Make structural changes early.

## Decisions already made (don't re-litigate without cause)

- **Imports are qualified by default:** `(import name)` / `(import name as alias)`, called as `name.fn`, and struct names likewise (`name.Struct`). The goal is code from many uncoordinated AI authors composing without silent name collisions.
- **The compile target is wasm + WASI.** It is the only vendor-neutral ABI with real I/O, and the semantic reference for every backend. Native code is produced *from* that wasm (next item), never directly from AIPL.
- **Native executables come from a wasm-to-machine-code backend written in AIPL** (decided 2026-10-02, replacing P13's "no native backend"; the earlier decision is P13 in docs/history/AUDIT_2026-10.md, and docs/design/NATIVE_BACKEND_PLAN.md opens with why it changed). Linux x86-64 first (`docs/design/NATIVE_BACKEND_PLAN.md`); wasm plus the `aipl-run` launcher on every other platform. The earlier `elf_emitter` experiment was deleted with the attic (2026-10-06; git keeps it).
- **Rust is for primitives and infrastructure, not compiler logic.** New opcodes and bootstrap bug fixes are fine. Parsing, resolution, and codegen policy belong in AIPL. Ask "is this a primitive or logic?" before reaching for Rust.
- **Wasm semantics are the spec.** When the VM and wasm disagree, fix the VM. Contracts and array bounds checks used to be VM-only; since 2026-10-05 compiled code runs them too (docs/design/CHECKS_PLAN.md).
- **Threads use the wasi-threads ABI** (shared memory, `wasi.thread-spawn`) with AIPL's own host, since wasmtime 47 dropped wasi-threads (decided 2026-10-02). Native builds create threads with Linux `clone` under the same semantics.

## Lessons that cost real time

- **Hand-typed deeply nested AIPL drops functions silently.** A premature `)` ends the `(module` early. The parser now rejects tokens after the module end, but a misplaced paren *inside* a function can still re-nest code validly. For anything nested more than 3–4 levels, generate the S-expression from a small builder script rather than typing it.
- **Hand-encoded byte strings in self-tests break silently.** Two test-harness bugs (docs/history/WORK_LOG.md) were each one wrong number in a long list of `mem.store8` calls. Prefer string literals plus `str.ptr` where the code under test allows it.
- **"Returns a number" is not a passing test.** Assert the value. Each new self-test should be checked by breaking an assertion on purpose and confirming it fails.
- **Re-run everything before believing a "done" claim.** Including `cargo test --no-fail-fast`: a test binary that aborts with a stack overflow hides the rest of its results.

