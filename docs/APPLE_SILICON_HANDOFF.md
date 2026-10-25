# AIPL on Apple Silicon: instructions for the agent on the Mac

Written 2026-10-25 by Claude (Opus 5.5) for Gemini, working on the owner's
new ARM64 Apple Silicon Mac. It says what AIPL does on that machine today,
what to build and test there, in what order, where to stop and report, and
the rules of this repository. Facts marked *checked* were read in the code
on 2026-10-25; everything else is an expectation for you to test. If what
you find disagrees with this file, the code and your measurements win: say
so in your report.

## 1. Who you work for and how

- The owner is an economist and a junior Python and SQL developer. Explain
  architecture in plain terms.
- AIPL is a programming language for AIs, written by AIs. Choose what is
  correct and least error-prone for a model writing it; human ergonomics
  do not matter.
- Every claim needs evidence: a command and its output, a test, or a file
  and line. The owner re-checks other agents' work, and an earlier outside
  review of this repository was checked claim by claim
  (docs/NOTES_REVIEW.md); several of its claims were wrong. Write your
  report so it can be checked the same way.
- Disagree when the evidence says so. Do not paper over a failure: a red
  test is reported with its output, not skipped silently.
- No hand-written cryptography, ever (the owner will not pay for an audit).

## 2. The repository, briefly

Read, in this order: README.md, DEVELOPING.md, docs/HANDOFF.md (the state
of the work and the rules), docs/design/NATIVE_BACKEND_PLAN.md (how the
native backend is built), and docs/notes/arm64_build.md with its verdicts
in docs/NOTES_REVIEW.md, section "arm64_build" (one claim there is wrong:
ARM64's `sdiv` not trapping is a cost, not an advantage, since WebAssembly
requires the trap, so an ARM64 backend must add the checks).

What runs where (*checked*):

- **Two toolchains, kept byte for byte equal.** The Rust toolchain
  (`src/`: parser, checker, VM, wasm compiler, the `aipl` command, and the
  `aipl-run` launcher, which embeds wasmtime) and the self-hosted one
  (`aipl_src/`, written in AIPL). Both turn a program into the same
  WebAssembly bytes. Any change to one is made to the other.
- **The native backend** (`aipl_src/native/`: wasm_reader.aipl, lower.aipl,
  x64.aipl, elf.aipl, runtime.aipl, wasi.aipl, sockets.aipl, fresh.aipl,
  native.aipl) translates WebAssembly into a static Linux x86-64 ELF
  executable that makes its own system calls. It exists only in AIPL; the
  Rust command runs it inside wasmtime (src/native.rs).
- `aipl_core::native::supported()` (src/native.rs) is true only on Linux
  x86-64. Elsewhere `aipl compile --exe` makes a **launcher bundle** instead:
  a copy of `aipl-run` with the program's wasm, its `--listen` grants, and a
  20-byte `AIPLEXE2` trailer appended (src/main.rs `write_executable`), and
  `--target native` is refused. install.sh says the same on Darwin.
- **The seed** `bootstrap/aipl-linux-x86_64` is a static Linux x86-64 binary
  that builds the self-hosted compiler with no Rust (`sh bootstrap/build.sh`).
  It cannot run on macOS directly.
- **CI** (.github/workflows/ci.yml) runs only on `ubuntu-latest` (x86-64):
  build, clippy with `-D warnings`, `cargo test --release`, the AIPL test
  suite, and two program fuzzers. `tools/ci.sh` runs the same steps locally
  (it also runs the seed build, so on the Mac run its steps one by one), and
  `tools/ci-status.sh` shows a run's progress.
- `.cargo/config.toml` sets `RUST_MIN_STACK` for tests (deep recursion);
  anything that runs test binaries without cargo must set it too.
- Tests that need the native backend: tests/test_bootstrap.rs is gated to
  Linux x86-64, and tests/test_runner.rs checks the platform; most of
  tests/test_native.rs and several others are **not** gated, so expect them
  to fail on macOS. That is the first thing to sort out (task 2).

## 3. Rules of the repository

- **Branch:** work on `features/apple-silicon` (this file is its first
  commit). Never commit to `main` or `development`; the owner merges. Do
  not touch `features/when` (Claude's branch, in progress).
- **Stay out of these files** (Claude is changing them on features/when):
  tests/test_serve.rs, tests/test_net.rs, src/net.rs, src/bin/aipl_run.rs,
  src/sugar.rs, src/parser.rs, aipl_src/parser.aipl, aipl_src/resolver.aipl,
  aipl_src/codegen.aipl. If a fix needs one of them, describe it in your
  report instead of making it.
- **Git safety:** never `git stash`, `git reset`, `git restore`, `git clean`,
  or `git checkout <branch>` in a checkout with uncommitted work (undo an
  edit by editing it back). Commit only your own files, by explicit path.
- **Never refresh the seed** (bootstrap/) without the owner's approval.
- **Do not change the language** (syntax, checker rules, std behaviour).
  This work is about platforms.
- **Gate, do not delete.** A test that cannot run on a platform gets a
  `#[cfg(...)]` or a runtime check with a comment saying why, and keeps
  running where it can. Never weaken an assertion to make it pass.
- **Run the checks before every commit you hand over**, on the Mac and,
  where it applies, in the Linux containers (task 4), and paste the
  summary into your report.
- Log your own mistakes writing AIPL in docs/design/AGENT_FRICTION.md (a new
  row at the end, with your name), as the other agents do.

## 4. The tasks, in order

Each has a deliverable. Stop after task 5 (a plan, no code) and wait for the
owner.

### Task 1: build and run AIPL on the Mac

1. Install Rust with rustup (stable), Python 3, and git. Record `uname -a`,
   `sw_vers`, `rustc --version`, and the chip (`sysctl -n machdep.cpu.brand_string`).
2. `cargo build --release`. Report any failure in full.
3. Try the toolchain by hand and record each result:
   - `cd examples && ../target/release/aipl eval word_count.aipl -- input.txt`
     (word_count reads a file named on its command line) and two or three
     other examples, `aipl test aipl_src/test_suite.aipl`,
     `aipl verify`, `aipl doc vec`, `aipl eval -e '(+ 40 2)'`;
   - `aipl compile examples/word_count.aipl -o wc.wasm`, then from examples/
     `../target/release/aipl run ../wc.wasm input.txt`;
   - a server: `aipl eval --listen 127.0.0.1:8080 tests/aipl/net/serve_demo.aipl -- 127.0.0.1:8080 3`
     and three `curl http://127.0.0.1:8080/` calls.

Deliverable: a section "Task 1" in docs/APPLE_SILICON_REPORT.md with the
environment and each command's result.

### Task 2: make the test suite honest on macOS

1. `cargo test --release --no-fail-fast 2>&1 | tee test-macos.log` (on an
   M-series Mac expect well under 15 minutes). `python3 tools/ci_tests.py`
   gives per-test times and per-binary memory in `target/ci_tests.jsonl` if
   you want them.
2. Sort every failure into: (a) needs the native backend or the seed, so
   cannot run on macOS (expected); (b) a real difference on macOS (a bug:
   paths, signals, sockets, file modes, line endings, stack size, timing);
   (c) unclear. Each with the test's name and the first lines of its output.
3. For (a): gate the test to Linux x86-64 the way tests/test_bootstrap.rs
   and tests/test_runner.rs already do, with a one-line comment. Where only
   part of a test needs native (often a loop over backends), skip only that
   part on other platforms.
4. For (b): fix it if the fix is small and outside the files in section 3,
   with a test that shows it; otherwise report it.
5. Clippy must stay clean: `cargo clippy --release --all-targets -- -D warnings`.
6. Run the two fuzzers: `python3 tools/fuzz.py --cases 3000 --seed 1` and
   `python3 tools/run_fuzz.py --cases 500 --seed 1`. run_fuzz compares the
   VM with compiled code; see whether it expects native executables.

Deliverable: commits on the branch (gates, small fixes), and the report's
"Task 2" section: the failure list with categories, what you gated and why,
and the final `cargo test` summary on macOS (0 failed is the goal; any test
still failing is listed with its reason).

### Task 3: macOS executables (the launcher bundle)

On Apple Silicon every executable must carry a valid code signature, or the
kernel kills it ("Killed: 9"). The Rust linker signs `aipl-run` ad hoc. A
bundle is `aipl-run` with bytes appended, which may or may not break that
signature. Nobody has tried it on a Mac. Find out:

1. `aipl compile --exe examples/word_count.aipl -o wc`, then from examples/
   `../wc input.txt`. Does it run? Record
   `codesign -dv --verbose=4 wc` and `codesign --verify --strict wc`.
2. If it is killed or refused: find the smallest correct fix and propose
   it (for example re-signing ad hoc after writing, `codesign --force -s -`,
   if that keeps the appended program readable; or carrying the program in
   a section of its own). src/bin/aipl_run.rs is on the list in section 3,
   so write the proposal with the evidence; a change to src/main.rs
   `write_executable` alone is yours to make and test.
3. Also try a bundle with `--listen` and a server program, and
   `aipl compile --exe --target wasm --serve 127.0.0.1:8081 tests/aipl/net/fresh_demo.aipl -o fresh`
   (a fresh instance per connection). Note: killing `aipl serve --fresh`
   by its process id leaves its `aipl-run` child serving (a known issue);
   kill the process group, or use the bundle.
4. Whether `tools/make_kit.sh` can make a macOS kit (today it makes a Linux
   x86-64 one): what would change. Report only.

Deliverable: the report's "Task 3" section, and any change to
`write_executable` with a test that runs on macOS and is skipped elsewhere.

### Task 4: the Linux side, in containers on the Mac

Use Docker Desktop, OrbStack, or Colima.

1. **linux/arm64** (runs at native speed): clone into a container
   (`rust:1` image), build, and run the test suite. Same expectations as
   macOS (no native backend); report what differs from macOS.
2. **linux/amd64** (through Rosetta, or QEMU if Rosetta is off; slower):
   run every step of CI as GitHub does, including `sh bootstrap/build.sh`
   and the native tests (`sh tools/ci.sh .` runs them all). This checks the
   x86-64 native backend and the seed under emulation. Report timings and
   any failure, and say whether failures look like emulation (timing,
   signals) or real.
3. Check that a native executable built in the amd64 container runs there,
   and that the same program's launcher bundle from the arm64 container
   runs there too.

Deliverable: the report's "Task 4" section, with the commands so anyone can
repeat it (a small script under tools/ is welcome).

### Task 5: a plan for native ARM64 (no code)

Write docs/design/ARM64_PLAN.md in the style of the other plans in
docs/design/ (a status line, "at a glance" table, numbered sections, open
questions for the owner). It should cover, with evidence from the code:

- **Linux aarch64 ELF first.** What stays (wasm_reader.aipl, native.aipl's
  driving, the ELF writer's structure), what is new (an AArch64 encoder in
  place of x64.aipl; lower.aipl's register model for 31 registers; the
  runtime's system calls, whose numbers differ on aarch64 Linux), and how
  WebAssembly's traps are kept (integer division by zero and `INT_MIN / -1`
  need explicit checks on ARM64; out-of-bounds memory through guard pages
  and the signal handler, as runtime.aipl does on x86-64).
- **Then macOS (Mach-O), separately.** macOS has no stable system-call
  interface: programs must call libSystem, so a static executable that
  makes its own calls (what the Linux backend makes) is not an option there.
  Every executable must be signed. Say what that means for the design
  (dynamic linking to libSystem, the Mach-O writer, signing), and whether
  the launcher bundle is a good enough answer for macOS for now.
- **Testing:** qemu-aarch64 user mode on Linux x86-64 (GitHub's runners),
  `ubuntu-24.04-arm` and `macos-14` runners for CI, and how the existing
  native tests (tests/test_native*.rs, tools/run_fuzz.py) would run against
  a second target.
- **Size of the work:** the native backend's current size
  (`wc -l aipl_src/native/*.aipl`) as a guide, in steps the owner can
  approve one at a time.
- **Open questions for the owner.**

Deliverable: the plan file, then stop. Nothing in it is built until the
owner approves it.

## 5. Handing back

When you stop (after task 5, or earlier if something blocks you):

- Commit by explicit path on `features/apple-silicon` and push the branch.
- The report, docs/APPLE_SILICON_REPORT.md, starts with a summary: what
  works on the Mac, what you changed (commit by commit), what is broken and
  why, and what you recommend next. Every claim with its evidence.
- Do not merge into `development` or `main`.
